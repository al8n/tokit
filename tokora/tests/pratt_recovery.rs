#![cfg(all(
  feature = "std",
  feature = "rowan",
  feature = "combinators",
  feature = "logos_0_16"
))]

//! The pins for `examples/expr_recovery.rs` — tokora's error-recovery posture, held to the
//! rust-analyzer contract it claims to reproduce.
//!
//! # Why these live here and not in the example
//!
//! The test lane never runs an example's own `#[cfg(test)] mod tests`: `cargo test
//! --all-features` *compiles* every example and runs none of them. The only step that passes
//! `--examples` lives in the pages workflow, and until the fix in this same change it ran under
//! `--no-default-features --features std,logos`, where every example in the tree is filtered
//! out by its `required-features` and cargo reports `no targets matched; this is a no-op` — so
//! a pin written inside the example was a pin nothing checked anywhere. That step now names
//! `rowan` and `combinators` and does run each example's `test_example` cell, but it belongs to
//! a deploy workflow rather than to the test job, so the pins stay where `cargo test
//! --all-features` reaches them unconditionally.
//!
//! So the example is pulled in as a module and pinned from a real integration test. That also
//! makes the example's own `main` — the assertions it prints — run for free, under the
//! `test_example` cell every example in this tree declares.
//!
//! # What each pin is for
//!
//! Every cell below stands for one claim tokora makes about recovery, and each is written so
//! that a *shape* has to change for it to pass, not a count:
//!
//! | claim | cell |
//! |---|---|
//! | a zero-width `Operand` report is legal, and the fold completes over it | `zero_width_error_operand_completes_the_binary_node` |
//! | the RHS loop continues past a recovered operand | `the_rhs_loop_continues_after_a_recovered_operand` |
//! | several diagnostics survive one parse | `two_diagnostics_survive_one_parse` |
//! | an `Err` out of a channel keeps what was emitted before it (`Fault::Keep`) | `an_err_out_of_a_channel_keeps_the_diagnostics_made_before_it` |
//! | `inplace_recover` resumes at the handback offset | `inplace_recovery_resumes_at_the_handback_offset` |
//! | `End` hands the deciding read back, so a recovery set can peek | `the_recovery_set_leaves_the_closing_paren_to_its_group` |
//! | the terminal `UnexpectedEoLhs`/`EoRhs` fire on grammar bugs, not on bad input | `no_malformed_input_reaches_the_driver_contract_arm` |
//! | a literal the lexer accepted and `i64` cannot hold recovers rather than panicking | `a_literal_too_large_for_i64_recovers_instead_of_panicking` |
//! | a recursion trip **ends** the parse — recorded, round-tripping, and not a panic | `a_recursion_trip_ends_the_parse_without_panicking` |
//! | that budget counts depth, not input length | `the_recursion_budget_counts_depth_and_not_length` |

#[path = "../examples/expr_recovery.rs"]
mod expr_recovery;

use expr_recovery::{Diag, Expr, Parsed, SyntaxKind, dump, parse};

// ── Helpers ─────────────────────────────────────────────────────────────────────

/// `(start, end, diagnostic)` for every recorded diagnostic, in emission order.
fn diags(p: &Parsed) -> Vec<(usize, usize, Diag)> {
  p.diagnostics
    .iter()
    .map(|(span, d)| (*span.start_ref(), *span.end_ref(), *d))
    .collect()
}

/// The byte ranges of every `ErrorExpr` node in the tree, in document order.
fn error_node_ranges(p: &Parsed) -> Vec<(usize, usize)> {
  p.tree
    .descendants()
    .filter(|n| n.kind() == SyntaxKind::ErrorExpr)
    .map(|n| {
      let r = n.text_range();
      (usize::from(r.start()), usize::from(r.end()))
    })
    .collect()
}

/// The one invariant every recovering parse owes: the tree is the source.
///
/// Compares the tree's text against the **caller's own `&str`**, never against anything
/// re-derived from the tree — an invariance check that compares a value to itself proves
/// nothing.
#[track_caller]
fn assert_round_trips(src: &str, p: &Parsed) {
  assert_eq!(p.tree.text().to_string(), src, "round trip for {src:?}");
}

// ── The five named inputs from the issue ────────────────────────────────────────

