//! Conformance test kit for custom [`Lexer`] implementations.
//!
//! tokora's input machinery (prefix replay after a checkpoint restore, cache
//! truncation, re-lexing a rewound region on demand) relies on the properties written
//! down in the [`Lexer`] contract. This module ships [`Harness`](crate::conformance::Harness),
//! a small builder that
//! **drives a lexer against that contract** and panics — with precise context — on the
//! first violation. It is a test tool: assert-with-context is the intended failure
//! mode, so custom-lexer authors run it from their own `#[test]`s.
//!
//! # What it checks
//!
//! **Trait tier** — driving the [`Lexer`] surface directly:
//!
//! 1. **Replay identity** — two fresh `L::new(src)` runs produce the identical
//!    token/error + span + slice sequence, to exhaustion. **Identical by value**: the whole
//!    [`Token`], the whole [`Token::Error`], never a rendering of either and never the token's
//!    [`kind`](Token::kind) standing in for the token. That is what
//!    [`run`](crate::conformance::Harness::run)'s two `PartialEq` bounds buy, and what a green
//!    from it did not mean before 0.10.0 — see `run`'s own docs for who pays.
//! 2. **State-resume faithfulness** — for *every* position `k`, capturing the lexer
//!    [`State`] there and resuming with
//!    `L::with_state(src, saved)` + [`bump`](crate::Lexer::bump) to `k`'s offset
//!    reproduces the original suffix from `k`. This is the prefix-replay assumption
//!    verbatim (position is threaded via `bump`, not encoded in `State`).
//! 3. **Monotone progress** — span starts are non-decreasing, every item's span is
//!    nonempty (`start < end`), and the run terminates within a generous multiple of
//!    the source length (an anti-hang guard so the kit itself never spins).
//! 4. **Sticky exhaustion** — after [`lex`](crate::Lexer::lex) returns `None`, further
//!    calls keep returning `None`; and the **span survives that exhaustion** —
//!    [`span`](crate::Lexer::span) keeps answering, well-formed, with the lexer's final
//!    position (at least the last item's end, at most the source length) and answers the same
//!    on every later call. The input layer reads that value at two sites, so an unspecified one
//!    is not academic: a `to`-shaped scan commits there at end of input, and the partial-input
//!    frontier reports `Incomplete(offset)` from it — the offset a refill driver resumes at.
//! 5. **Span / slice coherence** — every item's [`slice`](crate::Lexer::slice) equals
//!    the source over its [`span`](crate::Lexer::span), and spans lie within bounds.
//! 6. **Gap-free tiling** (optional, [`lossless`](crate::conformance::Harness::lossless)) — consecutive
//!    spans abut, the first starts at `0`, and the last ends at the source end. Off by
//!    default, since a syntactic lexer legitimately skips trivia.
//! 7. **Read-frontier purity** — [`read_frontier`](crate::Lexer::read_frontier) answers the same
//!    on every call within an item's window, and asking does not move the lexer. The *value* it
//!    reports is checked by the partial tier below, not here; this checks only that it is a read
//!    rather than a scan.
//!
//! **Integration tier** — driving an `Input` session through the machinery itself over
//! a fixed set of named, deterministic save/peek/drain/restore schedules
//! (`peek-heavy`, `save-early-restore-late`, `drain-then-restore-across-cache`,
//! `nested-savepoints`) and requiring every schedule to observe the layer the way the straight
//! drain did: the same **committed tokens** and the same **raised lexer errors**, both by value.
//! The straight drain's tokens are additionally required to equal the raw-lex tokens. No
//! randomness — the schedules are enumerated.
//!
//! The two arms are compared as two sequences rather than as one interleaved stream, and the
//! error arm is not cross-checked against the raw lexer. Both are facts about the input layer,
//! not weaker claims: a `peek` reports a refusal the moment it lexes the region, so where an
//! error sits *between* two committed tokens varies by schedule on a conforming lexer; and the
//! layer reports each refused region once, so a lexer that errors repeatedly over one span is
//! conforming while the layer raises one error. See `check_integration`.
//!
//! **Partial-input tier** (`run_partial`, a separate entry point for
//! `usize`-offset `str` / `[u8]` sources) — driving the input in
//! [`Partial`] mode over **every split point** and requiring
//! chunked-equivalence under the frontier rules: a non-final drain of each prefix yields a
//! **prefix of** the complete-parse items before the cut and always ends incomplete, while a
//! final drain of the whole source reproduces the complete parse **exactly**. Catches lexers that
//! are unfaithful under truncation (item identity depending on input beyond `(state, offset)`),
//! and it is what falsifies a wrong
//! [`read_frontier`](crate::Lexer::read_frontier) — including a wrong
//! [`SCAN_LOOKAHEAD`](crate::Token::SCAN_LOOKAHEAD) claim behind the logos
//! adapter.
//!
//! Both tiers that drive an `Input` are bounded by a single non-rewindable counter per input,
//! `LexTally`, and not by the item budgets their drain loops also carry. The rule is that **a
//! budget bounds only the loops that increment it**: between a drain loop and the `Lexer::lex` call
//! sit `InputRef::next`/`peek` and the scanner's own loop, neither of which the kit owns, and the
//! input layer builds a fresh lexer for every one of those calls — so an item budget per drain and
//! an attempt ceiling per lexer instance are both restarted by a loop underneath them. The private
//! `LexTally` carries the full statement, the argument for why the tier entry is the last boundary
//! available, and why the counter cannot be refunded by a checkpoint restore.
//!
//! An **item** is a committed token *or* a lexer error the input layer raised, and both halves are
//! load-bearing: the layer lexes bytes and then either commits or refuses, and truncation can turn
//! one into the other. A tier that compared only tokens could not see the error arm at all — a
//! refusal leaves no token behind, so a lexer that errors on a truncated prefix and tokenizes the
//! full input showed up as "nothing yielded yet", which is a legal prefix.
//!
//! Both arms are compared on **value**, and this is the rule for *every* tier rather than for
//! this one: a token contributes its kind, its span and the **token itself**; an error contributes
//! that it *was* an error, its span and the **payload itself**. Not a rendering — both
//! `run` and `run_partial` ask for `PartialEq` on the token and the error and compare the values,
//! because a `Debug` string is neither injective (two payloads can render alike, so a real drift
//! passes) nor stable (a rendering carrying an address or a counter fails on a conforming lexer).
//! Comparing only the token's *kind* had the same shape on the other arm: a payload decided by a
//! byte that has not arrived kept its kind and its span, so both runs passed while the value a
//! parser callback receives changed. The trait tier held the weaker key for one release longer
//! than the partial tier did, and that gap was #269.
//!
//! Nothing from the *diagnostic* channel is compared: no rendered message, no
//! [`Severity`](crate::emitter::Severity), no label stack.
//!
//! The non-final leg asks for a *prefix*, not equality, because **over-withholding is sound**: a
//! lexer that reports reading past what it emits withholds more than the pre-0.10.0 span rule did,
//! and one that reports [`Unbounded`](crate::ReadFrontier::Unbounded) withholds everything until
//! the stream is sealed. Requiring equality would fail every conforming lookahead lexer. The final
//! leg is where full equality is pinned. That relaxation is also why the items have to carry
//! errors: once the length may legitimately be short, "withheld" and "reported and thrown away"
//! are the same observation, and only an arm the harness *records* can be told apart.
//!
//! # Violation posture
//!
//! A failing check is a bug in the *lexer* (or a mismatch with the documented
//! contract), surfaced loudly. The kit never mutates the lexer's behavior; it only
//! observes and asserts.
//!
//! **The kit reports exactly what it established — no more and no less.** Those are one rule with
//! two halves, and both halves are enforced in the machinery every comparison is built from
//! rather than at the sites that draw verdicts from it.
//!
//! **No more: it never reports a failure that is a fact about a value type rather than about the
//! lexer.** Every verdict here rests on an equality the kit does not own. [`PartialEq`] promises
//! symmetry and transitivity but **not** reflexivity, so a payload holding an `f64` that can be
//! `NaN` is not equal to itself — and [`Eq`] and [`Ord`] *do* promise reflexivity while nothing
//! checks the promise, so a `Kind`, a span, an offset or a source slice can break it too. The
//! guarded population is therefore **every** equality any tier draws a verdict from, and it is
//! defined by construction rather than by a list: a comparison is built by `compare_by`, its
//! answer names the **component that decided it**, and building it is the only way to reach either
//! a verdict or a refusal. Once such a comparison has already failed, the kit asks whether *that*
//! operation is equal to itself, and a side whose is not gets a refusal tagged
//! `non-reflexive-payload` naming the component and the obligation, instead of a verdict whose
//! expected-vs-got renders identically on both sides (#295). The obligation is unchanged and is
//! the caller's: such a type hand-writes the impl that says what equality means for it.
//!
//! **No less: it never withholds a verdict it did establish.** Some differences rest on no caller
//! equality at all — a difference at the **discriminant** (an item that is a lexer error on one
//! side and a committed token on the other) or in item **count** — and a component comparison over
//! two values that each equal themselves is sound too, which is the ordinary case and the whole
//! population of honest failures. All of those are **conclusive**, and a conclusive difference
//! outranks an inconclusive one wherever both are available: the first inconclusive difference is
//! retained as a fallback while the search continues into the remaining components of the item,
//! the later positions of the stream, and the item count, and it is reported only if nothing
//! conclusive is found anywhere. `Ranking` carries that rule and the argument for it.
//!
//! Both halves were learned from the same failure, one direction at a time. Answering `false` and
//! then scanning the whole item for a non-reflexive value asked a different question from the one
//! the comparison had asked, and hid the truncation nonconformance the partial tier exists to
//! report behind a refusal saying the kit had convicted the lexer of nothing. Stopping at the
//! first difference reached hid a decisive item count behind a `NaN` at position 0, and a
//! corrupted `L::State` behind a `NaN` in the token beside it.

pub mod cache;
pub mod emitter;

#[cfg(all(test, feature = "logos_0_16", feature = "std"))]
mod cache_tests;

use core::cell::RefCell;
// `std` is `alloc` on a no-std build (`extern crate alloc as std`), and that is exactly the
// configuration `conformance` selects — the feature enables `alloc`, not `std`. So `Rc` comes
// through this alias and `RefCell` cannot: `cell` lives in `core`.
use std::{format, rc::Rc, string::String, vec, vec::Vec};

use crate::{
  Lexer, ReadFrontier, Slice, Source, Span, Token,
  cache::DefaultCache,
  emitter::Emitter,
  error::{Incomplete, MaybeIncomplete, token::UnexpectedTokenOf},
  input::{Complete, Cursor, Input, InputRef, Partial},
  span::Spanned,
};

/// Default anti-hang budget: a run may not exceed `8 * source_len + 64` items.
const DEFAULT_BUDGET_MULTIPLE: usize = 8;
/// Floor added to the budget so a short source still has generous headroom.
const BUDGET_FLOOR: usize = 64;
/// Full re-lexing passes of a source one drain of it may cost. See [`lex_attempt_ceiling`].
const ATTEMPTS_PER_DRAIN_MULTIPLE: usize = 4;
/// The largest per-unit multiple either budget knob honours —
/// [`Harness::budget_multiple`] and
/// [`CacheHarness::lex_attempts_multiple`](cache::CacheHarness::lex_attempts_multiple).
///
/// # A ceiling nobody arrives at is not a ceiling
///
/// Both knobs scale a per-source-unit multiple, and every ceiling either one feeds is compared
/// `>=` before its counter moves. Set one to [`usize::MAX`] and the ceiling it computes is
/// `usize::MAX` too — a value those comparisons *do* reach, but only after `usize::MAX` attempts,
/// which on a 64-bit target is not a refusal anybody is present for. The guard is then configured
/// into uselessness: an endless lexer spins for as long as the process lives, instead of reaching
/// the `lex-budget` refusal the knob's documentation promises. Under the `spent += 1; spent >
/// limit` order this section used to describe, it was worse than useless — the comparison
/// genuinely never held and the increment overflowed first — but a bound no run arrives at is the
/// same absent guard either way.
///
/// So the multiples are bounded here, and every ceiling derived from one is computed with checked
/// arithmetic (see [`Harness::budget`] and `CacheHarness::corpus`) so that the *product* cannot
/// silently saturate either. What the cap costs is the ability to certify a lexer that legitimately
/// emits more than 65536 items per source unit — which is not a lexer this kit can tell apart from
/// a nonterminating one, and saying so is the honest limit.
///
/// # A cap is enforced by refusing, not by clamping
///
/// Both knobs used to `clamp` to this value, and a clamp *is* a silent lowering of the caller's
/// budget. The kit then refuses the dense-but-finite lexer that fits the requested budget and not
/// the clamped one, reporting it as nonterminating — a conforming lexer rejected, from a setting
/// accepted without a word. Both knobs now panic above this cap and name themselves and this
/// number, so a caller who needs more is told the kit cannot certify it instead of being handed a
/// verdict about their lexer. Below `1` they still adjust silently: that direction only widens the
/// budget, and a wider budget cannot manufacture a failure.
///
/// # And a ceiling has to be one the arithmetic survives
///
/// The comparison is `>=` and it runs *before* the counter is incremented, on every counter here,
/// which is what keeps a ceiling at the top of a counter's range an enforced-but-useless one
/// rather than a destructive one: that many attempts pass and the next is refused. It used to be
/// `spent += 1; spent > limit`, which at such a ceiling overflowed before it compared — debug
/// panicked with the wrong message and release wrapped the count to zero and ran on.
///
/// The cap above keeps a *configured* ceiling in a range a run reaches. A *derived* one is kept in
/// range by width instead: [`lex_attempt_ceiling`] is quadratic in the source, so it computes and
/// counts in `u128` rather than in a `usize` it used to outgrow — see there for the ordinary lexer
/// over an ordinary source that a 32-bit `usize` was falsely refusing.
const MAX_BUDGET_MULTIPLE: usize = 1 << 16;

/// A conformance harness that drives a [`Lexer`] implementation `L` against the lexer
/// contract.
///
/// Build one over one or more source inputs, set any knobs, then call [`run`](Self::run).
/// `run` panics on the first contract violation with the input index, position,
/// operation, and expected-vs-got values; on success it returns normally. See the
/// [module docs](crate::conformance) for the full list of checks.
///
/// # Example
///
/// ```
/// use core::convert::Infallible;
/// use tokora::{Lexer, SimpleSpan, Source, Token};
/// use tokora::conformance::Harness;
///
/// // A tiny hand-rolled lexer: one token per byte, gap-free over the source.
/// #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// struct CharKind;
/// impl core::fmt::Display for CharKind {
///   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
///     f.write_str("char")
///   }
/// }
///
/// // `PartialEq` because `run` compares the token by VALUE (#269) — this is the one line
/// // the new bound costs a vocabulary that is plain data, which is nearly all of them.
/// #[derive(Clone, Debug, PartialEq)]
/// struct CharTok;
/// impl Token<'_> for CharTok {
///   type Kind = CharKind;
///   type Error = Infallible;
///   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
///   fn kind(&self) -> CharKind { CharKind }
///   fn is_trivia(&self) -> bool { false }
/// }
///
/// struct CharLexer<'a> { src: &'a str, start: usize, end: usize, state: () }
/// impl<'a> Lexer<'a> for CharLexer<'a> {
///   type State = ();
///   type Source = str;
///   type Token = CharTok;
///   type Span = SimpleSpan;
///   type Offset = usize;
///
///   fn new(src: &'a str) -> Self { Self { src, start: 0, end: 0, state: () } }
///   fn with_state(src: &'a str, state: ()) -> Self { Self { src, start: 0, end: 0, state } }
///   fn check(&self) -> Result<(), Infallible> { Ok(()) }
///   fn state(&self) -> &() { &self.state }
///   fn state_mut(&mut self) -> &mut () { &mut self.state }
///   fn into_state(self) -> () { self.state }
///   fn source(&self) -> &'a str { self.src }
///   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.start, self.end) }
///   fn slice(&self) -> &'a str { &self.src[self.start..self.end] }
///   fn lex(&mut self) -> Option<Result<CharTok, Infallible>> {
///     self.start = self.end;
///     if self.start >= self.src.len() { return None; }
///     let mut e = self.start + 1;
///     while e < self.src.len() && !self.src.is_char_boundary(e) { e += 1; }
///     self.end = e;
///     Some(Ok(CharTok))
///   }
///   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
///   fn bump(&mut self, n: &usize) { self.end += *n; }
/// }
///
/// // A gap-free per-byte lexer passes every check, including the lossless knob.
/// Harness::<CharLexer<'_>>::new("hello world").lossless().run();
/// ```
pub struct Harness<'inp, L>
where
  L: Lexer<'inp>,
{
  inputs: Vec<&'inp L::Source>,
  lossless: bool,
  budget_multiple: usize,
}

