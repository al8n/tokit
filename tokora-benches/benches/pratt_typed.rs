//! The **typed** Pratt driver — `ParseInput::parse_input` for [`Pratt`], built by
//! [`pratt(lhs, rhs, fold_prefix, fold_infix, fold_postfix)`][pratt].
//!
//! # Why this file exists
//!
//! tokora ships **two** Pratt drivers, and until this file only one of them was measured:
//!
//! * the **token-level** one — [`InputRef::pratt`] → `pratt_in`, in
//!   `src/input/input_ref/pratt.rs`. It opens no transaction at all: acceptance *is* the
//!   commit, the folds never see an `InputRef`, and there is nothing to roll back.
//! * the **typed** one — the [`Pratt`] combinator, in `src/parser/pratt/expr.rs`. Its folds
//!   take an `InputRef`, its LHS/RHS channels are arbitrary parsers, and it therefore has to
//!   probe for the next operator inside a transaction it can restore.
//!
//! `parser_combinators.rs`'s `pratt_expr` calls `inp.pratt(...)` — the **token-level** one.
//! No benchmark in this repository ever constructed the typed [`Pratt`] combinator, so a
//! change that rewrote the typed driver end to end could not move a single benchmark number.
//! That was not a suspicion: a harness running only `pratt_expr` compiled byte-identical
//! across a branch that replaced the typed driver's transaction structure wholesale.
//!
//! This file closes that hole. Everything in it drives the typed combinator and nothing else.
//!
//! # What the typed driver's transaction structure costs
//!
//! Both shapes of the driver open a `Commit`-policy guard per operator **probe** — the RHS
//! channel is grammar code holding a full `InputRef`, so "is the next token our operator?" is
//! a question that has to be asked speculatively and un-asked when the answer is no. What
//! differs is *how long the probe stays live*:
//!
//! * hold it across the recursive operand parse and the fold, and a **right-associative**
//!   chain retains one full checkpoint — cursor, span, an `L::State` clone, an emitter mark,
//!   three offsets — per live frame: `O(operator depth)`.
//! * commit it the instant the operator is accepted, and retention is `O(1)` — but the
//!   postures the wide guard used to cover then need one expression-scoped guard above the
//!   whole driver, which is an extra `save`/settle **per expression**.
//!
//! The cells below are laid out to price exactly that trade: one cell where the added
//! per-expression `save`/settle is fully exposed and nothing amortises it, and a depth sweep in
//! which every depth carries its own control and the recursion shape — the thing retention
//! depends on — is what varies between the two halves of a pair.
//!
//! # The cells (group `pratt/typed`), and what the reported result is
//!
//! The result this file reports is **the paired right-versus-left ratio at each depth**. It is
//! not a throughput trend along the depth axis: the depth cells do not carry the same work per
//! byte and cannot support one — see *What is and is not comparable*. Every right-chain depth
//! therefore has a left-chain control beside it, and two adjacent lines of criterion output are
//! one measurement.
//!
//! * `bare_operand` — expressions with **zero** operators (`1234 ;`). One `parse_input` call,
//!   one LHS parse, one probe that reports `End` and is rolled back. This is the worst case for
//!   a per-expression cost — nothing amortises it — and the only cell that can answer "what does
//!   the expression-scoped guard cost when it buys nothing?". It needs its own cell because
//!   `parser_combinators.rs`'s `expr_source()` writes `d ^ 3 + a * b - ( c / 2 ) + -a ;`, whose
//!   several operators per expression would hide such a delta. It has no pair and is read as an
//!   absolute figure for that one question, never against a chain cell — and the figure **bounds
//!   the guard from above rather than isolating it**, because [`typed_pratt_drain`] runs a
//!   `peek_one` and a `try_expect` per expression *outside* the combinator and this cell has the
//!   group's highest expression density, so the harness-side per-expression cost is least
//!   amortised here too.
//! * `right_chain/{1,2,8,32,64}` — right-associative `^` chains of exactly that many operators
//!   per expression. Right-associativity is what makes the driver *descend*: the right
//!   operand admits the operator's own power (`Inclusive`), so `a ^ b ^ c` recurses rather
//!   than looping, and under the wide shape the probe of every enclosing frame stays live
//!   while it does. That descent is where `O(d)` retention *would* live — the driver as it
//!   stands does not have it, see *What is and is not comparable* — and it is the only axis
//!   along which the two shapes can diverge.
//!   Depth **1** is the **break-even probe** rather than part of the retention sweep: a
//!   per-expression cost and a per-operator saving cross somewhere between zero operators and a
//!   few, and a sweep that starts at 2 can only bracket that crossing.
//! * `left_chain/{1,2,8,32,64}` — the **control, at every one of those depths**. Byte-for-byte
//!   the right-chain fixture with `+` substituted for `^`. A left-associative operator recurses
//!   with `Exclusive(p)`, so the inner call declines the equal-power operator immediately and
//!   returns: the loop runs `d` times at recursion depth **2**, never deeper, and peak retention
//!   is `O(1)` at every depth under *either* shape.
//!
//!   The control used to exist at depths 2 and 64 only. That left `right_chain/{1,8,32}`
//!   uncontrolled, and a throughput trend read across those three cells would have been read as
//!   evidence about retention when it was evidence about fixture mix. It is at every depth now,
//!   and the pair — never the lone cell — is the unit of result.
//!
//! # What is and is not comparable
//!
//! **The fixture facts are asserted, not described.** [`Cell::check_invariants`] and
//! [`check_pair`] run over every generated cell before the group is registered — on the criterion
//! path and the fixed-iteration path alike — and a fixture that stops satisfying them fails the
//! bench. The numbers are in the code; this section is what they mean.
//!
//! Every fixture is ~128 KiB (the repository's bench convention), is generated from a counter (no
//! randomness, no clock, byte-identical between runs and machines), and spends exactly
//! [`BYTES_PER_OPERAND`] bytes per operand — `"1234 ;\n"` for the bare cell, `" ^ 1234"` for each
//! link of a chain. So operand and token density **per byte** are one constant across the whole
//! group.
//!
//! That fixed width pins the **source-token** density and nothing else — not *lexer* work per
//! byte, which is counted in scans and so includes the probe re-lexes below. In particular it
//! does **not** make the depth cells comparable to one another, though an earlier revision of
//! this header claimed it did. A depth-`d` expression is `BYTES_PER_OPERAND * (d + 1)` bytes
//! carrying `d` operators, `d` folds and one `parse_input` call, so folds per byte rise with
//! depth while expressions per byte fall — between depths 1 and 64 the first very nearly doubles
//! and the second falls by a factor of 32. That mix shift **is** the depth axis, so a throughput
//! trend read along it is a trend in the fixture mix and says nothing about retention.
//!
//! What is comparable is the pair at a **single** depth. [`check_pair`] asserts that
//! `right_chain/d` and `left_chain/d` are equal in length and equal byte for byte except at
//! exactly [`Cell::operators`] positions, each holding `^` against `+` — which makes identical
//! operand values in identical positions, identical token count, identical operator count and
//! identical fold count facts of the built fixture rather than claims about it. Identical fold
//! *arithmetic* is [`fold_infix`] being associativity-blind on purpose; identical `parse_input`
//! call count follows from the equal expression count.
//!
//! Two quantities are **not** checked by anything here, because they are properties of the driver
//! rather than of the fixture: the `parse` frame count (`d + 1`) and the operator-probe count
//! (`2d + 1`). They were counted once and are recorded under *Recorded measurements*. The
//! per-cell checksum does not stand in for them — `bare_operand`'s expected value is just a sum
//! of operands, so a change that skipped or cheapened the RHS `End` probe would still consume
//! every terminator and match it.
//!
//! Be exact about what that makes the paired ratio. It is a **matched right-associative
//! stress-and-control signal**, and it is **not** a measurement of `O(d)` live probe checkpoints
//! against `O(1)`: the driver as it stands commits each probe *before* it recurses, so the right
//! half keeps `O(1)` of them live too and there is no `O(d)` retention here to measure. What the
//! pair is, is **regression-sensitive** to that retention — undoing the narrowing puts `d` live
//! checkpoints on the right against one on the left and changes nothing else in the pair.
//!
//! Two residuals survive the pairing, so a ratio a percent or two off 1.0 is not read as
//! retention. The **probe kinds** differ: the right chain's `2d + 1` probes are `d` accepts and
//! `d + 1` `End` declines all re-lexing the same `;`, the left chain's are `d` accepts, `d - 1`
//! floor declines each re-lexing a distinct `+`, and two `End` declines — the same number of
//! rollbacks over the same 64-byte state, at different positions. And the **recursion shape**
//! differs, which is the point of the pair rather than a defect in it.
//!
//! # The drain is measured with the driver, and what that does to the ratio
//!
//! The measured parser is [`typed_pratt_drain`], whose loop lexes on **both** sides of the
//! combinator — a `peek_one` exhaustion test before each `parse_input`, a `try_expect(Semi)` for
//! the terminator after it — so the first operand's scan and the `;` are charged to the harness
//! rather than to `Pratt::parse_input`. Every absolute throughput figure here is therefore
//! drain-inclusive; isolating the driver would take a harness that does neither, which this file
//! does not build.
//!
//! The pairing is what makes that acceptable, though not for the reason it is tempting to give.
//! The drain is **matched**: both halves run the identical loop over the identical number of
//! expressions, so it adds no unequal right-versus-left work and cannot create, reverse or
//! conceal the *direction* of a difference. Matched is not absent, though. The measured quantity
//! is `(driver_right + drain) / (driver_left + drain)`, and a matched additive term pulls a ratio
//! *toward* 1.0 rather than dropping out of it — for `R < L`, `(R + D) / (L + D)` is greater than
//! `R / L`, though both stay below 1.0. So the ratio's side of 1.0 is exact and only its distance
//! from 1.0 is not: a recorded delta is a **lower bound** on the driver-only effect in the same
//! direction, "no retention penalty" holds a fortiori, and where a range straddles 1.0 the true
//! driver-only spread is wider in *both* directions. What the drain dilutes is **magnitude**, and
//! this file makes no magnitude claim. Measuring one would need the driver isolated from the peek
//! and the terminator, or a difference model in place of a ratio — the shape the fixed-iteration
//! harness below already uses, where a common additive term genuinely does subtract out.
//!
//! The same argument covers [`State::check`]'s per-scan tax, matched the same way and cancelling
//! no better.
//!
//! # Every measured parse is checked against a value the parser did not produce
//!
//! `parse_once` used to assert only that the parse returned `Ok`, and criterion `black_box`es
//! the accumulator without looking at it — so "consumed every token" was the whole semantic
//! contract. A driver that consumed every token while skipping folds, flipping an associativity
//! or returning a cheaper accumulated value satisfied that contract completely, did **less**
//! work, and would have reported a *better* number for doing less. Each [`Cell`] therefore
//! carries the accumulator the drain must return, computed by the generator from the operands it
//! emitted and the associativity it wrote ([`expected_expression`]) — never by running the
//! parser, which would agree with any driver including a broken one. [`parse_once`] compares
//! every parse against it, on the criterion path and the fixed-iteration path alike.
//!
//! The check has two known structural blind spots, and both are pinned rather than described:
//!
//! * **depth 1 cannot verify associativity.** A single fold is the same value under either
//!   order, so `right_chain/1` and `left_chain/1` must have the *same* expected accumulator
//!   while every deeper pair must differ. [`check_pair`] asserts exactly that, both directions,
//!   so a change to [`expected_fold`] that made it associative — and silently deleted the
//!   associativity check from every depth — fails the bench.
//! * **the right chain is blind to a permutation of its leading operands.** See
//!   [`expected_expression`]; the left half of each pair is not blind to it, so the pair as a
//!   whole still catches such a reordering at `d >= 2`.
//!
//! That the check *fires* was established by mutation, once — see *Recorded measurements*.
//!
//! # The measurement knobs are deliberately left live
//!
//! The other bench files in this directory pin `measurement_time(3s)` and `warm_up_time(1s)`
//! **in code**, which makes `--measurement-time` / `--warm-up-time` inert and pins the
//! iteration count near whatever three seconds happens to buy. That is fine for a file whose
//! job is trend detection; it is fatal for a file whose job is pricing a *small* per-expression
//! delta, which needs long windows for wall clock and, more importantly, needs a
//! **fixed-iteration** run for anything counter-based. This file sets neither, so every
//! criterion CLI knob works. Setting them from the command line is a one-flag operation;
//! un-setting a value compiled into the file is not.
//!
//! # The fixed-iteration harness
//!
//! The same binary runs a deterministic, criterion-free workload when `TOKORA_PRATT_ITERS` is
//! set:
//!
//! ```text
//! TOKORA_PRATT_CELL=bare_operand TOKORA_PRATT_ITERS=400 ./pratt_typed
//! ```
//!
//! It builds one fixture, runs exactly that many parses of it — each one checked against the
//! fixture's expected accumulator, as on the criterion path — prints a checksum, and exits
//! before criterion is constructed. Two runs at `N` and `2N` differ by exactly `N` parses, so
//! subtracting them cancels process startup, fixture construction and harness overhead
//! without having to model any of it — which is what makes a counter-based A/B of a
//! sub-percent change tractable. Same fixtures, same driver, same binary as the criterion
//! path: there is no second copy of anything to drift.
//!
//! # Recorded measurements
//!
//! **Everything numeric below is a dated record of a run that happened, not a claim this file
//! checks.** Nothing here is re-derived by the committed code, by CI, or by any assertion; a
//! change to the driver, the fixture or the machine invalidates it silently. Read a figure here
//! as "this was measured, then, there" and re-run before relying on one. Every *structural*
//! claim about the fixtures has been moved out of this section and into
//! [`Cell::check_invariants`] and [`check_pair`], which do fail when they stop holding.
//!
//! Recorded **2026-08-03**, in the commit that introduced this file (`49e1a8d`, "perf(bench):
//! cover the typed pratt driver (#155)"), on an Apple-silicon laptop with unrelated work running
//! throughout. The toolchain was not recorded. Reproduce the sweep with:
//!
//! ```text
//! cargo bench -p tokora-benches --bench pratt_typed -- --warm-up-time 1 --measurement-time 3
//! ```
//!
//! ## The retention sweep, and the null it found
//!
//! Eleven reps at those knobs. **Two reps discarded** as visibly contended, under the rule "any
//! cell more than 10% above its own median across reps": one had four cells 20–70% high while
//! the machine's one-minute load average ran 4.2 → 9.4, the other two cells 25–29% high at
//! 4.7 → 6.0. Nine reps kept. Neither discarded rep supplied the minimum for any cell, so the
//! discards moved no figure below. Per-cell spread across the kept reps, slowest over fastest,
//! was +0.95% to +4.46% (`bare_operand` the worst).
//!
//! The **primary** figure is the per-rep paired ratio: the two halves run back to back within one
//! rep, so right-time-over-left-time computed *within that rep* holds everything but the driver
//! shape fixed. The two minimum columns are independent over the nine kept reps and are there for
//! scale only — the fastest right rep and the fastest left rep need not be the same rep, so their
//! difference is **not** a paired statistic.
//!
//! | depth | paired ratio (median, 9 reps) | as a delta | ratio range (9 kept reps)        | `right_chain` min | `left_chain` min |
//! |-------|--------------------------------|------------|------------------------------------|--------------------|--------------------|
//! | 1     | **0.9999**                     | −0.01%     | 0.9835 … 1.0042 (straddles 1.0)   | 1031.1 µs          | 1033.0 µs          |
//! | 2     | **0.9876**                     | −1.24%     | 0.9839 … 0.9949                   | 1010.8 µs          | 1023.1 µs          |
//! | 8     | **0.9743**                     | −2.57%     | 0.9624 … 0.9965                   |  981.6 µs          | 1006.0 µs          |
//! | 32    | **0.9687**                     | −3.13%     | 0.9608 … 0.9810                   |  961.7 µs          |  992.7 µs          |
//! | 64    | **0.9916**                     | −0.84%     | 0.9811 … 1.0053 (straddles 1.0)   |  981.4 µs          |  990.8 µs          |
//!
//! `bare_operand`, which has no pair: 1081.8 µs. As drain-inclusive throughput that is ~116 MiB/s
//! on operator-free input and ~121–130 MiB/s on chains — for this grammar, on that machine, and
//! **not** comparable to the `perfloop`-derived figures elsewhere in this project (different
//! harness, different accounting, different machine conditions). Criterion agreed with the
//! fixed-iteration harness to within 1–2% on this workload.
//!
//! **On the paired per-rep ratio, the retention penalty was not measurable at depths 2, 8 and
//! 32.** Every one of the nine kept reps put the right-associative side at or below its control
//! at those three depths — the deep-recursion side marginally **faster**, by roughly 1% to 3%,
//! and slower in not one kept rep. **Depths 1 and 64 do not support that statement.** Their
//! median ratio is also at or near 1.0, but the range straddles it, so at least one kept rep at
//! each of those two depths *did* run the right-associative side slower than its matched control.
//! An `O(d)` cost would appear as a ratio climbing above 1.0 with depth; nothing here does that,
//! and the median is not monotone in depth either (furthest below 1.0 around depth 32, back
//! toward it at 64), so it does not read as a retention curve either.
//!
//! That was a real finding — a bound on the trade this file was written to price, read off the
//! statistic that is actually paired. It is **not** a proof that retention is free: a heavier
//! `L::State`, a deeper chain, or a machine with a smaller cache could all move it, and the small
//! residual in the right side's favour at the middle depths is itself unexplained (the probe-kind
//! asymmetry above is large enough to account for it, in either direction).
//!
//! The null is unsurprising rather than mysterious, because the driver in
//! `src/parser/pratt/expr.rs` is already the **narrowed** shape: its `Infix` arm calls
//! `txn.commit()` *before* the recursive operand parse, so the quantity the pair is built to
//! expose is absent from the code the pair was run against. What the pair would catch is the
//! narrowing being undone — move that `commit()` back below the recursion and `right_chain/d`
//! retains `d` live checkpoints against the left chain's one, which should push the ratio above
//! 1.0 by a growing margin and leave every other quantity in the pair untouched. That sensitivity
//! is reasoned from the fixture design and **has not been measured**; no run of the wide shape
//! exists to compare against. It is the obvious next measurement and the honest limit of what the
//! null establishes.
//!
//! ## Counted once, with instrumentation that is not committed
//!
//! Temporary counters in [`parse_lhs`] and [`parse_rhs`] reported exactly `d + 1` `parse` frames
//! and `2d + 1` operator probes per expression on **both** halves of the pair, at all five
//! depths. That is how the pair was matched when this file was written. A future change to the
//! driver's recursion shape that altered either count would not be caught by anything here.
//!
//! ## The checksum has been shown to fire
//!
//! Two mutations of `src/parser/pratt/expr.rs`, both reverted:
//!
//! * replacing the infix fold with `lhs = rhs` — every fold skipped, every token still consumed
//!   — failed every cell that has an operator (exit 101) and correctly left `bare_operand`,
//!   which has none, passing;
//! * giving `PrattInfix::Right` the `Exclusive` floor — a right-associative chain parsed
//!   left-associatively: same tokens, same fold count, *shallower* recursion, so a **faster**
//!   wrong answer — failed `right_chain/{2,8,32,64}` under the fixed harness and under
//!   criterion, and correctly left `left_chain/*`, `right_chain/1` and `bare_operand` untouched.
//!
//! `right_chain/1` surviving the second mutation is the depth-1 associativity blind spot above,
//! observed rather than predicted; [`check_pair`] now pins it so it cannot widen unnoticed.
//!
//! ## Emitted code, read off a disassembly once
//!
//! Read from the `bench` profile on `aarch64-apple-darwin`, on both feature sets this bench
//! builds under. Nothing in the committed code re-verifies any of it.
//!
//! * [`parse_once`]'s accumulator check is **two instructions** — a `cmp`/`b.ne` at the tail,
//!   after the parse and outside every loop in it, branching to the `#[cold] #[inline(never)]`
//!   [`mismatch`]. A wall-clock A/B on the fixed-iteration harness could not resolve it: the
//!   checked build measured 1.5% *faster* than an unchecked one, against a 1.7% spread between
//!   two separate builds of the same checked code. Upper bound from measurement, 1.7%.
//! * [`note_operand`] inlines into `<ExprTok as Logos>::lex::state2` and nowhere else, and emits
//!   five stores and three loads to memory on the operand path — `indents[indent_top % 16]` via
//!   an `strh` through an address formed with `and #0xf`, `modes[mode_top % 29]`
//!   read-modify-written by an `ldrb`/`strb` pair through the magic-multiply `msub`, and three
//!   `strb`s for the two cursors and `last_kind`. Neither array is scalarized into registers and
//!   neither index is a constant the optimizer can bound.
//! * [`State::check`]'s two indexed loads are emitted unconditionally per scan **because of the
//!   `black_box`**, not because the veto is unreachable. A revision without the barrier had LLVM
//!   reorder the `indents` compare to the front and sink the `modes` load into the block that
//!   compare guards, so the `modes` array was read exactly *zero* times per parse at all four
//!   inlined `check` sites while the comment claimed two loads per token. See
//!   [`check`](State::check) for the whole argument.
//!
//! [`Pratt`]: tokora::parser::Pratt
//! [pratt]: tokora::parser::pratt
//! [`InputRef::pratt`]: tokora::InputRef::pratt

