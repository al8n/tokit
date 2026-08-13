use crate::{
  container::Container as ContainerT,
  emitter::{FromUnclosed, FullContainerEmitter, SeparatedEmitter, UnclosedEmitter},
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
      From<UnexpectedEot<L::Offset, Lang>> + FromUnclosed<'inp, L, Lang>,
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

    let mut state: State<L::Token, L::Span> = State::Start;
    let parser = &mut self.parser;
    let mut num_elems = 0;
    let mut full = false;

    let mut committed = inp.span().end();
    // The terminal-latch baseline for the absence exits below, taken AFTER the opener so the opener's
    // own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();
    // The scanner-trip baseline for the gates below — PER COLLECTION, taken beside the latch
    // and deliberately unlike the per-element descent one. It answers the latch's question
    // through a monotone session counter that no rollback reaches, which is what an element
    // catching a stop inside an `attempt` of its own leaves behind. See
    // `many::absence_after_element` for why the two granularities differ.
    let scans = inp.scanner_trip_snapshot();
    loop {
      // The descent witness's baseline, taken once per CYCLE — which is once per element, since a
      // cycle runs at most one, inside `handle_continue`. It sits above the separator-or-close scan
      // so that every exit of this cycle can read the same value; that widens the measured window
      // by that scan, the decision peek and `handle_separator`, none of which can descend, so the
      // reading is the one the element attempt would have given. See
      // `many::absence_after_element` for why it is per element and not per collection.
      let trips = inp.trip_snapshot();
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
            committed = inp.span().end();
            continue;
          }

          // A closer committed straight from this scan — a DIRECT closer, not a probe verdict, and
          // the one exit of this driver neither chokepoint covers. Not because it "rests on a real
          // token", but structurally: it is reached from the TOP of a cycle, before any element
          // attempt of its own, so only an *accepting* element can precede it (a stall stays inside
          // the `Action::Continue` arm below and returns from there) and there is no absence
          // conclusion to refuse. `sep/delim`'s mid-scan closer is exempt for the identical reason;
          // `GATE_CENSUS` counts both as `direct` rather than leaving the exemption implicit.
          parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;
          container.on_close_delimiter(tok);
          return Ok(inp.span_since(&anchor));
        }
        None => {
          // A short decision window can be a genuine end of input, but one truncated by a terminal
          // scanner stop is not: surface the committed end-of-input error before anything else.
          // `decide` is fallible and emitting, the continue-state work and the close probe would
          // otherwise preempt it, and a zero-width element leaves the trip-truncated front token at
          // the close-probe position where it reads as a spurious wrong closer. `end` is the
          // committed watermark, captured before the peek (the fill never advances it).
          let end = inp.span().end();
          let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;
          if terminal {
            return Err(UnexpectedEot::eot_of(end).into_terminal().into());
          }

          let front_span = match peeked.front() {
            None => {
              drop(peeked);

              // Front is empty: reclassify the close position with the four-way probe so
              // a terminal scanner stop is not misread as EOF. A real
              // non-sep/non-close token would have entered the cache and made `front()`
              // non-empty, so only genuine EOF or a terminal stop reaches here.
              //
              // PRIMARY — the close-status diagnostic first: under a fail-fast emitter
              // `handle_end`'s TooFew/trailing emission would otherwise short-circuit
              // before an unterminated list could surface as `Unclosed`.
              match inp.probe_close(|t| Delim::is_close(&t.data.kind()))? {
                // The closer is at hand: commit the carried token by value — no re-scan. Reached
                // from the TOP of a cycle, so the descent term is a constant `false` here, exactly
                // as at the direct closer above; the call stays so that "every probed closer passes
                // through `many::close_after_element`" needs no per-site exemption.
                CloseStatus::Close(ct) => {
                  close_after_element(inp, trips)?;
                  container.on_close_delimiter(inp.commit_probed(ct))
                }
                // (b) a wrong token was seen where the closer should be.
                CloseStatus::WrongToken(tok) => {
                  // An absence conclusion, so both witnesses are the chokepoint's — a constant
                  // `false` pair at this exit (the eager terminal gate above already refused a
                  // latched boundary, and this cycle's element has not run), routed through it so
                  // no close-miss arm of this driver spells a gate of its own.
                  absence_after_element(inp, &latch, scans, trips)?;
                  // One junk token, one report: a close expectation names a different token or
                  // nothing. Nothing committed since the wrong opener was flagged ⇒ the cache
                  // front still holds that same token (FIFO), so this probe is re-seeing it.
                  if !inp.front_report_live(tok.span_ref().end_ref()) {
                    inp
                      .emitter()
                      .emit_unexpected_token(Delim::unexpected_close_token(tok))?
                  }
                }
                // (a) end of input with the opener still open: the opener was never
                // closed.
                CloseStatus::Eof => {
                  // Same absence conclusion as the wrong-token arm above, so the same gate.
                  absence_after_element(inp, &latch, scans, trips)?;
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
              return Ok(inp.span_since(&anchor));
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
              // PRIMARY — classify the close position WITHOUT consuming (`probe_close`
              // leaves the scanned token cached) and emit the close-status diagnostic
              // before the end-state secondaries: under a fail-fast emitter
              // `handle_end`'s TooFew/trailing emission would otherwise short-circuit
              // first and an unterminated list would never surface as `Unclosed`. The
              // four-way probe also keeps a terminal scanner stop out of `Unclosed`.
              let mut close_carrier = None;
              match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
                // The closer is at hand: carry it out; committed by value below. The probe is
                // cache-first, so this verdict rests on a REAL pre-trip token: the list genuinely
                // closed and stays a success even if the element's lookahead latched a terminal stop
                // somewhere past that closer. Reached from the TOP of a cycle, so the descent term is
                // a constant `false` here too; the call stays for the same uniformity reason.
                CloseStatus::Close(ct) => {
                  close_after_element(inp, trips)?;
                  close_carrier = Some(ct)
                }
                // (b) a wrong token sits where the closer should be.
                CloseStatus::WrongToken(tok) => {
                  // No closer: the stop plus this verdict conclude *absence*, and the element's own
                  // lookahead can latch a terminal scanner stop and still return `Ok` with a short
                  // window, leaving the pre-trip tokens cached — the decision window above is then
                  // served whole from that cache, so it carries no terminal flag, and the same cached
                  // token reads as a spurious wrong closer here. Both witnesses are the chokepoint's;
                  // the scanner half is attempt-relative against the post-opener snapshot.
                  absence_after_element(inp, &latch, scans, trips)?;
                  // One junk token, one report: a close expectation names a different token or
                  // nothing. Nothing committed since the wrong opener was flagged ⇒ the cache
                  // front still holds that same token (FIFO), so this probe is re-seeing it.
                  if !inp.front_report_live(tok.span_ref().end_ref()) {
                    inp
                      .emitter()
                      .emit_unexpected_token(Delim::unexpected_close_token(tok))?
                  }
                }
                // (a) end of input with the opener still open: never closed.
                CloseStatus::Eof => {
                  // Same absence conclusion as the wrong-token arm above, so the same gate. No
                  // legitimate `Unclosed` is lost: a scan that reaches a live boundary stops there and
                  // reports the stop, so an `Eof` verdict cannot coexist with one.
                  absence_after_element(inp, &latch, scans, trips)?;
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
              return Ok(inp.span_since(&anchor));
            }
            Action::Continue => {
              // if the peeked token belongs to an element, check the current state
              state = parser.handle_continue(
                state,
                inp,
                &anchor,
                &front_span,
                &mut num_elems,
                &mut full,
                container,
                continue_state_handler,
              )?;

              // An `Action::Continue` cycle that consumed nothing re-sees the same lookahead and
              // would decide `Continue` forever. The progress metric is committed consumption
              // (`span().end()`), never the cache-front cursor — a lookahead fill moves that across
              // skipped trivia without consuming, reading a zero-width element as false progress.
              // `<=`, not `==`: the watermark cannot regress within a cycle, so anything not strictly
              // ahead is a stall.
              let new_committed = inp.span().end();
              if new_committed <= committed {
                let mut close_carrier = None;
                match inp.probe_close(|tok| Delim::is_close(&tok.data.kind()))? {
                  // The closer is at hand: carry it out; committed by value below. A cache-first
                  // verdict on a real pre-trip token, so the construct genuinely closed *at that
                  // position* and the scanner witness stays off this exit. The counter is the other
                  // fact and the closer settles nothing about it: the stalling attempt this cycle
                  // just ran may have caught a budget trip, which no later token unmakes. Descent
                  // only — see `many::close_after_element`. This is the driver's one real-closer
                  // exit that follows an element attempt of its own.
                  CloseStatus::Close(ct) => {
                    close_after_element(inp, trips)?;
                    close_carrier = Some(ct)
                  }
                  // (b) a wrong token sits where the closer should be.
                  CloseStatus::WrongToken(tok) => {
                    // No closer: the stall plus this verdict conclude *absence* from the element
                    // attempt this cycle just ran, and that attempt can hit either
                    // never-recoverable stop and still return `Ok` — a terminal scanner stop its
                    // own lookahead latched after the decision gate ran, or a descent budget trip
                    // it caught itself. Both are the chokepoint's; the scanner half is
                    // attempt-relative against the post-opener snapshot.
                    absence_after_element(inp, &latch, scans, trips)?;
                    // One junk token, one report: a close expectation names a different token
                    // or nothing. Nothing committed since the wrong opener was flagged ⇒ the
                    // cache front still holds that same token (FIFO), so this probe is
                    // re-seeing it.
                    if !inp.front_report_live(tok.span_ref().end_ref()) {
                      inp
                        .emitter()
                        .emit_unexpected_token(Delim::unexpected_close_token(tok))?
                    }
                  }
                  // (a) end of input with the opener still open: never closed.
                  CloseStatus::Eof => {
                    // Same absence conclusion as the wrong-token arm above, so the same gate. No
                    // legitimate `Unclosed` is lost: a scan that reaches a live boundary stops there
                    // and reports the stop, so an `Eof` verdict cannot coexist with one.
                    absence_after_element(inp, &latch, scans, trips)?;
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

                // Commit the carried closer by value (no re-scan) at the same program point as
                // the old deferred `try_expect` — after the end-state pass.
                if let Some(ct) = close_carrier {
                  container.on_close_delimiter(inp.commit_probed(ct));
                }
                return Ok(inp.span_since(&anchor));
              }
              committed = new_committed;
            }
          }
        }
      }
    }
  }
}