/// `1 + ` — the missing right-hand side.
///
/// The recursive operand parse reports an operand that consumed **nothing**, the driver folds
/// it, and the binary node completes. That is legal because `PrattLHS::Operand` is not held to
/// "consume what you report" — only `Prefix` is stall-checked, since only `Prefix` makes the
/// driver re-enter at the same position.
///
/// The error node is a real, zero-width node: it has a place in the tree, so an IDE can point
/// at it, and it has no text, so the round trip is unaffected.
#[test]
fn zero_width_error_operand_completes_the_binary_node() {
  let src = "1 + ";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(
    p.ast,
    Expr::Bin(
      expr_recovery::BinOp::Add,
      Box::new(Expr::Num(1)),
      Box::new(Expr::Error)
    )
  );
  assert_eq!(diags(&p), [(4, 4, Diag::ExpectedExpression)]);
  // Zero-width, and *inside* the binary node rather than beside it.
  assert_eq!(error_node_ranges(&p), [(4, 4)]);
  assert_eq!(
    dump(&p.tree),
    "\
Root
  BinExpr
    Num \"1\"
    Whitespace \" \"
    Plus \"+\"
    Whitespace \" \"
    ErrorExpr
"
  );
}

/// `1 + + 2` — the loop continues.
///
/// The inner operand parse recovers, the first `+` folds over the hole, and then the **outer
/// loop takes another cycle** and folds the second `+`. r-a's rule, and the one rustc does not
/// follow.
///
/// The pin is the AST *shape*, not a count: a driver that stopped after the first fold would
/// produce `(1 + <error>)` and leave `+ 2` for the trailing-input handler — same number of
/// folds attempted, different tree and a second diagnostic. Both halves are asserted.
#[test]
fn the_rhs_loop_continues_after_a_recovered_operand() {
  let src = "1 + + 2";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(p.ast.to_string(), "((1 + <error>) + 2)");
  // Exactly one problem: the second `+` was *folded*, not reported.
  assert_eq!(diags(&p), [(4, 4, Diag::ExpectedExpression)]);
  assert_eq!(error_node_ranges(&p), [(4, 4)]);
  // Two BinExpr nodes — one per fold. A loop that ran once would have one.
  assert_eq!(
    p.tree
      .descendants()
      .filter(|n| n.kind() == SyntaxKind::BinExpr)
      .count(),
    2
  );
}

/// `(1+)` — the recovery set.
///
/// `)` is in the set, so recovery reports and consumes **nothing**: the group's own `try_expect`
/// still finds its closer and the `ParenExpr` node completes normally. That works because the
/// RHS channel's `End` hands the deciding read back untouched, so a recovery decision can be
/// taken on a peek.
///
/// The AST here is identical to `1 + `'s — parens are a CST-only fact in this grammar — which
/// is what makes the tree assertion load bearing rather than decorative.
#[test]
fn the_recovery_set_leaves_the_closing_paren_to_its_group() {
  let src = "(1+)";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(p.ast.to_string(), "(1 + <error>)");
  assert_eq!(diags(&p), [(3, 3, Diag::ExpectedExpression)]);
  assert_eq!(error_node_ranges(&p), [(3, 3)]);
  assert_eq!(
    dump(&p.tree),
    "\
Root
  ParenExpr
    LParen \"(\"
    BinExpr
      Num \"1\"
      Plus \"+\"
      ErrorExpr
    RParen \")\"
"
  );
  // Same AST as the unparenthesised shape, different tree. Stated as an assertion so a change
  // that quietly starts recording parens in the AST fails here rather than passing silently.
  assert_eq!(p.ast, parse("1 + ").ast);
  assert_ne!(dump(&p.tree), dump(&parse("1 + ").tree));
}

/// Two diagnostics in one parse — and the fold depth proves the loop kept going between them.
///
/// `+ 1 +` recovers at the head (offset 0) and again at the tail (offset 5), and folds two
/// operators in between. The diagnostic list is pinned in **emission order with its offsets**,
/// so a parse that reported the same problem twice at the same place, or reported one and
/// stopped, fails.
#[test]
fn two_diagnostics_survive_one_parse() {
  let src = "+ 1 +";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(
    diags(&p),
    [
      (0, 0, Diag::ExpectedExpression),
      (5, 5, Diag::ExpectedExpression),
    ]
  );
  // `((<error> + 1) + <error>)`: two folds, so the loop took a second cycle after the first
  // recovery. One fold would be `(<error> + 1)` with `+` left over.
  assert_eq!(p.ast.to_string(), "((<error> + 1) + <error>)");
  assert_eq!(error_node_ranges(&p), [(0, 0), (5, 5)]);
}