use core::fmt::Write as _;
use std::hint::black_box;

use criterion::{Criterion, Throughput, criterion_group};

use tokora::{
  Emitter, FatalContext, InputRef, Parse, ParseContext, ParseInput, Parser, ParserContext, State,
  Token,
  emitter::Fatal,
  error::{NonAssociativeChain, RecursionLimitReached, UnexpectedEnd, token::UnexpectedToken},
  lexer::LogosLexer,
  logos::{self, Logos},
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced, pratt},
  state::recursion_tracker::RecursionLimiter,
};

// ── The lexer state ───────────────────────────────────────────────────────────
//
// A checkpoint clones `L::State`, and the state is *unconstrained*: tokora imposes no size
// bound on it, so "one retained checkpoint per operator depth" is only a cost if there is
// something to retain. `LogosLexer`'s default `Extras` is `()`, which makes every clone free
// and the whole retention question invisible — the existing `pratt_expr` bench runs on `()`.

/// The lexer state this file's fixtures carry: what a mode-switching, layout-sensitive lexer
/// really holds between tokens.
///
/// # Why this shape
///
/// The two things a stateful hand-written lexer almost always carries are a **mode stack**
/// (string interpolation, template literals, raw-string nesting, regex-vs-divide) and, if the
/// language is layout-sensitive, an **indent stack**. Neither can be a heap allocation in a
/// `no_std` lexer, so both are fixed-capacity inline arrays sized for the deepest nesting the
/// grammar admits. Sixteen indent levels and twenty-nine mode levels is a generous but
/// unremarkable budget; the two cursors and the classification byte are what a lexer needs to
/// interpret them.
///
/// # Why it is representative and not adversarial
///
/// It is **64 bytes — one cache line**, and that is the whole claim. It is a quarter of the
/// `HeavyState` in `input_scan.rs`, which that file explicitly labels "deliberately heavy …
/// an anti-pattern" and uses to establish an upper bound rather than a typical cost. A
/// benchmark that priced retention against 256 bytes would be measuring how badly a
/// pathological state behaves; one that priced it against `()` would be measuring nothing at
/// all. A single cache line is the smallest size at which a state clone is a real memory
/// operation rather than a register move, and it is what a real mode-stack lexer lands on.
///
/// # Why every field is live
///
/// `input_scan.rs`'s `HeavyState` can afford a write-only `scratch` array because its benches
/// contrast a *heavy* state against a *light* one and the contrast is the result. This file has
/// no such control: if the optimizer were to notice that nothing ever reads the two stacks and
/// shrink `ExprState` to the two bytes that are observable, the retention cells would silently
/// become a `()`-state experiment wearing a 64-byte type, and every number below would be
/// wrong in the direction that flatters the change. So the state is genuinely live in both
/// directions — [`note_operand`] writes it on every operand and [`check`](State::check) reads
/// it after every token, both through **runtime** indices, which is what makes the arrays real
/// memory rather than candidates for scalar replacement.
///
/// "Genuinely live" is a claim about the emitted code, so it is checked against the emitted code
/// and not asserted, in both directions: see [`check`](State::check) for the disassembly of the
/// revision where the read half was *not* true and for the `black_box` that makes it true now,
/// and [`note_operand`] for the stores the write half was disassembled to confirm.
///
/// Both halves are what a real lexer does anyway: a stateful lexer updates its stacks as it
/// scans, and `State::check` exists precisely so a state can veto after every token.
#[derive(Debug, Clone, Default)]
struct ExprState {
  /// Indent columns, innermost last — the layout-sensitive half. Sixteen levels.
  indents: [u16; 16],
  /// The lexical mode stack, innermost last. Twenty-nine levels — the number that fills the
  /// cache line beside the indent stack and the three cursors, and nothing more principled
  /// than that.
  modes: [u8; 29],
  /// Cursor into `indents`.
  indent_top: u8,
  /// Cursor into `modes`.
  mode_top: u8,
  /// The last significant token's class — the ASI / regex-vs-divide discriminator.
  last_kind: u8,
}

