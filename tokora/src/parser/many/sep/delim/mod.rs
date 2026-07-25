use core::mem;

use crate::{
  TryParseInput,
  container::Container as ContainerT,
  delimiter::Delimiter,
  emitter::{FullContainerEmitter, SeparatedEmitter, UnclosedEmitter},
  error::Unclosed,
  punct::Punctuator,
  try_parse_input::{Accept, Decline},
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

impl<'inp, L, P, Sep, O, Ctx, Delim, Lang: ?Sized, Cmpl>
  DelimitedBy<Separated<&mut P, Sep, O, L, Ctx, Lang, Cmpl>, Delim>
{
  fn parse_separated<'closure, Container, CH, SP, EH>(
    &mut self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    continue_state_handler: &CH,
    separator_state_handler: &SP,
    end_state_handler: &EH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    L: Lexer<'inp>,
    P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>
      + FullContainerEmitter<'inp, L, Lang>
      + UnclosedEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
      From<UnexpectedEot<L::Offset, Lang>> + From<Unclosed<Delim, L::Span, Lang>>,
    Container: DelimiterHandler<'inp, L> + SeparatorHandler<'inp, L> + ContainerT<O>,
    EH: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
    CH: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
    SP: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
  {
    trace_event!(inp, "separated");
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
    }

    let mut state: State<L::Token, L::Span> = State::Start;
    let parser = &mut self.parser;
    let mut num_elems = 0;

    let elems_start = inp.cursor().clone();
    let mut cursor = elems_start.clone();
    let state = loop {
      let mut ps = None;
      let peek_span = match inp.try_expect_map(|t| {
        if Sep::eval(&t.data.kind()) {
          Some(false)
        } else {
          match Delim::is_close(&t.data.kind()) {
            true => Some(true),
            false => {
              ps = Some(t.span().clone());
              None
            }
          }
        }
      })? {
        None => match ps {
          None => break state,
          Some(span) => span,
        },
        Some((is_closed, tok)) => {
          if is_closed {
            // The closer is committed mid-scan: no close miss, and (as before) no
            // end-state pass on this path.
            container.on_close_delimiter(tok);
            return Ok(inp.span_since(&elems_start));
          } else {
            state = parser.handle_separator(state, inp, container, separator_state_handler, tok)?;
            cursor = inp.cursor().clone();
            continue;
          }
        }
      };

      match parser.f.try_parse_input(inp) {
        // The never-recoverable gate and its terminal dual: a frontier `Incomplete` (const-false
        // under `Complete`) or a terminal scanner stop from the element parser re-raises untouched —
        // never spent as a diagnostic, since no further input clears either. The terminal witness
        // reads the *committed cursor* ([`at_committed_boundary`]), so a boundary a prior lookahead
        // already latched does not mis-charge an ordinary element failure short of it. Failure-arm
        // only — a successful element does zero terminal work; no `MaybeTerminal` bound needed.
        Err(e) if Cmpl::is_incomplete_error(&e) || inp.at_committed_boundary() => return Err(e),
        Err(e) => {
          let span = inp.span_since(&cursor);
          inp.emitter().emit_error(Spanned::new(span, e))?;
        }
        Ok(Decline) => break state,
        Ok(Accept(elem)) => {
          // if the peeked token belongs to an element, check the current state
          state = parser.handle_continue(
            state,
            inp,
            &anchor,
            peek_span,
            elem,
            &mut num_elems,
            container,
            continue_state_handler,
          )?;
        }
      }

      let new_cursor = inp.cursor().clone();
      if new_cursor.as_inner() == cursor.as_inner() {
        break state;
      }
      cursor = new_cursor;
    };

    // PRIMARY — classify the close position WITHOUT consuming (`probe_close` leaves the
    // scanned token cached) and emit the close-status diagnostic before the end-state
    // secondaries: under a fail-fast emitter `handle_end`'s TooFew/trailing emission
    // would otherwise short-circuit first and an unterminated list would never surface
    // as `Unclosed`. A FRESH token is probed here rather than carried out of the loop,
    // so the loop's last non-sep/non-close token cannot masquerade as the wrong closer.
    // The four-way probe also keeps a terminal scanner stop out of the `Unclosed` path.
    let mut close_carrier = None;
    match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
      // The closer is at hand: no close miss. Carry it out; committed by value after the
      // end-state pass.
      CloseStatus::Close(ct) => close_carrier = Some(ct),
      // (b) a wrong token sits where the closer should: unexpected-token, expected-close.
      CloseStatus::WrongToken(tok) => inp
        .emitter()
        .emit_unexpected_token(Delim::unexpected_close_token(tok))?,
      // (a) genuine end of input with the opener still open: never closed — the ONE
      // `Unclosed` path.
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
      // A terminal scanner stop (limit trip / latched poison): its own diagnostic
      // already explains the halt — propagate it and add no `Unclosed` on top.
      CloseStatus::Tripped => {
        return Err(
          UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
            .into_terminal()
            .into(),
        );
      }
    }

    // SECONDARY — the end-state diagnostics (counts, separator policy), recorded after
    // the primary under a recovering emitter.
    let elems_span = parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;

    // Commit the carried closer by value (no re-scan) at the same program point as the
    // old deferred `try_expect` — after the end-state pass.
    if let Some(ct) = close_carrier {
      container.on_close_delimiter(inp.commit_probed(ct));
    }

    Ok(elems_span)
  }
}