// ── The other half of the posture: propagate, then recover outside ──────────────

/// `Fault::Keep` commits: an `Err` out of a channel keeps every emission the expression already
/// made.
///
/// `(1 + ` makes two: the recovered operand at 5, then the unclosed group at 5. The second one
/// is followed by a `return Err(..)` out of the LHS channel, which crosses the driver as
/// `Fault::Keep`. Both must still be there.
///
/// The pin is deliberately the *pair*: if the driver rolled the expression back on this exit,
/// the first diagnostic would be the one to disappear, and a cell that only counted "at least
/// one" would not notice.
#[test]
fn an_err_out_of_a_channel_keeps_the_diagnostics_made_before_it() {
  let src = "(1 + ";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(
    diags(&p),
    [
      (5, 5, Diag::ExpectedExpression),
      (5, 5, Diag::ExpectedRParen),
    ]
  );
  // The expression's value is gone — the recovery point produced a hole — but its *emissions*
  // and its CST events are not: the binary node it had already folded is still in the tree.
  assert_eq!(p.ast, Expr::Error);
  assert_eq!(
    p.tree
      .descendants()
      .filter(|n| n.kind() == SyntaxKind::BinExpr)
      .count(),
    1
  );
}

/// `inplace_recover` resumes at the handback offset — not at the attempt origin, and not at
/// the end of input.
///
/// `(1 + 2 3` fails at byte 7 (`3` where `)` was owed). The recovery handler swallows whatever
/// is left into one error node, so the node's **start** is a direct read-out of where the
/// handler was resumed. Three assertions, because the two wrong answers are the interesting
/// ones: 0 is `recover`'s behaviour (restore to the attempt origin), 8 would mean the handler
/// ran after the input had already been drained.
#[test]
fn inplace_recovery_resumes_at_the_handback_offset() {
  let src = "(1 + 2 3";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(diags(&p), [(7, 7, Diag::ExpectedRParen)]);

  let ranges = error_node_ranges(&p);
  assert_eq!(ranges, [(7, 8)], "the recovery node covers the tail `3`");
  let (resumed_at, _) = ranges[0];
  assert_ne!(
    resumed_at, 0,
    "`recover` would restart at the attempt origin"
  );
  assert_ne!(
    resumed_at,
    src.len(),
    "the tail must not already be drained"
  );
  // The handback offset and the diagnostic's offset are the same position, by construction.
  assert_eq!(resumed_at, diags(&p)[0].0);
}

/// r-a's `err_and_bump`: a token in nobody's follow set becomes the error node's body.
///
/// `@` is lexable and unparseable. Leaving it in place would hand the next cycle the same
/// input, so recovery swallows exactly one token — and the error node is then one token wide
/// rather than zero, which is the observable difference between the two branches.
#[test]
fn a_token_no_production_wants_is_bumped_into_the_error_node() {
  let src = "1 + @";
  let p = parse(src);

  assert_round_trips(src, &p);
  assert_eq!(p.ast.to_string(), "(1 + <error>)");
  assert_eq!(diags(&p), [(4, 4, Diag::ExpectedExpression)]);
  // One token wide, and it contains the token.
  assert_eq!(error_node_ranges(&p), [(4, 5)]);
  assert_eq!(
    dump(&p.tree),
    "\
Root
  BinExpr
    Num \"1\"
    Whitespace \" \"
    Plus \"+\"
    Whitespace \" \"
    ErrorExpr
      At \"@\"
"
  );
}

