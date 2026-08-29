use crate::{
  container::Container as ContainerT, delimiter::Delimiter, emitter::UnclosedEmitter,
  error::Unclosed,
};

use super::*;

mod at_least;
mod at_most;
mod bounded;
mod unbounded;

impl<'inp, L, P, O, Condition, Ctx, Delim, W, Lang: ?Sized>
  DelimitedBy<&mut RepeatedWhile<P, Condition, O, W, L, Ctx, Lang>, Delim>
{
  /// `RepeatedWhile`'s element loop, run **under a delimiter**.
  ///
  /// This driver contributes an opener, a close position and nothing else. The elements are
  /// `RepeatedWhile::drive_elements`'s — one terminal-checked decision window per cycle, one
  /// `many::admit_element`, one stall test, all of them the plain driver's — and the count and
  /// stop hooks are the plain driver's [`RepeatedHandler`] rather than a shape of this file's own.
  /// What is left here is what the delimiter genuinely adds
  /// ([#259](https://github.com/al8n/tokora/issues/259)'s stage-2 verdict, and the one difference
  /// it found essential): a delimited list's element boundary is also a **close** position, so it
  /// is classified ahead of every decision and again after the loop, and a real closer passes
  /// through `many::close_after_element`, which appears in no undelimited driver.
  fn parse_repeated<'c, Container, RH>(
    &mut self,
    inp: &mut InputRef<'inp, 'c, L, Ctx, Lang>,
    container: &mut Container,
    // The count verdict and the end-of-construct pass, as ONE handler — see `delim/repeated.rs`'s
    // twin of this parameter.
    rh: &RH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Delim: Delimiter<'inp, L, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    W: Window,
    Ctx::Emitter: FullContainerEmitter<'inp, L, Lang> + UnclosedEmitter<'inp, L, Lang>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: ContainerT<O> + DelimiterHandler<'inp, L>,
    RH: RepeatedHandler<'inp, 'c, O, L, Ctx, Lang>,
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
    }

    let mut nums = 0;
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the opener's
    // own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();

    // The elements, driven by the plain loop with THIS driver's element boundary in it: the close
    // position, classified ahead of every decision so a closer at hand short-circuits before the
    // stop condition is consulted, exactly as the consuming `try_expect` did.
    let exit = self.parser.drive_elements(
      inp,
      container,
      rh,
      &anchor,
      &mut nums,
      |inp: &mut InputRef<'inp, 'c, L, Ctx, Lang>, container: &mut Container, trips| {
        // Probe the close position WITHOUT consuming, so a terminal scanner stop is not misread
        // as EOF and grown into a spurious `Unclosed`.
        match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
          CloseStatus::Close(ct) => {
            // A mid-scan closer, reached from the TOP of a cycle: only an *accepting* element can
            // precede it, since a stall breaks into the epilogue below, so the descent term is a
            // constant `false` here — this cycle's baseline was taken a line above and nothing since
            // it can trip. The call stays anyway, so that "every probed closer passes through
            // `many::close_after_element`" needs no per-site exemption to be true; `GATE_CENSUS`
            // scans it per verdict.
            close_after_element(inp, trips)?;
            // Commit the carried closer by value (no re-scan); the end handler runs once, below.
            container.on_close_delimiter(inp.commit_probed(ct));
            Ok(Probed::Closed(()))
          }
          // A terminal scanner stop: its own diagnostic already explains the halt —
          // propagate it and add no `Unclosed`.
          CloseStatus::Tripped => Err(
            UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
              .into_terminal()
              .into(),
          ),
          // The closer is absent (a wrong token or genuine EOF) — carry the verdict into this
          // cycle's stop exit, which is the only place it is read.
          close => Ok(Probed::Open(close)),
        }
      },
    )?;

    match exit {
      // The boundary closed the construct and has already gated, committed and told the container.
      WhileExit::Closed(()) => {}
      WhileExit::Stopped(trips, close) => {
        // No closer plus a stop conclude *absence*: "no more elements, and nothing closed them".
        // The element's own lookahead can latch a terminal scanner stop and still return `Ok` with
        // a short window, leaving the pre-trip tokens cached — this decision window is then served
        // whole from that cache, so it carries no terminal flag for the loop's own gate to see, and
        // the same cached token reads as a spurious wrong closer. The verdict in hand is
        // necessarily a close miss (a `Close` committed in the boundary above, a `Tripped`
        // propagated there), so no genuine close is gated. Both witnesses are the chokepoint's; the
        // scanner half is attempt-relative against the post-opener snapshot, and the descent half is
        // a constant `false` at this exit for the same structural reason the mid-scan closer gives —
        // the stop is reached before this cycle's element runs.
        absence_after_element(inp, &latch, scans, trips)?;
        // PRIMARY — the close-miss diagnostic first: under a fail-fast emitter this
        // short-circuits, so `Unclosed` (not the secondary bounds) surfaces.
        match close {
          // (b) a wrong token sits where the closer should: unexpected-token, expected-close
          // (the existing vocabulary).
          CloseStatus::WrongToken(tok) => {
            // One junk token, one report: a close expectation names a different token or nothing.
            // Nothing committed since the wrong opener was flagged ⇒ the cache front still holds
            // that same token (FIFO), so this probe is re-seeing it.
            if !inp.front_report_live(tok.span_ref().end_ref()) {
              inp
                .emitter()
                .emit_unexpected_token(Delim::unexpected_close_token(tok))?
            }
          }
          // (a) end of input with the opener still open: the opener was never closed.
          _ => {
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
        }
      }
      // No progress was made — treat as end of elements. The close position is probed AFRESH
      // rather than carried out of the stalling cycle: a zero-width element makes no progress that
      // would re-lex the frontier, so the verdict this cycle opened with may name a token the
      // element has since put back. The four-way probe keeps a terminal scanner stop out of the
      // `Unclosed` path.
      WhileExit::Stalled(elem_trips) => {
        match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
          // The closer is at hand: commit the carried token by value — no re-scan. A cache-first
          // verdict on a real pre-trip token, so the construct genuinely closed *at that position*
          // and the scanner witness stays off this exit — a boundary latched past the closer is not
          // about a construct that ended before it. The counter is the other fact, and the closer
          // settles nothing about it: the stalling attempt may have caught a budget trip, which no
          // later token unmakes. Descent only — see `many::close_after_element`.
          CloseStatus::Close(ct) => {
            close_after_element(inp, elem_trips)?;
            container.on_close_delimiter(inp.commit_probed(ct))
          }
          CloseStatus::WrongToken(tok) => {
            // No closer: the stall plus this verdict conclude *absence* from what the last element
            // attempt did, and that attempt can hit either never-recoverable stop and still return
            // `Ok` — a terminal scanner stop its own lookahead latched after the decision gate ran,
            // or a descent trip it caught itself. Surface either ahead of the close-miss diagnostic;
            // the scanner half is attempt-relative against the post-opener snapshot.
            absence_after_element(inp, &latch, scans, elem_trips)?;
            // One junk token, one report: emit unless the emitter already holds a live report
            // naming this very front token. See FRONT_REPORTED at the top of this body.
            if !inp.front_report_live(tok.span_ref().end_ref()) {
              inp
                .emitter()
                .emit_unexpected_token(Delim::unexpected_close_token(tok))?
            }
          }
          CloseStatus::Eof => {
            // Same absence conclusion as the wrong-token arm above, so the same gate. No legitimate
            // `Unclosed` is lost: a scan that reaches a live boundary stops there and reports the
            // stop, so an `Eof` verdict cannot coexist with one.
            absence_after_element(inp, &latch, scans, elem_trips)?;
            // EOI — no tokens left, no close delimiter: the opener was never closed.
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
          CloseStatus::Tripped => {
            return Err(
              UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                .into_terminal()
                .into(),
            );
          }
        }
      }
    }

    // SECONDARY — the end-of-construct pass, run after the primary close-miss diagnostic, and the
    // construct's span with it: opener through closer, measured after the closer commits. This
    // driver used to return from the stop exit WITHOUT running it, silently dropping the
    // `TooFew`/bounds diagnostic under a recovering emitter.
    rh.on_stop(nums, inp, &anchor)
  }
}