// One cache line, on the nose. Stated as a pin rather than left to a comment: the entire
// justification above is "64 bytes, because that is what a real mode-stack lexer costs", and a
// field added without re-reading that paragraph would quietly turn this fixture into a
// different experiment.
const _: () = assert!(
  size_of::<ExprState>() == 64,
  "pratt_typed: the fixture's lexer state is no longer one cache line. It is sized to be \
   representative — see `ExprState` — so re-state the justification in the same commit that \
   changes it, or the retention numbers this file produces are measuring a different state."
);

impl State for ExprState {
  type Error = ();

  /// The per-scan veto `LogosLexer` calls after every successful scan — a limiter's natural home,
  /// and here also the **read** that keeps [`ExprState`] from being optimized down to the two
  /// bytes anything observes.
  ///
  /// Both loads are through a runtime index (`indent_top` / `mode_top` are `u8` cursors the
  /// lexer advances), which is what makes them un-forwardable: an array indexed by a value the
  /// optimizer cannot bound has to exist in memory, be cloned in full by a checkpoint, and be
  /// written back in full by a restore.
  ///
  /// Per *scan*, not per source token. `lex` runs this after every successful scan, and a probe
  /// that declines rolls back a token that is then lexed again, so the fixture's source-token
  /// density does not bound how often it runs — an earlier revision of this comment justified the
  /// tax by that density ("exactly two tokens per seven bytes"), which is the wrong quantity and
  /// is withdrawn. What the **pairing** buys, and it is not a per-byte rate, is that the tax is
  /// *matched*: `right_chain/d` and `left_chain/d` agree on both quantities that drive the scan
  /// count — the same source tokens (asserted, see [`check_pair`]) and the same operator-probe
  /// count, of which the same number are declines that roll back (counted once, not asserted;
  /// see the header's *Recorded measurements*). So it falls equally on the two halves of every
  /// pair and adds no unequal right-versus-left work. It does **not** cancel: like the drain, it
  /// is a common additive cost, and a common additive cost pulls a ratio toward 1.0 rather than
  /// dropping out of it.
  ///
  /// # Why [`black_box`], and not the condition alone
  ///
  /// An earlier revision loaded both values and spelled the veto as
  /// `mode == MODE_POISON && indent == u16::MAX` with no barrier, on the theory that a
  /// condition the optimizer cannot fold is a condition it has to evaluate — and therefore two
  /// loads per token. A disassembly refuted that (recorded in the header); `&&` short-circuits,
  /// and the optimizer picks which side to short-circuit on, so "unreachable by construction" is
  /// not a property that keeps a load alive — it is a property the optimizer exploits.
  ///
  /// The observation is therefore explicit. `black_box` is an opaque barrier: each loaded value
  /// must be materialized before it and nothing downstream may reason about it, so both indexed
  /// loads are emitted **unconditionally and on the hot path**, once per scan, and neither
  /// compare can be folded away. That is the property this fixture needs, and it does not
  /// depend on the optimizer failing to prove something.
  ///
  /// The veto is kept rather than replaced by a plain `Ok(())` for a second reason: a `check`
  /// that provably cannot fail also proves `LogosLexer`'s `poisoned` latch (`lexer/logos`) is
  /// never set, which deletes the latch test at the top of every `lex` call. A real
  /// limiter-bearing lexer pays for that test, so this fixture keeps paying for it too.
  /// [`MODE_POISON`] is still never written, so the arm never fires; `black_box` is what makes
  /// the compiler unable to know that.
  fn check(&self) -> Result<(), ()> {
    let indent = black_box(self.indents[usize::from(self.indent_top) % self.indents.len()]);
    let mode = black_box(self.modes[usize::from(self.mode_top) % self.modes.len()]);
    if mode == MODE_POISON && indent == u16::MAX {
      Err(())
    } else {
      Ok(())
    }
  }
}