/// The two branches of `err_recover` are distinguishable, and the recovery set is what
/// distinguishes them.
///
/// Same diagnostic, same AST hole, different node width — which is exactly the fact a cell
/// that only counted diagnostics could not see.
#[test]
fn the_recovery_set_decides_the_error_nodes_width() {
  // In the set: report, consume nothing, zero-width node.
  assert_eq!(error_node_ranges(&parse("1 + ")), [(4, 4)]);
  assert_eq!(error_node_ranges(&parse("(1+)")), [(3, 3)]);
  // Not in the set: report, bump one token, one-token node.
  assert_eq!(error_node_ranges(&parse("1 + @")), [(4, 5)]);
  assert_eq!(error_node_ranges(&parse("@")), [(0, 1)]);
  // Both diagnostics say the same thing, so the width is the only signal.
  assert_eq!(diags(&parse("1 + "))[0].2, Diag::ExpectedExpression);
  assert_eq!(diags(&parse("1 + @"))[0].2, Diag::ExpectedExpression);
}

/// A literal the lexer accepts and `i64` cannot hold recovers; it does not panic.
///
/// This is the only place in the grammar where input the **lexer approved** can still be
/// unrepresentable: `[0-9]+` bounds the shape of a digit run and says nothing about its
/// magnitude, so the value conversion is fallible on a perfectly well-formed token. An
/// `expect()` there would make "this parser has no failing input" false for an input a user can
/// type — the one claim the whole file exists to demonstrate.
///
/// The pin is the boundary, one digit wide: `i64::MAX` is an ordinary operand and `i64::MAX + 1`
/// is a hole. Nothing about the two inputs differs except the value, so a fix that recovered
/// from *all* literals would fail the first half and a regression to the panic fails the second.
#[test]
fn a_literal_too_large_for_i64_recovers_instead_of_panicking() {
  // i64::MAX — the largest operand that is not a recovery.
  let src = "1 + 9223372036854775807";
  let p = parse(src);
  assert_round_trips(src, &p);
  assert_eq!(
    p.ast,
    Expr::Bin(
      expr_recovery::BinOp::Add,
      Box::new(Expr::Num(1)),
      Box::new(Expr::Num(i64::MAX))
    )
  );
  assert_eq!(diags(&p), []);
  assert_eq!(error_node_ranges(&p), []);

  // i64::MAX + 1 — same shape, same length, one digit different, and now a hole.
  let src = "1 + 9223372036854775808";
  let p = parse(src);

  // The interesting half: the digits are real text, so the round trip has to survive a
  // recovery that happened *after* the token was consumed.
  assert_round_trips(src, &p);
  assert_eq!(
    p.ast,
    Expr::Bin(
      expr_recovery::BinOp::Add,
      Box::new(Expr::Num(1)),
      Box::new(Expr::Error)
    )
  );
  // Spanned over the literal, not at a point — the input chose this width.
  assert_eq!(diags(&p), [(4, 23, Diag::NumberOutOfRange)]);
  assert_eq!(error_node_ranges(&p), [(4, 23)]);
  // A third error-node shape: the node has a body, and the body is the offending literal.
  assert_eq!(
    dump(&p.tree),
    "\
Root
  BinExpr
    Num \"1\"
    Whitespace \" \"
    Plus \"+\"
    Whitespace \" \"
    ErrorExpr
      Num \"9223372036854775808\"
"
  );

  // In head position, and nested in a group, so the recovery is not an artifact of the RHS
  // path. The group still closes and the parens still reach the tree.
  let src = "(99999999999999999999999) * 2";
  let p = parse(src);
  assert_round_trips(src, &p);
  assert_eq!(p.ast.to_string(), "(<error> * 2)");
  assert_eq!(diags(&p), [(1, 24, Diag::NumberOutOfRange)]);
  assert_eq!(error_node_ranges(&p), [(1, 24)]);
}

// ── Contract validation over a corpus ───────────────────────────────────────────