impl<'inp, L> Harness<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Creates a harness over a single source input.
  #[must_use]
  pub fn new(input: &'inp L::Source) -> Self {
    Self {
      inputs: vec![input],
      lossless: false,
      budget_multiple: DEFAULT_BUDGET_MULTIPLE,
    }
  }

  /// Creates a harness over many source inputs.
  #[must_use]
  pub fn over<I>(inputs: I) -> Self
  where
    I: IntoIterator<Item = &'inp L::Source>,
  {
    Self {
      inputs: inputs.into_iter().collect(),
      lossless: false,
      budget_multiple: DEFAULT_BUDGET_MULTIPLE,
    }
  }

  /// Adds another source input to the corpus (builder style).
  #[must_use]
  pub fn and_input(mut self, input: &'inp L::Source) -> Self {
    self.inputs.push(input);
    self
  }

  /// Additionally requires **gap-free tiling**: consecutive spans abut (`end` equals
  /// the next `start`), the first span starts at `0`, and the last span ends at the
  /// source end. Off by default — a syntactic lexer that skips trivia legitimately
  /// leaves gaps, so only enable this for a lossless lexer.
  #[must_use]
  pub fn lossless(mut self) -> Self {
    self.lossless = true;
    self
  }

  /// Overrides the anti-hang budget multiple: a run may produce at most
  /// `multiple * source_len + 64` items before the kit declares the lexer
  /// non-terminating. The default is `8`.
  ///
  /// Values below `1` are treated as `1`. That *raises* the budget above what was asked for, so it
  /// cannot turn a conforming lexer into a failure, which is why it stays a silent adjustment.
  ///
  /// Values above `65536` **panic**. See below — that direction is not symmetric.
  ///
  /// # Why the maximum is a refusal and not a clamp
  ///
  /// The cap itself is not tidiness, and what an uncapped `usize::MAX` would do is **three
  /// separate failures** rather than the single "the guards stop working" this paragraph used to
  /// claim. A budget of `usize::MAX` feeds three different guards and breaks each one differently:
  ///
  /// - **The item guards go out of reach, and only those.** `out.len() > budget` cannot hold when
  ///   `budget` is `usize::MAX`, because no `Vec` holds that many items. This is the one guard the
  ///   old wording described correctly.
  /// - **The per-instance ceiling *overflows*.** It is `budget + 1`, so at `usize::MAX` it wraps
  ///   to `0` in release and panics with an arithmetic message in debug. A per-instance ceiling of
  ///   zero is not an inert guard — it refuses every lexer on its **first** attempt. The failure
  ///   is the opposite of the one the old wording named.
  /// - **The aggregate tally still fires.** It compares `spent >= limit` before it increments and
  ///   counts in `u128`, over a ceiling that is *derived* from the budget rather than equal to it.
  ///   So it is enforced — just at a number no run reaches while anybody is waiting, which is a
  ///   *useless* ceiling and not an absent one.
  ///
  /// None of the three is the reason the cap is a **refusal**. That reason is below, and it is a
  /// different problem again: it is about what a clamp does to a caller the kit accepts.
  ///
  /// But *clamping* to the cap is the wrong way to enforce it, because a clamp is silent and it
  /// hands back a **smaller budget than the caller asked for**. The lexer contract permits finite
  /// density above the cap, so the clamp had a reachable victim: on a one-unit source
  /// `budget_multiple(65_601)` should allow 65,665 items, the clamp allowed 65,600, and a legal
  /// lexer that emits 65,601 errors and then `None` was refused on its exhaustion probe by
  /// [`run_partial`](Self::run_partial) while [`run`](Self::run) reported it as possibly
  /// nonterminating. Both are the same outcome — a conforming lexer rejected — reached from a
  /// setting the builder accepted without a word, which is the failure a test kit can least
  /// afford: the caller reads it as a verdict on their lexer.
  ///
  /// Above the cap this kit genuinely cannot tell a dense lexer from a nonterminating one. The
  /// honest answer to a caller who needs more is to say that, at the call site, rather than to
  /// certify a different budget and report the difference as their bug.
  ///
  /// # Panics
  ///
  /// Panics when `multiple` exceeds `65536`, naming this knob and that maximum. The panic is
  /// raised by the builder, before any lexing, so it can never be mistaken for a `lex-budget`
  /// refusal of the lexer under test.
  #[must_use]
  pub fn budget_multiple(mut self, multiple: usize) -> Self {
    let cap = MAX_BUDGET_MULTIPLE;
    assert!(
      multiple <= cap,
      "tokora conformance: Harness::budget_multiple is capped at {cap} items per source unit and \
       was given {multiple}. The request is refused rather than lowered to {cap}: a silent clamp \
       certifies a budget you did not ask for, and a lexer that legitimately emits more than the \
       clamped budget allows is then reported as nonterminating — a failure that reads as the \
       lexer's and is the kit's. Above {cap} per unit this kit cannot tell a dense lexer from an \
       endless one."
    );
    self.budget_multiple = multiple.max(1);
    self
  }

  /// The anti-hang budget for `src`: `budget_multiple * source_units + BUDGET_FLOOR`.
  ///
  /// Checked, not saturating. Saturation here would hand back [`usize::MAX`], and the guards built
  /// out of this number do not all survive that value: [`instance_ceiling`] adds one to it and
  /// **overflows** — wrapping to a ceiling of `0` in release, which refuses every lexer on its
  /// first attempt — and the item guards `out.len() > budget` become comparisons no `Vec` length
  /// can satisfy. (The aggregate tally is unaffected: it derives its own `u128` ceiling from this
  /// number rather than using it, so it would still fire.) One broken guard is enough, and a
  /// budget that has stopped describing the source it came from is not a budget in any case.
  /// [`MAX_BUDGET_MULTIPLE`] already bounds the multiple; this bounds the product, which on a
  /// 32-bit target the multiple alone does not.
  ///
  /// # Panics
  ///
  /// Panics when `budget_multiple * source_units + 64` does not fit in a `usize` with room above
  /// it for the exhaustion probe. Reaching it needs a source of more than `usize::MAX / 65536`
  /// units — 256 TiB on a 64-bit target — so what this replaces is not a run that used to work: it
  /// is a budget silently swapped for one the caller never asked for.
  fn budget(&self, src: &'inp L::Source) -> usize {
    let units = src.slice(..).map(|s| s.len()).unwrap_or(0);
    representable_budget(self.budget_multiple, units).unwrap_or_else(|| {
      panic!(
        "tokora conformance: the anti-hang budget for a source of {units} units at a multiple of \
         {} does not fit in a usize. This is a limit of the kit's arithmetic and not a verdict on \
         the lexer, which has not been run. Lower the multiple with Harness::budget_multiple: an \
         unrepresentable budget has to be replaced by some other number, and every replacement is \
         wrong — usize::MAX overflows the per-instance ceiling derived from it and puts the item \
         guards past any Vec length, and anything smaller certifies a budget you did not ask for.",
        self.budget_multiple
      )
    })
  }
}

impl<'inp, L> Harness<'inp, L>
where
  L: Lexer<'inp>,
  // Semantic equality on the two values every trait-tier comparison is drawn from. These are the
  // only bounds this entry point asks for beyond `Lexer` itself; see `run`'s own docs for who
  // pays and what the alternatives cost, and `Item::compare` for why a `Debug` rendering and a
  // `kind` projection are each the wrong key.
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  /// Runs every check against every input, panicking on the first violation.
  ///
  /// # Who pays for `PartialEq`, and what a green means without it
  ///
  /// This asks for `L::Token: PartialEq` and `<L::Token as Token>::Error: PartialEq`. Before
  /// 0.10.0 it asked for nothing beyond [`Lexer`], and **that is what the defect was** (#269):
  /// check 1 is documented as identity of the item, and with no equality to call the kit
  /// substituted the two things it could reach — the error's `Debug` rendering and the token's
  /// [`kind`](Token::kind). Neither is an equality relation. A rendering is not injective, so two
  /// unequal error values that print alike replayed green; it is not stable either, so a payload
  /// rendering a counter or an address reddened a *conforming* lexer; and a kind is a projection,
  /// so a token payload could move between two fresh runs with the kind, the span and the slice
  /// all holding still. A certificate that says "identical" and checks something weaker is worth
  /// less than no certificate, because the caller stops looking.
  ///
  /// The alternative to the bound is a caller-supplied comparator or key, and
  /// [`run_partial`](Self::run_partial) already settled that trade for the other tier: a
  /// hand-written projection is silently escaped by the next field added, which is the failure
  /// mode the rendering had. The bound is the narrower obligation and the stronger guarantee.
  ///
  /// **What it excludes.** A vocabulary whose token or error type cannot be `PartialEq` — a
  /// payload holding an `Arc<dyn Error>`, a closure, a handle — loses this entry point outright,
  /// and with it the whole kit, since every tier hangs off `run` or
  /// [`run_partial`](Self::run_partial) and the latter already required both bounds. Recovering
  /// it costs a `#[derive(PartialEq)]`, or a hand-written impl where derivation is wrong, or a
  /// newtype whose equality is the one the vocabulary means. Nothing about the *lexer* has to
  /// change. There is no in-tree vocabulary among the excluded: this crate ships no concrete
  /// [`Token`] implementation at all, and the bundled logos adapter is transparent —
  /// `LogosLexer<T>` takes its `Token` and `Error` from the caller and constrains neither.
  ///
  /// One caveat that comes with value equality rather than with this kit, and it is the same one
  /// [`run_partial`](Self::run_partial) carries: a payload holding an `f64` that can be `NaN` is
  /// not equal to itself, and such a type must hand-write an impl that says what it means. Until
  /// it does, this kit will not pretend to have checked it — it refuses, tagged
  /// `non-reflexive-payload`, rather than reporting the lexer as non-conforming.
  ///
  /// # Panics
  ///
  /// Panics — with the offending input index, position, operation, and expected-vs-got
  /// values — the moment a contract check fails. Returns normally on full conformance.
  pub fn run(&self) {
    assert!(
      !self.inputs.is_empty(),
      "tokora conformance: Harness has no inputs; construct it with `new`/`over` over at least one source"
    );
    for (idx, &src) in self.inputs.iter().enumerate() {
      let budget = self.budget(src);

      // Trait tier: capture a reference run (this enforces monotone progress, nonempty
      // spans, span/slice coherence, in-bounds spans, and the anti-hang budget inline).
      let reference = lex_run::<L>(idx, src, budget);

      // 1. Replay identity: a second fresh run must match the reference exactly.
      let replay = lex_run::<L>(idx, src, budget);
      assert_run_eq::<L>(idx, "replay-identity", &reference, &replay);

      // 4. Sticky exhaustion, and the span that must survive it.
      check_sticky::<L>(idx, src, budget);
      check_span_after_exhaustion::<L>(idx, src, budget);

      // 2. State-resume faithfulness (every position k).
      check_resume::<L>(idx, src, &reference, budget);

      // 6. Gap-free tiling (opt-in).
      if self.lossless {
        check_lossless::<L>(idx, src, &reference);
      }

      // Integration tier: the input machinery over deterministic schedules.
      check_integration::<L>(idx, src, &reference, budget);
    }
  }
}

impl<'inp, L> Harness<'inp, L>
where
  L: Lexer<'inp, Offset = usize>,
  L::State: Clone,
  // A prefix of the source is itself a `&L::Source` — true for the byte/`str` sources partial
  // parsing targets (`str[..k] : str`, `[u8][..k] : [u8]`). This is what lets the check feed the
  // lexer honest truncations without a growable source.
  L::Source: core::ops::Index<core::ops::RangeTo<usize>, Output = L::Source>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
  // Semantic equality on the compared values. Since 0.10.0 `run` asks for these two as well, so
  // what separates the tiers is the three above and nothing about equality. See
  // `StreamItem::compare` for why a `Debug` rendering cannot stand in, and `run_partial`'s own
  // docs for who pays.
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  /// Runs the **partial-input (Sans-I/O) chunked-equivalence** check against every input.
  ///
  /// A separate entry point from [`run`](Self::run) because it needs a `usize`-offset,
  /// prefix-sliceable source (`str` / `[u8]`) and drives the input in
  /// [`Partial`] mode. For every split point `k` of each source it verifies that a **non-final**
  /// drain of the prefix `src[0..k]` yields a **prefix of** the complete-parse items lying
  /// strictly before `k` and always terminates as incomplete, while a **final** drain of the whole
  /// source reproduces the complete parse **exactly**. Together these are the chunked-equivalence
  /// guarantee: reassembling the chunk-by-chunk prefixes reproduces the single-shot parse.
  ///
  /// The sequence is **tokens and lexer errors interleaved**, not tokens alone: refusing a region
  /// is as much a decision about bytes as tokenizing it, and turning one into the other on append
  /// is precisely the unfaithfulness this tier exists to reject. Both arms are compared on
  /// discriminant, span and **value** — the whole [`Token`], the whole
  /// [`Token::Error`] — never on a rendering. Nothing from the diagnostic
  /// channel is compared. See the [module docs](crate::conformance) for what that does and does not
  /// assert.
  ///
  /// # Who pays for `PartialEq`, and what it buys
  ///
  /// This entry point requires `L::Token: PartialEq` and `<L::Token as Token>::Error: PartialEq`.
  /// It asked for them one release before [`run`](Self::run) did, which is why the argument is
  /// written out here; it is a deliberate choice between the two ways to compare a value the kit
  /// does not own:
  ///
  /// - a **bound**, as here: the caller derives `PartialEq` — one line for the overwhelming
  ///   majority of vocabularies, which are plain data — and the comparison is then *total*. Every
  ///   field participates, including one added next year, so no future payload can slip past the
  ///   check by being invisible to it.
  /// - a **caller-supplied key or comparator**: no bound, so a token that genuinely cannot be
  ///   `PartialEq` stays testable — but the key is a *projection* written by hand, the next field
  ///   added escapes it silently, and a key that omits the drifting field re-opens exactly the hole
  ///   this closes. That failure mode is the one the `Debug` rendering already had.
  ///
  /// The bound is the narrower of the two obligations in practice and the stronger guarantee, so it
  /// is what this asks for. What still separates this entry point from [`run`](Self::run) is
  /// [`Offset = usize`](crate::Lexer::Offset), a prefix-sliceable source and `L::State: Clone` —
  /// all required here and not there. Equality is no longer part of that list: `run` compared an
  /// error's `Debug` rendering and a token's kind until 0.10.0, and reported an identity it had
  /// not checked for it (#269), so both entry points now ask for the same two bounds.
  ///
  /// **A vocabulary with neither** loses the kit. Recovering it costs a `#[derive(PartialEq)]`,
  /// or a hand-written impl where derivation is wrong.
  /// One caveat that comes with value equality rather than with this kit: a payload holding a
  /// `f64` that can be `NaN` is not equal to itself, and such a type must hand-write an impl that
  /// says what it means. The kit does not silently accept one — a comparison it decides is
  /// refused, tagged `non-reflexive-payload`, rather than reported as a conformance failure of
  /// the lexer (#295).
  ///
  /// This is where a lexer that is not faithful under truncation is caught — one whose item
  /// identity depends on input beyond what it reports having read (lookahead past a token that
  /// [`read_frontier`](crate::Lexer::read_frontier) does not admit to, or a
  /// [`with_state`](crate::Lexer::with_state) + [`bump`](crate::Lexer::bump) resume that does not
  /// reproduce the suffix) diverges from the complete prefix and trips this check.
  ///
  /// # Why the non-final leg asks for a prefix, not equality
  ///
  /// Because **over-withholding is sound and equality is not attainable**. A lexer that honestly
  /// reports reading past what it emits withholds more items than the pre-0.10.0 span rule did,
  /// and one that reports [`Unbounded`](crate::ReadFrontier::Unbounded) — the answer the bundled
  /// logos adapter gives by default — withholds every item until the stream is sealed. Both
  /// converge, because a refill strictly grows the buffer and sealing ends the game. Requiring
  /// equality would therefore reject every conforming lookahead lexer while testing precision
  /// rather than correctness. Yielding an item the complete parse does not have there, or a
  /// different one, still fails, and the final leg still pins full equality.
  ///
  /// # Panics
  ///
  /// Panics — tagged `partial-equivalence`, with the input index, split point, and expected-vs-got
  /// — on the first divergence. Returns normally on full conformance.
  pub fn run_partial(&self) {
    assert!(
      !self.inputs.is_empty(),
      "tokora conformance: Harness has no inputs; construct it with `new`/`over` over at least one source"
    );
    for (idx, &src) in self.inputs.iter().enumerate() {
      let budget = self.budget(src);
      check_partial::<L>(idx, src, budget);
    }
  }
}

/// The **one** bound on raw lexing the kit has, and the invariant that says why it sits here.
///
/// The counter lives in this private submodule so that its field is unreachable from the rest of
/// the kit; [`LexTally`](tally::LexTally) is the type, re-exported below.
///
/// > **Every [`Lexer::lex`] attempt the kit causes through the input layer, anywhere beneath one
/// > tier's work on one input, is charged to one `LexTally`. The tally is non-rewindable: its only
/// > mutator increments it, and no lexer construction, state clone, cache entry or checkpoint
/// > restore returns capacity to it. Every narrower budget — items per drain, attempts per lexer
/// > instance — exists for a sharper message and is not the bound.**
///
/// # A budget bounds only the loops that increment it
///
/// That sentence is the whole rule, and this counter is where four rounds of getting it wrong
/// ended up. The kit lexes in two shapes:
///
/// - **directly**, in [`lex_run`], [`check_sticky`], [`check_span_after_exhaustion`],
///   [`check_resume`] and [`CacheHarness::corpus`](cache::CacheHarness) — there the counter and the
///   `lex()` call are in the *same loop body*, so the counter is the loop's own trip count and
///   bounds it by construction. Those need nothing from here.
/// - **through the input layer**, where the nesting is `Harness` → prefix or schedule → drain loop
///   → [`InputRef::next`](crate::InputRef::next) / [`peek`](crate::InputRef::peek) → the scanner's
///   `while let Some(item)` loop → `lex()`. The kit owns the outermost loops and the innermost
///   call and **none of the ones in between**. Every guard placed on one of those boundaries was
///   escaped by a loop underneath it:
///
///   | guard at | escaped by |
///   |---|---|
///   | one clear per `lex` | `logos` resolving `Skip` with several scans inside one `lex` |
///   | items per `next()` | the scanner looping over the lexer errors it accepts, which the span-end dedup keeps out of the log |
///   | attempts per lexer instance | the layer building a **fresh lexer for every `next()`**, so the counter restarts on every call |
///
///   The third is the trap this type exists to close: a per-instance counter *looks* like it is
///   "inside `next()`", and it is — but a fresh instance per call makes it a per-call counter, and
///   a lexer that spends `limit - 1` attempts before each token walks a `run_partial` corpus in
///   Θ(n³) raw lex work with the item budget flat (the dedup hides the repeats) and the instance
///   budget reset (a new lexer holds it).
///
/// # Why this boundary is the last one
///
/// Two reasons, and the second is the one that matters.
///
/// **There is no loop left above it that lexer behaviour can drive.** The tally is created once per
/// input by the tier entry ([`check_partial`], [`check_integration`]); the only loop above that is
/// `for (idx, src) in self.inputs`, whose trip count is the caller's own input list — data, not
/// behaviour. Everything below it — prefixes, schedules, drains, `next`/`peek` calls, lexer
/// instances, scanner iterations — charges the same counter.
///
/// **And there is nothing left below it to subdivide.** Each guard before this one counted a
/// *proxy* for lexer work — probe clears, items a drain yielded, attempts by one instance — and
/// every proxy had a finer loop underneath it to hide in, which is why the same defect kept
/// reappearing one boundary further out. This counts [`Lexer::lex`] itself, which is the primitive
/// the kit is trying to bound: there is no finer operation for a loop to sit between it and the
/// counter, because the counter *is* at the operation. Raw lexing is also the only unbounded
/// resource down there — a token served from the cache costs no lex, and every other observable the
/// layer reads (`span`, `slice`, `read_frontier`, `bump`) is called a bounded number of times per
/// attempt or per item.
///
/// What that leaves open is only *where the handle is created*, and that is a parameter rather than
/// a mechanism. If a loop is ever added above a tier entry — a repeat knob, a second corpus pass —
/// the repair is to construct the tally one level further out and pass the same handle down. The
/// four relocations before this one each needed a new mechanism; this one would need an argument
/// moved.
///
/// # What it still cannot bound, and why nothing could
///
/// A [`Lexer::lex`] call that never returns. Every lever the kit has is *between* calls, so a lexer
/// that loops inside one is beyond this counter and beyond any counter a kit could hold. What this
/// bounds is the case that looks identical from outside and is not: a lexer that returns promptly,
/// every time, forever.
///
/// # Why it cannot be refunded
///
/// The count lives in a `Cell` behind an [`Rc`] and the handle rides in the lexer's `State`
/// ([`Tallied`]), which is the one channel [`Lexer::with_state`] has. Everything the input layer
/// does to a state — clone it into a [`CachedToken`](crate::cache::CachedToken), stash it in a
/// checkpoint, restore an older one, hand it to a rebuilt lexer — copies the **handle**, and every
/// handle addresses the one cell. So a rollback rewinds the lexer and cannot rewind the tally:
/// what it restores is a pointer, not the count.
///
/// That is the difference from a tally kept *as a field* of the thing being rolled back, which is
/// refunded by the rollback and is therefore not a budget at all.
///
/// # Monotone by construction, not by convention
///
/// The count is a private field of a type in a **private submodule**, so "nothing writes it but
/// [`spend`](LexTally::spend)" is enforced by the compiler rather than asserted in this comment:
/// outside `tally`, the only reachable operations are constructing one, charging one attempt, and
/// reading the total. There is no reset and no setter to reach.
///
/// Nor is there a constructor reachable from a lexer: [`Lexer::new`] is handed only a source, so
/// [`Budgeted::new`] cannot obtain a tally — it builds an already-spent one, so a drive the kit did
/// not seed refuses on its first attempt instead of running unbounded.
mod tally {
  use core::cell::Cell;

