//! The **partial-input** driver: fuzzes the `Partial` completeness typestate — the driver's seal,
//! the frontier holdback, and the `Incomplete` channel — via chunked equivalence, the same law the
//! conformance kit's `run_partial` checks, but over fuzzed (error-bearing) streams and randomized
//! cuts rather than an exhaustive sweep.
//!
//! Three shapes are checked against the single-shot complete parse (the oracle):
//!
//! - **In-place two-phase.** One `Partial` input over the whole source: a non-final drain yields
//!   every token strictly before the buffer end and ends `Incomplete` (the frontier withholds the
//!   last token / trailing error); the driver then **seals** it and drains on, reproducing the rest.
//!   The concatenation must equal the complete parse.
//! - **Seal survives rollback.** A sealed stream, put through every rollback shape the crate has —
//!   `attempt`, `try_attempt`, explicit and drop-path transaction rollback, a stacked savepoint, a
//!   session point — is *still sealed* afterwards, and drains to genuine end of input rather than
//!   asking for a refill that can never come. This is the law that forecloses the mirror bug (see
//!   below).
//! - **Chunked prefixes.** For random cut points `k`, a non-final drain of `src[..k]` yields exactly
//!   the complete tokens whose span ends strictly before `k` and always ends `Incomplete`.
//!
//! # What the harness can and cannot express about finality
//!
//! It cannot express "a speculative branch flips finality and rolls back", and that is the point:
//! finality is a **world fact**, so it is settable only through the owning input
//! ([`Input::seal`](crate::input::Input::seal), monotone), which an `InputRef` mutably borrows for
//! its whole life. There is no method on a handle, at any depth, in any speculative branch, that
//! could flip it — so there is no such operation to fuzz, and the leak it used to cause is
//! *unrepresentable* rather than merely untriggered. What remains fuzzable is the mirror half — a
//! rollback must not un-*end* a stream — and `seal_survives_rollback` is exactly that oracle.
//!
//! This is error-agnostic (it compares committed token streams, which skip lexer errors
//! identically in both modes), so — unlike the consume driver — the fuzzed streams here **do**
//! contain lexer-error bytes, exercising the error-skipping and dedup paths under truncation.
//!
//! # The third shape: a limit trip AT the cut (terminal beats incomplete)
//!
//! The two shapes above fuzz what the frontier rules *withhold*. The third fuzzes what they must
//! **not** withhold. Under a token limiter ([`ScriptState::with_limit`]) the `(limit + 1)`-th token
//! is reported as a lexer error carrying that token's span — so choosing the cut `k` at that span's
//! end puts a **terminal** condition exactly on the non-final frontier, the alignment an attacker
//! picks. The oracle is the [law](crate::input#terminal-beats-incomplete-and-they-never-substitute):
//!
//! - a prefix that **contains** the trip terminates on the trip — tokens up to it, the limit
//!   diagnostic emitted — and never on the `Incomplete` channel, *including* when the tripping token
//!   ends exactly at the cut;
//! - a prefix that does **not** contain the trip still ends `Incomplete`, so the narrowing did not
//!   turn the holdback off.
//!
//! Every trip-bearing case forces `k` to the trip's span end alongside its random cuts, so the
//! boundary alignment is covered on every such seed rather than waited for.
//!
//! # Fuzz coverage (`OP_SURFACE_CENSUS`)
//!
//! This adds **oracles**, not operations: they drive [`Op::Next`] and [`Op::Seal`], both already in
//! the alphabet and already marked here. The alphabet's size is unchanged, so `EXPECTED_OP_COUNT`
//! does not move (grep `OP_SURFACE_CENSUS`).

use std::vec::Vec;

use super::{
  fixtures::{
    CountEmitter, FuzzCtx, FuzzError, FuzzKind, ScriptLexer, ScriptState, cache, initial_state,
    is_err, kind_of,
  },
  ops::{Coverage, Op},
  rng::Rng,
};
use crate::{Token, input::Partial, span::SimpleSpan};

/// A committed token, reduced to the facts the oracle compares.
type Tok = (FuzzKind, SimpleSpan);

