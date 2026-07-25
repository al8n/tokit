use core::mem;

use crate::{
  TryParseInput,
  container::Container as ContainerT,
  delimiter::Delimiter,
  emitter::{FullContainerEmitter, UnclosedEmitter},
  error::{Unclosed, syntax::FullContainer},
  span::Span as _,
  try_parse_input::{Accept, Decline},
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

impl<'inp, L, P, O, Ctx, Delim, Lang: ?Sized, Cmpl>
  DelimitedBy<&mut Repeated<P, O, L, Ctx, Lang, Cmpl>, Delim>
{
  fn parse_repeated<Container>(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    on_stop: impl FnOnce(
      usize,
      &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
      &L::Span,
    ) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
  ) -> Result<Container, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    L: Lexer<'inp>,
    P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang> + UnclosedEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error:
      From<UnexpectedEot<L::Offset, Lang>> + From<Unclosed<Delim, L::Span, Lang>>,
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
    };

    let mut nums = 0;
    let mut elem_cur = inp.cursor().clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the
    // opener's own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();

    loop {
      match self.parser.f.try_parse_input(inp) {
        // The never-recoverable gate and its terminal dual: a frontier `Incomplete` (const-false
        // under `Complete`) or a terminal scanner stop from the element parser re-raises untouched —
        // never spent as a diagnostic, since no further input clears either. The terminal witness
        // reads the *committed cursor* ([`at_committed_boundary`]), so a boundary a prior lookahead
        // already latched does not mis-charge an ordinary element failure short of it. Failure-arm
        // only — a successful element does zero terminal work; no `MaybeTerminal` bound needed.
        Err(err) if Cmpl::is_incomplete_error(&err) || inp.at_committed_boundary() => {
          return Err(err);
        }
        Err(err) => {
          let span = inp.span_since(&elem_cur);
          inp.emitter().emit_error(Spanned::new(span, err))?;
        }
        Ok(Accept(nxt)) => {
          // TODO(al8n): tracing dropped element
          if let Err(_e) = container.push(nxt) {
            let span = inp.span_since(&anchor);
            inp.emitter().emit_full_container(FullContainer::of(
              span,
              nums,
              container.max_capacity(),
            ))?;
          }
          nums += 1;
        }
        // no more elemnts.
        Ok(Decline) => {
          // Classify the close position with the four-way probe so a terminal scanner
          // stop is not misread as EOF and grown into a spurious `Unclosed`.
          match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
            // The closer is at hand: commit the carried token by value — no re-scan,
            // and cache-independent (a blackhole `()` would drop a pushed-back closer).
            //
            // The probe is cache-first, so this verdict rests on a REAL pre-trip token: the construct
            // genuinely closed and stays a success even if the element's lookahead latched a terminal
            // stop somewhere past that closer.
            CloseStatus::Close(ct) => container.on_close_delimiter(inp.commit_probed(ct)),
            // A wrong token where the closer belongs: unexpected-token, expected-close.
            CloseStatus::WrongToken(tok) => {
              // No closer: the decline plus this verdict conclude *absence*, and the element's own
              // lookahead can latch a terminal scanner stop and still return `Ok` with a short
              // window, so that conclusion may rest on a truncated view. Surface the stop ahead of
              // the close-miss diagnostic; attempt-relative, so an inherited boundary is not
              // mis-charged here.
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
            // EOI — no close delimiter found: the opener was never closed.
            CloseStatus::Eof => {
              // Same absence conclusion as the wrong-token arm above, so the same gate. No legitimate
              // `Unclosed` is lost: a scan that reaches a live boundary stops there and reports the
              // stop, so an `Eof` verdict cannot coexist with one.
              if inp.latched_during_attempt(&latch) {
                return Err(
                  UnexpectedEot::eot_of(inp.span().end())
                    .into_terminal()
                    .into(),
                );
              }
              if let Some(open_span) = open_span.clone() {
                inp
                  .emitter()
                  .emit_unclosed(Unclosed::<Delim, L::Span, Lang>::of(
                    open_span,
                    Delim::name(),
                  ))?;
              }
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
          }

          let span = inp.span_since(&anchor);
          return on_stop(nums, inp, &span).map(|_| mem::take(container));
        }
      }

      // A cycle that consumed nothing re-sees the same input and would retry forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill (a `probe_close` or `try_expect` decline pushing a token back) moves that across skipped
      // trivia without consuming, reading a zero-width element as false progress. `<=`, not `==`: the
      // watermark cannot regress within a cycle, so anything not strictly ahead is a stall.
      // `elem_cur` stays the error-span anchor.
      let new_committed = inp.span().end();
      if new_committed <= committed {
        break;
      }
      committed = new_committed;
      elem_cur = inp.cursor().clone();
    }

    // No progress was made — treat as end of elements. Classify the close position
    // with the four-way probe so a terminal scanner stop is not misread as EOF.
    match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
      // The closer is at hand: commit the carried token by value — no re-scan. A cache-first
      // verdict on a real pre-trip token, so the construct genuinely closed and stays a success.
      CloseStatus::Close(ct) => container.on_close_delimiter(inp.commit_probed(ct)),
      CloseStatus::WrongToken(tok) => {
        // No closer: the stall plus this verdict conclude *absence*, and the element's own lookahead
        // can latch a terminal scanner stop and still return `Ok` with a short window, so that
        // conclusion may rest on a truncated view. Surface the stop ahead of the close-miss
        // diagnostic; attempt-relative against the post-opener snapshot.
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