  use super::{AttemptCeiling, Rc};

  /// See the module-level block above this `mod` for the invariant this type carries.
  pub(super) struct LexTally {
    /// Attempts charged so far. Unreachable outside this module, which is what makes
    /// [`spend`](Self::spend) the only writer.
    ///
    /// `u128` rather than `usize` because the ceiling it is compared against is quadratic in the
    /// source while a `usize` is not — see [`lex_attempt_ceiling`](super::lex_attempt_ceiling).
    /// A counter narrower than its own ceiling is a counter that decides verdicts by target width.
    spent: Cell<u128>,
    /// The ceiling, fixed at construction.
    limit: u128,
    /// Whether `limit` is the derived bound or the kit's own counting capacity — the thing that
    /// decides whether exceeding it is a statement about the lexer or about this kit. See
    /// [`AttemptCeiling`](super::AttemptCeiling).
    capacity_bound: bool,
    /// The tier's early-refusal ceiling for a *single* lexer instance, fixed at construction.
    ///
    /// It rides on the tally because the tally is the only thing that reaches a lexer the kit
    /// never constructs — the same channel argument the count itself rests on — and because that
    /// is what makes it a property of the *tier's configured budget* rather than of whichever
    /// prefix the instance happens to be lexing. See [`Budgeted`](super::Budgeted).
    per_instance: usize,
    /// Source units, so the refusal can say what the ceiling was derived from.
    units: usize,
    /// The input index, so the refusal reads like every other message the kit prints.
    idx: usize,
    /// Which tier's work this tally covers.
    tier: &'static str,
  }

  impl LexTally {
    /// A tally for one tier's work on one input.
    pub(super) fn new(
      idx: usize,
      tier: &'static str,
      units: usize,
      ceiling: AttemptCeiling,
      per_instance: usize,
    ) -> Rc<Self> {
      Rc::new(Self {
        spent: Cell::new(0),
        limit: ceiling.limit(),
        capacity_bound: matches!(ceiling, AttemptCeiling::Capacity),
        per_instance,
        units,
        idx,
        tier,
      })
    }

    /// A tally that starts with `spent` attempts already charged — **test-only**, and the one
    /// thing in this module that is not reachable from a shipped build.
    ///
    /// It is a *constructor*, not a writer, which is why it does not weaken the wall above: it
    /// mints a new count, so it cannot return capacity to a tally that already exists, and every
    /// handle a run holds still addresses the one cell it was built with. Nothing outside gains a
    /// reset or a setter.
    ///
    /// It exists because the boundaries [`spend`](Self::spend) is asserted at sit at
    /// [`u128::MAX`], and the only way to reach that count by charging it is to charge it
    /// `u128::MAX` times. A regression that cannot be run is not a regression.
    #[cfg(test)]
    pub(super) fn preloaded(
      idx: usize,
      tier: &'static str,
      units: usize,
      spent: u128,
      ceiling: AttemptCeiling,
      per_instance: usize,
    ) -> Rc<Self> {
      Rc::new(Self {
        spent: Cell::new(spent),
        limit: ceiling.limit(),
        capacity_bound: matches!(ceiling, AttemptCeiling::Capacity),
        per_instance,
        units,
        idx,
        tier,
      })
    }

    /// Attempts charged so far. Read-only: there is no counterpart that writes it.
    pub(super) fn spent(&self) -> u128 {
      self.spent.get()
    }

    /// The ceiling this tally was built with.
    pub(super) fn limit(&self) -> u128 {
      self.limit
    }

    /// The single-instance early-refusal ceiling this tally was built with.
    pub(super) fn per_instance(&self) -> usize {
      self.per_instance
    }

    /// Charges one [`Lexer::lex`](crate::Lexer::lex) attempt. The only mutator, and it only
    /// increments.
    ///
    /// # The check comes before the increment, which is what makes the arithmetic total
    ///
    /// `spent += 1; if spent > limit` cannot be evaluated at all when `limit` is the largest
    /// value the counter holds — the increment overflows before the comparison is reached — so
    /// such a ceiling is not merely one the count stops short of, it is *destructive*. That was a
    /// ceiling the kit handed itself while both were `usize`:
    /// [`lex_attempt_ceiling`](super::lex_attempt_ceiling) saturated to `usize::MAX`, which a
    /// 32-bit target reached at the default multiple over an 11,580-unit source.
    ///
    /// The failure was profile-dependent and release held the bad arm. Debug panicked on the
    /// overflow: the wrong message, but a stop. Release **wrapped the count to zero** and let the
    /// run carry on from there, wrapping again as often as the work asked, with no `lex-budget`
    /// refusal ever printed — precisely the switched-off guard the tally exists to prevent,
    /// arrived at by arithmetic instead of by configuration.
    ///
    /// Asking `spent >= limit` first and incrementing only on the allowed path *removes* the
    /// overflow rather than making it unlikely: the `+ 1` runs only when `spent < limit`, so its
    /// result is at most `limit`, which is at most [`u128::MAX`]. The allowance is unchanged —
    /// `limit` attempts pass and the `limit + 1`-th refuses — so no lexer's verdict moves. The
    /// order is kept even though the ceiling is now `u128` and
    /// [`AttemptCeiling`](super::AttemptCeiling) is exact, because a counter that stays total only
    /// because of a bound proved somewhere else is the one that breaks when that bound moves.
    ///
    /// # Two refusals, because there are two things that can be exceeded
    ///
    /// A [`Derived`](super::AttemptCeiling::Derived) ceiling is the tier's budget, and exceeding
    /// it is a statement about the lexer: it did more raw work than the configured budget allows.
    /// A [`Capacity`](super::AttemptCeiling::Capacity) ceiling is `u128::MAX` because the
    /// derivation did not fit, and exceeding it is a statement about **this kit** — the counter
    /// ran out, and nothing was learned about the lexer either way.
    ///
    /// They are different verdicts, so they are different messages under different tags. Printing
    /// the second as the first is the kit blaming the lexer for the kit's own limit, and a caller
    /// who reads `lex-budget` reasonably takes it as a verdict on their code.
    ///
    /// # Panics
    ///
    /// Panics on the attempt that would exceed the ceiling — tagged `lex-budget` when the ceiling
    /// is the derived bound, and `kit-capacity`, reported as inconclusive, when it is the kit's
    /// counting capacity. The `lex-budget` arm is the intended failure mode: the alternative is
    /// not a passing run, it is a run that does not end, or ends after cubic work.
    pub(super) fn spend(&self) {
      let spent = self.spent.get();
      if spent >= self.limit {
        self.refuse_aggregate();
      }
      self.spent.set(spent + 1);
    }

    /// The aggregate refusal, in whichever of its two kinds this tally's ceiling calls for.
    ///
    /// Split out of [`spend`](Self::spend) so the hot path is the comparison and the increment,
    /// and so the two messages sit beside each other where the difference between them is legible.
    fn refuse_aggregate(&self) -> ! {
      let (idx, tier, limit, units) = (self.idx, self.tier, self.limit, self.units);
      if self.capacity_bound {
        panic!(
          "tokora conformance [input #{idx} {tier} kit-capacity] INCONCLUSIVE — this is a limit \
           of the conformance kit and NOT a verdict on the lexer. The aggregate attempt ceiling \
           this tier derives from its budget over {units} source units does not fit in the u128 \
           the tally counts in, so the kit counted to {limit} attempts and stopped. Nothing here \
           says the lexer failed to terminate, exceeded its budget or broke the contract: the run \
           was abandoned before any of those could be decided. Certify this lexer over a shorter \
           source — the ceiling is quadratic in the source length, so a shorter one is \
           representable long before the lexer's behaviour changes."
        );
      }
      panic!(
        "tokora conformance [input #{idx} {tier} lex-budget] the lexer was asked to lex more \
         than {limit} times over a source of {units} units. This counts every raw Lexer::lex \
         attempt the whole tier made, across every lexer instance the input layer built, in \
         every prefix, schedule and checkpoint lineage — because each of those is a loop the \
         kit does not own, and a per-call or per-instance ceiling resets inside them. The lexer \
         errors one InputRef::next consumes internally are counted too: the span-end dedup keeps \
         repeats out of the item log, so the item budget never sees them. Spans must be monotone \
         and nonempty and the lexer must exhaust."
      );
    }

    /// The refusal for [`Budgeted`](super::Budgeted)'s per-instance early guard, built here so
    /// every `lex-budget` message and every field it reads stays beside the counter.
    pub(super) fn refuse_instance(&self) -> ! {
      let (idx, tier, units, ceiling) = (self.idx, self.tier, self.units, self.per_instance);
      panic!(
        "tokora conformance [input #{idx} {tier} lex-budget] one lexer instance was asked to lex \
         more than {ceiling} times without yielding a token or exhausting — that is one attempt \
         for every item this run's budget allows over its {units} units, plus the probe that \
         would end it, spent inside a single scan. One \
         InputRef::next is served by one lexer, so a single instance spending a whole run's worth \
         of attempts is a scan that is not going to return. This is the early-refusal guard and \
         not the bound — the bound is the tier's whole-run tally, which every one of these \
         attempts was charged to first. Spans must be monotone and nonempty and the lexer must \
         exhaust."
      );
    }
  }
}

use tally::LexTally;

/// A lexer state carrying the run's [`LexTally`] handle beside the wrapped lexer's own state.
///
/// This is how the counter reaches a lexer the kit never constructs: the input layer rebuilds the
/// lexer from `(source, state)` on every operation, so the state is the only channel that survives
/// a rebuild — and, because the handle is an [`Rc`], the only one a checkpoint restore cannot
/// rewind. See [`LexTally`].
struct Tallied<S> {
  /// The wrapped lexer's own state, mirrored — see [`Budgeted`].
  inner: S,
  /// The shared, non-rewindable counter. Cloning this state shares it; nothing copies the count.
  tally: Rc<LexTally>,
}

impl<S> Clone for Tallied<S>
where
  S: Clone,
{
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      tally: Rc::clone(&self.tally),
    }
  }
}

impl<S> core::fmt::Debug for Tallied<S>
where
  S: core::fmt::Debug,
{
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.debug_struct("Tallied")
      .field("inner", &self.inner)
      .field("spent", &self.tally.spent())
      .field("limit", &self.tally.limit())
      .finish()
  }
}

impl<S> crate::State for Tallied<S>
where
  S: crate::State,
{
  type Error = S::Error;

  fn check(&self) -> Result<(), Self::Error> {
    self.inner.check()
  }

  fn take_probe(&mut self) -> Option<crate::state::Probe> {
    self.inner.take_probe()
  }
}

/// `L` under the kit's attempt tally — the lexer every [`Input`] in this module actually runs.
type Bud<'inp, L> = Budgeted<L, <L as Lexer<'inp>>::State>;

/// A [`Lexer`] wrapper that charges every [`lex`](Lexer::lex) attempt to a shared [`LexTally`].
///
/// Every observable delegates, `read_frontier` included, so the wrapped lexer is the lexer under
/// test: the wrapper adds a counter and nothing else.
///
/// # The instance ceiling is an optimisation, not the bound
///
/// [`spend`](Self::spend) also refuses once *one instance* has made
/// [`instance_ceiling(budget)`](instance_ceiling) attempts — one for every item the tier's own
/// budget allows, plus the exhaustion probe. It is **not** what bounds the run: [`LexTally`] is. A
/// guard is allowed to be narrower than the bound; it is not allowed to *be* the bound while a
/// loop underneath it resets it, which is exactly what this ceiling used to do.
///
/// ## The case it makes clearer
///
/// A lexer that never returns from a single [`InputRef::next`](crate::InputRef::next) — the
/// scanner accepting the same error forever inside one call. The tally catches that too, but only
/// after the whole tier's allowance: `O(units²)` attempts in the partial tier, against `O(units)`
/// here. And the message differs in kind, not only in latency — "one lexer instance was asked to
/// lex more than N times without yielding a token or exhausting" names a scan that will not
/// return, where the aggregate can only report that the tier as a whole did too much work.
///
/// ## Why the number may only come from the tally
///
/// It used to be `DEFAULT_BUDGET_MULTIPLE * units + BUDGET_FLOOR`, computed in
/// [`wrap`](Self::wrap) from the source *this instance* was handed, and that was wrong twice over:
/// it ignored [`Harness::budget_multiple`], so a lexer certified under a raised budget was refused
/// at the default anyway; and in the partial tier the source is a *prefix*, so the ceiling shrank
/// with the cut while the run it was bounding did not. Both directions produced the same failure —
/// a **conforming lexer rejected** — which is the one failure a narrower guard may never cause,
/// because the sharper message it exists to give is worth nothing if it is a lie.
///
/// So the number is derived once, from the configured budget, and carried on the tally: every
/// instance under one tier gets the same ceiling, and it is one this run's own item budget already
/// says a conforming lexer stays under. A scan produces at most the items left in the run, and the
/// run may produce at most `budget` of them, so `budget + 1` attempts inside one scan cannot be
/// reached by a lexer the item budget accepts.
///
/// # Why the state is mirrored, and why the mirror is read-only until someone writes it
///
/// `Self::State` has to be [`Tallied<L::State>`](Tallied) — that is the channel — and
/// [`Lexer::state`] must hand out a `&Tallied<L::State>`, which cannot be synthesised from the
/// inner lexer's `&L::State`. So this holds the composite: the inner lexer stays the authority and
/// the mirror is refreshed from it after every call that can change it (`lex`, `bump`).
///
/// The reverse direction — writing the mirror back into the inner lexer — happens **only if
/// someone actually mutated through [`Lexer::state_mut`]**, tracked by `written`. That condition is
/// not an optimisation: writing back unconditionally would call `L::state_mut` on every `lex`, and
/// a lexer whose `State` is `()` is entitled to leave that method `unreachable!()` — an in-tree
/// fixture does. A wrapper that only exists to count must not make a call the unwrapped kit never
/// made.
///
/// Nothing in the kit or the input layer writes through a lexer's `state_mut` today —
/// `InputRef::state_mut` re-keys the *input's* stored state and never reaches the lexer's — so the
/// flag stays false and the two copies stay equal because only one of them is ever written.
struct Budgeted<L, S> {
  inner: L,
  /// The authoritative state: the tally handle, plus a mirror of the inner lexer's own state.
  state: Tallied<S>,
  /// Attempts *this instance* has made. The ceiling it fast-fails at is the tally's — see the
  /// type docs for why it may not be computed here. Not the bound.
  spent_here: usize,
  /// Whether [`Lexer::state_mut`] handed out the mirror, so the inner lexer needs it written back.
  written: bool,
}

impl<L, S> Budgeted<L, S> {
  /// Charges one attempt to the shared tally, then to this instance's fast-fail ceiling.
  ///
  /// Both charges check *before* they increment, and this one for the same reason
  /// `LexTally::spend` does: the ceiling it compares against is
  /// [`instance_ceiling(budget)`](instance_ceiling), which is `budget + 1`, and
  /// [`representable_budget`] is permitted to return `usize::MAX - 1` — so `per_instance` can be
  /// [`usize::MAX`], a value `spent_here` cannot exceed.
  ///
  /// Under an increment-first order that is where `spent_here += 1` would overflow. Checking first
  /// is what removes it rather than making it unlikely: the increment runs only when
  /// `spent_here < per_instance`, so the count lands exactly on the ceiling and the next attempt
  /// takes the refusal. The tally is charged before either, so the aggregate bound sees every
  /// attempt this guard is about to refuse.
  fn spend(&mut self) {
    self.state.tally.spend();
    if self.spent_here >= self.state.tally.per_instance() {
      self.state.tally.refuse_instance();
    }
    self.spent_here += 1;
  }

  /// Carries a mutation made through [`Lexer::state_mut`] into the inner lexer, and **only** then:
  /// see the type docs for why an unconditional write-back is not allowed.
  fn write_back<'inp>(&mut self)
  where
    L: Lexer<'inp, State = S>,
    S: Clone,
  {
    if core::mem::take(&mut self.written) {
      *self.inner.state_mut() = self.state.inner.clone();
    }
  }

  /// Wraps `inner` around the state that carries the tally — which is also where the instance
  /// ceiling comes from, so nothing about this instance's own source enters it.
  fn wrap(inner: L, state: Tallied<S>) -> Self {
    Self {
      inner,
      state,
      spent_here: 0,
      written: false,
    }
  }
}