// ── The token enum ────────────────────────────────────────────────────────────
//
// The smallest grammar that can express all three cell shapes: an operand, one
// right-associative operator, one left-associative operator, and a statement terminator. Every
// cell runs this *same* grammar and differs only in its fixture, so no cell-to-cell comparison
// can be confounded by a difference in the parser.

/// Records the operand in the lexer state the way a stateful lexer does — push the column it
/// opened at, push the mode it was read in, remember its class — and returns its value.
///
/// This is the **write** half of what keeps [`ExprState`] live; [`State::check`] is the read
/// half. Both index through the `u8` cursors, and — as with the read half — the write half was
/// checked against emitted code rather than asserted. What that disassembly showed, and when, is
/// in the header's *Recorded measurements*; nothing in the committed code re-verifies it.
///
/// Mutating `extras` mid-scan is legal and deterministic under tokora's lexer contract: a
/// restore puts the saved state back with the saved position, so re-lexing from a restored
/// checkpoint reproduces exactly the tokens and exactly the state transitions it did the first
/// time. Only instrumentation held *outside* the state can observe the abandoned scans.
fn note_operand(lex: &mut logos::Lexer<'_, ExprTok>) -> Option<i64> {
  let value = lex.slice().parse::<i64>().ok()?;
  let column = u16::try_from(lex.span().start % 4096).unwrap_or(0);
  let state = &mut lex.extras;

  let indent_slot = usize::from(state.indent_top) % state.indents.len();
  state.indents[indent_slot] = column;
  state.indent_top = state.indent_top.wrapping_add(1);

  let mode_slot = usize::from(state.mode_top) % state.modes.len();
  let previous_mode = state.modes[mode_slot];
  state.modes[mode_slot] = MODE_OPERAND;
  state.mode_top = state.mode_top.wrapping_add(1);
  state.last_kind = MODE_OPERAND.wrapping_add(previous_mode);

  Some(value)
}

/// The mode an operand is read in — the only mode this grammar has, since it has no strings and
/// no interpolation. A real lexer's vocabulary is longer; the stack it pushes onto is the same.
const MODE_OPERAND: u8 = 1;

/// A mode that is never pushed, so [`State::check`]'s failure arm never fires. Named rather
/// than spelled `0xFF` inline so the "never written" claim is checkable by searching for one
/// identifier.
///
/// Being unreachable is *not* what keeps the check's loads alive — [`State::check`] documents
/// the disassembly showing that it does the opposite. The `black_box` there is what makes the
/// compiler unable to fold the comparison, and this constant is only what keeps the arm from
/// ever being taken at runtime.
const MODE_POISON: u8 = 0xFF;

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, extras = ExprState, skip r"[ \t\r\n]+")]
enum ExprTok {
  #[regex(r"[0-9]+", note_operand)]
  Int(i64),
  /// The **right**-associative operator: its right operand admits its own power, so a chain of
  /// these makes the driver descend one frame per link.
  #[token("^")]
  Caret,
  /// The **left**-associative operator: its right operand stops strictly above its power, so a
  /// chain of these loops at a fixed recursion depth. The control.
  #[token("+")]
  Plus,
  /// Statement terminator — the token the RHS channel answers `End` on.
  #[token(";")]
  Semi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ExprKind {
  Int,
  Caret,
  Plus,
  Semi,
}

impl core::fmt::Display for ExprKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      ExprKind::Int => "integer",
      ExprKind::Caret => "'^'",
      ExprKind::Plus => "'+'",
      ExprKind::Semi => "';'",
    })
  }
}

impl core::fmt::Display for ExprTok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      ExprTok::Int(n) => write!(f, "{n}"),
      other => core::fmt::Display::fmt(&other.kind(), f),
    }
  }
}

impl Token<'_> for ExprTok {
  type Kind = ExprKind;
  type Error = ();

  fn kind(&self) -> ExprKind {
    match self {
      ExprTok::Int(_) => ExprKind::Int,
      ExprTok::Caret => ExprKind::Caret,
      ExprTok::Plus => ExprKind::Plus,
      ExprTok::Semi => ExprKind::Semi,
    }
  }

  fn is_trivia(&self) -> bool {
    false
  }
}

type ExprLexer<'a> = LogosLexer<'a, ExprTok>;

// ── The error absorber ────────────────────────────────────────────────────────
//
// Every fixture is well-formed, so none of these is constructed at runtime; they exist to
// satisfy the bounds the typed driver and `Parser::new` require. The typed `Pratt`'s
// `ParseInput` impl in particular demands `From<UnexpectedEoLhs<_, _>> +
// From<UnexpectedEoRhs<_, _>> + From<RecursionLimitReached<_, _>> +
// From<NonAssociativeChain<_, _>>` (`src/parser/pratt/expr.rs`). The first two are aliases of
// `UnexpectedEnd` and are covered by the one generic impl below; the other two get their own
// impls just below that, converting to the same unit `BenchError`. `RecursionLimitReached` in
// particular is not merely unreached here the way `NonAssociativeChain` is — see
// `BENCH_RECURSION_LIMIT`'s comment for why the bound exists but every fixture is kept clear of
// tripping it.

#[derive(Debug, Default, Clone, PartialEq)]
struct BenchError;

impl From<()> for BenchError {
  fn from(_: ()) -> Self {
    BenchError
  }
}

impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for BenchError {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    BenchError
  }
}

/// Covers `UnexpectedEot` and the Pratt driver's `UnexpectedEoLhs` / `UnexpectedEoRhs` in one
/// impl — all three are aliases of `UnexpectedEnd` with different hints.
impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for BenchError {
  fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self {
    BenchError
  }
}

impl From<RecursionLimitReached> for BenchError {
  fn from(_: RecursionLimitReached) -> Self {
    BenchError
  }
}

impl From<NonAssociativeChain> for BenchError {
  fn from(_: NonAssociativeChain) -> Self {
    BenchError
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for BenchError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    BenchError
  }
}

// ── The grammar ───────────────────────────────────────────────────────────────
//
// Binding powers are plain `i64` — tokora implements `PrattPower` for the integers, so no
// newtype sits between the fixture and the engine.

/// The left-associative rung.
const PREC_SUM: i64 = 1;
/// The right-associative rung, above it, so a mixed source would nest `^` inside `+`. The
/// fixtures are single-operator, so the two never meet; the gap is here so the ladder is a
/// ladder and the `admits` comparisons are not all against one value.
const PREC_EXP: i64 = 2;

