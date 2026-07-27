//! PRATT_CENSUS — the source census of the two pratt drivers' loop conditions.
//!
//! # Why a census, and why here
//!
//! Where a pratt expression ends is the RHS channel's answer: the typed driver's
//! [`ParsePrattRHS`](super::ParsePrattRHS) report, and the token driver's
//! `try_expect_map_or_stop` verdict. Both loops used to *pre-gate* that answer on a position
//! test — "has the scanner's frontier reached the end of the buffer?" — which is a different
//! question, and a legal lookahead is enough to make the two disagree: a peek through the end
//! of input moves the frontier while unconsumed operators still sit in front of the consumer,
//! and the loop then ends the expression early with an `Ok` and no diagnostic on any channel.
//!
//! The behavioural pins (`tests/parser_pratt.rs`'s lookahead-equivalence pin, and the
//! preloaded-cache and trailing-trivia pins in `tests/pratt_floor.rs`) fix the *histories*
//! that were measured. They do not fix the *mechanism*: a pre-gate rewritten against a
//! cache-aware predicate — `is_exhausted` rather than `is_eoi` — passes every one of them
//! while putting a position test back in charge of an end-of-expression decision, and the
//! next history that separates the two is found the same way the last one was. This census
//! pins the absence of the mechanism instead, which is the thing that is actually invariant.
//!
//! Termination no longer needs a gate: the typed driver carries the house progress guard
//! (committed consumption, `span().end()`) at the report boundary — before the recursion and
//! before any fold, either of which can move input the report did not — and the token
//! driver's progress is structural, since acceptance there *is* the commit of exactly one
//! token of nonzero width and no token fold is handed the input at all.
//!
//! The second census covers the **sentinel idiom**: "no operator here" spelled as a
//! below-minimum postfix, on the belief that the floor will always reject it. It will not.
//! A sentinel is a real report, subject to the floor like any other, so whether it binds
//! depends on grammar-wide power discipline that no signature can check — and over an
//! unsigned `Power`, whose minimum representable value *is* the default floor, no sentinel
//! exists to write. [`PrattRHS::End`](super::PrattRHS) is the answer, and it is not a power.
//!
//! Adding that variant does not retire the idiom on its own: it forces every exhaustive
//! *match* to answer the end case, and the idiom lives entirely in *construction* sites, so
//! the fixtures kept compiling untouched. This census is what actually retires it, in the
//! grammars this crate ships as worked examples.
//!
//! Counting is line-based and skips `//`-prefixed lines, so prose may name these predicates
//! freely; only code counts. `grep PRATT_CENSUS` finds every anchor.

/// The two driver sources under census: the typed driver's RHS loop and the token driver's.
const DRIVERS: &[(&str, &str)] = &[
  ("parser/pratt/expr.rs", include_str!("expr.rs")),
  (
    "input/input_ref/pratt.rs",
    include_str!("../../input/input_ref/pratt.rs"),
  ),
];

/// The input-layer source that *defines* the position predicates, censused so the rail below
/// cannot pass because a predicate was renamed out from under it.
const INPUT_REF_MOD: &str = include_str!("../../input/input_ref/mod.rs");

/// Counts occurrences of `needle` on the non-comment lines of `hay`.
///
/// Line-based on purpose: `//`-prefixed lines (`//`, `///`, `//!`) are documentation and may
/// name the censused predicates freely. The censused files keep code mentions of these names
/// off comment-trailing positions, so this is exact for them.
fn count(hay: &str, needle: &str) -> usize {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .map(|line| line.matches(needle).count())
    .sum()
}

/// PRATT_CENSUS — neither driver consults a position predicate. Both RHS loops are
/// unconditional; the expression ends when the RHS channel says so, and termination is
/// carried by the progress guard (typed) and by one committed token per accepted report
/// (token).
#[test]
fn pratt_census_no_driver_gates_on_a_position_predicate() {
  for (name, src) in DRIVERS {
    for predicate in ["is_eoi", "is_exhausted"] {
      let got = count(src, predicate);
      assert!(
        got == 0,
        "PRATT_CENSUS drift: `{name}` names `{predicate}` {got} time(s) in code. A pratt loop \
         must not decide where an expression ends from position state — that is a different \
         question from the RHS channel's, and any legal lookahead separates the two, \
         truncating the expression with an `Ok` and no diagnostic. Let the RHS channel answer \
         and let the progress guard terminate (grep PRATT_CENSUS)."
      );
    }
  }
}