impl<'inp, L> Lexer<'inp> for Budgeted<L, L::State>
where
  L: Lexer<'inp>,
{
  type State = Tallied<L::State>;
  type Source = L::Source;
  type Token = L::Token;
  type Span = L::Span;
  type Offset = L::Offset;

  const SURFACES_TRIVIA: bool = L::SURFACES_TRIVIA;

  /// The one constructor with no channel for the shared tally, so it mints **no capacity**: the
  /// tally it builds is already spent and the first attempt refuses. The kit never takes this
  /// path — every drive is seeded through [`Input::with_state_and_context`] with a state the tier
  /// built — and a drive that did would be a drive with no bound, which is the defect this whole
  /// type exists to make impossible.
  fn new(src: &'inp Self::Source) -> Self {
    let units = src.slice(..).map(|s| s.len()).unwrap_or(0);
    let inner = L::new(src);
    let state = Tallied {
      inner: inner.state().clone(),
      tally: LexTally::new(usize::MAX, "unseeded", units, AttemptCeiling::Derived(0), 0),
    };
    Self::wrap(inner, state)
  }

  fn with_state(src: &'inp Self::Source, state: Self::State) -> Self {
    let inner = L::with_state(src, state.inner.clone());
    Self::wrap(inner, state)
  }

  fn check(&self) -> Result<(), <Self::Token as Token<'inp>>::Error> {
    self.inner.check()
  }

  fn state(&self) -> &Self::State {
    &self.state
  }

  fn state_mut(&mut self) -> &mut Self::State {
    self.written = true;
    &mut self.state
  }

  fn into_state(self) -> Self::State {
    let Self {
      mut inner,
      state,
      written,
      ..
    } = self;
    let Tallied {
      inner: mirrored,
      tally,
    } = state;
    if written {
      *inner.state_mut() = mirrored;
    }
    Tallied {
      inner: inner.into_state(),
      tally,
    }
  }

  fn source(&self) -> &'inp Self::Source {
    self.inner.source()
  }

  fn span(&self) -> Self::Span {
    self.inner.span()
  }

  fn slice(&self) -> <Self::Source as Source<Self::Offset>>::Slice<'inp> {
    self.inner.slice()
  }

  fn lex(&mut self) -> Option<Result<Self::Token, <Self::Token as Token<'inp>>::Error>> {
    self.spend();
    self.write_back();
    let out = self.inner.lex();
    self.state.inner = self.inner.state().clone();
    out
  }

  fn read_frontier(&self) -> ReadFrontier<Self::Offset> {
    self.inner.read_frontier()
  }

  fn bump(&mut self, n: &Self::Offset) {
    self.write_back();
    self.inner.bump(n);
    self.state.inner = self.inner.state().clone();
  }
}

/// `multiple * units + BUDGET_FLOOR`, or `None` when that does not fit in a `usize` **with room
/// left above it for the exhaustion probe**.
///
/// The one place either budget knob turns into a number, so the "a ceiling must be a value a
/// counter can exceed" rule is stated once. `None` is not a clamp on purpose, and the two
/// directions fail differently rather than both being "the guard stops working":
///
/// - **Clamping to [`usize::MAX`]** does not leave the guards inert. It makes
///   [`instance_ceiling`]'s `+ 1` **overflow** — a ceiling of `0` in release, refusing every lexer
///   immediately — and puts the item guards `out.len() > budget` past any length a `Vec` reaches.
///   The one guard that keeps working is the aggregate tally, which derives a `u128` ceiling from
///   this number rather than comparing against it.
/// - **Clamping to anything smaller** silently substitutes a ceiling the caller did not ask for,
///   and then reports the difference as their lexer's fault.
///
/// Both callers panic instead and say which knob to lower.
///
/// The **strict** upper bound is what makes [`instance_ceiling`]'s `+ 1` total.
/// `checked_add(BUDGET_FLOOR)` alone permits `usize::MAX`, and a derivation that adds to the
/// budget overflows there — arithmetic reaching the same broken ceiling the clamp above would have
/// configured.
fn representable_budget(multiple: usize, units: usize) -> Option<usize> {
  let budget = multiple.checked_mul(units)?.checked_add(BUDGET_FLOOR)?;
  (budget < usize::MAX).then_some(budget)
}

/// The per-lexer-instance early-refusal ceiling for a tier whose item budget is `budget`.
///
/// One full pass: every item the run is allowed to produce, plus the exhaustion probe that ends
/// it. See [`Budgeted`] for why the guard exists and why this is the only number it may use.
///
/// Cannot overflow: every budget in the kit comes from [`representable_budget`], which returns only
/// values strictly below [`usize::MAX`].
fn instance_ceiling(budget: usize) -> usize {
  budget + 1
}

/// A tier's aggregate attempt ceiling, and **whose limit it is** — the lexer's budget, or the
/// kit's own arithmetic.
///
/// The distinction exists because the two are different verdicts and only one of them is about the
/// lexer. Exceeding a [`Derived`](Self::Derived) ceiling says the lexer did more raw work than the
/// configured budget allows; exceeding a [`Capacity`](Self::Capacity) one says the kit stopped
/// counting. Reporting the second as the first is the kit blaming the lexer for its own limit, and
/// the caller reads a `lex-budget` refusal as a verdict on their code. See
/// [`LexTally::spend`](tally::LexTally::spend), which chooses the message from this.
#[derive(Clone, Copy)]
enum AttemptCeiling {
  /// The number [`lex_attempt_ceiling`]'s formula asks for, represented exactly.
  Derived(u128),
  /// The derivation exceeded [`u128::MAX`], so this is the widest total the tally can count to
  /// rather than the bound the formula asks for. Unreachable by any source a machine can hold —
  /// see [`lex_attempt_ceiling`] — and handled anyway, because a caller who does arrive here must
  /// be told the truth about why.
  Capacity,
}

impl AttemptCeiling {
  /// The number the tally compares against, either way.
  fn limit(self) -> u128 {
    match self {
      Self::Derived(limit) => limit,
      Self::Capacity => u128::MAX,
    }
  }
}

/// The aggregate ceiling for a tier that performs `drains` full drains of a source whose item
/// budget is `budget`.
///
/// Derived, not guessed, and deliberately loose. One drain yields at most `budget` items before
/// its own item guard fires, and a conforming lexer spends about one attempt per item plus one
/// exhaustion probe — so `budget + 1` is a full pass. [`ATTEMPTS_PER_DRAIN_MULTIPLE`] passes per
/// drain is the slack for everything the layer legitimately re-lexes: a peek window refilled
/// behind a restore, a region re-scanned after a checkpoint, a partial-mode holdback re-read once
/// the buffer grows.
///
/// Loose is the right direction here. The failure this bounds is *asymptotic* — a lexer evading it
/// needs work that grows faster than the ceiling, so any constant works — while the failure a
/// tight ceiling would cause is a **false refusal** of a legitimate lexer, which no constant fixes.
///
/// # Why the arithmetic is `u128` and not the host's `usize`
///
/// The formula is **quadratic in the source** — `drains` is `units + 3` in the partial tier,
/// because [`check_partial`] drives every split point — while a `usize` is not. Computed in
/// `usize` it saturated, and on a 32-bit target it saturated at **127** units at the maximum
/// multiple and at **11,580** units at the default one. The tally then stopped being a bound
/// derived from the source and became a flat cap of `usize::MAX` attempts, flat while the formula
/// it replaced kept growing: about **75× short** at 100,000 units, against the 320,035,600,844 the
/// derivation asks for.
///
/// **That cap was reachable by an ordinary lexer over an ordinary source**, which is what made it
/// a defect rather than a recorded cost. A conforming [`SpanEnd`](ReadFrontier::SpanEnd) lexer
/// emitting one item per byte spends about one attempt per item, and the partial sweep drives it
/// over every split of the source: over 100 KB that is on the order of five billion prefix
/// attempts — already past `usize::MAX` at 32 bits before the two full-input drains are counted.
/// Such a lexer passed at 64 bits and took an ordinary `lex-budget` refusal at 32, and no
/// [`budget_multiple`](Harness::budget_multiple) setting helped, because every permitted one
/// collapses to the same cap.
///
/// So the ceiling and the count are `u128`, which is width-independent: the number this returns is
/// the same on every target, and the 32-bit refusal is gone rather than merely raised. What that
/// costs is 16-byte arithmetic on a comparison made once per [`Lexer::lex`] attempt in a
/// conformance kit — a test harness that already clones a lexer state per attempt.
///
/// # The residue, and why it is reported as the kit's limit
///
/// `u128` is still finite, so the product is computed with **checked** arithmetic and
/// [`AttemptCeiling::Capacity`] records the one case it does not fit. That is exact rather than a
/// saturation sniffed after the fact: `Capacity` is returned when and only when the derivation
/// genuinely overflowed, so the flag never mislabels a ceiling that happens to be large.
///
/// Reaching it needs roughly 2⁵⁵ source units — about 36 petabytes in one slice — so nothing that
/// runs will see it. It is distinguished anyway, because the alternative is the failure this
/// function just stopped committing: telling a caller their lexer violated the contract when what
/// happened is that the kit ran out of counter. A `Capacity` refusal is reported as
/// **inconclusive** and tagged `kit-capacity`, not `lex-budget`.
///
/// Checked here does not mean panicking here, which is the choice the saturating version got right
/// and this one keeps. Refusing at derivation time rejects *every* lexer over such a source rather
/// than only the one that outruns the counter, and a kit that will not run is worth less than one
/// that runs and says honestly where it stopped.
fn lex_attempt_ceiling(drains: usize, budget: usize) -> AttemptCeiling {
  (drains as u128)
    .checked_mul(ATTEMPTS_PER_DRAIN_MULTIPLE as u128)
    // `budget` is a `usize`, so `+ 1` is exact in `u128` at every width.
    .and_then(|passes| passes.checked_mul(budget as u128 + 1))
    .and_then(|total| total.checked_add(BUDGET_FLOOR as u128))
    .map_or(AttemptCeiling::Capacity, AttemptCeiling::Derived)
}

/// The emitter error the partial check surfaces: the input layer builds it from the frontier
/// [`Incomplete`] (via [`SurfaceIncomplete`](crate::input::SurfaceIncomplete)), and it is the only
/// thing that reaches the `Err` channel — [`ItemRecorder`] never rejects a diagnostic. The
/// check never inspects the payload, only that `next` returned `Err` at the frontier.
#[derive(Debug)]
enum PartialProbe {
  Incomplete,
}

impl<O> From<Incomplete<O>> for PartialProbe {
  #[inline(always)]
  fn from(_: Incomplete<O>) -> Self {
    PartialProbe::Incomplete
  }
}

impl MaybeIncomplete for PartialProbe {
  #[inline(always)]
  fn is_incomplete(&self) -> bool {
    true
  }
}

/// One item of a committed stream — the unit **both** tiers that drive an `Input` compare, the
/// partial one and the integration one.
///
/// A committed **token** and a **lexer error** are both items, because the input layer lexes bytes
/// and then either commits a token or refuses the region and reports it. Truncation can turn one
/// into the other, and a comparison over tokens alone cannot see that: a refusal leaves no trace
/// in the token stream at all. So the error is an item here, and the prefix property is asked of
/// the interleaved sequence.
///
/// The integration tier holds the same values in the same enum, and reached them a release later:
/// it reduced every committed token to `(kind, span)` and discarded the `L::Token` its drain loop
/// already had in hand, which is #269's projection surviving at the one tier neither #269 nor
/// #295 touched. The two tiers differ in *how* they sequence the arms and not in what an item is
/// or in how two of them are compared — see [`check_integration`] for the sequencing difference
/// and why it is a property of the input layer rather than of this type.
enum StreamItem<'inp, L>
where
  L: Lexer<'inp>,
{
  /// A committed token — the **whole token**, not only its kind, plus its span.
  ///
  /// The kind alone was not enough, and the gap was not academic: a `Value(last_byte)`-shaped
  /// token can hold one kind and a one-byte span while its payload is decided by a byte that has
  /// not arrived, so the prefix and the complete parse yield tokens that agree on everything this
  /// tier compared and disagree on the value a parser callback receives. Both runs passed and the
  /// AST changed. The token is `Clone`, so keeping it costs a clone per item in a test kit.
  Token(L::Token, L::Span),
  /// A lexer error the input layer raised over bytes it lexed and refused: its span and the
  /// **payload itself** — see [`ItemRecorder`] for what that does and does not assert.
  LexerError(L::Span, <L::Token as Token<'inp>>::Error),
}

impl<'inp, L> Clone for StreamItem<'inp, L>
where
  L: Lexer<'inp>,
{
  fn clone(&self) -> Self {
    match self {
      Self::Token(tok, span) => Self::Token(tok.clone(), span.clone()),
      Self::LexerError(span, payload) => Self::LexerError(span.clone(), payload.clone()),
    }
  }
}

impl<'inp, L> StreamItem<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The item's span end — the offset the "strictly before the cut" filter reads.
  fn end(&self) -> &L::Offset {
    match self {
      Self::Token(_, span) | Self::LexerError(span, _) => span.end_ref(),
    }
  }

  /// Whether two items agree on discriminant, span, and **value** — the token itself on one arm,
  /// the error payload on the other — and, when they do not, **which of those decided it**.
  ///
  /// # The order the components are consulted in, and what rests on it
  ///
  /// Discriminant, then the components bounded to a *total* equality ([`kind`](Token::kind) is
  /// `Eq`, the span is `Ord`), then the payload, whose [`PartialEq`] promises no reflexivity.
  /// Two sides can differ in several components at once, and this names the first **conclusive**
  /// one in that order — the first whose own comparison the kit can convict on. A divergence a
  /// sound total equality settles is therefore never attributed to a partial one, and the answer
  /// is a true statement about a comparison that really failed either way.
  ///
  /// The word doing the work is *conclusive*. `Eq` promises reflexivity and nothing checks the
  /// promise, so a `Kind` or a span can be as unusable as a `NaN`; when the earlier component is
  /// the unusable one, [`Ranking`] holds it as a fallback and the payload behind it gets to
  /// speak. That is not the order failing — it is the order applying to the evidence the kit
  /// actually has.
  ///
  /// Nothing about **correctness** rests on the order, which is the point of stating it. The
  /// refusal asks its question of whichever component is named, so it fires exactly when nothing
  /// the kit could convict on was found — under this order or any other. What the order buys is a
  /// *diagnosis*: the strongest evidence available gets to speak.
  ///
  /// # Semantically, never by rendering
  ///
  /// The error arm compared `format!("{payload:?}")` until this. Both sides do come from the same
  /// build of the same lexer inside one [`run_partial`](Harness::run_partial) call, so that was
  /// proof against *wording drift* — and silent about the two properties a comparison key actually
  /// needs:
  ///
  /// - `Debug` is not **injective**. Two distinct payloads may render identically (a hand-written
  ///   `Debug` that prints one string for several variants is legal and not rare), and then a
  ///   payload that genuinely moves between the prefix and the complete input compares equal. The
  ///   defect is present and the check is green.
  /// - `Debug` is not **stable**. A rendering that includes an address, a counter or any other
  ///   incidental fails between the separately constructed oracle and prefix lexers even though
  ///   the payloads are equal — a red on a conforming lexer.
  ///
  /// The two errors run in opposite directions, so no amount of care with the *rendering* fixes
  /// both. Value equality fixes both at once, which is why [`run_partial`](Harness::run_partial)
  /// asks for `PartialEq` on the token and the error rather than for a well-behaved `Debug`.
  ///
  /// # The kind is compared as well as the token
  ///
  /// Redundant for any token whose `PartialEq` agrees with its [`kind`](Token::kind), which is
  /// every sane one. It is kept because the two are independent caller code: a `PartialEq` that
  /// ignores a field [`kind`](Token::kind) reads is coarser than the classification the parser
  /// sees, and this comparison is the only thing standing between that and a silent pass. It can
  /// only ever red *more*.
  fn compare(&self, other: &Self) -> Comparison
  where
    <L::Token as Token<'inp>>::Kind: PartialEq,
    L::Token: PartialEq,
    <L::Token as Token<'inp>>::Error: PartialEq,
  {
    match (self, other) {
      (Self::Token(a, sa), Self::Token(b, sb)) => Comparison::ranked(&[
        &|| compare_by(Decided::KIND, &a.kind(), &b.kind()),
        &|| compare_by(Decided::SPAN, sa, sb),
        &|| compare_by(Decided::TOKEN_PAYLOAD, a, b),
      ]),
      (Self::LexerError(sa, a), Self::LexerError(sb, b)) => {
        Comparison::ranked(&[&|| compare_by(Decided::SPAN, sa, sb), &|| {
          compare_by(Decided::ERROR_PAYLOAD, a, b)
        }])
      }
      // A committed token against a lexer error. This tier exists to catch exactly that, and no
      // caller equality was consulted to find it — see [`Decided::Discriminant`].
      _ => Comparison::Differs(Decided::Discriminant),
    }
  }

  /// A human-readable one-line rendering for panic context.
  ///
  /// `Debug` is the right tool *here* and the wrong one in [`compare`](Self::compare): this text is
  /// read by a person diagnosing a failure, and nothing is decided from it.
  fn describe(&self) -> String
  where
    <L::Token as Token<'inp>>::Kind: core::fmt::Debug,
  {
    match self {
      Self::Token(tok, span) => format!("token {:?}@{span:?} (payload {tok:?})", tok.kind()),
      Self::LexerError(span, payload) => format!("lexer error {payload:?}@{span:?}"),
    }
  }
}

/// The shared, ordered log both partial-tier drains append to: [`ItemRecorder`] pushes each
/// lexer error the input layer reports, the drain loop pushes each token `next` hands back, and
/// because the layer reports an error during the very `next` call that goes on to find the
/// following token, the vector ends up being the interleaved item stream in production order.
type StreamLog<'inp, L> = Rc<RefCell<Vec<StreamItem<'inp, L>>>>;