/// Every malformed input the grammar can be handed: parses to completion, round-trips, and
/// never reaches the driver's terminal contract errors.
///
/// This is the claim from the study that the recovery posture depends on most:
/// `UnexpectedEoLhs` / `UnexpectedEoRhs` fire on a **grammar bug** — a report the driver acted
/// on that consumed nothing where it required progress — and never on malformed *input*. If one
/// of them fired here it would arrive as `Err` out of the driver and reach `Diag::DriverContract`;
/// in a debug build the driver's own `debug_assert` would name the violated rule first. Either
/// way this cell is the tripwire.
///
/// **Do not add a deeply nested input to this corpus.** A recursion trip is terminal and
/// *expected* from ordinary input, so it would fire this assertion for the wrong reason. It has
/// its own diagnostic (`Diag::DepthExceeded`, kept separate from `DriverContract` for exactly
/// this) and its own cell, `a_recursion_trip_ends_the_parse_without_panicking`. Everything here
/// stays comfortably inside the budget.
///
/// The corpus is also the termination proof: recovery that consumed nothing at a position the
/// next cycle re-reads is the non-terminating shape, and every one of these inputs returns.
#[test]
fn no_malformed_input_reaches_the_driver_contract_arm() {
  const CORPUS: &[&str] = &[
    "",
    " ",
    "@",
    "@@@",
    "+",
    "++",
    "+++",
    "-",
    "--",
    "*",
    "/",
    ")",
    "(",
    "()",
    "(()",
    "())",
    "1",
    "1 +",
    "1 + ",
    "1 ++ 2",
    "1 + + 2",
    "1 + + + 2",
    "+ 1 +",
    "* 1 *",
    "1 2",
    "1 2 3",
    "(1+)",
    "(1 + ",
    "(1 + 2 3",
    "(1 + 2))",
    "((1)",
    "-(",
    "- -",
    "- - -",
    "1 * / 2",
    "1 / * 2",
    "@ + @",
    "1 + @ + 2",
    "(@)",
    "(+)",
    "((((((((((1))))))))))",
    "1 + (2 * (3 - ",
    // Lexically valid, arithmetically unrepresentable — the boundary and both sides of it.
    "9223372036854775807",
    "9223372036854775808",
    "99999999999999999999999999999999",
    "1 + 99999999999999999999",
    "(99999999999999999999",
    "00000000000000000000000000000001",
  ];

  for src in CORPUS {
    let p = parse(src);
    assert_round_trips(src, &p);
    for (start, end, d) in diags(&p) {
      assert_ne!(
        d,
        Diag::DriverContract,
        "{src:?} reached the driver contract arm at {start}..{end}"
      );
      assert!(start <= end, "{src:?}: malformed diagnostic span");
      assert!(end <= src.len(), "{src:?}: diagnostic span past the source");
    }
    // A malformed input must still produce a diagnostic; a *valid* one must not. Empty and
    // whitespace-only input is *not* clean here: an expression was owed and none arrived, so
    // the head-position recovery fires exactly as it does for `+`.
    //
    // The last two are the guard against a range check written as a *length* check: 19 digits
    // is `i64::MAX` and 32 leading zeros is the value 1, so both are clean, while the 20- and
    // 32-digit entries above are not. Only the conversion itself separates them.
    let clean = matches!(
      *src,
      "1" | "((((((((((1))))))))))" | "9223372036854775807" | "00000000000000000000000000000001"
    );
    assert_eq!(
      p.diagnostics.is_empty(),
      clean,
      "{src:?} diagnosed {} problem(s)",
      p.diagnostics.len()
    );
  }
}

/// A clean parse pays nothing for the machinery: no diagnostics, no error nodes, and the
/// precedence the grammar declares.
///
/// The complement of the corpus cell. Without it, a "recovery" that fired on every input would
/// pass everything above.
#[test]
fn a_valid_parse_records_no_recovery_at_all() {
  for (src, expected) in [
    ("1", "1"),
    ("-2 * 3", "((-2) * 3)"),
    ("1 + 2 * 3", "(1 + (2 * 3))"),
    ("(1 + 2) * 3", "((1 + 2) * 3)"),
    ("10 / 2 / 5", "((10 / 2) / 5)"),
    ("- - 1", "(-(-1))"),
  ] {
    let p = parse(src);
    assert_round_trips(src, &p);
    assert_eq!(p.diagnostics, [], "{src:?} should parse cleanly");
    assert_eq!(error_node_ranges(&p), [], "{src:?} should have no holes");
    assert_eq!(p.ast.to_string(), expected, "{src:?}");
  }
}

// ── The boundary of the posture: what ends the parse ────────────────────────────