/// PRATT_CENSUS — the census above is only meaningful while the predicates it names are the
/// ones that exist. Renaming `is_eoi`/`is_exhausted` would silence the rail without changing
/// anything it guards against, so both keep exactly one definition, in the input layer.
#[test]
fn pratt_census_the_censused_predicates_still_exist() {
  for predicate in ["fn is_eoi(", "fn is_exhausted("] {
    let got = count(INPUT_REF_MOD, predicate);
    assert!(
      got == 1,
      "PRATT_CENSUS drift: `input/input_ref/mod.rs` defines `{predicate}` {got} time(s), \
       expected 1. The driver census greps for these names; a rename makes it pass \
       vacuously. Rename the census's needles in the same commit (grep PRATT_CENSUS)."
    );
  }
}

/// The pratt grammars this crate ships as worked examples. Each has migrated its "no
/// operator here" answer to [`PrattRHS::End`](super::PrattRHS).
///
/// Deliberately **not** listed: `tests/pratt_floor.rs` and `tests/pratt_progress_guard.rs`,
/// which keep the retired idiom on purpose — they pin what happens to a downstream grammar
/// that never migrates. Also not listed: the eleven other files whose "sentinel" is an
/// unrelated list-end token, which have nothing to do with this idiom and would make the
/// census fail on correct code.
const SENTINEL_FREE_GRAMMARS: &[(&str, &str)] = &[
  (
    "tests/parser_pratt.rs",
    include_str!("../../../tests/parser_pratt.rs"),
  ),
  (
    "tests/parser_misc.rs",
    include_str!("../../../tests/parser_misc.rs"),
  ),
  (
    "tests/parser_node.rs",
    include_str!("../../../tests/parser_node.rs"),
  ),
  (
    "examples/c_expression.rs",
    include_str!("../../../examples/c_expression.rs"),
  ),
  (
    "examples/c_expression_cst.rs",
    include_str!("../../../examples/c_expression_cst.rs"),
  ),
  (
    "examples/calculator_cst.rs",
    include_str!("../../../examples/calculator_cst.rs"),
  ),
];

/// PRATT_CENSUS — the sentinel idiom stays retired in the shipped grammars.
///
/// Counted over the whole file, prose included: the word is the idiom's only name, and a
/// grammar that reintroduces it will say so in a comment before it says so in code. The
/// compiler cannot reach this — `PrattRHS::End` breaks matches, and the idiom is built from
/// constructions — so the census is the enforcement.
#[test]
fn pratt_census_the_sentinel_idiom_stays_retired() {
  for (name, src) in SENTINEL_FREE_GRAMMARS {
    let got = src.to_lowercase().matches("sentinel").count();
    assert!(
      got == 0,
      "PRATT_CENSUS drift: `{name}` names `sentinel` {got} time(s). \"No operator here\" is \
       `PrattRHS::End`, not a below-minimum postfix: a sentinel is a real report and the \
       floor admits it whenever a recursion or a lowered `min_precedence` reaches its power, \
       at which point it binds as a phantom operator and steals the token the surrounding \
       grammar was waiting for (grep PRATT_CENSUS)."
    );
  }
}

/// PRATT_CENSUS — the census above is only meaningful while the files it reads are the pratt
/// grammars. A file that stops using `PrattRHS` at all would satisfy it for the wrong reason.
#[test]
fn pratt_census_the_censused_grammars_are_still_pratt_grammars() {
  for (name, src) in SENTINEL_FREE_GRAMMARS {
    assert!(
      count(src, "PrattRHS::End") > 0,
      "PRATT_CENSUS drift: `{name}` no longer answers `PrattRHS::End` anywhere. Either it \
       stopped being a pratt grammar — in which case drop it from this census — or its end \
       channel was respelled (grep PRATT_CENSUS)."
    );
  }
}
