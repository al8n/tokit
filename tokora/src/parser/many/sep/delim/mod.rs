use core::mem;

use crate::{
  TryParseInput,
  container::Container as ContainerT,
  delimiter::Delimiter,
  emitter::{FromUnclosed, FullContainerEmitter, SeparatedEmitter, UnclosedEmitter},
  error::Unclosed,
  punct::Punctuator,
  span::Span as _,
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
      From<UnexpectedEot<L::Offset, Lang>> + FromUnclosed<'inp, L, Lang>,
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
    // The committed-consumption watermark at the moment a wrong opener is flagged, iff one is.
    // It is the suppression witness for the close-miss arm below — see the law comment there.
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

    let mut state: State<L::Token, L::Span> = State::Start;
    let parser = &mut self.parser;
    let mut num_elems = 0;
    let mut full = false;

    let mut cursor = inp.cursor().clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the element-absence exits below, taken AFTER the opener so the
    // opener's own scan is not charged to the element loop. One offset clone per collection.
    let latch = inp.latch_snapshot();
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
            // A closer committed mid-scan closes the construct — and a closed construct is
            // still a construct whose count bounds and separator policy must be judged. This
            // arm used to return straight out, so `handle_end` ran only when the loop broke,
            // i.e. only on MALFORMED input: on every well-formed, properly closed list the
            // entire end-state policy was dead code, while the while-sibling
            // (`sep_while/delim/mod.rs`) ran it on the identical path.
            //
            // No close-miss primary belongs here — the closer is present. No terminal-latch
            // gate belongs here either: the closer was READ AND COMMITTED, so this exit rests
            // on a real token rather than on an absence conclusion, and `latched_during_attempt`
            // guards only absence exits.
            parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;
            container.on_close_delimiter(tok);
            return Ok(inp.span_since(&anchor));
          } else {
            state = parser.handle_separator(state, inp, container, separator_state_handler, tok)?;
            cursor = inp.cursor().clone();
            committed = inp.span().end();
            continue;
          }
        }
      };

      match parser.f.try_parse_input(inp) {
        // The never-recoverable gate and its two terminal witnesses: a frontier `Incomplete`
        // (const-false under `Complete`), a terminal *scanner* stop, or a *descent* budget trip from
        // the element parser re-raises untouched — never spent as a diagnostic, since no further
        // input clears any of the three. The scanner witness reads the *committed cursor*
        // ([`at_committed_boundary`]), so a boundary a prior lookahead already latched does not
        // mis-charge an ordinary element failure short of it. The descent witness is the set-once
        // session cell ([`resource_trip`]), latched before the grammar's `From` runs, so a trip
        // re-raises for a delegating error type and for a discarding one alike — `()` included.
        // Failure-arm only — a successful element does zero terminal work; both witnesses are
        // positional/session facts, so no `MaybeTerminal` bound needed.
        Err(e)
          if Cmpl::is_incomplete_error(&e)
            || inp.at_committed_boundary()
            || inp.resource_trip() =>
        {
          return Err(e);
        }
        Err(e) => {
          let span = inp.span_since(&cursor);
          inp.emitter().emit_error(Spanned::new(span, e))?;
        }
        // The decline concludes *absence*. The gate for that conclusion sits in the epilogue's
        // close-miss arms below, where a real cached closer has already been ruled out.
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
            &mut full,
            container,
            continue_state_handler,
          )?;
        }
      }

      // A cycle that consumed nothing re-sees the same input and would retry forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill (a `try_expect_map` decline pushing a token back) moves that across skipped trivia
      // without consuming, reading a zero-width element as false progress. `<=`, not `==`: the
      // watermark cannot regress within a cycle, so anything not strictly ahead is a stall. `cursor`
      // stays the error-span anchor.
      let new_committed = inp.span().end();
      let new_cursor = inp.cursor().clone();
      if new_committed <= committed {
        break state;
      }
      committed = new_committed;
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
      // end-state pass. A cache-first verdict on a real pre-trip token, so the construct
      // genuinely closed and stays a success even if an element's lookahead latched a
      // terminal stop past that closer.
      CloseStatus::Close(ct) => close_carrier = Some(ct),
      // (b) a wrong token sits where the closer should: unexpected-token, expected-close.
      CloseStatus::WrongToken(tok) => {
        // No closer: the loop's absence exit plus this verdict conclude "no more elements", and an
        // element's own lookahead can latch a terminal scanner stop and still return `Ok` with a
        // short window, so that conclusion may rest on a truncated view. Surface the stop ahead of
        // the close-miss diagnostic; attempt-relative against the post-opener snapshot, so an
        // inherited boundary is not mis-charged here.
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
      // (a) genuine end of input with the opener still open: never closed — the ONE
      // `Unclosed` path.
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
    // the primary under a recovering emitter. Its returned span is a DIAGNOSTIC anchor, not
    // the construct span, and is deliberately discarded: a delimited driver returns the whole
    // construct, opener through closer, measured after the closer commits — the same
    // convention `delim/repeated{,_while}.rs` and `sep_while/delim/mod.rs` use.
    parser.handle_end(state, inp, &anchor, num_elems, end_state_handler)?;

    // Commit the carried closer by value (no re-scan) at the same program point as the
    // old deferred `try_expect` — after the end-state pass.
    if let Some(ct) = close_carrier {
      container.on_close_delimiter(inp.commit_probed(ct));
    }

    Ok(inp.span_since(&anchor))
  }
}