/// The LHS channel: an operand, always. There is no prefix operator in this grammar — a prefix
/// report makes the driver recurse at the *same* input position, which is a second and
/// independent source of depth, and mixing it into a fixture whose axis is operator depth would
/// confound the two.
fn parse_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
) -> Result<PrattLHS<i64, (), i64>, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  match inp.next()? {
    Some(tok) => match tok.into_data() {
      ExprTok::Int(n) => Ok(PrattLHS::Operand(n)),
      _ => Err(BenchError),
    },
    None => Err(BenchError),
  }
}

/// The RHS channel, and the reason the typed driver needs a transaction at all: this is
/// ordinary grammar code holding a full `InputRef`, and it decides by **consuming**. On `End`
/// — a `;`, or exhaustion — the token it took has to go back, which is the probe guard's whole
/// job.
fn parse_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
) -> Result<PrattRHS<(), (), (), (), i64>, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  Ok(match inp.next()? {
    Some(tok) => match tok.data() {
      ExprTok::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM)),
      ExprTok::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      // A `;` is not this expression's operator; it stays for the drain loop.
      _ => PrattRHS::End,
    },
    None => PrattRHS::End,
  })
}

/// Never reached — this grammar has no prefix operator — but the driver is generic over a fold
/// for one, so a fold has to exist.
fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), i64>,
) -> Result<i64, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  Ok(operand.wrapping_neg())
}

/// The one fold the fixtures reach, and it is **associativity-blind on purpose**: the same
/// arithmetic runs for `PrattInfix::Left` and `PrattInfix::Right`, so `left_chain` and
/// `right_chain` at equal depth perform an identical number of identical folds. Any difference
/// between them is the driver's, not the fold's.
fn fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
  left: i64,
  right: i64,
  _op: Precedenced<PrattInfix<(), (), ()>, i64>,
) -> Result<i64, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  Ok(left.wrapping_mul(31).wrapping_add(right))
}

/// Never reached — this grammar has no postfix operator.
fn fold_postfix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
  operand: i64,
  _op: Precedenced<(), i64>,
) -> Result<i64, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  Ok(operand)
}

/// **The measured parser.** A drain loop around one `Pratt` combinator, built once and reused,
/// driven once per `;`-terminated expression to the end of input.
///
/// `pratt(...)` — and therefore `Pratt::parse_input` — is the whole point of this file: it is
/// the entry point `parser_combinators.rs` never reaches, and `#[cold] #[inline(never)]`
/// symbols in `src/parser/pratt/expr.rs` reachable from nowhere else are how a compiled binary
/// can be shown to contain it.
///
/// It is not, however, the only thing under the clock. This loop lexes on **both** sides of the
/// combinator — [`peek_one`](InputRef::peek_one) for the exhaustion test before each
/// `parse_input`, `try_expect(Semi)` for the terminator after it — so the first operand's scan
/// and the `;` are charged here rather than to `Pratt::parse_input`. The header's *The drain is
/// measured with the driver* works through what that does to an absolute figure and to the
/// paired ratio; the short form is that a recorded delta bounds the driver-only effect rather
/// than measuring it.
///
/// # Why a missing terminator is an error and not an exit
///
/// The loop has exactly one exit: [`peek_one`](InputRef::peek_one) reporting exhaustion
/// *before* an expression is started. Everything else — a `parse_input` that stopped short of
/// its `;`, a rollback that regressed and consumed the `;`, a fixture whose construction
/// drifted — is a failure.
///
/// It used to be an exit. `try_expect` returning `None` simply `break`ed and the drain returned
/// `Ok(acc)`, which means a driver regression that ate the first expression's terminator would
/// have parsed **one** expression of a 128 KiB fixture, left the other ~18 000 unread, and
/// reported success — while criterion went on charging `Throughput::Bytes(src.len())` for every
/// byte never lexed. Throughput inflated over unparsed input is the one way this file can be
/// wrong that its own numbers cannot reveal: the figure simply gets better. So the terminator
/// is required, and requiring it is what makes "the loop ended" and "the fixture was consumed"
/// the same statement.
///
/// [`InputRef::peek_one`]: tokora::InputRef::peek_one
fn typed_pratt_drain<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, ExprLexer<'inp>, Ctx>,
) -> Result<i64, BenchError>
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, ExprLexer<'inp>, Error = BenchError>,
{
  let mut parser = pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix);
  let mut acc = 0i64;
  loop {
    // Scope the peek borrow: exhaustion ends the drain before the driver is asked for an
    // expression that is not there.
    let done = {
      let peeked = inp.peek_one()?;
      peeked.is_none()
    };
    if done {
      break;
    }
    acc = acc.wrapping_add(parser.parse_input(inp)?);
    // Required, not optional — see this function's header. A `;` that is missing or displaced
    // fails the parse, and `parse_once`'s `expect` turns that into a panic that stops the
    // bench; it does not quietly end the drain over a fixture that is mostly unread.
    if inp
      .try_expect(|t| matches!(t.data(), ExprTok::Semi))?
      .is_none()
    {
      return Err(BenchError);
    }
  }
  Ok(acc)
}

// ── Deterministic sources ─────────────────────────────────────────────────────

/// Every fixture targets this size, matching the other bench files, so a full parse dwarfs the
/// fixed per-parse setup.
const TARGET: usize = 128 * 1024;

/// The byte width of one operand **and its punctuation**, in every fixture in this group:
/// `"1234 ;\n"` for the bare cell, `" ^ 1234"` for each link of a chain. This is the whole of
/// the byte normalisation the header's *What is and is not comparable* is written on top of, so
/// it is pinned per cell by [`Cell::check_invariants`] rather than stated in prose.
const BYTES_PER_OPERAND: usize = 7;

/// A four-digit operand, from a counter. The fixed width is what makes
/// [`BYTES_PER_OPERAND`] a constant rather than an average. Lexer *scans* are a different and
/// larger count — a declined probe re-lexes; see [`check`](State::check) — so it does not pin
/// lexer work per byte.
fn operand(i: u32) -> u32 {
  1000 + i.wrapping_mul(2654435761) % 9000
}

/// What a generated fixture's bytes actually contain, counted **from the bytes**.
///
/// The point of counting rather than deriving is that the generator's loop bounds and the bytes
/// it emitted are two different things, and the header's comparability argument rests on the
/// second. A generator change that emitted a different shape while keeping its counters would
/// pass a derived check and fail this one.
struct Census {
  /// Maximal runs of ASCII digits.
  operands: usize,
  /// `^` plus `+` bytes. This grammar has no other use for either.
  operators: usize,
  /// `;` bytes — one per expression, since every expression is terminated.
  expressions: usize,
}

impl Census {
  fn of(src: &str) -> Self {
    let mut operands = 0usize;
    let mut operators = 0usize;
    let mut expressions = 0usize;
    let mut in_operand = false;
    for b in src.bytes() {
      let digit = b.is_ascii_digit();
      if digit && !in_operand {
        operands += 1;
      }
      in_operand = digit;
      match b {
        b'^' | b'+' => operators += 1,
        b';' => expressions += 1,
        _ => {}
      }
    }
    Self {
      operands,
      operators,
      expressions,
    }
  }
}

/// Which operator a chain fixture is built from, and therefore which shape the driver must fold
/// it into. One value decides **both** the byte written into the source and the tree the expected
/// accumulator is computed over, so a fixture cannot be generated with one operator and predicted
/// under the other associativity.
///
/// The `char` here and the `#[token]` spellings on [`ExprTok::Caret`] / [`ExprTok::Plus`] are the
/// two halves of one mapping, and nothing in the type system ties them together — but nothing has
/// to: the expected accumulator is derived from *this* half and checked against a parse driven by
/// *that* half, so a mismatch between them is a loud panic on the first iteration of the affected
/// cell. See [`parse_once`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Assoc {
  /// `+`, `PrattInfix::Left`, `Exclusive` floor: `((a ⊕ b) ⊕ c)`.
  Left,
  /// `^`, `PrattInfix::Right`, `Inclusive` floor: `(a ⊕ (b ⊕ c))`.
  Right,
}

impl Assoc {
  /// The byte the generator writes between two operands.
  const fn op(self) -> char {
    match self {
      Assoc::Left => '+',
      Assoc::Right => '^',
    }
  }

  /// The criterion id prefix — `right_chain` / `left_chain`.
  const fn cell_prefix(self) -> &'static str {
    match self {
      Assoc::Left => "left_chain",
      Assoc::Right => "right_chain",
    }
  }
}

/// The generator's own copy of [`fold_infix`]'s arithmetic.
///
/// Deliberately a **second** spelling rather than a shared helper the fold calls. The expected
/// accumulator's independence from the driver is structural — it is computed from the operand
/// sequence the generator emitted and the tree shape the operator implies, with no lexer, no
/// `InputRef` and no `Pratt` involved — and keeping the scalar kernel here as well means an edit
/// to `fold_infix` *alone* changes the parse result without changing the prediction, which is
/// exactly the class of regression [`Cell::expected`] exists to catch. The cost of the
/// duplication is that an intentional change to the fold has to be made in both places; the
/// first parse after a one-sided change panics with both values, so the drift cannot be silent.
fn expected_fold(left: i64, right: i64) -> i64 {
  left.wrapping_mul(31).wrapping_add(right)
}

