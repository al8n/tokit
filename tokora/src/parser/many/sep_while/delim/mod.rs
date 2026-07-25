use core::mem;

use crate::{
  container::Container as ContainerT,
  emitter::{FullContainerEmitter, SeparatedEmitter, UnclosedEmitter},
  error::Unclosed,
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

mod allow_leading;
mod allow_leading_require_trailing;
mod allow_surrounded;
mod allow_trailing;

mod require_leading;
mod require_leading_allow_trailing;
mod require_surrounded;
mod require_trailing;

impl<'c, 'inp, L, P, Sep, O, Condition, Ctx, Delim, W, Lang: ?Sized>
  DelimitedBy<SeparatedWhile<&'c mut P, Sep, &'c mut Condition, O, W, L, Ctx, Lang>, Delim>
{
  fn parse_separated<'closure, Container, CH, SP, EH>(
    &mut self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang>,
    container: &mut Container,
    continue_state_handler: &CH,
    separator_state_handler: &SP,
    end_state_handler: &EH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    L: Lexer<'inp>,
    P: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
      + FullContainerEmitter<'inp, L, Lang>
      + UnclosedEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
      From<UnexpectedEot<L::Offset, Lang>> + From<Unclosed<Delim, L::Span, Lang>>,
    Container: DelimiterHandler<'inp, L> + SeparatorHandler<'inp, L> + ContainerT<O>,
    EH: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
    CH: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
    SP: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
  {
    trace_event!(inp, "separated_while");
    // Sync the input to the next token boundary, any lexer errors will be emitted during this process.
    let anchor = inp.cursor().clone();
    let mut first_kind = None;
    let left_delimiter = inp.try_expect_or_stop(|tok| {
      let (span, tok) = tok.into_components();
      match Delim::is_open(&tok.kind()) {
        false => {
          first_kind = Some(Delim::unexpected_open_token(Spanned::new(
            span.clone(),
            tok.clone(),
          )));
          false
        }
        true => true,
      }
    })?;

    // The opener's span, captured iff an opener is actually committed. It is the anchor of
    // the `Unclosed` diagnostic below: no opener, no unclosed.
    let mut open_span: Option<L::Span> = None;
    // Discriminate on the captured evidence, NOT on `is_eoi`: the opener predicate lexes the
    // candidate token, so a wrong FINAL token leaves the lexer at EOI even though a real token
    // sat at the opener position (issue #85). `first_kind` records that observation.
    match (left_delimiter, first_kind) {
      // An opener is committed — behavior unchanged.
      (Some(open), _) => {
        open_span = Some(open.span_ref().clone());
        container.on_open_delimiter(open);
      }
      // A wrong opener was observed: emit the captured unexpected-open-token regardless of the
      // lexer's EOI state. The token stays cached/unconsumed, exactly like the non-EOI path.
      (None, Some(wrong)) => {
        inp.emitter().emit_unexpected_token(wrong)?;
      }
      // Nothing was observed at the opener position: a genuinely empty opener slot — the one
      // genuine EOI path. A terminal scanner stop no longer lands here — `try_expect_or_stop`
      // surfaces it directly above — so this end-of-input error stays recoverable.
      (None, None) => {
        return Err(UnexpectedEot::eot_of(inp.cursor().as_inner().clone()).into());
      }
    };

    let mut state: State<L::Token, L::Span> = State::Start;
    let parser = &mut self.parser;
    let mut num_elems = 0;

    let elems_start = inp.cursor().clone();
    loop {
      let mut is_sep = false;
      match inp.try_expect(|tok| {
        if Sep::eval(&tok.data.kind()) {
          is_sep = true;
          true
        } else {
          Delim::is_close(&tok.data.kind())
        }
      })? {
        Some(tok) => {
          if is_sep {
            state = parser.handle_separator(state, inp, tok, container, separator_state_handler)?;
            continue;
          }

          parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;
          container.on_close_delimiter(tok);
          return Ok(inp.span_since(&anchor));
        }
        None => {
          // Decision-window gate. `peek_with_emitter_terminal` reports (at no extra hot-path cost)
          // whether the fill came back short because of a terminal scanner stop. The empty-window
          // path below already reclassifies the close position with `probe_close` (which surfaces
          // `Tripped`); this flag closes the remaining gap — a mid-window trip *past* the cached
          // front token, where the condition reads `Stop` and `probe_close` would then see the
          // front token and miss the stop.
          let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;

          let front_span = match peeked.front() {
            None => {
              drop(peeked);

              // Front is empty: reclassify the close position with the four-way probe so
              // a terminal scanner stop is not misread as EOF (finding 1). A real
              // non-sep/non-close token would have entered the cache and made `front()`
              // non-empty, so only genuine EOF or a terminal stop reaches here.
              //
              // PRIMARY — the close-status diagnostic first: under a fail-fast emitter
              // `handle_end`'s TooFew/trailing emission would otherwise short-circuit
              // before an unterminated list could surface as `Unclosed`.
              match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
                // The closer is at hand: commit the carried token by value — no re-scan.
                CloseStatus::Close(ct) => container.on_close_delimiter(inp.commit_probed(ct)),
                // (b) a wrong token was seen where the closer should be.
                CloseStatus::WrongToken(tok) => inp
                  .emitter()
                  .emit_unexpected_token(Delim::unexpected_close_token(tok))?,
                // (a) end of input with the opener still open: the opener was never
                // closed.
                CloseStatus::Eof => {
                  if let Some(open_span) = open_span.clone() {
                    inp
                      .emitter()
                      .emit_unclosed(Unclosed::<Delim, L::Span, Lang>::of(
                        open_span,
                        Delim::name(),
                      ))?;
                  }
                }
                // A terminal scanner stop: its own diagnostic already explains the
                // halt — propagate it and add no `Unclosed`.
                CloseStatus::Tripped => {
                  return Err(
                    UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                      .into_terminal()
                      .into(),
                  );
                }
              }

              // SECONDARY — the end-state diagnostics (counts, separator policy),
              // recorded after the primary under a recovering emitter.
              parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;

              return Ok(inp.span_since(&elems_start));
            }
            Some(front) => front
              .as_maybe_ref()
              .map(|t| t.token().copied(), |t| t.token())
              .into_inner()
              .span()
              .clone(),
          };

          match parser.condition.decide(peeked, emitter)? {
            Action::Stop => {
              // A mid-window terminal scanner stop is not a clean end of list: surface it ahead of
              // any close-miss diagnostic, so an enclosing recovery re-raises it.
              if terminal {
                return Err(
                  UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                    .into_terminal()
                    .into(),
                );
              }
              // PRIMARY — classify the close position WITHOUT consuming (`probe_close`
              // leaves the scanned token cached) and emit the close-status diagnostic
              // before the end-state secondaries: under a fail-fast emitter
              // `handle_end`'s TooFew/trailing emission would otherwise short-circuit
              // first and an unterminated list would never surface as `Unclosed`. The
              // four-way probe also keeps a terminal scanner stop out of `Unclosed`.
              let mut close_carrier = None;
              match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
                // The closer is at hand: carry it out; committed by value below.
                CloseStatus::Close(ct) => close_carrier = Some(ct),
                // (b) a wrong token sits where the closer should be.
                CloseStatus::WrongToken(tok) => inp
                  .emitter()
                  .emit_unexpected_token(Delim::unexpected_close_token(tok))?,
                // (a) end of input with the opener still open: never closed.
                CloseStatus::Eof => {
                  if let Some(open_span) = open_span.clone() {
                    inp
                      .emitter()
                      .emit_unclosed(Unclosed::<Delim, L::Span, Lang>::of(
                        open_span,
                        Delim::name(),
                      ))?;
                  }
                }
                // A terminal scanner stop: its own diagnostic already explains the
                // halt — propagate it and add no `Unclosed`.
                CloseStatus::Tripped => {
                  return Err(
                    UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                      .into_terminal()
                      .into(),
                  );
                }
              }

              // SECONDARY — the end-state diagnostics, after the primary.
              parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;

              // Commit the carried closer by value (no re-scan) at the same program
              // point as the old deferred `try_expect` — after the end-state pass.
              if let Some(ct) = close_carrier {
                container.on_close_delimiter(inp.commit_probed(ct));
              }
              return Ok(inp.span_since(&elems_start));
            }
            Action::Continue => {
              // if the peeked token belongs to an element, check the current state
              state = parser.handle_continue(
                state,
                inp,
                &anchor,
                &front_span,
                &mut num_elems,
                container,
                continue_state_handler,
              )?;
            }
          }
        }
      }
    }
  }
}
