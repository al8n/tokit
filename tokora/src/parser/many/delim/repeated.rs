use crate::{
  TryParseInput, container::Container as ContainerT, delimiter::Delimiter,
  emitter::UnclosedEmitter, error::Unclosed,
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

impl<'inp, L, P, O, Ctx, Delim, Lang: ?Sized, Cmpl>
  DelimitedBy<&mut Repeated<P, O, L, Ctx, Lang, Cmpl>, Delim>
{
  /// `Repeated`'s element loop, run **under a delimiter**.
  ///
  /// This driver contributes an opener, a close position and nothing else. The elements are
  /// `Repeated::drive_elements`'s — one stall test, one `many::admit_element`, one
  /// `many::file_element_failure`, all of them the plain driver's — and the count and stop hooks
  /// are the plain driver's [`RepeatedHandler`] rather than a shape of this file's own. What is
  /// left here is what the delimiter genuinely adds
  /// ([#259](https://github.com/al8n/tokora/issues/259)'s stage-2 verdict, and the one difference
  /// it found essential): a delimited list's element boundary is also a **close** position, so
  /// every exit below classifies that position and a real closer passes through
  /// `many::close_after_element`, which appears in no undelimited driver.
  fn parse_repeated<'c, Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    // The count verdict and the end-of-construct pass, as ONE handler — the same `RepeatedHandler`
    // the undelimited driver takes. It used to be two arguments here, a count hook and a bare
    // `FnOnce` stop closure, which made this family the third spelling of one end-of-construct
    // pass; the handler already carries both, and `many::admit_element` runs the count half in
    // front of the push for the same reason it does under `Repeated::parse`.
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    P: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang> + UnclosedEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: ContainerT<O> + DelimiterHandler<'inp, L>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang, Cmpl>,
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
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the
    // opener's own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();

    // The elements themselves, and the trip baseline of the attempt that ended them — a decline,
    // or a cycle that committed nothing. Both conclude *absence* on the strength of that one
    // attempt, and this driver's exits below are the only place either conclusion is acted on, so
    // the baseline travels out of the loop with it. The two used to be separate code paths here,
    // the decline arm carrying an inline copy of the epilogue below; one loop with one exit means
    // one copy of it.
    let elem_trips = self
      .parser
      .drive_elements(inp, container, rh, &anchor, scans, &mut nums)?;

    // The element boundary is also the close position — this driver's whole contribution.
    // Classify it with the four-way probe so a terminal scanner stop is not misread as EOF and
    // grown into a spurious `Unclosed`.
    match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
      // The closer is at hand: commit the carried token by value — no re-scan, and
      // cache-independent (a blackhole `()` would drop a pushed-back closer).
      //
      // The probe is cache-first, so this verdict rests on a REAL pre-trip token — and that token
      // settles the POSITION question, not the COUNTER one. The construct closed ahead of any
      // boundary the element's lookahead went on to latch, so the scanner witness is genuinely not
      // about this exit and gating on it would fail a parse a wider window completes identically. A
      // descent trip is the other kind of fact: it happened *inside* the attempt that just
      // concluded, and a valid closer arriving after it does not unmake it — without the gate this
      // is a closed collection that silently spent a resource-limit stop. Descent only, therefore;
      // see `many::close_after_element`.
      CloseStatus::Close(ct) => {
        close_after_element(inp, elem_trips)?;
        container.on_close_delimiter(inp.commit_probed(ct))
      }
      // A wrong token where the closer belongs: unexpected-token, expected-close.
      CloseStatus::WrongToken(tok) => {
        // No closer: the loop's absence exit plus this verdict conclude *absence* from what the
        // last element attempt did, so surface a terminal stop it hit ahead of the close-miss
        // diagnostic rather than reporting a close that never happened.
        absence_after_element(inp, &latch, scans, elem_trips)?;
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
        absence_after_element(inp, &latch, scans, elem_trips)?;
        if let Some(open_span) = open_span {
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

    // The end-of-construct pass, and the construct's span with it: opener through closer, measured
    // after the closer commits. It is `RepeatedHandler::on_stop`'s own return value here, the way
    // it is under `Repeated::parse` — the delimited families used to rebuild the identical span
    // themselves and throw the handler's away.
    rh.on_stop(nums, inp, &anchor)
  }
}