/// Drains a fresh **complete** input over `src`, returning the committed token stream — the oracle.
fn complete_stream(src: &[u8], budget: usize) -> Vec<Tok> {
  let context = crate::input::InputContext::new(CountEmitter::new(), cache());
  let state = initial_state(src);
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, ()>::with_state_and_context(
      src, state, context,
    );
  let mut ir = input.as_ref();
  let mut out = Vec::new();
  loop {
    assert!(
      out.len() <= budget,
      "complete drain exceeded budget (non-terminating?)"
    );
    match ir.next().expect("complete + non-fatal emitter never errs") {
      Some(sp) => {
        let (span, tok) = sp.into_components();
        out.push((tok.kind(), span));
      }
      None => return out,
    }
  }
}

/// Drains a fresh **partial** input over `src` at `is_final`, returning the committed token stream
/// and whether the drain ended `Incomplete` (rather than at genuine end of input).
fn partial_stream(src: &[u8], is_final: bool, budget: usize) -> (Vec<Tok>, bool) {
  let context = crate::input::InputContext::new(CountEmitter::new(), cache());
  let state = initial_state(src);
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, (), Partial>::with_state_and_context(
      src, state, context,
    );
  if is_final {
    input.seal();
  }
  let mut ir = input.as_ref();
  let mut out = Vec::new();
  loop {
    assert!(
      out.len() <= budget,
      "partial drain exceeded budget (non-terminating?)"
    );
    match ir.next() {
      Ok(Some(sp)) => {
        let (span, tok) = sp.into_components();
        out.push((tok.kind(), span));
      }
      Ok(None) => return (out, false),
      Err(e) => {
        assert_eq!(
          e,
          FuzzError::Incomplete,
          "partial drain surfaced a non-Incomplete error"
        );
        return (out, true);
      }
    }
  }
}

/// Drains one `Partial` input over the whole `src` in two phases across a **live seal**: non-final
/// until `Incomplete`, then sealed and drained to genuine end of input. Returns the concatenated
/// committed stream.
///
/// This is the driver shape the seal exists for — the last chunk lands carrying *no new bytes*, so
/// the buffer is already whole and only the world fact changed. Note where the seal sits: **between
/// handles**. `Input::seal` takes `&mut Input`, and a handle borrows the input for its whole life,
/// so the borrow checker admits the flip exactly where a driver can honestly make it — with no
/// parser, guard, or speculative branch in flight.
fn two_phase_stream(src: &[u8], budget: usize) -> Vec<Tok> {
  let context = crate::input::InputContext::new(CountEmitter::new(), cache());
  let state = initial_state(src);
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, (), Partial>::with_state_and_context(
      src, state, context,
    );
  let mut out = Vec::new();

  // Phase 1: non-final. Drains everything before the frontier, ending Incomplete (or genuine EOF
  // for an empty stream — either way, stop).
  {
    let mut ir = input.as_ref();
    loop {
      assert!(out.len() <= budget, "two-phase drain exceeded budget");
      match ir.next() {
        Ok(Some(sp)) => {
          let (span, tok) = sp.into_components();
          out.push((tok.kind(), span));
        }
        Ok(None) => break,
        Err(e) => {
          assert_eq!(
            e,
            FuzzError::Incomplete,
            "phase-1 drain surfaced a non-Incomplete error"
          );
          break;
        }
      }
    }
  }

  // The socket closed: the driver seals. Monotone, and only reachable here.
  input.seal();

  // Phase 2: a fresh handle over the same input — same cache, same cursor, same lexer state, and
  // now necessarily the same emitter, because the input owns it — sealed. It drains the withheld
  // remainder to genuine end of input.
  {
    let mut ir = input.as_ref();
    assert!(ir.is_final(), "the seal is visible to the next handle");
    loop {
      assert!(out.len() <= budget, "two-phase drain exceeded budget");
      match ir.next() {
        Ok(Some(sp)) => {
          let (span, tok) = sp.into_components();
          out.push((tok.kind(), span));
        }
        Ok(None) => break,
        Err(_) => panic!("a final input must never surface Incomplete"),
      }
    }
  }
  out
}

