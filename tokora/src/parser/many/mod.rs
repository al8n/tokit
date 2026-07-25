use crate::{
  Decision, Emitter, ParseContext, ParseInput, Window,
  input::{CloseStatus, InputRef},
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

#[cfg(test)]
mod gate_census {
  //! GATE_CENSUS — the section-4 never-recoverable gate sites, locked by count.
  //!
  //! Every resilient emit-and-continue loop body in the try-driven collection families
  //! must gate on `Cmpl::is_incomplete_error` FIRST, and re-raise a terminal scanner stop
  //! (`inp.at_committed_boundary()`) alongside it, so neither a frontier `Incomplete` nor a
  //! tripped limit from the element parser is spent as a diagnostic. The terminal witness reads
  //! the *committed cursor* (attempt-relative: a boundary a prior lookahead already latched does
  //! not mis-charge an ordinary element failure short of it), and rides the failure arm so a
  //! successful element does zero terminal work. One gate per swallow site; the census pins the
  //! total, the per-file placement, and the terminal re-raise so a new resilient loop cannot land
  //! ungated or without the terminal dual (extend the list, then gate it both ways).

  #[test]
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
      let gated = src.matches("if Cmpl::is_incomplete_error(&").count();
      assert_eq!(
        swallows, gated,
        "{name}: every emit-and-continue swallow needs exactly one incomplete gate"
      );
      // The terminal dual: every incomplete gate carries the terminal re-raise in the same
      // guard, so a tripped limit re-raises instead of being emitted-and-continued. The witness
      // is the attempt-relative, committed-cursor form (never the lex-offset `at_latched_boundary`,
      // which a prefilled cache would make false-positive on an ordinary element failure).
      let terminal = src.matches("|| inp.at_committed_boundary()").count();
      assert_eq!(
        gated, terminal,
        "{name}: every incomplete gate must re-raise a terminal stop too \
         (`|| inp.at_committed_boundary()`)"
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
  fn progress_guard_sites() -> [(&'static str, &'static str); 12] {
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