/// tokora's parser-facing recursion budget, which [`parse`] inherits.
///
/// **This example did not choose the number, and that is now a choice rather than a wall.** It
/// used to be a wall: `parse_lossless` built its own `ParseContext`, and the only door to a
/// caller-chosen budget — `ParserContext::with_recursion_limiter` — was unreachable through it
/// (`Cst::from_sink`, `Sink::finish` and `Input::into_emitter` are all `pub(crate)`, so the
/// plumbing could not be hand-rolled either). `cst::parse_lossless_with_context` is that door, and
/// `a_lossless_parse_can_raise_the_budget_the_default_refused_at` below is this file's pin on it.
/// [`parse`] stays defaulted because an unconfigured parse is what the rest of the file is about.
///
/// **It used to be the literal `64`, and it is now read off the library.** The literal was a
/// canary — "a change to the default turns this cell red instead of silently moving what the
/// example demonstrates" — and it fired exactly as intended when the default was re-derived.
/// Reading it off the library is right for *this* file, whose subject is the boundary and not the
/// number; what a derived expectation cannot do is notice that the number moved, so that job is
/// held by literal cells in `src/state/recursion_tracker/tests.rs` and by the `const` assertions
/// beside the constant, which red when the default moves without its derivation.
const PARSE_DEPTH_BUDGET: usize =
  tokora::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH;

/// The deepest nesting that still parses. One frame short of the budget.
const DEEPEST_CLEAN: usize = PARSE_DEPTH_BUDGET - 1;

/// A **resource** trip ends the parse; it does not recover — and it does not panic either.
///
/// The other half of the file's claim, and the one an error-recovery example is most likely to
/// leave unsaid. Every cell above shows a *grammar* error becoming a hole. This one shows the
/// kind that cannot: the recursion budget is terminal by design — `inplace_recover` re-raises
/// the attempt the trip stopped rather than spending it, because a budget a recovery point could
/// swallow would not be a budget. So no error node is synthesized and there is nothing to
/// resume from.
///
/// The scoping is per **attempt**, not per session: the trip is counted on a monotone session
/// cell and every recovery point compares it against a baseline taken before its own attempt, so
/// a grammar that catches a trip and parses on keeps ordinary recovery afterwards
/// (`tests/pratt_limit_unit_sink.rs`, `a_caught_trip_does_not_disable_a_later_recovery`). This
/// grammar catches it nowhere, which is why the stop reaches `parse` at all.
///
/// What `parse` owes is therefore not recovery but **no panic**, and that is what is pinned:
/// the trip is recorded, the root is a hole, and `Parsed::terminated` says which happened. It
/// used to `expect()` here, so 64 nested parentheses took the process down.
///
/// Pinned one frame either side of the budget, by both routes into the descent — a nested group
/// and a nested prefix `-` — because they reach it differently and a fix could plausibly cover
/// one and miss the other.
#[test]
fn a_recursion_trip_ends_the_parse_without_panicking() {
  // Both shapes, built to the same depth.
  let group = |n: usize| format!("{}1{}", "(".repeat(n), ")".repeat(n));
  let prefix = |n: usize| format!("{}1", "-".repeat(n));

  for build in [&group as &dyn Fn(usize) -> String, &prefix] {
    // ── One frame inside the budget: an ordinary parse, nothing recorded. ──
    let src = build(DEEPEST_CLEAN);
    let p = parse(&src);
    assert_round_trips(&src, &p);
    assert_eq!(p.terminated, None, "{} frames must not trip", DEEPEST_CLEAN);
    assert_eq!(p.diagnostics, [], "a parse inside the budget is clean");
    assert_eq!(error_node_ranges(&p), [], "and leaves no holes");
    assert_ne!(p.ast, Expr::Error, "and produces a real AST");

    // ── One frame past it: terminal, recorded, and still standing. ──
    let src = build(PARSE_DEPTH_BUDGET);
    let p = parse(&src);

    // The property most likely to break when a parse ends early — and it holds, but only
    // because `parse` takes the terminal case out through `finish_partial`, which tiles the
    // un-parsed tail as one `Gap` run. Through `finish` this input cannot produce a tree at
    // all: the sink refuses it with `UncoveredGap { start: 64, .. }`.
    assert_round_trips(&src, &p);

    assert_eq!(
      p.terminated,
      Some(Diag::DepthExceeded),
      "{} frames must trip",
      PARSE_DEPTH_BUDGET
    );
    // The root is a hole, not a partial tree pretending to describe the input.
    assert_eq!(p.ast, Expr::Error);
    // Exactly one problem, spanning the whole source: a budget trip is a property of the
    // parse, not of a byte.
    assert_eq!(diags(&p), [(0, src.len(), Diag::DepthExceeded)]);
    // And no error *node* — there was no position at which to synthesize one, which is
    // precisely how this differs from every other cell in this file.
    assert_eq!(error_node_ranges(&p), []);
  }

  // Well past the boundary, to show the trip is a floor and not a one-off at the edge. Relative
  // to the budget rather than a literal, because a literal chosen against one default silently
  // stops being past the boundary when the default moves — which is what happened to the 200 that
  // used to be here.
  let src = group(PARSE_DEPTH_BUDGET * 3);
  let p = parse(&src);
  assert_round_trips(&src, &p);
  assert_eq!(p.terminated, Some(Diag::DepthExceeded));
}