/// The emitter **both** `Input`-driving tiers run under, and the reason neither is [`Silent`].
///
/// `Silent` discards every lexer error, which made the whole error arm invisible to chunked
/// equivalence: a lexer could refuse a region on a truncated buffer and tokenize the same region
/// once the missing bytes arrived, and the check saw an empty token list on one side and a token
/// on the other — a legal *prefix*, and green.
///
/// The integration tier ran under `Silent` for one release longer, and the same sentence applies
/// to it with "truncated buffer" replaced by "schedule": a refusal raised by the straight drain
/// and not by `peek-heavy` — or raised by both with a payload that moved — left the two token
/// streams identical, so the whole error arm of the committed stream was outside every one of
/// that tier's five comparisons. See [`check_integration`].
///
/// # What it compares, and what it deliberately does not
///
/// It records exactly three things per error: that the item **was** an error, its **span**, and
/// the lexer's error **payload itself**. That is the minimum that separates the two divergences the
/// token-only comparison could not see — an error that becomes a token at the same span, and an
/// error whose payload changes once the missing bytes arrive.
///
/// It deliberately records **nothing from the diagnostic channel**: no rendered message, no
/// [`Severity`](crate::emitter::Severity), no label stack, no `Diagnostic`. That is why this is a
/// purpose-built recorder and not [`Verbose`](crate::emitter::Verbose) — collecting diagnostics
/// would put presentation into a correctness check. Nor does it record the parser-facing doors
/// ([`emit_error`](Emitter::emit_error),
/// [`emit_unexpected_token`](Emitter::emit_unexpected_token)); the tier runs no parser, so the
/// only route into [`emit_lexer_error`](Emitter::emit_lexer_error) is the input layer's own
/// deduped refusal, arriving through the default
/// [`commit_lexer_error`](Emitter::commit_lexer_error) forward.
///
/// # It records the payload, not a rendering of it
///
/// This recorded `format!("{payload:?}")` until 0.11.0, defended on the ground that both sides of
/// every comparison come from the same build of the same lexer inside one
/// [`run_partial`](Harness::run_partial) call — the complete parse is the oracle, never a recorded
/// string — so editing an error type's `Debug` moves `expected` and `got` together. That argument
/// is **true**, and it answers only the question of wording drift. It is silent about the two
/// properties a comparison key has to have, and `Debug` has neither: it is not injective, so a
/// payload that really does move can render identically and pass; and it is not stable, so a
/// payload rendering an address or a counter reds on a conforming lexer. See
/// [`StreamItem::compare`] for the full argument and for what the bound on
/// [`run_partial`](Harness::run_partial) buys.
struct ItemRecorder<'inp, L>
where
  L: Lexer<'inp>,
{
  log: StreamLog<'inp, L>,
}

impl<'inp, L> Emitter<'inp, L> for ItemRecorder<'inp, L>
where
  L: Lexer<'inp>,
{
  type Error = PartialProbe;

  fn emit_lexer_error(
    &mut self,
    err: Spanned<<L::Token as Token<'inp>>::Error, L::Span>,
  ) -> Result<(), Self::Error> {
    let (span, payload) = err.into_components();
    self
      .log
      .borrow_mut()
      .push(StreamItem::LexerError(span, payload));
    Ok(())
  }

  fn emit_unexpected_token(&mut self, _: UnexpectedTokenOf<'inp, L>) -> Result<(), Self::Error> {
    Ok(())
  }

  fn emit_error(&mut self, _: Spanned<Self::Error, L::Span>) -> Result<(), Self::Error> {
    Ok(())
  }

  /// A recording emitter owes the log the same rollback discipline a collecting one does, so the
  /// mark is the log's length and a rewind truncates to it. The partial tier's two drains are
  /// straight forward drains and never save or restore, so this was written for a schedule that
  /// did not exist yet; three of the integration tier's five now do exactly that, and the
  /// discipline is what makes their error streams comparable at all — a restore that rewinds the
  /// dedup watermark without rewinding the log would leave every re-lexed refusal recorded twice.
  fn checkpoint(&mut self) -> u64 {
    self.log.borrow().len() as u64
  }

  fn rewind(&mut self, _: &Cursor<'inp, '_, L>, checkpoint: u64) {
    self.log.borrow_mut().truncate(checkpoint as usize);
  }
}

/// The context both `Input`-driving tiers parse under: an [`ItemRecorder`] over [`PartialProbe`]
/// and the default cache.
///
/// One alias for both, because the tiers differ in the input's *completeness mode* and in nothing
/// the context carries. [`PartialProbe`] is uninhabited on the integration tier — its only
/// constructor is `From<Incomplete>`, which a [`Complete`](crate::input::Complete) input never
/// raises — so a `next()` there still cannot fail, exactly as it could not under [`Silent`].
type RecordingCtx<'inp, L> = (ItemRecorder<'inp, L>, DefaultCache<'inp, L>);

/// Seeds the state every drive in this module starts from: the caller's lexer state under the
/// tier's shared [`LexTally`].
///
/// This is the *only* way an `Input` in this module is constructed, and it is why no drive site can
/// forget the tally: [`Lexer::new`] on [`Budgeted`] mints no capacity, so a drive that skipped this
/// would refuse on its first attempt rather than run unbounded.
fn seeded<'inp, L>(src: &'inp L::Source, tally: &Rc<LexTally>) -> Tallied<L::State>
where
  L: Lexer<'inp>,
{
  Tallied {
    inner: L::new(src).into_state(),
    tally: Rc::clone(tally),
  }
}

/// Drives a partial input over `src` at `is_final`, returning the committed **item** stream —
/// tokens and lexer errors interleaved in production order — and whether the drain terminated
/// incomplete (rather than at genuine end of input).
fn partial_stream<'inp, L>(
  src: &'inp L::Source,
  tally: &Rc<LexTally>,
  is_final: bool,
  budget: usize,
  idx: usize,
) -> (Vec<StreamItem<'inp, Bud<'inp, L>>>, bool)
where
  L: Lexer<'inp>,
  L::State: Clone,
{
  let log: StreamLog<'inp, Bud<'inp, L>> = Rc::new(RefCell::new(Vec::new()));
  let context = crate::input::InputContext::new(
    ItemRecorder {
      log: Rc::clone(&log),
    },
    DefaultCache::<'inp, Bud<'inp, L>>::default(),
  );
  let state = seeded::<L>(src, tally);
  let mut input = Input::<
    'inp,
    Bud<'inp, L>,
    RecordingCtx<'inp, Bud<'inp, L>>,
    (),
    Partial,
  >::with_state_and_context(src, state, context);
  // The driver states the world fact before any handle exists — the only place it can.
  if is_final {
    input.seal();
  }
  let mut ir = input.as_ref();
  let incomplete = loop {
    if log.borrow().len() > budget {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] a partial drain exceeded the budget of {budget} items (the lexer may not terminate or re-lexes without progress)"
      );
    }
    match ir.next() {
      Ok(Some(spanned)) => {
        let (span, tok) = spanned.into_components();
        log.borrow_mut().push(StreamItem::Token(tok, span));
      }
      Ok(None) => break false,
      Err(_) => break true,
    }
  };
  let out = core::mem::take(&mut *log.borrow_mut());
  (out, incomplete)
}

/// Drives a complete input over `src`, returning the committed item stream — the oracle a chunked
/// partial run is checked against.
fn complete_stream<'inp, L>(
  src: &'inp L::Source,
  tally: &Rc<LexTally>,
  budget: usize,
  idx: usize,
) -> Vec<StreamItem<'inp, Bud<'inp, L>>>
where
  L: Lexer<'inp>,
  L::State: Clone,
{
  let log: StreamLog<'inp, Bud<'inp, L>> = Rc::new(RefCell::new(Vec::new()));
  let context = crate::input::InputContext::new(
    ItemRecorder {
      log: Rc::clone(&log),
    },
    DefaultCache::<'inp, Bud<'inp, L>>::default(),
  );
  let state = seeded::<L>(src, tally);
  let mut input = Input::<
    'inp,
    Bud<'inp, L>,
    RecordingCtx<'inp, Bud<'inp, L>>,
    (),
    Complete,
  >::with_state_and_context(src, state, context);
  let mut ir = input.as_ref();
  loop {
    if log.borrow().len() > budget {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] a complete drain exceeded the budget of {budget} items"
      );
    }
    match ir
      .next()
      .unwrap_or_else(|_| unreachable!("complete mode never surfaces Incomplete"))
    {
      Some(spanned) => {
        let (span, tok) = spanned.into_components();
        log.borrow_mut().push(StreamItem::Token(tok, span));
      }
      None => break,
    }
  }
  core::mem::take(&mut *log.borrow_mut())
}

/// The partial-equivalence check for one source: exhaustive over every split point.
fn check_partial<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp, Offset = usize>,
  L::State: Clone,
  L::Source: core::ops::Index<core::ops::RangeTo<usize>, Output = L::Source>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  // ONE tally for the whole tier's work on this input, and it is the bound. The `log.len()`
  // budgets these drains also carry are per-`next()`; the instance ceiling inside `Budgeted` is
  // per lexer; both are loops the layer restarts underneath. This counter is restarted by nothing
  // — see `LexTally`, which also says why nothing above it can loop.
  //
  // The drain count it is sized for: one complete drain, one final partial drain, and one per
  // split point of the source, which is at most `len + 1`.
  let units = src.slice(..).map(|s| s.len()).unwrap_or(0);
  let tally = LexTally::new(
    idx,
    "partial-equivalence",
    units,
    lex_attempt_ceiling(units.saturating_add(3), budget),
    instance_ceiling(budget),
  );

  let complete = complete_stream::<L>(src, &tally, budget, idx);

  // The "complete over the full input" leg: a final partial drain reproduces the complete parse.
  let (final_items, final_incomplete) = partial_stream::<L>(src, &tally, true, budget, idx);
  if final_incomplete {
    panic!(
      "tokora conformance [input #{idx} partial-equivalence] a FINAL partial drain surfaced Incomplete; a final input must reach genuine end of input like a complete parse"
    );
  }
  assert_partial_stream_eq::<Bud<'inp, L>>(idx, usize::MAX, &complete, &final_items);

  let len = src.len();
  for k in 0..=len {
    if !src.is_boundary(k) {
      continue;
    }
    let prefix: &L::Source = &src[..k];
    let (prefix_items, incomplete) = partial_stream::<L>(prefix, &tally, false, budget, idx);

    // A non-final prefix yields a PREFIX of the complete ITEMS strictly before the cut. Equality
    // is not the property, and since 0.10.0 it is not even attainable: the holdback keys on the
    // lexer's reported read frontier, so a lexer that reads past what it emits withholds more than
    // the span rule did, and one that reports `Unbounded` withholds everything. Over-withholding
    // is sound — it converges, since a refill strictly grows the buffer and sealing ends the game
    // — so requiring equality here would fail every conforming lookahead lexer, and the check
    // would be a check on precision rather than on correctness.
    //
    // What is never sound, and still fails: an item the complete parse does not have before the
    // cut, or a different one at the same position. "Different" includes an error where the
    // complete parse has a token and an error whose payload moved, which is why the sequence
    // carries lexer errors and not only tokens: relaxing the LENGTH to a prefix is what makes
    // "withheld" indistinguishable from "reported and discarded", so an arm the harness discards
    // is an arm the relaxation makes invisible. Those are exactly what a lexer whose items change
    // under truncation produces, and they are what the *final* leg above pins in full.
    let expected: Vec<_> = complete
      .iter()
      .filter(|item| *item.end() < k)
      .cloned()
      .collect();
    assert_partial_prefix_of::<Bud<'inp, L>>(idx, k, &expected, &prefix_items);

    if !incomplete {
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}: a non-final prefix drain reached genuine end of input; it must surface Incomplete (more input may arrive)"
      );
    }
  }
}

/// Asserts two committed item streams are identical, tagged `partial-equivalence`.
fn assert_partial_stream_eq<'inp, L>(
  idx: usize,
  k: usize,
  expected: &[StreamItem<'inp, L>],
  got: &[StreamItem<'inp, L>],
) where
  L: Lexer<'inp>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  match diverge(expected, got, expected.len() != got.len(), |a, b| {
    a.compare(b)
  }) {
    None => {}
    Some(Divergence::At(i, decided)) => {
      // The final leg compares the complete stream against a final drain of the SAME source, so
      // this is the site #295 arrived through. See `refuse_non_reflexive`: reached only after the
      // comparison has already failed, so it re-labels a failure and never creates one — and it
      // is asked about the component that failed, so a divergence the discriminant settled falls
      // straight through to the verdict below.
      let at = format!("split k={k}, position {i}");
      refuse_decided(
        idx,
        "partial-equivalence",
        &at,
        &decided,
        &expected[i].describe(),
        &got[i].describe(),
      );
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}, position {i}: prefix item diverges from the complete prefix: expected {}, got {}",
        expected[i].describe(),
        got[i].describe()
      );
    }
    Some(Divergence::Count) => panic!(
      "tokora conformance [input #{idx} partial-equivalence] split k={k}: prefix item count diverges from the complete prefix: expected {}, got {}",
      expected.len(),
      got.len()
    ),
  }
}

/// Asserts a non-final prefix drain is a **prefix of** the complete items before the cut: every
/// item it yielded matches — a token against a token, an error against the same error — and it
/// yielded no *extra* one. Withholding more is allowed; changing what an item **is** never was —
/// see [`check_partial`] for why that is the property rather than equality.
fn assert_partial_prefix_of<'inp, L>(
  idx: usize,
  k: usize,
  expected: &[StreamItem<'inp, L>],
  got: &[StreamItem<'inp, L>],
) where
  L: Lexer<'inp>,
  <L::Token as Token<'inp>>::Kind: PartialEq + core::fmt::Debug,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  // `got` longer than `expected` is this leg's cardinality difference and nothing else:
  // withholding MORE is sound here, so a short `got` is not a divergence to rank at all.
  match diverge(expected, got, got.len() > expected.len(), |a, b| {
    a.compare(b)
  }) {
    None => {}
    Some(Divergence::At(i, decided)) => {
      // The sibling guard to `assert_partial_stream_eq`'s. A guard at one comparison and not the
      // other is the same defect relocated: this leg compares two different drains, so a payload
      // that will not equal itself fails here too, and reads as a truncation defect.
      //
      // And this is the leg where the whole-item scan cost the most: an error on the truncated
      // buffer against a token on the full one is what the panic below is *about*, and a scan
      // that found a `NaN` on either side buried it under a refusal.
      let at = format!("split k={k}, position {i}");
      refuse_decided(
        idx,
        "partial-equivalence",
        &at,
        &decided,
        &expected[i].describe(),
        &got[i].describe(),
      );
      panic!(
        "tokora conformance [input #{idx} partial-equivalence] split k={k}, position {i}: prefix item diverges from the complete prefix: expected {}, got {}. An item that is an ERROR on the truncated buffer and a TOKEN on the full one — or an error whose payload moved — was decided from bytes that had not arrived.",
        expected[i].describe(),
        got[i].describe()
      );
    }
    Some(Divergence::Count) => panic!(
      "tokora conformance [input #{idx} partial-equivalence] split k={k}: a non-final prefix drain yielded {} items but the complete parse has only {} ending strictly before the cut. The extra one was decided from bytes that had not arrived; report a read frontier that reaches the buffer end (Lexer::read_frontier) so it is withheld.",
      got.len(),
      expected.len()
    ),
  }
}

/// The second half of every `non-reflexive-payload` refusal, shared by the trait/partial tiers
/// here and by the cache kit in [`cache`] so that the two cannot drift apart on what the
/// diagnosis means or on what it asks the caller to do about it.
const NON_REFLEXIVE_BODY: &str = "`PartialEq` is partial: it promises symmetry and transitivity, \
   never reflexivity, and a payload holding an `f64` that can be `NaN` is the case that reaches \
   this kit. `Eq` and `Ord` DO promise it, and nothing checks that promise — so a component \
   bounded to a TOTAL equality reaches this line too, and a refusal naming one reports a broken \
   total-equality promise rather than the partial one a payload carries. Either way, such a type \
   must hand-write the impl that says what equality means for it — `Harness::run_partial`'s \
   documentation has carried that obligation since the value comparison landed. Until it does, \
   this kit cannot tell a value that will not compare from an implementation that diverges, and \
   the failure it would otherwise report renders expected and got identically.";

/// Whether `value` compares equal to **itself**.
///
/// Written as a call rather than as `value == value` because the two read as opposites: the
/// operator form is a tautology — `clippy::eq_op` refuses it, and rightly, in the code this kit is
/// made of — while here the answer really can be `false`. [`PartialEq`] is *partial*, and it
/// promises symmetry and transitivity and **not** reflexivity; a payload holding an `f64` that can
/// be `NaN` is the case that reaches this kit.
fn self_equal<T>(value: &T) -> bool
where
  T: PartialEq + ?Sized,
{
  let mirror = value;
  value == mirror
}

/// What a verdict-drawing comparison found: the two sides agreed, or they differed and the
/// answer says **which operation decided it**.
///
/// A `bool` said only *whether*. The refusal below then had to guess *why*, and the only tool it
/// had was a scan of everything the item holds — a second question, asked of values the
/// comparison may never have looked at. Nothing linked the two, and the gap is not academic: a
/// committed token on one side and a lexer error on the other differ at the **discriminant**,
/// with no caller comparison run at all, and a whole-item scan still found a `NaN` in the payload
/// and refused. That refusal states a diagnosis the kit did not establish, over exactly the
/// truncation nonconformance the partial tier exists to report.
enum Comparison {
  /// Every component the comparison consults agreed.
  Equal,
  /// The two sides differ, and [`Decided`] names the operation the verdict rests on.
  Differs(Decided),
}

impl Comparison {
  /// [`Ranking`] over a fixed list of comparisons, in the caller's declared order: the first
  /// **conclusive** difference, or — when none of them is conclusive — the first inconclusive
  /// one, or [`Equal`](Self::Equal).
  ///
  /// A thin adapter and deliberately so: the rule lives in [`Ranking`] and this only spells it
  /// for the sites that need no tag on the step that answered. Each step is a closure because
  /// **the search short-circuits**: the moment one of them is conclusive the rest are never run,
  /// so a comparison that agrees, and one that fails the ordinary way, pay exactly what they paid
  /// before.
  fn ranked(steps: &[&dyn Fn() -> Self]) -> Self {
    let mut ranking = Ranking::new();
    for step in steps {
      if let Some(((), decided)) = ranking.settle((), step().differs()) {
        return Self::Differs(decided);
      }
    }
    ranking
      .refusal()
      .map_or(Self::Equal, |((), decided)| Self::Differs(decided))
  }