/// Folds one expression's operands the way the driver must, given the associativity the fixture
/// was written with — right to left for `^`, left to right for `+`.
///
/// This is where a flipped associativity is caught: for `d >= 2` the two orders give different
/// values over the same operands, because [`expected_fold`] is not associative. At `d == 1` there
/// is a single fold and the two orders coincide, so depth 1 checks fold *count* and fold
/// arithmetic but cannot check associativity — the deeper cells are what do that. Both halves of
/// that sentence are asserted in [`check_pair`], so a change here that made the fold associative
/// cannot silently disable the check at every depth.
///
/// The **right**-chain form has a second, structural blind spot, distinct from the one above and
/// stated where the fold shape makes it visible: the tree collapses to
/// `31 * (a_0 + … + a_{d-1}) + a_d`, because every one of the `d - 1` intermediate folds
/// contributes its operand to the running total with the *same* factor of 31, and only the last
/// operand, `a_d`, is added un-multiplied. That value still catches a wrong fold count and a
/// wrong fold arithmetic result — skip a fold, or change what `fold_infix` computes, and the sum
/// changes. It does **not** catch the *order* of the leading `d` operands among themselves: the
/// formula depends only on their sum, so a regression that permuted `a_0 … a_{d-1}`, or that
/// otherwise changed which operand is folded at which step while preserving their sum, would
/// still produce this exact value and pass. The **left**-chain form is not blind this way: its
/// Horner expansion (`expected_fold` applied left to right, in this function's `Assoc::Left` arm
/// below) gives every operand — leading or last — a distinct power of 31 by position, so
/// permuting any of them changes the result whenever `d >= 2`. The pair as a whole therefore
/// still catches a leading-operand reordering on its left half, even at a depth where the right
/// half would miss it. Closing the right half's gap would mean changing `fold_infix`'s arithmetic
/// itself, which adds work per fold and would renormalise every throughput figure in this file's
/// header — not worth paying for a bench-only check, so the gap is left open knowingly rather
/// than by oversight.
fn expected_expression(operands: &[i64], assoc: Assoc) -> i64 {
  // Unreachable: every expression this file generates opens with an operand.
  const EMPTY: &str = "pratt_typed: an expression was generated with no operands";
  match assoc {
    Assoc::Left => {
      let (first, rest) = operands.split_first().expect(EMPTY);
      rest
        .iter()
        .fold(*first, |left, &right| expected_fold(left, right))
    }
    Assoc::Right => {
      let (last, rest) = operands.split_last().expect(EMPTY);
      rest
        .iter()
        .rfold(*last, |right, &left| expected_fold(left, right))
    }
  }
}

/// One benchmark cell: the criterion id, the generated source, the accumulator
/// [`typed_pratt_drain`] **must** return over it, and the shape facts
/// [`check_invariants`](Cell::check_invariants) holds it to.
///
/// # Why an expected value, when the parse already returns `Ok`
///
/// It did not used to have one. [`parse_once`] asserted only that the parse reached the end of
/// its input, and criterion `black_box`es the accumulator without looking at it — so full
/// consumption was the entire semantic contract. That leaves a whole class of regression
/// invisible *and flattering*: a driver that consumes every token but skips folds, flips an
/// associativity, or returns a cheaper accumulated value passes every cell, does **less** work
/// than it should, and reports a better number for it. "It parsed" and "it parsed correctly" are
/// different statements and only the second one makes a throughput figure mean anything.
///
/// [`expected`](Cell::expected) is computed by the generator from the operands it emitted and the
/// associativity it wrote, in [`expected_expression`]. Deriving it by running the parser would
/// prove nothing — it would agree with any driver, including a broken one.
struct Cell {
  /// The criterion id within the `pratt/typed` group.
  name: String,
  /// The generated fixture, ~128 KiB.
  src: String,
  /// The fold checksum the drain must return, computed without the parser.
  expected: i64,
  /// Operators per expression: `0` for `bare_operand`, `d` for a chain cell.
  depth: usize,
  /// Expressions the generator emitted, as its own loop counted them.
  /// [`check_invariants`](Cell::check_invariants) cross-checks it against the bytes.
  expressions: usize,
  /// The associativity the chain was written with; `None` for the operator-free cell.
  assoc: Option<Assoc>,
}

impl Cell {
  /// One operand per operator, plus the head of the expression.
  fn operands(&self) -> usize {
    self.expressions * (self.depth + 1)
  }

  /// One operator per link — and, since [`fold_infix`] runs once per operator, one fold.
  fn operators(&self) -> usize {
    self.expressions * self.depth
  }

  /// Source tokens: every operand, every operator, and every terminator.
  fn tokens(&self) -> usize {
    self.operands() + self.operators() + self.expressions
  }

  /// The fixture facts the header's *What is and is not comparable* is written on top of,
  /// asserted against the generated bytes.
  ///
  /// Run from [`cells`], so both the criterion path and the fixed-iteration path get it. Cheap:
  /// one linear pass over ~128 KiB per cell, once per process.
  fn check_invariants(&self) {
    let seen = Census::of(&self.src);
    assert_eq!(
      seen.expressions, self.expressions,
      "pratt_typed: {}: the generator counted {} expressions but wrote {} terminators",
      self.name, self.expressions, seen.expressions
    );
    assert_eq!(
      seen.operands,
      self.operands(),
      "pratt_typed: {}: a depth-{} fixture over {} expressions must carry {} operands, not {}",
      self.name,
      self.depth,
      self.expressions,
      self.operands(),
      seen.operands
    );
    assert_eq!(
      seen.operators,
      self.operators(),
      "pratt_typed: {}: a depth-{} fixture over {} expressions must carry {} operators, not {}",
      self.name,
      self.depth,
      self.expressions,
      self.operators(),
      seen.operators
    );
    // The byte normalisation itself, which is what makes operand and token density per byte one
    // constant across the whole group rather than a per-cell number.
    assert_eq!(
      self.src.len(),
      BYTES_PER_OPERAND * self.operands(),
      "pratt_typed: {}: {} bytes over {} operands is not {BYTES_PER_OPERAND} bytes an operand, \
       so the cells no longer share a token density and the header's comparability section does \
       not hold",
      self.name,
      self.src.len(),
      self.operands()
    );
    assert_eq!(
      self.tokens(),
      2 * self.operands(),
      "pratt_typed: {}: token count is no longer twice the operand count",
      self.name
    );
    // "~128 KiB": the generator stops at the first expression that reaches TARGET, so the
    // fixture overshoots by less than one expression and never undershoots.
    let expression_bytes = BYTES_PER_OPERAND * (self.depth + 1);
    assert!(
      self.src.len() >= TARGET && self.src.len() < TARGET + expression_bytes,
      "pratt_typed: {}: {} bytes is not within one expression of the {TARGET}-byte target every \
       fixture in this repository is generated to",
      self.name,
      self.src.len()
    );
  }
}

/// The pair at one depth, asserted to differ in exactly the one way the header claims.
///
/// `right_chain/d` and `left_chain/d` are the unit of result in this group, and the whole of that
/// claim is that they are the same fixture with one byte per operator changed. Everything the
/// header lists as "identical" — length, operand values and positions, token count, operator
/// count, fold count — follows from the two assertions below and is therefore a fact of the built
/// fixture rather than a description of it.
///
/// The last assertion pins the checksum's **depth-1 blind spot** in both directions: a single
/// fold is the same value under either associativity, so the depth-1 pair must agree and every
/// deeper pair must not. A change to [`expected_fold`] that made it associative would delete the
/// associativity check from every depth at once and pass every other check in this file.
fn check_pair(right: &Cell, left: &Cell) {
  assert_eq!(
    right.src.len(),
    left.src.len(),
    "pratt_typed: {} and {} are different lengths, so they are not a pair",
    right.name,
    left.name
  );
  assert_eq!(
    right.depth, left.depth,
    "pratt_typed: {} and {} are not at the same depth",
    right.name, left.name
  );

  let mut differing = 0usize;
  for (i, (r, l)) in right
    .src
    .as_bytes()
    .iter()
    .zip(left.src.as_bytes())
    .enumerate()
  {
    if r != l {
      differing += 1;
      assert!(
        *r == b'^' && *l == b'+',
        "pratt_typed: {} and {} differ at byte {i} by something other than the operator \
         ({:?} against {:?}), so the pair is not byte-matched",
        right.name,
        left.name,
        *r as char,
        *l as char
      );
    }
  }
  assert_eq!(
    differing,
    right.operators(),
    "pratt_typed: {} and {} differ at {differing} bytes where the fixture has {} operators",
    right.name,
    left.name,
    right.operators()
  );

  if right.depth >= 2 {
    assert_ne!(
      right.expected, left.expected,
      "pratt_typed: {} and {} predict the same accumulator, so the checksum cannot tell a \
       flipped associativity from a correct parse at this depth",
      right.name, left.name
    );
  } else {
    assert_eq!(
      right.expected, left.expected,
      "pratt_typed: {} and {} predict different accumulators at depth {}, where one fold makes \
       the two orders coincide — so one of the two generators is not folding the way its \
       associativity says",
      right.name, left.name, right.depth
    );
  }
}