/// **The escape hatch, on the exact input the default refuses.**
///
/// The cell above establishes that `PARSE_DEPTH_BUDGET` nested groups ends the parse. This one
/// takes that same source, hands the lossless driver an `InputContext` carrying a larger
/// `RecursionLimiter`, and requires it to parse **clean**: no trip, no diagnostic, no hole, and a
/// real AST rather than `Expr::Error`.
///
/// # What it is a regression for
///
/// Lowering the parser's default from 64 to 16 is right — 64 sat above the depth at which a
/// measured consumer grammar aborts — but on its own it would have been a break with no remedy for
/// exactly this population: a lossless consumer whose documents legitimately nest between the two
/// figures, and whose door built its own context. The lowering and the hatch belong in one change,
/// and this is the cell that says so. **A fix that raised the default back instead would pass the
/// cell above and fail this one's premise**, which is why the two are written as a pair and why
/// this one asserts the default still refuses first.
///
/// The raised budget is a bounded number and not `unlimited()`, deliberately: the point is that a
/// caller can pick a ceiling appropriate to their own stack, not that the ceiling can be removed.
#[test]
fn a_lossless_parse_can_raise_the_budget_the_default_refused_at() {
  let src = format!(
    "{}1{}",
    "(".repeat(PARSE_DEPTH_BUDGET),
    ")".repeat(PARSE_DEPTH_BUDGET)
  );

  // The premise, restated here rather than assumed from the cell above: if this ever stops
  // holding, the rest of this cell is testing nothing.
  let defaulted = parse(&src);
  assert_eq!(
    defaulted.terminated,
    Some(Diag::DepthExceeded),
    "the default must still refuse this input, or the hatch below has nothing to open"
  );

  // Four times the default: past the input's depth, and still a figure someone chose.
  let raised = expr_recovery::parse_with_depth(&src, PARSE_DEPTH_BUDGET * 4);
  assert_round_trips(&src, &raised);
  assert_eq!(
    raised.terminated, None,
    "a lossless caller that asked for the depth must get it"
  );
  assert_eq!(raised.diagnostics, [], "and the parse is otherwise clean");
  assert_eq!(error_node_ranges(&raised), [], "with no holes");
  assert_ne!(raised.ast, Expr::Error, "and a real AST");

  // And the hatch is a *budget*, not an opt-out: one frame short of the input's depth still trips.
  let still_short = expr_recovery::parse_with_depth(&src, PARSE_DEPTH_BUDGET - 1);
  assert_eq!(
    still_short.terminated,
    Some(Diag::DepthExceeded),
    "a raised-but-insufficient budget must still refuse — the caller chose a number, not `off`"
  );
}

/// The budget is a *depth* limit, not a length limit.
///
/// The analogue of the overflow cell's leading-zeros guard. A long but shallow input must parse
/// cleanly, or the cell above would also pass for an implementation that tripped on size.
#[test]
fn the_recursion_budget_counts_depth_and_not_length() {
  // Far longer than the tripping inputs, and one frame deep.
  let src = (1..=400)
    .map(|n| n.to_string())
    .collect::<Vec<_>>()
    .join(" + ");
  assert!(
    src.len() > group_len_at_budget(),
    "the flat input must be the longer one"
  );

  let p = parse(&src);
  assert_round_trips(&src, &p);
  assert_eq!(p.terminated, None, "a flat expression must not trip");
  assert_eq!(p.diagnostics, [], "nor diagnose anything");
  assert_eq!(error_node_ranges(&p), []);
}

fn group_len_at_budget() -> usize {
  PARSE_DEPTH_BUDGET * 2 + 1
}