/// The **seal-survives-rollback** oracle: a stream the driver has already ended cannot be un-ended
/// by a parser rolling back.
///
/// This is the law that rules out the mirror bug — "checkpoint the finality flag and restore it" —
/// which closes the frontier leak by opening a *hang*: a rollback across a legitimate seal would
/// revert `is_final` to `false`, and the parser would sit asking for bytes that will never arrive.
///
/// So: seal (the last chunk landed), then throw the whole speculative surface at the input and roll
/// every bit of it back. The input must still be final afterwards, and the drain must reach genuine
/// end of input — never `Incomplete`.
fn seal_survives_rollback(src: &[u8], budget: usize, complete: &[Tok]) {
  let context = crate::input::InputContext::new(CountEmitter::new(), cache());
  let state = initial_state(src);
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, (), Partial>::with_state_and_context(
      src, state, context,
    );
  // The world fact: the stream has ENDED.
  input.seal();

  let mut ir = input.as_ref();
  assert!(ir.is_final(), "the input is sealed");

  // Speculate across every rollback shape the crate has, and abandon all of it.
  let declined: Option<()> = ir.attempt(|i| {
    let _ = i.next();
    let _ = i.next();
    None
  });
  assert!(declined.is_none());

  let errored: Result<(), ()> = ir.try_attempt(|i| {
    let _ = i.next();
    Err(())
  });
  assert!(errored.is_err());

  {
    let mut txn = ir.begin();
    let _ = txn.next();
    txn.rollback();
  }
  {
    // Undecided drop: the rollback-on-drop policy.
    let mut txn = ir.begin();
    let _ = txn.next();
  }
  {
    let mut txn = ir.begin_stacked();
    let _ = txn.next();
    let sp = txn.savepoint();
    let _ = txn.next();
    txn.rollback_to(sp);
    txn.rollback();
  }
  let point = ir.begin_point();
  let _ = ir.next();
  ir.rollback_point(point);

  // The stream is STILL ended. Nothing a parser did could have un-ended it.
  assert!(
    ir.is_final(),
    "A ROLLBACK UN-ENDED A SEALED STREAM: the parser will now wait forever for bytes that will \
     never arrive (this is what checkpointing the finality flag would buy)"
  );

  // And the drain proves it observably: genuine end of input, never Incomplete.
  let mut out = Vec::new();
  loop {
    assert!(out.len() <= budget, "sealed drain exceeded budget");
    match ir.next() {
      Ok(Some(sp)) => {
        let (span, tok) = sp.into_components();
        out.push((tok.kind(), span));
      }
      Ok(None) => break,
      Err(e) => panic!(
        "a SEALED stream surfaced {e:?} after a rollback — the refill it asks for can never come"
      ),
    }
  }
  assert_eq!(
    out, complete,
    "the sealed drain after a full rollback diverged from the complete parse"
  );
}

// ── The limit oracle: a terminal trip is not an incomplete frontier ──────────────────────────────

/// The shadow model of a limited run over `src`: the index of the **tripping byte** — the
/// `(limit + 1)`-th token byte — if the source reaches the limit at all.
///
/// A pure function of the bytes and the limit, exactly as the rest of the model is: error bytes
/// ([`is_err`]) are not billed, every other byte is one token.
fn trip_index(src: &[u8], limit: usize) -> Option<usize> {
  let mut tokens = 0usize;
  for (i, &b) in src.iter().enumerate() {
    if is_err(b) {
      continue;
    }
    tokens += 1;
    if tokens > limit {
      return Some(i);
    }
  }
  None
}

/// What a limited drain observed: the committed tokens, how it terminated (`true` = on the
/// `Incomplete` channel), and how many diagnostics reached the emitter.
struct Limited {
  tokens: Vec<Tok>,
  incomplete: bool,
  emitted: u64,
}

