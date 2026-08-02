use crate::{
  Decision, Emitter, ParseContext, ParseInput, Window,
  container::Container as ContainerT,
  emitter::FullContainerEmitter,
  error::syntax::FullContainer,
  input::{CloseStatus, Cursor, InputRef},
  lexer::Lexer,
  span::Spanned,
};

use super::*;
use handler::*;

pub use delim::*;
pub use handler::{DelimiterHandler, SeparatorHandler};
pub use options::*;
pub use repeated::*;
pub use repeated_while::*;
pub use sep::*;
pub use sep_while::*;

#[macro_use]
mod macros;

mod delim;
mod handler;
mod repeated;
mod repeated_while;

mod options;
mod sep;
mod sep_while;

/// Pushes one parsed element and reports a refused push exactly once per construct.
///
/// The single chokepoint for both container-accounting laws; it replaced twelve separate
/// emission sites that had grown three different conventions between them.
///
/// * **`nums` counts elements the driver PARSED**, not elements the container stored. Count
///   bounds are a property of the input: a container that ran out of room must not turn a
///   satisfied `at_least` into a `TooFew`, nor silently swallow a violated `at_most` by
///   clamping the count it is judged on. Container inadequacy has its own diagnostic, below.
/// * **`FullContainer` is emitted once**, at the first refusal, latched by `full`. A container
///   that refuses one push refuses every later one, so the old per-dropped-element re-emission
///   produced a count that climbed past the capacity it named. `nums` in the payload is the
///   count at the refusal *including* the refused element, so the type's "found N … exceeds …
///   C" reading is true.
///
/// The latch is read only on the refusal arm, so the success path is exactly the pre-existing
/// `push` plus the increment.
#[inline(always)]
pub(super) fn push_element<'inp, 'closure, C, O, L, Ctx, Lang: ?Sized, Cmpl>(
  nums: &mut usize,
  full: &mut bool,
  container: &mut C,
  item: O,
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  anchor: &Cursor<'inp, 'closure, L>,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::Completeness,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>,
  C: ContainerT<O>,
{
  if container.push(item).is_err() && !*full {
    *full = true;
    let span = inp.span_since(anchor);
    let stored = container.len();
    inp.emitter().emit_full_container(FullContainer::of(
      span,
      stored + 1,
      container.max_capacity(),
    ))?;
  }
  *nums += 1;
  Ok(())
}

#[cfg(test)]
mod gate_census {
  //! GATE_CENSUS — the section-4 never-recoverable gate sites, locked by count.
  //!
  //! Every resilient emit-and-continue loop body in the try-driven collection families must gate on
  //! `Cmpl::is_incomplete_error` FIRST, and re-raise **both** terminal witnesses alongside it, so
  //! none of a frontier `Incomplete`, a tripped *scanner* limit, or a tripped *descent* budget from
  //! the element parser is spent as a diagnostic:
  //!
  //! * `inp.at_committed_boundary()` — the **scanner** stop. Reads the *committed cursor*, so it is
  //!   attempt-relative: a boundary a prior lookahead already latched does not mis-charge an
  //!   ordinary element failure short of it. Never the lex-offset `at_latched_boundary`, which a
  //!   prefilled cache would make false-positive on that same ordinary failure.
  //! * `inp.tripped_during_attempt(trips)` — the **descent** budget trip, which latches no boundary
  //!   (it has no position to latch, only a control stack) and so is invisible to the witness
  //!   above. It compares the session trip counter
  //!   [`InputRef::descend`](crate::InputRef::descend) bumps before the grammar's `From` runs
  //!   against this element's baseline, which is what makes the re-raise independent of the error
  //!   type: a [`RecursionLimitReached`](crate::error::RecursionLimitReached) that a discarding sink
  //!   erases on conversion — `()` does — still cannot be emitted-and-continued here.
  //!
  //! Both are positional/session facts read on the failure arm, so a successful element does zero
  //! terminal work and neither costs a trait bound: no `MaybeTerminal` appears in these families.
  //!
  //! **Both are attempt-relative, and the descent one is so only because of where its baseline is
  //! taken.** The counter behind it is a monotone session fact — "this parse tripped a budget, this
  //! many times" — and is never cleared, so a site that reads it absolutely re-raises every later
  //! element failure once anything in the parse has caught a trip and carried on: an ordinary
  //! syntax error in an unrelated construct then ends the collection instead of being emitted, and
  //! one deep expression early in a document suppresses every diagnostic after it. The baseline is
  //! therefore `inp.trip_snapshot()` taken **inside the element loop**, once per element — not
  //! hoisted out beside `latch_snapshot()`, whose absence witness genuinely is per driver attempt.
  //! Hoisting it would restore the defect one nesting level up: a trip inside an *inner* collection
  //! that an element swallows would be charged to the enclosing driver's next ordinary failure.
  //!
  //! One gate per swallow site; the census pins the total, the per-file placement, **both**
  //! re-raises *inside the same guard* — not merely somewhere in the same file — and the baseline's
  //! placement inside the loop that hosts the gate. So a new resilient loop cannot land ungated,
  //! nor land carrying only two of the three (extend the list, then gate it all three ways), nor
  //! land with the descent witness back in its session-absolute form.

  /// The needle that opens a never-recoverable gate. Counting it is counting gates: none of the
  /// four sources spells it anywhere but in the guard.
  const GATE: &str = "if Cmpl::is_incomplete_error(&";

  /// The two terminal re-raises the gate must carry, and the end of the guard they must sit in.
  const WITNESSES: [&str; 2] = ["inp.at_committed_boundary()", "inp.tripped_during_attempt("];
  const GUARD_END: &str = "=>";

  /// The descent witness's baseline, and the loop opener it must be taken after. Scanning between
  /// the two is what pins the baseline as per-element rather than per-collection — the difference
  /// between "this element tripped" and "this parse has tripped".
  const BASELINE: &str = "inp.trip_snapshot()";
  const HOSTING_LOOP: &str = "loop {";

  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_resilient_swallow_site_is_gated() {
    let sites = [
      ("many/repeated/mod.rs", include_str!("repeated/mod.rs")),
      ("many/delim/repeated.rs", include_str!("delim/repeated.rs")),
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      ("many/sep/delim/mod.rs", include_str!("sep/delim/mod.rs")),
    ];
    let mut gates = 0;
    for (name, src) in sites {
      let swallows = src.matches("emit_error(Spanned::new(span,").count();
      let gated = src.matches(GATE).count();
      assert_eq!(
        swallows, gated,
        "{name}: every emit-and-continue swallow needs exactly one incomplete gate"
      );

      // Exactly one baseline per gate, counted over code lines only so a prose mention of the
      // needle cannot stand in for the binding. Paired with the region scan below, this is what
      // rules out a second, stray read of the counter being used as a session-absolute test.
      assert_eq!(
        super::end_state_census::code_matches(src, BASELINE),
        gated,
        "{name}: exactly one `trip_snapshot()` baseline per never-recoverable gate"
      );

      // Both terminal re-raises, in the SAME guard as the incomplete gate and after it. Scanning
      // the *guard region* — from the gate to the `=>` that closes the arm's pattern — rather than
      // counting each needle over the whole file is what keeps this non-vacuous: three independent
      // per-file tallies are equally satisfied by three witnesses scattered across three different
      // arms, and this is not. It is also indentation- and wrap-independent, which matters because
      // the three-way guard no longer fits on one line at the crate's rustfmt width.
      let mut checked = 0;
      let mut from = 0;
      while let Some(at) = src[from..].find(GATE) {
        let gate_at = from + at;
        let after = &src[gate_at..];
        let guard = &after[..after
          .find(GUARD_END)
          .unwrap_or_else(|| panic!("{name}: an incomplete gate with no `=>` closing its arm"))];
        for witness in WITNESSES {
          assert!(
            guard.contains(witness),
            "{name}: the incomplete gate must re-raise both terminal witnesses in the same guard — \
             `{witness}` is missing from `{}`",
            guard.trim()
          );
        }

        // …and the descent witness's baseline is taken inside the loop that hosts the gate, so the
        // comparison is per ELEMENT. A baseline hoisted above the loop — the natural place, since
        // `latch_snapshot()`'s belongs there — is arithmetically identical to reading the monotone
        // counter absolutely for every element after the first, which is the defect this scan
        // exists to keep out. The `rfind` panics rather than passing when a gate is not inside a
        // loop at all, so this cannot be satisfied by a source that has stopped looking like a
        // driver.
        let loop_at = src[..gate_at].rfind(HOSTING_LOOP).unwrap_or_else(|| {
          panic!("{name}: a never-recoverable gate that is not inside a repetition loop")
        });
        assert_eq!(
          super::end_state_census::code_matches(&src[loop_at..gate_at], BASELINE),
          1,
          "{name}: the descent witness is attempt-relative — take the `trip_snapshot()` baseline \
           INSIDE the element loop, once per element, and compare it with `tripped_during_attempt`. \
           Hoisted out of the loop it reads the monotone session counter, and every element failure \
           after the parse's first trip re-raises, ordinary syntax errors included"
        );

        checked += 1;
        from = gate_at + GATE.len();
      }
      assert_eq!(
        checked, gated,
        "{name}: every gate occurrence must have been inspected"
      );
      gates += gated;
    }
    assert_eq!(
      gates, 4,
      "the try-driven families carry exactly four gated loop bodies"
    );
  }

  /// Every `*_while` driver's decision-window peek is terminal-aware: it uses the
  /// terminal-reporting `peek_with_emitter_terminal` (never the bare `peek_with_emitter`, whose
  /// short window a mid-window trip would hide) and surfaces the stop with `into_terminal`, so a
  /// resource-limit trip during the decision peek is never read as a clean end of list.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_while_decision_gate_is_terminal_aware() {
    let sites = [
      (
        "many/repeated_while/mod.rs",
        include_str!("repeated_while/mod.rs"),
      ),
      (
        "many/delim/repeated_while.rs",
        include_str!("delim/repeated_while.rs"),
      ),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
      (
        "many/sep_while/delim/mod.rs",
        include_str!("sep_while/delim/mod.rs"),
      ),
      ("fold/fold_while.rs", include_str!("../fold/fold_while.rs")),
      (
        "fold/rfold_while.rs",
        include_str!("../fold/rfold_while.rs"),
      ),
    ];
    for (name, src) in sites {
      assert!(
        src.contains("peek_with_emitter_terminal::<"),
        "{name}: the decision-window peek must use the terminal-reporting variant \
         (`peek_with_emitter_terminal`)"
      );
      assert!(
        !src.contains("peek_with_emitter::<"),
        "{name}: no bare decision-window peek may remain — a mid-window trip would hide in its \
         short window; use `peek_with_emitter_terminal`"
      );
      assert!(
        src.contains("into_terminal()"),
        "{name}: a decision-window terminal stop must be surfaced (`into_terminal`)"
      );
      // Exactly one eager terminal gate per decision peek: the flag is consulted immediately after
      // the peek and nowhere else. Per-arm re-scatter — a fallible or emitting call between the peek
      // and the check — reopens the terminal-precedence hole, so the gate count must track the peek
      // count (the turbofish form keeps prose mentions from skewing either tally).
      assert_eq!(
        src.matches("peek_with_emitter_terminal::<").count(),
        src.matches("if terminal").count(),
        "{name}: exactly one terminal gate per decision peek — the eager gate immediately after the \
         peek; per-arm re-scatter reopens the terminal-precedence hole"
      );
    }
  }

  /// Every non-delimited separated driver's separator-slot decision gate is terminal-aware: it
  /// probes with `try_expect_or_stop` (never the bare `try_expect`, whose `Ok(None)` folds a trip
  /// together with genuine absence and ends the list cleanly). The delimited separated drivers
  /// route their separator-slot `None` through `probe_close`, whose `Tripped` arm surfaces the stop
  /// instead — so they are exempt here and covered by that path.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_nondelim_separator_slot_surfaces_terminal() {
    let sites = [
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
    ];
    for (name, src) in sites {
      assert!(
        src.contains("try_expect_or_stop(|t"),
        "{name}: the separator-slot decision gate must probe with `try_expect_or_stop`, so a \
         terminal stop surfaces instead of folding into a clean end"
      );
    }
  }

  /// Every guard-bearing driver carries the absence witness, paired with its baseline. A driver's
  /// absence exits — the no-progress stall, the element-decline break, and a condition's
  /// `Action::Stop` — conclude "no more elements" from what the element saw, but an element's own
  /// lookahead latches a terminal scanner stop and still returns `Ok` with a short window, leaving
  /// the pre-trip tokens cached, so that conclusion can rest on a truncated view (the eager decision
  /// gate does not see it: the next window is served whole from that cache and carries no terminal
  /// flag). `latched_during_attempt` is the witness (presence-plus-change against a per-attempt
  /// `latch_snapshot` baseline, never a positional reading — a rollback over the latch moves the
  /// offset back behind a boundary that survives). In the delimited drivers the gate belongs *inside*
  /// the close probe's `WrongToken`/`Eof` arms, never ahead of the probe: the probe is cache-first, so
  /// a `Close` verdict on a real pre-trip token is a genuine close and must keep parsing — as is a
  /// reached bound. This pins both calls in every guard-bearing source so a new driver cannot land
  /// with an ungated absence exit or an unbaselined witness.
  #[test]
  fn every_absence_exit_carries_the_latch_witness() {
    for (name, src) in progress_guard_sites() {
      assert!(
        src.contains("latched_during_attempt("),
        "{name}: every absence exit (the no-progress stall, the element-decline break, a condition's \
         `Action::Stop`, and in the delimited drivers the close probe's `WrongToken`/`Eof` arms) must \
         witness a terminal stop this attempt latched (`latched_during_attempt`) before concluding \
         the construct ended"
      );
      assert!(
        src.contains("latch_snapshot()"),
        "{name}: the absence witness is attempt-relative — take the `latch_snapshot()` baseline once \
         per attempt, or a boundary an enclosing lookahead latched is mis-charged to this driver"
      );
    }
  }

  /// Every no-progress guard measures committed consumption (`span().end()`), never a cache-front
  /// cursor comparison. A lookahead fill moves the cursor across skipped trivia without consuming
  /// anything, which a cursor-keyed guard reads as false progress — a zero-width element behind a
  /// trivia gap then runs an extra cycle. This pins the metric across every guard-bearing driver so
  /// the class cannot regrow.
  #[test]
  fn every_progress_guard_reads_committed_progress() {
    for (name, src) in progress_guard_sites() {
      assert!(
        !src.contains(".as_inner() == ") && !src.contains(".as_inner() != "),
        "{name}: no-progress guards must compare committed consumption (`span().end()`), never a \
         cache-front cursor (`.as_inner()` equality) — a lookahead fill moves the cursor without \
         committing"
      );
    }
  }

  /// The guard-bearing driver sources: every collection and fold driver that runs a repetition loop
  /// and therefore needs both the committed-progress metric and the absence witness.
  pub(super) fn progress_guard_sites() -> [(&'static str, &'static str); 12] {
    [
      ("many/repeated/mod.rs", include_str!("repeated/mod.rs")),
      (
        "many/repeated_while/mod.rs",
        include_str!("repeated_while/mod.rs"),
      ),
      ("many/delim/repeated.rs", include_str!("delim/repeated.rs")),
      (
        "many/delim/repeated_while.rs",
        include_str!("delim/repeated_while.rs"),
      ),
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      ("many/sep/delim/mod.rs", include_str!("sep/delim/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
      (
        "many/sep_while/delim/mod.rs",
        include_str!("sep_while/delim/mod.rs"),
      ),
      ("fold/mod.rs", include_str!("../fold/mod.rs")),
      ("fold/rfold.rs", include_str!("../fold/rfold.rs")),
      ("fold/fold_while.rs", include_str!("../fold/fold_while.rs")),
      (
        "fold/rfold_while.rs",
        include_str!("../fold/rfold_while.rs"),
      ),
    ]
  }
}

#[cfg(test)]
mod end_state_census {
  //! END_STATE_CENSUS and its companions — the accounting laws of the repetition drivers,
  //! locked by count.
  //!
  //! Every exit that returns `Ok` from a repetition driver runs that driver's end-state pass
  //! exactly once before producing the success value; `Err` exits run it never. The defect this
  //! guards against is a second `Ok`-returning exit that jumps over the pass — and because the
  //! skipping exit is the one taken on WELL-FORMED input, the pass becomes dead code precisely
  //! where it matters and every test that feeds conforming input stays green and blind.

  /// Counts occurrences of `needle` on lines that are not whole-line comments, so prose
  /// mentions of a counted name do not skew a tally.
  ///
  /// Shared with [`gate_census`](super::gate_census), whose baseline tallies need the same
  /// comment-blindness for the same reason.
  pub(super) fn code_matches(src: &str, needle: &str) -> usize {
    src
      .lines()
      .filter(|line| !line.trim_start().starts_with("//"))
      .map(|line| line.matches(needle).count())
      .sum()
  }

  /// This module's own file with the census modules cut off the end — several needles below
  /// appear verbatim in census source and would otherwise be counted as production sites.
  fn many_mod_production() -> &'static str {
    let src = include_str!("mod.rs");
    src
      .split_once("#[cfg(test)]")
      .expect("the census marker must be present in its own source")
      .0
  }

  /// The eight collection drivers: source, the literal that spells the end-state pass, the
  /// number of times it must appear, the literal that spells a success exit, and its count.
  const SITES: &[(&str, &str, &str, usize, &str, usize)] = &[
    (
      "many/repeated/mod.rs",
      include_str!("repeated/mod.rs"),
      "rh.on_stop(",
      1,
      "rh.on_stop(",
      1,
    ),
    (
      "many/repeated_while/mod.rs",
      include_str!("repeated_while/mod.rs"),
      "rh.on_stop(",
      2,
      "rh.on_stop(",
      2,
    ),
    (
      "many/delim/repeated.rs",
      include_str!("delim/repeated.rs"),
      "on_stop(nums, inp, &span)",
      2,
      ".map(|_| mem::take(container))",
      2,
    ),
    (
      "many/delim/repeated_while.rs",
      include_str!("delim/repeated_while.rs"),
      "on_stop(nums, inp, &span)",
      3,
      ".map(|_| mem::take(container))",
      3,
    ),
    (
      "many/sep/parse/mod.rs",
      include_str!("sep/parse/mod.rs"),
      "self.handle_end(",
      3,
      "return self.handle_end(",
      3,
    ),
    (
      "many/sep/delim/mod.rs",
      include_str!("sep/delim/mod.rs"),
      "parser.handle_end(",
      2,
      "Ok(inp.span_since(&anchor))",
      2,
    ),
    (
      "many/sep_while/parse/mod.rs",
      include_str!("sep_while/parse/mod.rs"),
      "self.handle_end(",
      3,
      "return self.handle_end(",
      3,
    ),
    (
      "many/sep_while/delim/mod.rs",
      include_str!("sep_while/delim/mod.rs"),
      "parser.handle_end(",
      4,
      "Ok(inp.span_since(&anchor))",
      4,
    ),
  ];

  /// END_STATE_CENSUS. What this actually pins, row by row — stated precisely, because the
  /// equality is not equally informative everywhere:
  ///
  /// * **Every row**: the *pinned totals*. Deleting an end-state pass, or adding a driver exit
  ///   without one, moves a count away from its literal and fails.
  /// * `sep/delim` and `sep_while/delim` (the two rows where the defect actually lived): the
  ///   pass literal and the exit literal are **independent**, so pass-count == exit-count is a
  ///   real assertion. Reverting the mid-scan-closer arm moves both, independently.
  /// * `sep{,_while}/parse`: the exit literal is a superstring of the pass literal, so equality
  ///   asserts that every `handle_end` call is the `return` of it — a non-returning call fails.
  /// * `repeated`, `repeated_while`, `delim/repeated{,_while}`: the exit and the pass are the
  ///   same physical expression by construction (`rh.on_stop(…)` is the tail;
  ///   `return on_stop(…).map(|_| mem::take(container))` fuses both), so equality holds
  ///   structurally there and only the totals bite.
  ///
  /// The bare-`return Ok(` check below is what closes the gap the equality leaves: it catches a
  /// *new* success exit of any shape, in any of the eight sources, including the six where the
  /// equality cannot.
  #[test]
  fn every_driver_ok_exit_runs_the_end_state_pass() {
    for (name, src, pass, pass_n, exit, exit_n) in SITES {
      let passes = code_matches(src, pass);
      let exits = code_matches(src, exit);
      assert_eq!(
        passes, *pass_n,
        "{name}: expected {pass_n} end-state pass call(s) (`{pass}`), found {passes}"
      );
      assert_eq!(
        exits, *exit_n,
        "{name}: expected {exit_n} success exit(s) (`{exit}`), found {exits}"
      );
      assert_eq!(
        passes, exits,
        "{name}: every `Ok`-returning exit runs the end-state pass exactly once"
      );

      // A driver's only bare `return Ok(...)` form is the whole-construct span the two
      // delimited separated drivers return; the other six have none at all. Any other
      // `return Ok(` is a success exit that was added without an end-state pass.
      let bare = code_matches(src, "return Ok(");
      let anchored = code_matches(src, "return Ok(inp.span_since(&anchor))");
      assert_eq!(
        bare, anchored,
        "{name}: a success exit that returns anything but the whole-construct span \
         (`return Ok(inp.span_since(&anchor))`) has been added without its end-state pass"
      );
    }
  }

  /// LIMIT_PAYLOAD_CENSUS 2a — `FullContainer` has exactly one emission site in the whole
  /// `many` tree: the `push_element` chokepoint. Twelve separate sites is how three different
  /// counting conventions grew between them, and how the once-per-construct latch was lost.
  #[test]
  fn full_container_has_one_emission_site() {
    let mut total = code_matches(many_mod_production(), "FullContainer::of(");
    for (name, src, ..) in SITES {
      let n = code_matches(src, "FullContainer::of(");
      assert_eq!(
        n, 0,
        "{name}: drivers must push through `push_element`, never emit `FullContainer` directly"
      );
      total += n;
    }
    assert_eq!(
      total, 1,
      "`FullContainer::of(` is constructed once, inside `push_element`"
    );
  }

  /// LIMIT_PAYLOAD_CENSUS 2b — every `TooMany` names a count that actually exceeds its limit.
  ///
  /// Both `TooMany` and `FullContainer` render as "found {nums} … exceeds … {limit}", so a
  /// `nums` equal to the limit renders a self-contradicting sentence. Each emission site
  /// therefore passes `limit + 1`, which is also the smallest count every one of the eight
  /// drivers can produce at its own detection point — the only value that makes one history
  /// yield one payload whichever builder produced it.
  ///
  /// The per-line conjunction assumes the site fits on one line, which all eight do at the
  /// current rustfmt width; a future wrap would need the needle re-cut rather than dropped.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_too_many_payload_exceeds_its_limit() {
    let sites: [(&str, &str); 7] = [
      ("parser/with.rs", include_str!("../with.rs")),
      (
        "many/handler/maximum.rs",
        include_str!("handler/maximum.rs"),
      ),
      (
        "many/handler/bounded.rs",
        include_str!("handler/bounded.rs"),
      ),
      (
        "many/delim/repeated/at_most.rs",
        include_str!("delim/repeated/at_most.rs"),
      ),
      (
        "many/delim/repeated/bounded.rs",
        include_str!("delim/repeated/bounded.rs"),
      ),
      (
        "many/delim/repeated_while/at_most.rs",
        include_str!("delim/repeated_while/at_most.rs"),
      ),
      (
        "many/delim/repeated_while/bounded.rs",
        include_str!("delim/repeated_while/bounded.rs"),
      ),
    ];
    let mut total = 0;
    for (name, src) in sites {
      for line in src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains("TooMany::of("))
      {
        assert!(
          line.contains("+ 1,"),
          "{name}: `TooMany`'s count must exceed the limit it names (`limit + 1`): {}",
          line.trim()
        );
        total += 1;
      }
    }
    assert_eq!(
      total, 8,
      "the eight `TooMany` emission sites: two end checks in `with.rs`, the two mid-loop \
       `on_element` hooks, and the four delimited-repeated `on_stop` closures"
    );
  }

  /// MID_LOOP_PAIRING_CENSUS — `RepeatedHandler::on_stop` deliberately no longer re-checks the
  /// maximum, which is sound *only* because every consumer calls `on_element` for every
  /// element: a construct exceeds `max` iff some element saw the pre-count equal to it, so the
  /// mid-loop hook has already reported the violation exactly once. This pins both halves of
  /// that pairing, and pins that no third consumer can land without them.
  #[test]
  fn every_repeated_handler_consumer_calls_the_mid_loop_hook() {
    // No `Vec` here: this module is compiled under `--no-default-features` too.
    let mut consumers = [""; 2];
    let mut found = 0;
    for (name, src) in super::gate_census::progress_guard_sites() {
      if code_matches(src, "rh.on_stop(") == 0 {
        continue;
      }
      assert!(
        code_matches(src, "rh.on_element(") > 0,
        "{name}: a `RepeatedHandler` consumer must call `on_element` for every element — \
         `on_stop` does not re-check the maximum"
      );
      assert!(
        found < consumers.len(),
        "{name}: a third `RepeatedHandler` consumer has landed; extend this census and check \
         that it pairs the mid-loop hook with the end pass"
      );
      consumers[found] = name;
      found += 1;
    }
    assert_eq!(
      consumers,
      ["many/repeated/mod.rs", "many/repeated_while/mod.rs"],
      "exactly two sources consume `RepeatedHandler`; a third must pair the mid-loop hook \
       with the end pass before it lands"
    );
  }

  /// SEPARATOR_DELIVERY_CENSUS — every separator a driver consumes goes through
  /// `observe_separator`, whose clone lives inside the `OBSERVES_SEPARATORS` guard. Four arms
  /// per file is the whole of `handle_separator`'s `State` match: leading, happy path,
  /// duplicate, and (via the state it leaves behind) trailing. A driver calling `on_separator`
  /// directly would bypass the guard and clone a token for a container that ignores it.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_separator_arm_delivers_through_the_guard() {
    let sites = [
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
    ];
    for (name, src) in sites {
      assert_eq!(
        code_matches(src, "observe_separator("),
        4,
        "{name}: all four `handle_separator` arms deliver their separator"
      );
      assert_eq!(
        code_matches(src, "on_separator("),
        0,
        "{name}: drivers deliver through `observe_separator`, never `on_separator` directly — \
         the direct call clones unconditionally"
      );
    }
  }
}