  /// The difference, when there is one — [`Ranking`]'s input shape. `Equal` and "nothing to
  /// rank" are the same fact, and this is where the two spellings meet.
  const fn differs(self) -> Option<Decided> {
    match self {
      Self::Equal => None,
      Self::Differs(decided) => Some(decided),
    }
  }
}

/// One component's equality, asked as a [`Comparison`] so that the answer carries the operation
/// that produced it.
///
/// **Every equality this kit draws a verdict from is built here.** That is what makes the guarded
/// population a matter of construction rather than of a count in a comment: a comparison that
/// does not come through this cannot produce a [`Decided`], and so cannot reach either the
/// verdict or the refusal. Reflexivity is probed only on the failure path — see [`decided_by`].
fn compare_by<T>(name: &'static str, expected: &T, got: &T) -> Comparison
where
  T: PartialEq + ?Sized,
{
  if expected == got {
    Comparison::Equal
  } else {
    Comparison::Differs(decided_by(name, expected, got))
  }
}

/// The ranked search for the difference a verdict is drawn from: **a conclusive difference
/// outranks an inconclusive one, wherever both are available.**
///
/// # The other half of the rule
///
/// [`refuse_non_reflexive`] is the half that says the kit never reports a diagnosis it did not
/// establish. This is the half that says it never withholds one it did. Without it the first
/// difference a comparison happens to reach decides everything, and an inconclusive one reached
/// first buries every conclusive one behind it:
///
/// - `[Tok(NaN)]` against `[Tok(NaN), Tok(0.0)]` refused at position 0 over the payload and never
///   reached the length check — while the **extra item** proves replay divergence with no caller
///   equality consulted at all.
/// - A cache that returns the right span and the right token beside a corrupted, perfectly
///   reflexive `L::State` refused over the token, because `NaN != NaN` came first in
///   `triple_compare`'s order — while the unexamined state difference convicts the cache on its
///   own.
///
/// So the first inconclusive difference is *retained as a fallback* and the search goes on. It is
/// reported only if nothing conclusive is found anywhere: in the remaining components of the same
/// item, at a later position of the same stream, or in the item **count**.
///
/// # What counts as conclusive
///
/// [`Decided::is_conclusive`] is the test, and it is a question about the two values the
/// comparison actually ran on rather than about the bound the component carries:
///
/// - a difference at the **discriminant** — no caller equality ran at all;
/// - a difference in item **count** — likewise, which is why it carries no [`Decided`] and is
///   conclusive by construction (see [`Divergence::Count`]);
/// - a component whose comparison **is** sound on both sides: each value equals itself. That
///   covers every component bounded to a total equality whose promise holds, and it covers the
///   ordinary `PartialEq` case — two self-equal payloads that disagree with each other. The
///   ordinary case is the whole population of honest failures and is **not** demoted by any of
///   this.
///
/// Inconclusive is the complement and nothing else: a comparison one of whose two values will not
/// equal itself.
///
/// # Ordering still chooses among conclusive answers
///
/// Each site declares the order its components are consulted in, and that order still picks which
/// *conclusive* difference speaks — `triple_compare`'s "span first, always" is unchanged for
/// every entry whose span comparison is sound. Ranking only ever moves an answer past a step the
/// kit could not have drawn a verdict from.
struct Ranking<T, D = Decided> {
  /// The first inconclusive difference seen, and what the caller calls the step that produced
  /// it. Never overwritten: "first" is in the site's own declared order.
  inconclusive: Option<(T, D)>,
}

impl<T, D> Ranking<T, D>
where
  D: Conclusive,
{
  /// A search with nothing found yet.
  const fn new() -> Self {
    Self { inconclusive: None }
  }

  /// Feeds the next step's answer, tagged with whatever the caller needs back to render its own
  /// message — which component of an entry, which position in a stream, which half of a buffer,
  /// or `()` where the answer already says everything.
  ///
  /// `Some` ends the search: this step is conclusive and outranks anything held. `None` means
  /// keep going, either because the step found nothing or because it is now the fallback.
  fn settle(&mut self, tag: T, answer: Option<D>) -> Option<(T, D)> {
    let found = answer?;
    if found.is_conclusive() {
      return Some((tag, found));
    }
    if self.inconclusive.is_none() {
      self.inconclusive = Some((tag, found));
    }
    None
  }

  /// The answer once every step has run without one of them being conclusive: the retained
  /// fallback, which is what the refusal is drawn from, or `None` when nothing differed.
  fn refusal(self) -> Option<(T, D)> {
    self.inconclusive
  }
}

/// What [`Ranking`] needs of a step's answer, and the only thing it needs: whether the kit may
/// convict on it.
///
/// A trait rather than a method on [`Decided`] because the rule composes. A whole stream's
/// [`Divergence`] is a step of a bigger search — the two arms of one integration observation, the
/// two halves of one prefilled peek — and those compositions were the two extra instances round
/// 5's header audit turned up. One rule, one place, three levels.
trait Conclusive {
  /// Whether this difference rests on no equality the kit cannot trust.
  fn is_conclusive(&self) -> bool;
}

impl Conclusive for Decided {
  fn is_conclusive(&self) -> bool {
    Self::is_conclusive(self)
  }
}

impl Conclusive for Divergence {
  fn is_conclusive(&self) -> bool {
    match self {
      // A count is the kit's own `len()`; see [`Divergence::Count`].
      Self::Count => true,
      Self::At(_, decided) => decided.is_conclusive(),
    }
  }
}

/// The difference between two item streams a verdict is drawn from — [`Ranking`]'s answer with
/// the one step a [`Comparison`] cannot express folded in.
enum Divergence {
  /// A position differs, and [`Decided`] names the operation that decided it.
  At(usize, Decided),
  /// Every shared position agreed, or every difference among them was inconclusive: the two
  /// streams differ in item **count**.
  ///
  /// **Conclusive by construction.** A count is the kit's own `len()`, so no caller equality
  /// stands behind it and there is nothing here that could fail to equal itself — which is
  /// exactly why it outranks a retained fallback instead of queueing behind one.
  Count,
}

/// [`Ranking`] over two item streams: the first **conclusive** difference among the shared
/// positions, else a cardinality difference if there is one, else the first inconclusive
/// position, else `None`.
///
/// The four stream comparisons in this module and [`cache`]'s purity comparison all read their
/// answer from here, so the ordering between a position and a count cannot come to hold at one of
/// them and not at another — the shape rounds 2 through 4 each hit by repairing a site.
///
/// `counts_differ` is the caller's, because the two legs do not agree on what a count difference
/// *is*: inequality for a stream compared against a stream, and `got` longer than `expected` for
/// the prefix leg, where withholding more is sound (see [`assert_partial_prefix_of`]).
fn diverge<T, F>(expected: &[T], got: &[T], counts_differ: bool, compare: F) -> Option<Divergence>
where
  F: Fn(&T, &T) -> Comparison,
{
  let mut ranking = Ranking::new();
  for i in 0..expected.len().min(got.len()) {
    if let Some((i, decided)) = ranking.settle(i, compare(&expected[i], &got[i]).differs()) {
      return Some(Divergence::At(i, decided));
    }
  }
  if counts_differ {
    return Some(Divergence::Count);
  }
  ranking
    .refusal()
    .map(|(i, decided)| Divergence::At(i, decided))
}

/// The operation a failed comparison was decided by, carrying the reflexivity question asked of
/// **that** operation — and of nothing else — on each side.
///
/// The probe is computed by [`decided_by`] from the same two values the failed comparison was
/// drawn from, on the same statement. That adjacency is the repair: a refusal can no longer speak
/// about a component the comparison never consulted, because the only way to build one is from
/// the comparison that ran.
enum Decided {
  /// The kit's own `match` on the two discriminants: a committed token on one side and a lexer
  /// error on the other.
  ///
  /// **No caller comparison ran.** There is no equality behind this to be non-reflexive, so
  /// there is nothing to refuse and the divergence is reported directly — which is right, since
  /// an item that is an error on a truncated buffer and a token on the full one is precisely the
  /// truncation nonconformance the partial tier is for.
  Discriminant,
  /// One named component's comparison decided it, and it is sound on whichever side is marked
  /// reflexive.
  Component {
    /// The component, named as the refusal names it.
    name: &'static str,
    /// Whether the expected side's `name` compares equal to itself.
    expected_reflexive: bool,
    /// Whether the got side's `name` compares equal to itself.
    got_reflexive: bool,
  },
}

impl Decided {
  /// ``"the token's `Kind`"`` — bounded [`Eq`], so a refusal naming it is a broken **total**
  /// equality promise rather than the partial one a payload carries.
  const KIND: &'static str = "the token's `Kind`";
  /// `"the span"` — [`Lexer::Span`] is bounded `Ord`, so this too is a total promise.
  const SPAN: &'static str = "the span";
  /// `"the slice"` — [`Slice`] is bounded `Eq`, so this too is a total promise.
  const SLICE: &'static str = "the slice";
  /// `"the token payload"` — the caller's [`PartialEq`] on [`Lexer::Token`], where reflexivity is
  /// promised by nothing. This is #295's home.
  const TOKEN_PAYLOAD: &'static str = "the token payload";
  /// `"the lexer error payload"` — the caller's [`PartialEq`] on [`Token::Error`], the other half
  /// of that population.
  const ERROR_PAYLOAD: &'static str = "the lexer error payload";
  /// ``"the `L::State`"`` — the caller's [`PartialEq`] on [`Lexer::State`], which only the cache
  /// tier compares.
  const STATE: &'static str = "the `L::State`";
  /// `"the span offset"` — [`Lexer::Offset`] is bounded `Ord`, another total promise. The
  /// endpoint comparisons the tiling and combined-span laws are drawn from name this rather than
  /// [`SPAN`](Self::SPAN), because a start or an end is what those laws compare and a reader
  /// looking for the value in the message needs the same noun.
  const OFFSET: &'static str = "the span offset";
  /// `"the read frontier"` — [`ReadFrontier`] derives its equality from [`Lexer::Offset`]'s, so
  /// this is the total promise again, one type further out.
  const FRONTIER: &'static str = "the read frontier";
  /// `"the span vector"` — a whole run of spans compared as **one** value, which is what the
  /// cache kit's order laws do. A slice's equality is elementwise, so one lying [`Lexer::Span`]
  /// anywhere in the run makes the run unequal to itself and the refusal names the run.
  const SPANS: &'static str = "the span vector";

  /// Whether the difference this describes is **conclusive**: the kit can convict on it, because
  /// no equality it cannot trust decided it.
  ///
  /// This is [`Ranking`]'s test, written here so it is one statement rather than an inference
  /// from two `Option`s at each site. `true` exactly when the refusal has nothing to say — at the
  /// discriminant, where no caller comparison ran, and at a component both of whose values equal
  /// themselves, which is the ordinary case and the one that must never be demoted.
  const fn is_conclusive(&self) -> bool {
    self.expected().is_none() && self.got().is_none()
  }

  /// The component to refuse over on the **expected** side, and `None` when the operation that
  /// failed is sound there — or when no caller comparison ran at all.
  const fn expected(&self) -> Option<&'static str> {
    match self {
      Self::Discriminant => None,
      Self::Component {
        name,
        expected_reflexive,
        ..
      } => {
        if *expected_reflexive {
          None
        } else {
          Some(*name)
        }
      }
    }
  }

  /// [`expected`](Self::expected) on the **got** side.
  const fn got(&self) -> Option<&'static str> {
    match self {
      Self::Discriminant => None,
      Self::Component {
        name,
        got_reflexive,
        ..
      } => {
        if *got_reflexive {
          None
        } else {
          Some(*name)
        }
      }
    }
  }
}

/// One component's comparison that **failed**, together with the reflexivity question asked of
/// that same comparison on both sides.
///
/// Every [`Decided::Component`] in the kit is built here, from the two values the caller has just
/// compared, so the probe and the failure cannot be about different operations. Reached only on
/// the failure path, so a passing comparison pays for none of it.
fn decided_by<T>(name: &'static str, expected: &T, got: &T) -> Decided
where
  T: PartialEq + ?Sized,
{
  Decided::Component {
    name,
    expected_reflexive: self_equal(expected),
    got_reflexive: self_equal(got),
  }
}

/// Both sides' [`refuse_non_reflexive`] for one failed item comparison, asked of the operation
/// that decided it. Shared by the four item-comparing sites so the rule cannot come to hold at
/// one of them and not at another.
fn refuse_decided(idx: usize, op: &str, at: &str, decided: &Decided, expected: &str, got: &str) {
  refuse_component(idx, op, at, "item", decided, expected, got);
}

/// [`refuse_decided`] where the two sides are not items: one `slice()` against the source, one
/// `read_frontier()` reading against the next, one span endpoint against another.
///
/// `noun` is what the side being refused *is*, and it is a parameter rather than the constant it
/// used to be because these comparisons draw verdicts from values no item holds. A refusal that
/// called a source slice an "item" would be describing something the comparison never had.
fn refuse_component(
  idx: usize,
  op: &str,
  at: &str,
  noun: &str,
  decided: &Decided,
  expected: &str,
  got: &str,
) {
  refuse_non_reflexive(idx, op, at, "expected", noun, decided.expected(), expected);
  refuse_non_reflexive(idx, op, at, "got", noun, decided.got(), got);
}

/// Refuses to draw a conformance verdict from a comparison that a value decided by failing to
/// equal itself, and returns without a word when there is nothing to refuse.
///
/// # Why the kit refuses instead of reporting the failure it found
///
/// Every tier here decides pass/fail with equality on values it does not own — the token, the
/// lexer's error payload, the cache kit's lexer state, and, no differently, the token's `Kind`,
/// the span, the source slice, the span offsets and the read frontier. Reflexivity is the one law
/// of an equivalence relation that `PartialEq` does not promise, and when a payload breaks it the
/// comparison fails for a reason that is a **fact about the value type** and not about the lexer,
/// the cache or the input layer under test.
///
/// The components bounded to a *total* equality reach this line too, and that is the round-5
/// residue rather than an oversight. `Eq` and `Ord` do promise reflexivity — but they are markers,
/// nothing verifies the promise, and a caller who breaks one has broken a **stronger** contract,
/// not put themselves outside this kit's reach. Refusing there says the same true thing: the
/// comparison that failed was the caller's and the kit convicted nobody. The tag stays
/// `non-reflexive-payload` for every component, because the tag names the *diagnosis* #295
/// established and every cell and downstream grep keys on it; the component the refusal names is
/// what says whether a total or a partial promise was the one broken.
///
/// The failure that produced is not merely mislabelled, it is unreadable: a final partial drain
/// compares a stream against itself, so `expected` and `got` are the same item, and the
/// `partial-equivalence` panic hands the consumer a red accusing their lexer with two values that
/// **render identically** (`NaN` and `NaN`). That was #295, and 38 corpus queries in one
/// downstream reached it.
///
/// So before a tier reports divergence it asks whether the **operation that decided it** is equal
/// to itself, and a side whose is not gets this refusal instead — a distinct tag, the component
/// that will not compare, and the obligation it comes from. The kit says what it cannot check
/// rather than naming a culprit it has not identified.
///
/// # It asks about the comparison that ran, and about nothing else
///
/// `component` comes from [`Decided`], which every [`Comparison`] in the kit produces from the
/// two values whose comparison failed. It is never a scan of the item, and the difference is the
/// whole of what a scan got wrong: a scan runs afterwards and knows nothing about *why*, so a
/// divergence settled at the **discriminant** — a committed token on one side, a lexer error on
/// the other, no caller equality consulted at all — still turned up a `NaN` somewhere in the
/// payload and was refused. That is a diagnosis stated without having been established, and the
/// thing it hid is the truncation nonconformance the partial tier is built to report. A
/// difference at the discriminant, or at a component whose comparison is sound, is therefore
/// terminal here: this returns without a word and the ordinary verdict follows.
///
/// # It can only ever re-label a failure, never create one
///
/// Every call site reaches this **after** its own comparison has already failed, so a run that
/// passes cannot be turned red by it and a run this refuses was already going to be reported as a
/// failure. That is what keeps the repair from trading a false red for a false green: a
/// genuinely non-conforming lexer whose payloads *are* reflexive never reaches this line and
/// still fails the ordinary way, with the ordinary tag.
///
/// Since round 5 it is reached later still: only once [`Ranking`] has run every remaining step of
/// the same search — the rest of the item, the rest of the stream, the item count, the other arm
/// — and found nothing the kit could convict on. A refusal is now the answer of last resort in
/// the strict sense, and that is the second half of the same rule.
///
/// # This is not a relaxation of the contract
///
/// The obligation predates the report: [`Harness::run_partial`]'s documentation has said since
/// 0.10.0 that a payload holding a `NaN`-capable `f64` must hand-write the impl that says what
/// equality means for it. Nothing here excuses that. What changes is the *diagnosis* — the kit
/// stops calling an unmet obligation a lexer defect.
fn refuse_non_reflexive(
  idx: usize,
  op: &str,
  at: &str,
  side: &str,
  noun: &str,
  component: Option<&'static str>,
  rendered: &str,
) {
  let Some(component) = component else {
    return;
  };
  panic!(
    "tokora conformance [input #{idx} non-reflexive-payload] {op} {at}: INCONCLUSIVE — \
     {component} of the {side} {noun} {rendered} is not equal to ITSELF, so the comparison that \
     just failed was decided by the value type and not by the lexer, which this kit has \
     therefore not convicted of anything. {NON_REFLEXIVE_BODY}"
  )
}

/// One observed lexer item, captured with everything the checks compare or resume from.
struct Item<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The item **itself**, by value: the token the lexer yielded or the error it raised.
  ///
  /// [`Lexer::lex`] hands this back owned, so keeping it costs *less* than the pair it replaced —
  /// a `format!("{err:?}")` `String` allocated per error item, plus the kind projected out of the
  /// token. See [`compare`](Self::compare) for why the value and never a rendering of it.
  item: Result<L::Token, <L::Token as Token<'inp>>::Error>,
  /// The item's span.
  span: L::Span,
  /// The item's slice.
  slice: <L::Source as Source<L::Offset>>::Slice<'inp>,
  /// The span end = the offset a resume from *after* this item bumps to.
  end: L::Offset,
  /// The lexer state observed right after this item was produced.
  state: L::State,
}