/// Drains a fresh input over `src` behind a `limit`-token limiter, in `Partial` mode at `is_final`.
fn limited_stream(src: &[u8], limit: usize, is_final: bool, budget: usize) -> Limited {
  let context = crate::input::InputContext::new(CountEmitter::new(), cache());
  let mut input =
    crate::input::Input::<'_, ScriptLexer<'_>, FuzzCtx<'_>, (), Partial>::with_state_and_context(
      src,
      ScriptState::with_limit(limit),
      context,
    );
  if is_final {
    input.seal();
  }
  let (tokens, incomplete) = {
    let mut ir = input.as_ref();
    let mut tokens = Vec::new();
    let incomplete = loop {
      assert!(
        tokens.len() <= budget,
        "limited drain exceeded budget (non-terminating?)"
      );
      match ir.next() {
        Ok(Some(sp)) => {
          let (span, tok) = sp.into_components();
          tokens.push((tok.kind(), span));
        }
        Ok(None) => break false,
        Err(e) => {
          assert_eq!(
            e,
            FuzzError::Incomplete,
            "a limited drain surfaced a non-Incomplete error"
          );
          break true;
        }
      }
    };
    (tokens, incomplete)
  };
  Limited {
    tokens,
    incomplete,
    emitted: input.emitter().count(),
  }
}

/// Checks the terminal-dominance law over `src[..k]` under `limit`: a prefix that contains the trip
/// must stop **on the trip** — never on the `Incomplete` channel — even when the tripping token ends
/// exactly at the cut; a prefix that does not must still end `Incomplete`.
fn check_limited_prefix(src: &[u8], limit: usize, k: usize, budget: usize) {
  let prefix = &src[..k];
  let run = limited_stream(prefix, limit, /* is_final */ false, budget);
  let trip = trip_index(prefix, limit);

  match trip {
    Some(t) => {
      // Every token before the trip ends at `t` or earlier, hence strictly before the cut: the
      // holdback cannot touch them, and the trip stops the scan at exactly `t`.
      let expected: Vec<Tok> = prefix[..t]
        .iter()
        .enumerate()
        .filter(|&(_, &b)| !is_err(b))
        .map(|(i, &b)| (kind_of(b), SimpleSpan::new(i, i + 1)))
        .collect();
      assert_eq!(
        run.tokens, expected,
        "limited prefix (k={k}, limit={limit}) diverged before the trip at {t}"
      );
      assert!(
        !run.incomplete,
        "TERMINAL BEATS INCOMPLETE: the trip at {t} ends the prefix (k={k}, limit={limit}) — a \
         tripping token ending exactly at the cut ({}) must not be withheld as Incomplete",
        t + 1 == k
      );
      // The limit diagnostic, plus one per lexer-error byte crossed before the trip. If the trip
      // were withheld, the limit diagnostic would be missing from this count.
      let errs_before = prefix[..t].iter().filter(|&&b| is_err(b)).count() as u64;
      assert_eq!(
        run.emitted,
        errs_before + 1,
        "the limit diagnostic IS emitted at the trip (k={k}, limit={limit}, trip={t})"
      );
    }
    None => assert!(
      run.incomplete,
      "no trip in the prefix (k={k}, limit={limit}): the holdback still applies, so a non-final \
       drain must end Incomplete"
    ),
  }
}