/// `1234 ;` — **zero** operators. Seven bytes per operand, one `parse_input` call per operand,
/// and nothing whatsoever to amortise a per-expression cost against.
///
/// The drain sums the per-expression results, and with no operator there is no fold: the expected
/// accumulator is the wrapping sum of the operands themselves.
///
/// Returns the source, its expected accumulator, and the number of expressions the loop emitted —
/// which [`Cell::check_invariants`] then checks against the bytes.
fn bare_operand_source() -> (String, i64, usize) {
  let mut s = String::with_capacity(TARGET + 64);
  let mut i = 0u32;
  let mut expected = 0i64;
  let mut expressions = 0usize;
  while s.len() < TARGET {
    let value = operand(i);
    let _ = writeln!(s, "{value} ;");
    expected = expected.wrapping_add(i64::from(value));
    expressions += 1;
    i = i.wrapping_add(1);
  }
  (s, expected, expressions)
}

/// `1234 OP 5678 OP … ;` with exactly `depth` operators per expression, and the accumulator the
/// driver must produce over it.
///
/// Called with [`Assoc::Right`] and with [`Assoc::Left`], and the two results are the same length
/// with the same operands in the same places: one byte per operator differs. That is the control,
/// and it is what the paired result at each depth is measured against.
///
/// The expected accumulator is built in the same loop as the bytes, from the same operand values,
/// before any of them is lexed: the source and its prediction cannot drift because there is only
/// one place either is produced.
fn chain_source(assoc: Assoc, depth: usize) -> (String, i64, usize) {
  let op = assoc.op();
  let mut s = String::with_capacity(TARGET + 64);
  let mut operands: Vec<i64> = Vec::with_capacity(depth + 1);
  let mut i = 0u32;
  let mut expected = 0i64;
  let mut expressions = 0usize;
  while s.len() < TARGET {
    operands.clear();
    let head = operand(i);
    operands.push(i64::from(head));
    let _ = write!(s, "{head}");
    i = i.wrapping_add(1);
    for _ in 0..depth {
      let value = operand(i);
      operands.push(i64::from(value));
      let _ = write!(s, " {op} {value}");
      i = i.wrapping_add(1);
    }
    let _ = writeln!(s, " ;");
    expected = expected.wrapping_add(expected_expression(&operands, assoc));
    expressions += 1;
  }
  (s, expected, expressions)
}

/// The depths the right/left pair is measured at.
///
/// Depth **1** is the break-even probe rather than part of the retention sweep — see the header —
/// but it carries its control like every other depth, because "the pair is the result" has no
/// exceptions in this file.
const DEPTHS: [usize; 5] = [1, 2, 8, 32, 64];

/// The cells, in group order: the criterion id, the fixture behind it, and the accumulator it must
/// produce. One list, used by both the criterion path and the fixed-iteration harness, so the two
/// cannot drift.
///
/// The chain cells are emitted **pair-adjacent** — `right_chain/d` immediately followed by
/// `left_chain/d` — because the pair at one depth is the unit of result here, and criterion prints
/// cells in the order they are registered. Two adjacent lines of its output are one measurement.
///
/// Every cell is put through [`Cell::check_invariants`] and every pair through [`check_pair`]
/// before this returns, so no path in this file — criterion or fixed-iteration — can measure a
/// fixture the header's comparability argument does not describe.
fn cells() -> Vec<Cell> {
  let (src, expected, expressions) = bare_operand_source();
  let mut out = vec![Cell {
    name: "bare_operand".to_string(),
    src,
    expected,
    depth: 0,
    expressions,
    assoc: None,
  }];
  for depth in DEPTHS {
    for assoc in [Assoc::Right, Assoc::Left] {
      let (src, expected, expressions) = chain_source(assoc, depth);
      out.push(Cell {
        name: format!("{}/{depth}", assoc.cell_prefix()),
        src,
        expected,
        depth,
        expressions,
        assoc: Some(assoc),
      });
    }
  }

  for cell in &out {
    cell.check_invariants();
  }
  // The chain cells are laid down right-then-left at each depth, so the pairs are the adjacent
  // windows after the lone `bare_operand`. Asserted rather than assumed: a reordering here would
  // otherwise silently compare a right chain against the wrong control.
  for pair in out[1..].chunks(2) {
    let [right, left] = pair else {
      unreachable!("pratt_typed: a chain depth was registered without its control")
    };
    assert_eq!(
      (right.assoc, left.assoc),
      (Some(Assoc::Right), Some(Assoc::Left)),
      "pratt_typed: {} and {} are not a right/left pair in that order",
      right.name,
      left.name
    );
    check_pair(right, left);
  }

  // The deepest right chain is the cell `BENCH_RECURSION_LIMIT` exists for, so it is the one both
  // halves of that constant's justification are run against.
  let deepest = out
    .iter()
    .filter(|cell| cell.assoc == Some(Assoc::Right))
    .max_by_key(|cell| cell.depth)
    .expect("pratt_typed: no right chain was generated");
  check_recursion_budget(deepest);

  out
}

// ── The recursion budget ──────────────────────────────────────────────────────
//
// `right_chain/64` does not merely mismeasure on the library default — it crashes the whole
// binary, so the ceiling gets its own named constant and configuration site rather than being
// buried in `parse_once`.

/// The recursion ceiling every parse in this file runs against, in place of the one a default
/// [`Parser`] installs.
///
/// # Why the default is not enough
///
/// The typed Pratt driver's private `parse` (`src/parser/pratt/expr.rs`) takes one level of the
/// recursion budget per call — an `InputRef::descend` in its frame prologue — and its `Infix` arm
/// recurses into `parse` again for a right-associative operator's right operand. So a chain of
/// `d` right-associative operators reaches depth `d + 1`, and `right_chain/64`, this file's
/// deepest fixture, is one past the ceiling `ParserContext` and the input layer install for a
/// Pratt-driven parse. Left uncorrected, the very first `right_chain/64` iteration returns
/// `RecursionLimitReached` (absorbed into `BenchError` by the `From` impl above), criterion's
/// `.unwrap()` on that `Err` panics during warm-up, and the panic takes the whole bench binary
/// down — every other cell included, measured or not. `left_chain/*` never approaches this at
/// any depth: a left-associative operator recurses under `PrattInfix::Left`'s `Exclusive`
/// floor, which declines the equal-power operator immediately, so the loop runs `d` times at a
/// constant depth of 2 rather than descending.
///
/// **That whole paragraph is a claim about another crate's default, so it is checked and not
/// asserted** — see [`check_recursion_budget`], which runs the deepest fixture under a default
/// [`Parser`] and requires it to fail, then under this ceiling and requires it to produce the
/// right accumulator. The figure the library actually installs is deliberately not named here:
/// it is `RecursionLimiter::PARSE_DEFAULT_DEPTH`, crate-private, and an earlier revision of this
/// comment named `RecursionLimiter::new` for it — which is a *different* default, and a larger
/// one.
///
/// # Why the fix is here, not in the fixture
///
/// This file's job is to time the drain around that driver; enforcing its recursion limit is
/// `tokora/tests/pratt_limit.rs`'s job. Shrinking the fixture to `right_chain/63` would sit
/// *exactly* on the ceiling and re-break on any future change to how frames are counted — e.g.
/// one more `descend()` added to a shared prologue. So the ceiling is raised instead, once, here.
///
/// # Why a bound and not `unlimited`
///
/// [`RecursionLimiter::unlimited`] would also fix `right_chain/64`, but this file prefers an
/// explicit ceiling: nothing here ever needs more than `max(DEPTHS) + 1`, and keeping a finite
/// (if generous) bound means a future regression that makes the driver recurse without limit
/// still fails with a catchable, reported `RecursionLimitReached` — not a raw native stack
/// overflow with no diagnosis. The headroom over the depth this file reaches is a `const`
/// assertion below rather than arithmetic in a sentence. At the other end, `2048` is comfortably
/// under the ~3871 frames the *typed* driver measured on a 2 MiB thread stack in a **release**
/// build, per [`RecursionLimiter`]'s own `Default Limit` section — the row that applies here,
/// since `[profile.bench]` in the workspace `Cargo.toml` sets `opt-level = 3` and
/// `debug-assertions = false`. That upper figure is another crate's recorded measurement and is
/// not re-derived here; the backstop it buys holds on a constrained thread, not only on a
/// generously sized one.
const BENCH_RECURSION_LIMIT: usize = 2_048;

