use core::mem;

use crate::{
  container::Container as ContainerT,
  delimiter::Delimiter,
  emitter::{FromUnclosed, FullContainerEmitter, UnclosedEmitter},
  error::Unclosed,
  span::Span as _,
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

impl<'inp, L, P, O, Condition, Ctx, Delim, W, Lang: ?Sized>
  DelimitedBy<&mut RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>, Delim>
{
  fn parse_repeated<Container>(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    container: &mut Container,
    on_stop: impl FnOnce(
      usize,
      &mut InputRef<'inp, '_, L, Ctx, Lang>,
      &L::Span,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    L: Lexer<'inp>,
    P: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang> + UnclosedEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
      From<UnexpectedEot<L::Offset, Lang>> + FromUnclosed<'inp, L, Lang>,
    Container: Default + ContainerT<O> + DelimiterHandler<'inp, L>,
  {
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

    let mut nums = 0;
    let mut full = false;
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the opener's
    // own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();

    loop {
      // Probe the close position WITHOUT consuming, so a terminal scanner stop is not
      // misread as EOF. `Close` short-circuits before the stop condition is
      // consulted, exactly as the consuming `try_expect` did.
      match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
        CloseStatus::Close(ct) => {
          // Commit the carried closer by value (no re-scan) and run the end handler.
          container.on_close_delimiter(inp.commit_probed(ct));
          let span = inp.span_since(&anchor);
          return on_stop(nums, inp, &span).map(|_| mem::take(container));
        }
        // A terminal scanner stop: its own diagnostic already explains the halt —
        // propagate it and add no `Unclosed`.
        CloseStatus::Tripped => {
          return Err(
            UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
              .into_terminal()
              .into(),
          );
        }
        // The closer is absent (a wrong token or genuine EOF) — consult the stop
        // condition to decide whether another element is expected.
        close => {
          // A short decision window can be a genuine end of input, but one truncated by a terminal
          // scanner stop is not: surface the committed end-of-input error before anything else.
          // `decide` is fallible and emitting, and a zero-width element makes no progress that would
          // re-lex the frontier — leaving the trip-truncated front token at the epilogue's close
          // probe where it reads as a spurious wrong closer — so neither may run first: no call may
          // preempt the terminal stop. `end` is the committed watermark, captured before the peek
          // (the fill never advances it).
          let end = inp.span().end();
          let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
          if terminal {
            return Err(UnexpectedEot::eot_of(end).into_terminal().into());
          }
          match self.parser.condition.decide(peeked, emitter)? {
            // missing ending delimiter
            Action::Stop => {
              // No closer plus a stop conclude *absence*: "no more elements, and nothing closed
              // them". The element's own lookahead can latch a terminal scanner stop and still return
              // `Ok` with a short window, leaving the pre-trip tokens cached — this decision window
              // is then served whole from that cache, so it carries no terminal flag for the gate
              // above to see, and the same cached token reads as a spurious wrong closer. The verdict
              // in hand is necessarily a close miss (a `Close` committed and returned in the outer arm
              // above, a `Tripped` propagated there), so no genuine close is gated; attempt-relative
              // against the post-opener snapshot.
              if inp.latched_during_attempt(&latch) {
                return Err(
                  UnexpectedEot::eot_of(inp.span().end())
                    .into_terminal()
                    .into(),
                );
              }
              // PRIMARY — the close-miss diagnostic first: under a fail-fast emitter
              // this short-circuits, so `Unclosed` (not the secondary bounds) surfaces.
              match close {
                // (b) a wrong token sits where the closer should: unexpected-token,
                // expected-close (the existing vocabulary).
                CloseStatus::WrongToken(tok) => inp
                  .emitter()
                  .emit_unexpected_token(Delim::unexpected_close_token(tok))?,
                // (a) end of input with the opener still open: the opener was never
                // closed.
                _ => {
                  if let Some(open_span) = open_span.clone() {
                    inp
                      .emitter()
                      .emit_unclosed(Unclosed::<Delim, L::Span, Lang>::of(
                        open_span,
                        Delim::KIND,
                        Delim::name(),
                      ))?;
                  }
                }
              }
              // SECONDARY — the delimited driver used to return here WITHOUT
              // running the repeated end handler, silently dropping the `TooFew`/bounds
              // diagnostic under a recovering emitter (the plain `Repeated` driver runs
              // it). Run it after the primary, matching the primary-then-secondary order
              // the separated drivers established.
              let span = inp.span_since(&anchor);
              return on_stop(nums, inp, &span).map(|_| mem::take(container));
            }
            Action::Continue => {
              // TODO(al8n): tracing dropped element
              let item = self.parser.f.parse_input(inp)?;
              push_element(&mut nums, &mut full, container, item, inp, &anchor)?;
            }
          }
        }
      }

      // A `Continue` cycle that consumed nothing re-sees the same close-delimiter check and lookahead
      // and would loop forever. The progress metric is committed consumption (`span().end()`), never
      // the cache-front cursor — a lookahead fill moves that across skipped trivia without consuming,
      // reading a zero-width element as false progress. `<=`, not `==`: the watermark cannot regress
      // within a cycle, so anything not strictly ahead is a stall. Break to the close-delimiter
      // epilogue below, exactly as the plain `Repeated` driver does.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break;
      }
      committed = new_committed;
    }

    // No progress was made — treat as end of elements (the same epilogue as
    // `DelimitedBy<Repeated>`): accept a close delimiter if it is at hand, report it
    // otherwise, then run the stop handler on the delimited span. The four-way probe
    // keeps a terminal scanner stop out of the `Unclosed` path.
    match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
      // The closer is at hand: commit the carried token by value — no re-scan. A cache-first
      // verdict on a real pre-trip token, so the construct genuinely closed and stays a success.
      CloseStatus::Close(ct) => container.on_close_delimiter(inp.commit_probed(ct)),
      CloseStatus::WrongToken(tok) => {
        // No closer: the stall plus this verdict conclude *absence*, and the element's own lookahead
        // can latch a terminal scanner stop after the decision gate ran and still return `Ok` with a
        // short window, so that conclusion may rest on a truncated view. Surface the stop ahead of
        // the close-miss diagnostic; attempt-relative against the post-opener snapshot.
        if inp.latched_during_attempt(&latch) {
          return Err(
            UnexpectedEot::eot_of(inp.span().end())
              .into_terminal()
              .into(),
          );
        }
        inp
          .emitter()
          .emit_unexpected_token(Delim::unexpected_close_token(tok))?
      }
      CloseStatus::Eof => {
        // Same absence conclusion as the wrong-token arm above, so the same gate. No legitimate
        // `Unclosed` is lost: a scan that reaches a live boundary stops there and reports the stop,
        // so an `Eof` verdict cannot coexist with one.
        if inp.latched_during_attempt(&latch) {
          return Err(
            UnexpectedEot::eot_of(inp.span().end())
              .into_terminal()
              .into(),
          );
        }
        // EOI — no tokens left, no close delimiter: the opener was never closed.
        if let Some(open_span) = open_span.clone() {
          inp
            .emitter()
            .emit_unclosed(Unclosed::<Delim, L::Span, Lang>::of(
              open_span,
              Delim::KIND,
              Delim::name(),
            ))?;
        }
      }
      CloseStatus::Tripped => {
        return Err(
          UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
            .into_terminal()
            .into(),
        );
      }
    }

    let span = inp.span_since(&anchor);
    on_stop(nums, inp, &span).map(|_| mem::take(container))
  }
}