/// Runs one partial case: builds a fuzzed (error-bearing) stream and checks both chunked-equivalence
/// shapes against the complete parse, then the limit oracle at the chunk boundary.
pub(crate) fn run(src: &[u8], seed: u64, cov: &mut Coverage) {
  cov.mark(Op::Seal);
  let budget = src.len() + 4;
  let complete = complete_stream(src, budget);

  // 1. A final partial drain of the whole source reproduces the complete parse.
  let (final_tokens, final_incomplete) = partial_stream(src, true, budget);
  assert!(
    !final_incomplete,
    "a FINAL drain must reach genuine end of input, not Incomplete"
  );
  assert_eq!(
    final_tokens, complete,
    "final partial drain diverged from the complete parse"
  );

  // 2. In-place two-phase: non-final, then a live driver seal, reproduces the complete parse.
  assert_eq!(
    two_phase_stream(src, budget),
    complete,
    "in-place seal two-phase diverged from the complete parse"
  );

  // 2b. A sealed stream cannot be un-ended by a parser rolling back (the no-hang law).
  seal_survives_rollback(src, budget, &complete);

  // 3. Chunked prefixes at random cut points: a non-final prefix yields exactly the complete
  //    tokens ending strictly before the cut, and always ends Incomplete.
  let len = src.len();
  let mut rng = Rng::new(seed ^ 0x5015_5EED);
  let cuts = (len + 1).min(6);
  for _ in 0..cuts {
    let k = rng.below(len + 1);
    let (prefix_tokens, incomplete) = partial_stream(&src[..k], false, budget);
    let expected: Vec<Tok> = complete
      .iter()
      .filter(|(_, span)| span_end(span) < k)
      .copied()
      .collect();
    assert_eq!(
      prefix_tokens, expected,
      "chunked prefix (k={k}) diverged from the complete prefix"
    );
    assert!(
      incomplete,
      "a non-final prefix drain (k={k}) must end Incomplete"
    );
  }

  // 4. The limit oracle: put a TERMINAL condition on the frontier and check it outranks it. The
  //    limit is drawn to land inside the stream, and the cut is forced to the tripping token's span
  //    end — the exact chunk-boundary alignment that used to mask the trip as Incomplete — as well
  //    as sampled randomly.
  let tokens_total = src.iter().filter(|&&b| !is_err(b)).count();
  if tokens_total > 0 {
    let limit = rng.below(tokens_total);
    if let Some(t) = trip_index(src, limit) {
      cov.mark(Op::Next);
      // The alignment the finding turns on: the tripping token ends exactly at the cut.
      check_limited_prefix(src, limit, t + 1, budget);
      // Its neighbours: the trip strictly inside the prefix, and the prefix stopping just short of
      // it (no trip yet — the holdback must still apply).
      check_limited_prefix(src, limit, len, budget);
      check_limited_prefix(src, limit, t, budget);
      // Random cuts, so the boundary case is not the only shape the limiter ever sees.
      for _ in 0..cuts {
        check_limited_prefix(src, limit, rng.below(len + 1), budget);
      }
    }
  }
}

/// The end offset of a span.
fn span_end(span: &SimpleSpan) -> usize {
  *span.end_ref()
}

// ─────────────────────────────────────────────────────────────────────────────────────
// The Partial event gate (deferred)
// ─────────────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod event_gate {
  /// GATED OFF — the chunked-event-equivalence property (the event-channel twin of this
  /// driver's token-stream oracle): a chunked partial parse and the single-shot parse
  /// must emit identical event streams.
  ///
  /// **The two prerequisites this was parked on are now MET, and a different one replaced
  /// them.** Re-derived rather than re-parked, because a stale gate string is how a deferral
  /// becomes permanent:
  ///
  /// - *"the scanner family becoming `Cmpl`-generic"* — **done.** `sync_to`, `sync_through`,
  ///   `skip_while` and `consume_cached` are all generic over the completeness parameter.
  ///   (The earlier check for this looked for the bound spelled `Cmpl: Completeness` and found
  ///   none; the scanners spell it `Cmpl: SurfaceIncomplete`. A census that greps one spelling
  ///   cannot see another, and its zero was not evidence of absence.)
  /// - *"the cache-transparency matrix growing a Partial column"* — **done.**
  ///   `try_expect_partial_matrix`, `skip_while_partial_matrix`, `sync_to_partial_matrix` and
  ///   `sync_through_partial_matrix` are the column.
  ///
  /// What blocks it now postdates both. `PartialSession::parse` requires
  /// `Ctx::Emitter: ValueKeyedEmitter`, and a recording `Sink` deliberately is not one — so the
  /// **session** route to a chunked CST is uncompilable by design, not by omission (see the
  /// release notes' *Streaming CST is not in this release*). The supported shape is a
  /// hand-rolled refill loop with a **fresh sink per attempt**, and writing this oracle against
  /// that shape is the remaining work: it needs a per-attempt sink harness that the suite does
  /// not have yet.
  ///
  /// The law it would pin is the one repeatedly circled and stated by no automated check:
  /// a chunked replay equals the one-shot parse.
  #[test]
  #[ignore = "gated: needs a fresh-sink-per-attempt harness. The two originally-named \
              prerequisites (Cmpl-generic scanners, matrix Partial column) are BOTH met; the \
              session route is type-excluded by `PartialSession::parse`'s ValueKeyedEmitter \
              bound, so this must drive a hand-rolled refill loop instead"]
  fn chunked_event_equivalence_awaits_the_partial_gate() {
    unimplemented!(
      "drive a hand-rolled refill loop with a FRESH Sink per attempt against a single-shot \
       parse of the same source, and assert identical materialized trees"
    );
  }
}