// The headroom, as a compile-time fact rather than a sentence: the deepest fixture descends
// `max(DEPTHS) + 1` frames, and the ceiling has to be above that or every measured iteration of
// the deepest cell returns `RecursionLimitReached` instead of a parse.
const _: () = {
  let mut deepest = 0;
  let mut i = 0;
  while i < DEPTHS.len() {
    if DEPTHS[i] > deepest {
      deepest = DEPTHS[i];
    }
    i += 1;
  }
  assert!(
    BENCH_RECURSION_LIMIT > deepest + 1,
    "pratt_typed: BENCH_RECURSION_LIMIT is at or below the depth this file's deepest right \
     chain descends to. Raise it, or drop the depth from DEPTHS."
  );
};

/// The [`BENCH_RECURSION_LIMIT`] rationale, run rather than argued.
///
/// Its whole justification is "the default budget is not enough for the deepest cell, and this
/// one is" — two statements about a library default this file cannot name (it is crate-private)
/// and must not guess at. So both are executed once, at fixture-construction time, against the
/// real deepest fixture:
///
/// * under a default [`Parser`] the parse must **fail**. If it ever stops failing, the whole
///   constant is unnecessary and the paragraph explaining it is stale — which is exactly the
///   state an earlier revision of that paragraph was already in.
/// * under [`bench_parser`] it must succeed **and** produce the generator's accumulator, so a
///   ceiling that merely stops the panic without letting the fixture parse is not mistaken for
///   a fix.
fn check_recursion_budget(deepest: &Cell) {
  assert!(
    Parser::new()
      .apply(typed_pratt_drain)
      .parse_str(deepest.src.as_str())
      .is_err(),
    "pratt_typed: {} now parses under the default recursion budget, so BENCH_RECURSION_LIMIT \
     and everything its comment says about needing it are stale",
    deepest.name
  );
  assert_eq!(
    bench_parser()
      .apply(typed_pratt_drain)
      .parse_str(deepest.src.as_str())
      .ok(),
    Some(deepest.expected),
    "pratt_typed: {} does not parse to its expected accumulator under BENCH_RECURSION_LIMIT",
    deepest.name
  );
}

/// This file's `Parser`, built fresh per call exactly as `Parser::new()` was before it — the
/// only change is [`BENCH_RECURSION_LIMIT`] in place of the library's default budget. See that
/// constant's comment for why the default is not enough and why this value was chosen.
fn bench_parser<'inp>()
-> Parser<(), ExprLexer<'inp>, i64, FatalContext<'inp, ExprLexer<'inp>, BenchError>> {
  Parser::with_context(
    ParserContext::of(Fatal::of())
      .with_recursion_limiter(RecursionLimiter::with_limitation(BENCH_RECURSION_LIMIT)),
  )
}

/// One parse of `src` through the typed driver, checked against the accumulator the generator
/// predicted for it and returning it.
///
/// The single point at which every measurement in this file — criterion and fixed-iteration
/// alike — enters the driver, and the single point at which two specific failures stop the run:
/// a parse that did not consume its whole fixture, and a parse whose **fold count or fold
/// arithmetic** disagrees with what the generator wrote.
///
/// Those two, and not "did the work the fixture describes" in general. Nothing here re-derives a
/// frame count or a probe count, so a driver that skipped or cheapened the RHS `End` probe would
/// consume every terminator and match every accumulator (module header, *What is and is not
/// comparable*), and the accumulator has two blind spots of its own inside fold semantics — depth
/// 1's associativity, and the leading-operand permutation in [`expected_expression`]. Both are
/// pinned by [`check_pair`] so they cannot widen without failing the bench.
///
/// # The two checks, and why one was not enough
///
/// `expect` covers *consumption*: every fixture here is generated well-formed, so the only ways
/// to reach it are a driver regression or a fixture-construction change, and both must be loud
/// because criterion charges [`Throughput::Bytes`] for the whole source whether or not the parse
/// read it.
///
/// The comparison against [`Cell::expected`] covers *fold* semantics, which consumption does not.
/// Reaching the end of the input says the tokens went by; it says nothing about how many folds
/// ran, in what order, or over what. A driver that consumed every byte and skipped folds, flipped
/// an associativity, or short-circuited to a cheaper accumulated value would have satisfied the
/// old contract completely — while doing strictly less work and therefore reporting a *faster*
/// cell. That is the worst shape a benchmark bug can take, because the number improves.
///
/// # Why it is inside the measured region
///
/// It costs one integer compare and one never-taken branch per parse — [`mismatch`] is `#[cold]`
/// and `#[inline(never)]`, so the hot path is `cmp`/`b.ne` to an out-of-line block and nothing
/// else — against a parse of ~128 KiB. Putting it here rather than in a pre-flight pass means
/// **every measured iteration** is checked, warm-up included, on the criterion path and the
/// fixed-iteration path alike, and there is no configuration in which the bench measures a parse
/// nobody validated.
fn parse_once(src: &str, expected: i64) -> i64 {
  let got = bench_parser()
    .apply(typed_pratt_drain)
    .parse_str(black_box(src))
    .expect(
      "pratt_typed: a fixture did not parse to the end of its input. Every source this file \
       generates is well-formed and every expression is `;`-terminated, so this is a driver \
       regression or a fixture-construction change, not a bad input — see \
       `typed_pratt_drain` for why a missing or displaced terminator fails rather than ends \
       the drain.",
    );
  if got != expected {
    mismatch(src.len(), expected, got);
  }
  got
}

/// The failure arm of [`parse_once`]'s semantic check, kept out of line so the check itself is a
/// compare and a branch on the hot path.
#[cold]
#[inline(never)]
fn mismatch(bytes: usize, expected: i64, got: i64) -> ! {
  panic!(
    "pratt_typed: a fixture parsed to the end of its {bytes} bytes but produced the wrong \
     accumulator: expected {expected}, got {got}. The expectation is computed by the generator \
     from the operands it emitted and the associativity it wrote (`expected_expression`), never \
     by running the parser, so the parse consumed every token while folding differently than the \
     fixture describes — a skipped fold, a flipped associativity, or a changed fold value. If \
     `fold_infix` was changed on purpose, `expected_fold` is the other half and has to change \
     with it."
  )
}

// ── The criterion group ───────────────────────────────────────────────────────

fn typed_pratt_bench(c: &mut Criterion) {
  let cells = cells();

  let mut group = c.benchmark_group("pratt/typed");
  // No `measurement_time`/`warm_up_time` here, deliberately — see the module header.

  for cell in &cells {
    group.throughput(Throughput::Bytes(cell.src.len() as u64));
    group.bench_function(&cell.name, |b| {
      b.iter(|| black_box(parse_once(cell.src.as_str(), cell.expected)))
    });
  }

  group.finish();
}

criterion_group!(benches, typed_pratt_bench);

// ── The fixed-iteration harness ───────────────────────────────────────────────

/// A deterministic, criterion-free run of one cell: build the fixture once, parse it exactly
/// `iters` times, print a checksum.
///
/// Two runs at `N` and `2N` differ by exactly `N` parses of the same bytes, so their difference
/// is the workload with every fixed cost — process startup, fixture construction, the print —
/// subtracted exactly rather than estimated.
fn run_fixed(cell: &str, iters: u64) -> Result<(), String> {
  let Cell {
    name,
    src,
    expected,
    ..
  } = cells()
    .into_iter()
    .find(|candidate| candidate.name == cell)
    .ok_or_else(|| {
      let known: Vec<String> = cells()
        .into_iter()
        .map(|candidate| candidate.name)
        .collect();
      format!("unknown cell {cell:?}; known cells: {}", known.join(", "))
    })?;

  // Order-dependent and non-cancelling on purpose: an `^=` of a constant value folds to zero on
  // every even iteration count, which prints a checksum that proves nothing. Every parse folded
  // in here has already been checked against `expected` by `parse_once`; this checksum is the
  // A/B identity witness between two runs of this harness, not the correctness check.
  let mut checksum = 0i64;
  for _ in 0..iters {
    checksum = checksum
      .wrapping_mul(31)
      .wrapping_add(parse_once(src.as_str(), expected));
  }

  println!(
    "cell={name} iters={iters} bytes={} expected={expected} checksum={checksum}",
    src.len()
  );
  Ok(())
}

/// `TOKORA_PRATT_ITERS` selects the fixed-iteration harness; `TOKORA_PRATT_CELL` picks the cell
/// (default `bare_operand`). Unset, this is an ordinary criterion bench binary.
fn main() {
  match std::env::var("TOKORA_PRATT_ITERS") {
    Ok(raw) => {
      let iters: u64 = raw.trim().parse().unwrap_or_else(|_| {
        panic!("TOKORA_PRATT_ITERS must be a non-negative integer, got {raw:?}")
      });
      let cell = std::env::var("TOKORA_PRATT_CELL").unwrap_or_else(|_| "bare_operand".to_string());
      if let Err(message) = run_fixed(&cell, iters) {
        eprintln!("pratt_typed: {message}");
        std::process::exit(2);
      }
    }
    Err(_) => {
      benches();
      Criterion::default().configure_from_args().final_summary();
    }
  }
}
