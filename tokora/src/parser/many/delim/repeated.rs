use core::mem;

use crate::{
  TryParseInput,
  container::Container as ContainerT,
  delimiter::Delimiter,
  emitter::{FromUnclosed, FullContainerEmitter, UnclosedEmitter},
  error::Unclosed,
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
    // The committed-consumption watermark at the moment a wrong opener is flagged, iff one is.
    // It is the suppression witness for the close-miss arms below — see the law comment there.
    // FRONT_REPORTED — the close-miss suppression asks *is the report naming this front token
    // still live?*. That is a transactional fact about the emitter's log, so it is not kept here.
    //
    // Three driver-local witnesses were tried — committed position; then position paired with a
    // re-key count; then that pair with its record published before the front is destroyed — and
    // each leaked, for one reason: a stack frame has no rollback semantics. A restore is precisely
    // the operation that erases frame-visible traces. It rewinds the emitter, destroys the front,
    // and returns position to a value no frame-local flag can tell apart from "nothing happened".
    //
    // So the witness lives on the `Input`, is captured into every `Checkpoint`, and is restored in
    // the same block as the emitter mark it describes: a rollback below the flag disarms the
    // watermark and truncates the report as one state, and a rollback above it keeps both, because
    // the report predates the mark. See `InputRef::front_report_live` for the read side and
    // `Input::front_reported_end` for the invariant.
    //
    // The predicate is frame-independent, which is why all eight close-miss arms read it with one
    // identical line: uniformity here is a property of the mechanism rather than a claim about
    // each driver's control flow.
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
        // The watermark is published by the same body that appends the report, and only if the
        // append succeeded — which is what keeps "watermark implies a live report" inductive.
        let front_end = wrong.span_ref().end_ref().clone();
        inp.emit_unexpected_front(wrong, front_end)?;
      }
      // Nothing was observed at the opener position: a genuinely empty opener slot — the one
      // genuine EOI path. A terminal scanner stop no longer lands here — `try_expect_or_stop`
      // surfaces it directly above — so this end-of-input error stays recoverable.
      (None, None) => {
        return Err(UnexpectedEot::eot_of(inp.cursor().as_inner().clone()).into());
      }
    };

    let mut nums = 0;
    let mut full = false;
    let mut elem_cur = inp.cursor().clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the
    // opener's own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();

    loop {
      // The descent witness's baseline, taken once per ELEMENT — the attempt this cycle's gate
      // judges. See the gate below, and `many/repeated/mod.rs` for the reasoning in full.
      let trips = inp.trip_snapshot();
      match self.parser.f.try_parse_input(inp) {
        // The never-recoverable gate and its two terminal witnesses: a frontier `Incomplete`
        // (const-false under `Complete`), a terminal *scanner* stop, or a *descent* budget trip from
        // the element parser re-raises untouched — never spent as a diagnostic, since no further
        // input clears any of the three. The scanner witness reads the *committed cursor*
        // ([`at_committed_boundary`]), so a boundary a prior lookahead already latched does not
        // mis-charge an ordinary element failure short of it. The descent witness compares the
        // session trip counter against this cycle's baseline ([`tripped_during_attempt`]), the
        // counter being bumped before the grammar's `From` runs, so a trip re-raises for a
        // delegating error type and for a discarding one alike — `()` included. Failure-arm only —
        // a successful element does zero terminal work; both witnesses are positional/session
        // facts, so no `MaybeTerminal` bound needed.
        //
        // Per element, not per collection: the counter is monotone and never cleared, so reading it
        // absolutely would re-raise every later element failure — ordinary syntax errors included —
        // once anything in the parse had caught a trip and carried on.
        Err(err)
          if Cmpl::is_incomplete_error(&err)
            || inp.at_committed_boundary()
            || inp.tripped_during_attempt(trips) =>
        {
          return Err(err);
        }
        Err(err) => {
          let span = inp.span_since(&elem_cur);
          inp.emitter().emit_error(Spanned::new(span, err))?;
        }
        // TODO(al8n): tracing dropped element
        Ok(Accept(nxt)) => push_element(&mut nums, &mut full, container, nxt, inp, &anchor)?,
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
              // One junk token, one report: emit unless the emitter already holds a live report
              // naming this very front token. See FRONT_REPORTED at the top of this body.
              if !inp.front_report_live(tok.span_ref().end_ref()) {
                inp
                  .emitter()
                  .emit_unexpected_token(Delim::unexpected_close_token(tok))?
              }
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
                    Delim::KIND,
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
        // One junk token, one report: emit unless the emitter already holds a live report naming
        // this very front token. See FRONT_REPORTED at the top of this body.
        if !inp.front_report_live(tok.span_ref().end_ref()) {
          inp
            .emitter()
            .emit_unexpected_token(Delim::unexpected_close_token(tok))?
        }
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