impl<'inp, L> Item<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Whether two items agree on discriminant, **value**, span and slice — and, when they do not,
  /// **which of those decided it**.
  ///
  /// The components are consulted in [`StreamItem::compare`]'s order and for its reason, with
  /// the slice — bounded [`Eq`], another total promise — between the span and the payload.
  ///
  /// # Semantically, never by rendering
  ///
  /// This compared `format!("{err:?}")` on the error arm, and the token's
  /// [`kind`](Token::kind) alone on the token arm, until 0.10.0 — and both halves reported an
  /// identity the kit had not checked (#269). The argument is [`StreamItem::compare`]'s,
  /// verbatim, and it had already been made there for the partial tier:
  ///
  /// - `Debug` is not **injective**, so two distinct error payloads may render identically — a
  ///   hand-written `Debug` printing one label for a family of variants is legal and not rare —
  ///   and then a lexer whose error *value* changes between two fresh runs passes replay
  ///   identity.
  /// - `Debug` is not **stable**, so a rendering carrying an address, a counter or any other
  ///   incidental differs between the two separately constructed lexers and reds a **conforming**
  ///   one. Both directions have in-tree cells; a rendering is the wrong key in each of them, and
  ///   no amount of care with the rendering fixes both, because they run in opposite directions.
  /// - the token arm compared only the kind, so a payload that moved between two fresh runs kept
  ///   its kind, its span and its slice — everything this looked at — while the value a parser
  ///   callback receives changed.
  ///
  /// The module header states check 1 as identity of the *item*, and identity of a rendering or
  /// of a projection is a weaker claim wearing that word. Value equality is the claim, which is
  /// what [`run`](Harness::run) now asks `PartialEq` for.
  fn compare(&self, other: &Self) -> Comparison
  where
    L::Token: PartialEq,
    <L::Token as Token<'inp>>::Error: PartialEq,
  {
    match (&self.item, &other.item) {
      // The kind is compared as well as the token, for [`StreamItem::compare`]'s reason: the two
      // are independent caller code, and a `PartialEq` coarser than the classification the parser
      // sees is exactly what this comparison stands between.
      (Ok(a), Ok(b)) => Comparison::ranked(&[
        &|| compare_by(Decided::KIND, &a.kind(), &b.kind()),
        &|| self.compare_shared(other),
        &|| compare_by(Decided::TOKEN_PAYLOAD, a, b),
      ]),
      (Err(a), Err(b)) => Comparison::ranked(&[&|| self.compare_shared(other), &|| {
        compare_by(Decided::ERROR_PAYLOAD, a, b)
      }]),
      // A token on one side and a lexer error on the other — see [`Decided::Discriminant`].
      _ => Comparison::Differs(Decided::Discriminant),
    }
  }

  /// The span and the slice — the two components both arms of
  /// [`compare`](Self::compare) share, consulted in its order and answering in its terms.
  ///
  /// Both are bounded to a *total* equality (the span is `Ord`, the slice is `Eq`), which is why
  /// they are asked before either payload: a divergence one of them can settle is never
  /// attributed to a `PartialEq` that promises no reflexivity. "Can settle" is
  /// [`Ranking`]'s test and not the bound — a span whose own `Ord` will not equal itself settles
  /// nothing, and the payload behind it is then what speaks.
  fn compare_shared(&self, other: &Self) -> Comparison {
    Comparison::ranked(&[
      &|| compare_by(Decided::SPAN, &self.span, &other.span),
      &|| compare_by(Decided::SLICE, &self.slice, &other.slice),
    ])
  }

  /// A human-readable one-line rendering for panic context.
  ///
  /// `Debug` is the right tool *here* and the wrong one in [`compare`](Self::compare): this text is
  /// read by a person diagnosing a failure, and nothing is decided from it.
  fn describe(&self) -> String {
    match &self.item {
      Ok(tok) => format!(
        "token {:?}@{:?} (payload {tok:?}, slice {:?})",
        tok.kind(),
        self.span,
        self.slice
      ),
      Err(err) => format!(
        "lexer error {err:?}@{:?} (slice {:?})",
        self.span, self.slice
      ),
    }
  }
}

/// Runs `L::new(src)` to exhaustion, enforcing the always-on trait-tier invariants
/// inline (monotone progress, nonempty + in-bounds spans, span/slice coherence, the
/// anti-hang budget) and returning the captured items.
fn lex_run<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize) -> Vec<Item<'inp, L>>
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let mut lexer = L::new(src);
  let mut out: Vec<Item<'inp, L>> = Vec::new();
  let mut prev_start: Option<L::Offset> = None;

  loop {
    if out.len() > budget {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {}: lex() produced more than the budget of {budget} items without exhausting; the lexer may not terminate",
        out.len()
      );
    }
    let Some(res) = lexer.lex() else { break };
    let span = lexer.span();
    let slice = lexer.slice();
    let state = lexer.state().clone();
    let start = span.start_ref().clone();
    let end = span.end_ref().clone();
    let pos = out.len();

    // 3. Nonempty span.
    if end <= start {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {pos}: zero-width or reversed span {span:?}; every item must satisfy start < end"
      );
    }
    // 3. Monotone (non-decreasing) span starts.
    if let Some(ps) = &prev_start
      && start < *ps
    {
      panic!(
        "tokora conformance [input #{idx} monotone-progress] position {pos}: span start moved backward: previous start {ps:?}, this span {span:?}"
      );
    }
    prev_start = Some(start.clone());

    // 5. Spans within source bounds.
    if end > src_len {
      panic!(
        "tokora conformance [input #{idx} span/slice-coherence] position {pos}: span {span:?} ends past the source length {src_len:?}"
      );
    }
    // 5. slice() equals the source content at span().
    //
    // Through `compare_by` for the reason every other verdict-drawing equality here is: the
    // `Slice` bound is `Eq`, but `Eq` is a marker and nothing checks its promise, so a caller
    // whose slice comparison answers `false` to everything — itself included — got a red naming
    // their lexer for a fact about their own `PartialEq`. Same population, same refusal.
    match src.slice(start.clone()..end.clone()) {
      Some(from_source) => {
        if let Comparison::Differs(decided) = compare_by(Decided::SLICE, &from_source, &slice) {
          let at = format!("position {pos}");
          refuse_component(
            idx,
            "span/slice-coherence",
            &at,
            "reading",
            &decided,
            &format!("{from_source:?}"),
            &format!("{slice:?}"),
          );
          panic!(
            "tokora conformance [input #{idx} span/slice-coherence] position {pos}: slice() disagrees with the source at span {span:?}: source {from_source:?}, slice() {slice:?}"
          );
        }
      }
      None => panic!(
        "tokora conformance [input #{idx} span/slice-coherence] position {pos}: span {span:?} does not address a valid source slice"
      ),
    }

    // 7. The read frontier is a pure read of recorded fact: repeated calls agree, and asking does
    // not advance the lexer or probe new input. Only the after-an-item window is specified, which
    // is exactly where this asks — the same window `span()` and `slice()` answer in.
    let frontier = lexer.read_frontier();
    for probe in 0..3 {
      let again = lexer.read_frontier();
      if let Comparison::Differs(decided) = compare_by(Decided::FRONTIER, &frontier, &again) {
        let at = format!("position {pos}");
        refuse_component(
          idx,
          "read-frontier",
          &at,
          "reading",
          &decided,
          &format!("{frontier:?}"),
          &format!("{again:?}"),
        );
        panic!(
          "tokora conformance [input #{idx} read-frontier] position {pos}: read_frontier() changed between calls: first read {frontier:?}, probe #{probe} read {again:?}. It must be a pure read of recorded fact — the input layer's contract lets it be called at most once per item, but nothing may depend on that."
        );
      }
    }
    let after = lexer.span();
    if let Comparison::Differs(decided) = compare_by(Decided::SPAN, &span, &after) {
      let at = format!("position {pos}");
      refuse_component(
        idx,
        "read-frontier",
        &at,
        "reading",
        &decided,
        &format!("{span:?}"),
        &format!("{after:?}"),
      );
      panic!(
        "tokora conformance [input #{idx} read-frontier] position {pos}: read_frontier() moved the lexer: span() was {span:?} before the call and {after:?} after. It must not advance the lexer or probe new input."
      );
    }

    out.push(Item {
      item: res,
      span,
      slice,
      end,
      state,
    });
  }

  out
}

/// Asserts two captured runs are item-for-item identical (check 1 / used by resume).
fn assert_run_eq<'inp, L>(idx: usize, op: &str, expected: &[Item<'inp, L>], got: &[Item<'inp, L>])
where
  L: Lexer<'inp>,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  // The item comparisons and the length check are ONE ranked search, and the order they used to
  // run in was the defect: a `[Tok(NaN)]` against a `[Tok(NaN), Tok(0.0)]` refused at position 0
  // and never reached the length check, while the extra item proves replay divergence with no
  // caller equality consulted at all. See `diverge` and `Ranking`.
  match diverge(expected, got, expected.len() != got.len(), |a, b| {
    a.compare(b)
  }) {
    None => {}
    Some(Divergence::At(i, decided)) => {
      // Before naming a culprit, ask whether the comparison that decided this is even comparable
      // to itself. See `refuse_non_reflexive`: reached only once the comparison has failed, and
      // only once the search has established that nothing conclusive outranks it, so it can
      // re-label this failure and can never manufacture one.
      let at = format!("position {i}");
      refuse_decided(
        idx,
        op,
        &at,
        &decided,
        &expected[i].describe(),
        &got[i].describe(),
      );
      panic!(
        "tokora conformance [input #{idx} {op}] position {i}: item mismatch: expected {}, got {}",
        expected[i].describe(),
        got[i].describe()
      );
    }
    Some(Divergence::Count) => panic!(
      "tokora conformance [input #{idx} {op}] length mismatch: expected {} items, got {}",
      expected.len(),
      got.len()
    ),
  }
}

/// Check 4: after the first `None`, `lex()` must keep returning `None`.
fn check_sticky<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp>,
{
  let mut lexer = L::new(src);
  let mut n = 0usize;
  while lexer.lex().is_some() {
    n += 1;
    if n > budget {
      panic!(
        "tokora conformance [input #{idx} sticky-exhaustion] lex() produced more than the budget of {budget} items without exhausting"
      );
    }
  }
  for probe in 0..4 {
    if lexer.lex().is_some() {
      panic!(
        "tokora conformance [input #{idx} sticky-exhaustion] position {n}: lex() returned Some on probe #{probe} after returning None; exhaustion must be sticky"
      );
    }
  }
}

/// Check 4b: the span survives exhaustion — well-formed, ending at the lexer's final position
/// within `[last item end, source len]`, and stable across repeated calls.
///
/// This is the clause's executable form. The clause was written because the input layer grew a
/// *second* reader of the value (the partial-input frontier, after the `to`-shaped
/// end-of-input commit), and the first fixture that reader met left `start > end`.
fn check_span_after_exhaustion<'inp, L>(idx: usize, src: &'inp L::Source, budget: usize)
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let mut lexer = L::new(src);
  let mut last_end: Option<L::Offset> = None;
  let mut n = 0usize;
  while lexer.lex().is_some() {
    n += 1;
    if n > budget {
      panic!(
        "tokora conformance [input #{idx} span-survives-exhaustion] lex() produced more than the budget of {budget} items without exhausting"
      );
    }
    last_end = Some(lexer.span().end());
  }

  let span = lexer.span();
  let start = span.start_ref().clone();
  let end = span.end_ref().clone();

  if end < start {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() is malformed: {span:?} has start > end. It must stay well-formed — the partial-input frontier reports this offset to a refill driver."
    );
  }
  if let Some(item_end) = &last_end
    && end < *item_end
  {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() ends at {end:?}, before the last item's end {item_end:?}. The final position can only be at or past the last item the lexer yielded; a retracting span hands a refill driver an offset it has already consumed."
    );
  }
  if end > src_len {
    panic!(
      "tokora conformance [input #{idx} span-survives-exhaustion] after exhaustion span() ends at {end:?}, past the source length {src_len:?}. The final position is bounded by the source; an over-reporting span hands a refill driver an offset past its own buffer."
    );
  }
  for probe in 0..3 {
    let again = lexer.span();
    if let Comparison::Differs(decided) = compare_by(Decided::SPAN, &span, &again) {
      refuse_component(
        idx,
        "span-survives-exhaustion",
        "after exhaustion",
        "reading",
        &decided,
        &format!("{span:?}"),
        &format!("{again:?}"),
      );
      panic!(
        "tokora conformance [input #{idx} span-survives-exhaustion] span() changed after exhaustion: first read {span:?}, probe #{probe} read {again:?}. Repeated reads must agree — the input layer reads it more than once."
      );
    }
  }
}

/// Check 2: for every position `k`, resuming from the captured (state, offset) pair via
/// `with_state` + `bump` reproduces the original suffix from `k`.
fn check_resume<'inp, L>(
  idx: usize,
  src: &'inp L::Source,
  reference: &[Item<'inp, L>],
  budget: usize,
) where
  L: Lexer<'inp>,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  for k in 0..=reference.len() {
    // The resume point before item k: for k == 0 the initial state at offset 0, else
    // the state captured right after item k-1, at that item's span end.
    let (state, offset) = if k == 0 {
      (L::new(src).into_state(), L::Offset::default())
    } else {
      (reference[k - 1].state.clone(), reference[k - 1].end.clone())
    };

    let mut resumed = L::with_state(src, state);
    resumed.bump(&offset);

    // Collected and then ranked, rather than compared item-by-item as it is produced. The
    // streaming shape settled the verdict at the first position that differed, which is the same
    // ordering defect `assert_run_eq` carried: an inconclusive difference at position 0 buried a
    // decisive item-count difference the suffix comparison had already established. The cost is
    // paid only on a failure path — a run that would have stopped early now lexes on to
    // exhaustion, under the SAME budget the passing path is already bounded by, and holds the
    // suffix it produced for the length of one `k`.
    let mut observed: Vec<Item<'inp, L>> = Vec::new();
    loop {
      if observed.len() > budget {
        panic!(
          "tokora conformance [input #{idx} state-resume] resume-from k={k}: produced more than the budget of {budget} items without exhausting"
        );
      }
      let Some(res) = resumed.lex() else { break };
      let span = resumed.span();
      let slice = resumed.slice();
      let end = span.end_ref().clone();
      observed.push(Item::<L> {
        item: res,
        span,
        slice,
        end,
        state: resumed.state().clone(),
      });
    }

    let suffix = &reference[k..];
    let produced = observed.len();
    match diverge(suffix, &observed, produced != suffix.len(), |a, b| {
      a.compare(b)
    }) {
      None => {}
      Some(Divergence::At(i, decided)) => {
        // The same guard `assert_run_eq` carries, for the same reason and under the same "only
        // after a failed comparison, and only about the comparison that failed" rule. See
        // `refuse_non_reflexive`.
        let at = format!("resume-from k={k}, position {i}");
        refuse_decided(
          idx,
          "state-resume",
          &at,
          &decided,
          &suffix[i].describe(),
          &observed[i].describe(),
        );
        panic!(
          "tokora conformance [input #{idx} state-resume] resume-from k={k}, position {i}: resumed item diverges from the original suffix: expected {}, got {}",
          suffix[i].describe(),
          observed[i].describe()
        );
      }
      // One cardinality difference, reported from the side it fell on so both wordings survive.
      Some(Divergence::Count) if produced > suffix.len() => panic!(
        "tokora conformance [input #{idx} state-resume] resume-from k={k}, position {}: resume produced MORE items than the original suffix ({} remaining)",
        suffix.len(),
        suffix.len()
      ),
      Some(Divergence::Count) => panic!(
        "tokora conformance [input #{idx} state-resume] resume-from k={k}: resume produced FEWER items than the original suffix: expected {}, got {produced}",
        suffix.len()
      ),
    }
  }
}

/// Check 6: gap-free tiling — first span starts at 0, consecutive spans abut, the last
/// ends at the source end.
fn check_lossless<'inp, L>(idx: usize, src: &'inp L::Source, reference: &[Item<'inp, L>])
where
  L: Lexer<'inp>,
{
  let src_len = src.len();
  let zero = L::Offset::default();

  // Every comparison below is an `L::Offset` equality, and `Lexer::Offset` is bounded `Ord` — a
  // TOTAL promise, and one nothing checks. So they are drawn through `compare_by` exactly as the
  // item comparisons are: an offset that will not equal itself is a fact about the caller's `Ord`
  // impl, and reporting it as a tiling defect names the wrong culprit.
  let Some(first) = reference.first() else {
    // An empty stream is gap-free tiling of an empty source; a non-empty source with
    // no tokens leaves the whole thing untiled.
    if let Comparison::Differs(decided) = compare_by(Decided::OFFSET, &zero, &src_len) {
      refuse_component(
        idx,
        "lossless",
        "source length",
        "endpoint",
        &decided,
        &format!("{zero:?}"),
        &format!("{src_len:?}"),
      );
      panic!(
        "tokora conformance [input #{idx} lossless] the lexer produced no items but the source is non-empty (length {src_len:?}); lossless tiling requires covering the whole source"
      );
    }
    return;
  };

  let first_start = first.span.start_ref();
  if let Comparison::Differs(decided) = compare_by(Decided::OFFSET, &zero, first_start) {
    refuse_component(
      idx,
      "lossless",
      "position 0",
      "endpoint",
      &decided,
      &format!("{zero:?}"),
      &format!("{first_start:?}"),
    );
    panic!(
      "tokora conformance [input #{idx} lossless] position 0: first span {:?} does not start at 0",
      first.span
    );
  }

  let mut prev_end = first.span.end_ref().clone();
  for (i, item) in reference.iter().enumerate().skip(1) {
    let start = item.span.start_ref().clone();
    if let Comparison::Differs(decided) = compare_by(Decided::OFFSET, &prev_end, &start) {
      let at = format!("position {i}");
      refuse_component(
        idx,
        "lossless",
        &at,
        "endpoint",
        &decided,
        &format!("{prev_end:?}"),
        &format!("{start:?}"),
      );
      panic!(
        "tokora conformance [input #{idx} lossless] position {i}: gap or overlap — previous span ended at {prev_end:?} but this span {:?} starts at {start:?}",
        item.span
      );
    }
    prev_end = item.span.end_ref().clone();
  }

  if let Comparison::Differs(decided) = compare_by(Decided::OFFSET, &src_len, &prev_end) {
    refuse_component(
      idx,
      "lossless",
      "source end",
      "endpoint",
      &decided,
      &format!("{src_len:?}"),
      &format!("{prev_end:?}"),
    );
    panic!(
      "tokora conformance [input #{idx} lossless] the last span ends at {prev_end:?} but the source ends at {src_len:?}; lossless tiling must reach the source end"
    );
  }
}

/// One integration schedule's observation of the input layer: the **committed token** stream it
/// drained and the **lexer errors** the layer raised underneath it, both as [`StreamItem`]s and
/// both compared by value.
///
/// Two vectors rather than one interleaved log, and that is a fact about the input layer rather
/// than a weaker claim. The layer emits a refusal the moment it *lexes* the region — a
/// `peek` that fills the cache past the drain position reports the errors it finds there
/// immediately, so a peek-and-stop caller never loses them (see the input layer's
/// `emit_lexer_error_deduped`). The interleaving is therefore schedule-dependent **by design**:
/// on `"a?b"` the straight drain records `token, error, token` and `peek-heavy` records
/// `error, token, token`, and both are conforming. What is *not* schedule-dependent is either
/// sequence taken on its own — the dedup watermark makes the recorded refusals strictly
/// increasing in span end under every schedule, and a restore rewinds the watermark and the log
/// together. Comparing the interleaving would red a conforming lexer, which is the failure
/// direction this module refuses to trade for reach.
struct Observed<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The committed tokens `next()` handed back, in drain order.
  ///
  /// Bounded by the drain loop's own `budget` guard, as it always was.
  tokens: Vec<StreamItem<'inp, L>>,
  /// The lexer errors the layer raised, in the order it raised them.
  ///
  /// Bounded by [`LexTally`] and by nothing else, which is the tier's own rule rather than an
  /// omission: no drain loop counts this vector, and it grows at one site — the layer's refusal,
  /// which the layer reaches only by *lexing* the region it refuses. Every push therefore charges
  /// an attempt to the tally first, so the length is at most the tier's attempt ceiling, and the
  /// tally is the boundary because it is the one counter a checkpoint restore cannot refund.
  errors: Vec<StreamItem<'inp, L>>,
}

/// Builds a fresh `Input` session over `src`, hands its [`InputRef`] to `f`, and returns `f`'s
/// value beside the lexer errors the layer raised while it ran.
///
/// The errors come back from the emitter rather than from `f` because `f` cannot see them: the
/// layer commits or refuses on its own, and a refusal is reported to the emitter and leaves no
/// trace in what `next()` hands back. That is the whole reason [`Silent`](crate::emitter::Silent)
/// was the wrong choice here — a discarded arm is not an arm a schedule comparison can check.
fn drive<'inp, L, R>(
  src: &'inp L::Source,
  tally: &Rc<LexTally>,
  f: impl FnOnce(&mut InputRef<'inp, '_, Bud<'inp, L>, RecordingCtx<'inp, Bud<'inp, L>>, ()>) -> R,
) -> (R, Vec<StreamItem<'inp, Bud<'inp, L>>>)
where
  L: Lexer<'inp>,
{
  let log: StreamLog<'inp, Bud<'inp, L>> = Rc::new(RefCell::new(Vec::new()));
  let context = crate::input::InputContext::new(
    ItemRecorder {
      log: Rc::clone(&log),
    },
    DefaultCache::<'inp, Bud<'inp, L>>::default(),
  );
  let state = seeded::<L>(src, tally);
  let mut input =
    Input::<'inp, Bud<'inp, L>, RecordingCtx<'inp, Bud<'inp, L>>, ()>::with_state_and_context(
      src, state, context,
    );
  let out = {
    let mut input_ref = input.as_ref();
    f(&mut input_ref)
  };
  let errors = core::mem::take(&mut *log.borrow_mut());
  (out, errors)
}

/// One integration schedule, run: [`drive`] with a drain that collects the committed tokens, both
/// arms returned together.
fn observe<'inp, L>(
  src: &'inp L::Source,
  tally: &Rc<LexTally>,
  f: impl FnOnce(
    &mut InputRef<'inp, '_, Bud<'inp, L>, RecordingCtx<'inp, Bud<'inp, L>>, ()>,
  ) -> Vec<StreamItem<'inp, Bud<'inp, L>>>,
) -> Observed<'inp, Bud<'inp, L>>
where
  L: Lexer<'inp>,
{
  let (tokens, errors) = drive::<L, _>(src, tally, f);
  Observed { tokens, errors }
}

/// Drives `next()` to exhaustion, collecting the committed token stream — **the tokens
/// themselves**, never a projection of them.
///
/// This collected `(kind, span)` until 0.11.0, on streams whose `L::Token` values `next()` had
/// just handed over: the kind is a projection, so a payload could move between two schedules of
/// the same input while the kind, the span and the item count all held still, and the tier that
/// exists to compare the committed stream reported them equal. That is #269's defect at the one
/// tier #269 did not reach — see [`StreamItem::compare`] for the argument, which is unchanged.
/// Retention is free here: `next()` yields the token owned, [`Token`] is bounded `Clone`, and the
/// pair being replaced was itself built by cloning the span.
fn drain_all<'inp, L>(
  input_ref: &mut InputRef<'inp, '_, L, RecordingCtx<'inp, L>, ()>,
  budget: usize,
) -> Vec<StreamItem<'inp, L>>
where
  L: Lexer<'inp>,
{
  let mut out = Vec::new();
  loop {
    if out.len() > budget {
      panic!("tokora conformance integration: next() exceeded the budget of {budget} tokens");
    }
    match input_ref.next().expect(NEVER_ERRS) {
      Some(spanned) => {
        let (span, tok) = spanned.into_components();
        out.push(StreamItem::Token(tok, span));
      }
      None => break,
    }
  }
  out
}

/// The `expect` every integration drain carries. [`ItemRecorder`] answers `Ok` on every door, and
/// its [`PartialProbe`] error has no constructor a [`Complete`](crate::input::Complete) input can
/// reach — see [`RecordingCtx`].
const NEVER_ERRS: &str = "the conformance kit's recording emitter never returns Err";

/// Integration tier: the committed stream from each named schedule must equal the
/// straight-lex stream, and the straight stream must equal the raw-lex items.
///
/// **Both arms, both compared by value.** A committed token contributes the token itself and a
/// raised refusal contributes its payload, which is the module header's rule for every tier —
/// and until 0.11.0 this tier kept neither. It reduced each token to `(kind, span)` and ran under
/// [`Silent`], so the token arm was compared through a projection and the error arm was not
/// compared at all. See [`drain_all`] for the first half and [`ItemRecorder`] for the second.
///
/// The two arms are compared as two sequences rather than as one interleaved stream. That is
/// [`Observed`]'s statement and it is a property of the input layer: a `peek` reports the
/// refusals it finds while filling the cache immediately, so where an error sits *between* two
/// tokens is schedule-dependent on a conforming lexer, while each sequence on its own is not.
///
/// The arms also differ in **what they can be compared against**. The token stream is checked
/// both against the raw lexer and across the schedules; the error stream is checked across the
/// schedules only, because the layer deduplicates a refusal per region and the raw lexer does
/// not, so the two sequences are legitimately different lengths. The cross-check that would have
/// asserted otherwise was written, run, and removed by a positive cell.
fn check_integration<'inp, L>(
  idx: usize,
  src: &'inp L::Source,
  reference: &[Item<'inp, L>],
  budget: usize,
) where
  L: Lexer<'inp>,
  // The same two `run` already asks for, reached through `StreamItem::compare` — this tier adds
  // no obligation of its own. Retaining the values costs no bound at all: [`Token`] is bounded
  // `Clone` and [`Token::Error`] is bounded `Clone`, both by the trait.
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  use generic_arraydeque::typenum::U3;

  // ONE tally for all five schedules on this input, and it is the bound. Each `out.len() > budget`
  // guard here, and each fixed `for _ in 0..n` prefix consume, is keyed on `next()` — a loop that
  // can spin inside one call without ever appending to the vector the guard measures — and the
  // instance ceiling inside `Budgeted` restarts on the fresh lexer every `next()` builds. That the
  // trait-tier checks above happen to reject such a lexer before this tier is reached is an
  // argument about check ORDER, not a bound. See `LexTally`.
  //
  // Sized for five schedules; the per-drain multiple carries the re-lexing their restores and peek
  // refills cost.
  //
  // Wrapping is invisible to the comparison: `Bud<'inp, L>` carries L's `Token`, `Span` and error
  // type, so the streams collected here are the same type as `raw_tokens` below.
  let units = src.slice(..).map(|s| s.len()).unwrap_or(0);
  let tally = LexTally::new(
    idx,
    "integration",
    units,
    lex_attempt_ceiling(5, budget),
    instance_ceiling(budget),
  );

  // The straight-lex reference: `next()` to exhaustion, no backtracking.
  let straight = observe::<L>(src, &tally, |ir| drain_all::<Bud<'inp, L>>(ir, budget));

  // Cross-check: the input layer's committed TOKEN stream must equal the raw-lex tokens.
  //
  // The token arm only, and the error arm is left out on purpose rather than forgotten. Raw
  // `lex()` errors and the refusals the layer raises are not the same sequence and are not
  // supposed to be: the layer reports each refused *region* exactly once, keyed on the error
  // span's end against a high-water mark (`emit_lexer_error_deduped`), so a lexer that errors
  // eighty times over one span is conforming and the layer raises one. Asking for equality here
  // would red it — the in-tree `RepeatErrLexer` is exactly that lexer, and it is a positive cell.
  // What the dedup does NOT vary with is the schedule, which is where the error arm is compared
  // below.
  let raw_tokens: Vec<StreamItem<'inp, Bud<'inp, L>>> = reference
    .iter()
    .filter_map(|it| {
      it.item
        .as_ref()
        .ok()
        .map(|tok| StreamItem::Token(tok.clone(), it.span.clone()))
    })
    .collect();
  assert_stream_eq::<Bud<'inp, L>>(
    idx,
    "raw-lex-vs-next",
    COMMITTED,
    &raw_tokens,
    &straight.tokens,
  );

  // peek-heavy: fill the cache before every consume; the drain path re-serves cached
  // tokens and re-lexes past the window.
  let peek_heavy = observe::<L>(src, &tally, |ir| {
    let mut out = Vec::new();
    loop {
      if out.len() > budget {
        panic!("tokora conformance [input #{idx} integration/peek-heavy] exceeded budget");
      }
      let _ = ir.peek::<U3>().expect(NEVER_ERRS);
      match ir.next().expect(NEVER_ERRS) {
        Some(spanned) => {
          let (span, tok) = spanned.into_components();
          out.push(StreamItem::Token(tok, span));
        }
        None => break,
      }
    }
    out
  });
  assert_observed_eq::<Bud<'inp, L>>(idx, "peek-heavy", &straight, &peek_heavy);

  // save-early-restore-late: save at 0, consume a prefix, abandon it, then drain the
  // whole stream — which must re-lex the rewound prefix identically.
  //
  // EVERY restoring schedule saves at position 0, and the two arms are aligned by that rather
  // than by a shared rewind. The abandoned prefix's tokens are dropped by never being collected;
  // its errors are dropped by `ItemRecorder::rewind`, which truncates to the log length the save
  // captured — zero here, because nothing has been raised yet. A schedule that saved MID-stream
  // would keep the errors before its save and start its token vector empty, and the two arms
  // would no longer describe the same suffix; such a schedule owes the drain loop the same
  // rewind, and this is the note that says so.
  let save_early = observe::<L>(src, &tally, |ir| {
    let ckp = ir.save();
    for _ in 0..3 {
      if ir.next().expect(NEVER_ERRS).is_none() {
        break;
      }
    }
    ir.restore(ckp);
    drain_all::<Bud<'inp, L>>(ir, budget)
  });
  assert_observed_eq::<Bud<'inp, L>>(idx, "save-early-restore-late", &straight, &save_early);

  // drain-then-restore-across-cache: fill the cache, drain it and lex past it, then
  // restore to a save that predates the cache — the post-save cache is dropped and the
  // region re-lexes on demand.
  let across_cache = observe::<L>(src, &tally, |ir| {
    let ckp = ir.save();
    let _ = ir.peek::<U3>().expect(NEVER_ERRS);
    for _ in 0..4 {
      if ir.next().expect(NEVER_ERRS).is_none() {
        break;
      }
    }
    ir.restore(ckp);
    drain_all::<Bud<'inp, L>>(ir, budget)
  });
  assert_observed_eq::<Bud<'inp, L>>(
    idx,
    "drain-then-restore-across-cache",
    &straight,
    &across_cache,
  );

  // nested-savepoints: outer save, consume, inner save, consume, restore inner (LIFO),
  // consume, restore outer, drain. Exercises nested last-in-first-out restores.
  let nested = observe::<L>(src, &tally, |ir| {
    let outer = ir.save();
    let _ = ir.next().expect(NEVER_ERRS);
    let inner = ir.save();
    let _ = ir.next().expect(NEVER_ERRS);
    ir.restore(inner);
    let _ = ir.next().expect(NEVER_ERRS);
    ir.restore(outer);
    drain_all::<Bud<'inp, L>>(ir, budget)
  });
  assert_observed_eq::<Bud<'inp, L>>(idx, "nested-savepoints", &straight, &nested);
}

/// Which arm of a committed stream a divergence was found on, in the two places the report names
/// it: the noun the verdict uses, and the position label the refusal is asked at.
///
/// The arm is carried rather than inferred because both arms hold the same [`StreamItem`] type —
/// each sequence is homogeneous, so the value cannot say which sequence it came from, and a
/// verdict that called a raised refusal a "committed token" would be describing the wrong half of
/// the layer's behavior.
#[derive(Clone, Copy)]
struct Arm {
  /// The stream's noun, as in "committed token stream diverges".
  noun: &'static str,
  /// What a position on this arm is called, so a `non-reflexive-payload` refusal names the arm
  /// the failed comparison was drawn from.
  at: &'static str,
}

/// The tokens `next()` handed back.
const COMMITTED: Arm = Arm {
  noun: "committed token",
  at: "position",
};

/// The lexer errors the input layer raised underneath the drain.
const RAISED: Arm = Arm {
  noun: "raised lexer-error",
  at: "error position",
};

/// Asserts two schedules observed the input layer identically — the same committed tokens and the
/// same raised lexer errors, both by value.
///
/// # The two arms are ONE verdict, and the ranking runs across them
///
/// They were two `assert` calls in a fixed order, and the second could not be reached until the
/// first had returned — so an inconclusive divergence on the committed arm withheld a conclusive
/// one on the raised-error arm, which is round 5's finding one level above the position/count
/// ordering [`diverge`] repairs. Both arms carry the same tag and describe one schedule's one
/// observation of the layer; the header splits them into two sequences for a documented reason
/// about *where* an error sits, not because they are two laws. So [`Ranking`] composes here too,
/// over whole [`Divergence`]s rather than over components, and the committed arm is still run and
/// still reported first whenever it settles anything conclusively.
fn assert_observed_eq<'inp, L>(
  idx: usize,
  sched: &str,
  expected: &Observed<'inp, L>,
  got: &Observed<'inp, L>,
) where
  L: Lexer<'inp>,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  let mut ranking = Ranking::new();
  for (arm, expected_arm, got_arm) in [
    (COMMITTED, &expected.tokens, &got.tokens),
    (RAISED, &expected.errors, &got.errors),
  ] {
    let found = diverge(
      expected_arm,
      got_arm,
      expected_arm.len() != got_arm.len(),
      |a, b| a.compare(b),
    );
    if let Some(((arm, e, g), divergence)) = ranking.settle((arm, expected_arm, got_arm), found) {
      report_stream_divergence::<L>(idx, sched, arm, e, g, &divergence);
    }
  }
  if let Some(((arm, e, g), divergence)) = ranking.refusal() {
    report_stream_divergence::<L>(idx, sched, arm, e, g, &divergence);
  }
}

/// Asserts one item stream matches another, with position context — the single-arm form, for the
/// raw-lex cross-check that has no second arm to be ranked against.
fn assert_stream_eq<'inp, L>(
  idx: usize,
  sched: &str,
  arm: Arm,
  expected: &[StreamItem<'inp, L>],
  got: &[StreamItem<'inp, L>],
) where
  L: Lexer<'inp>,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  if let Some(divergence) = diverge(expected, got, expected.len() != got.len(), |a, b| {
    a.compare(b)
  }) {
    report_stream_divergence::<L>(idx, sched, arm, expected, got, &divergence);
  }
}

/// Reports one arm's [`Divergence`], with position context. Never returns.
///
/// The comparison behind it is [`StreamItem::compare`] — the same component-aware,
/// reflexivity-aware path the partial tier draws its verdicts from, and the reason this tier has
/// no comparison of its own. It replaced a local `(kind, span)` one, which was both a projection
/// of the item (#269 at this tier) and a second implementation of a rule that already existed:
/// the two could drift, and the older one already had, since it could not see the arm at all.
///
/// Reporting is separated from comparing because [`assert_observed_eq`] ranks the two arms
/// against each other before either of them may speak — see its own docs.
fn report_stream_divergence<'inp, L>(
  idx: usize,
  sched: &str,
  arm: Arm,
  expected: &[StreamItem<'inp, L>],
  got: &[StreamItem<'inp, L>],
  divergence: &Divergence,
) -> !
where
  L: Lexer<'inp>,
  L::Token: PartialEq,
  <L::Token as Token<'inp>>::Error: PartialEq,
{
  match divergence {
    Divergence::At(i, decided) => {
      let i = *i;
      // `Token::Kind` is bounded `Eq`, `Lexer::Span` is bounded `Ord` and the two payloads are
      // bounded only `PartialEq`, so a refusal here is drawn from whichever of those decided it —
      // exactly as it is at every other comparison this kit draws a verdict from. Guarding some
      // and not the rest is the defect relocated, and this one costs a call on a path already
      // panicking.
      let op = format!("integration/{sched}");
      let at = format!("{} {i}", arm.at);
      refuse_decided(
        idx,
        &op,
        &at,
        decided,
        &expected[i].describe(),
        &got[i].describe(),
      );
      panic!(
        "tokora conformance [input #{idx} integration/{sched}] {} {i}: {} stream diverges: expected {}, got {}",
        arm.at,
        arm.noun,
        expected[i].describe(),
        got[i].describe()
      );
    }
    Divergence::Count => panic!(
      "tokora conformance [input #{idx} integration/{sched}] {} stream length differs: straight-lex has {}, schedule has {}",
      arm.noun,
      expected.len(),
      got.len()
    ),
  }
}

#[cfg(test)]
mod tests;
