//! Concrete Syntax Tree (CST) utilities: a rowan-free event vocabulary, and — under the
//! `rowan` feature — the typed-tree infrastructure built on top of
//! [rowan](https://docs.rs/rowan).
//!
//! This module provides infrastructure for building and working with typed concrete syntax trees.
//! Unlike Abstract Syntax Trees (ASTs), CSTs preserve all source information including whitespace,
//! comments, and exact token positions, making them ideal for:
//!
//! - **Code formatters**: Preserve exact formatting and whitespace
//! - **Linters**: Access complete source information for analysis
//! - **Language servers**: Provide accurate position information for IDE features
//! - **Refactoring tools**: Transform code while preserving formatting
//! - **Documentation generators**: Extract and preserve comments
//!
//! # Architecture
//!
//! Tree support is a **flat event stream that rides the emitter**: the parser records events
//! (through the [`CstEmitter`] capability subtrait), and the tree
//! is *derived* from the surviving events exactly once — never mutated mid-parse. Because the
//! events live in the emitter's rewindable channel, backtracking rewinds the tree for free:
//! the same mark that unwinds diagnostics unwinds tree events.
//!
//! The components, front half (rowan-free, available in every build) first:
//!
//! 1. **[`event`](crate::cst::event)**: The event vocabulary, the era-branded
//!    [`EventMark`](crate::cst::event::EventMark), and the
//!    [`Marker`](crate::cst::event::Marker) retro-wrap typestate
//! 2. **`parse_lossless`** / **`parse_lossless_partial`**, and their `_with_context` siblings
//!    (`rowan`): The only doors that mint a `Sink`. They take the source once and use that one
//!    argument for both the sink and the input, so the buffer the tree's text comes from and the
//!    buffer the parse reads cannot be two different buffers. The `_with_context` pair takes the
//!    parse's `InputContext` — and so its `RecursionLimiter` — from the caller; the sink is still
//!    minted here from `src`, which is what keeps the one-argument property true of all four
//! 3. **`Sink`** (`rowan`): The recording emitter — buffers events under the one
//!    checkpoint/rewind mark, configured by a `CstProfile` (the dialect's kind space: mapper,
//!    `KindValidator`, error and gap kinds) and reading its text through `CstText`
//! 4. **`Cst`** (`rowan`): The spent sink a driver hands back — the one door to
//!    materialization (`finish` / `finish_partial`), and deliberately **not** an emitter, so
//!    the artefact cannot be fed back in as a second parse's context
//! 5. **`SyntaxTreeBuilder`** (`rowan`): The low-level append-only builder over rowan's
//!    green tree builder (no rollback of its own — that is what the event buffer is for). It
//!    has no event log behind it, so it carries the `MAX_TREE_DEPTH` ceiling itself
//! 6. **`Element`** / **`Node`** / **`Token`** (`rowan`): Typed views over the finished
//!    tree
//! 7. **`cast`** (`rowan`): Utility functions for the typed layer
//! 8. **`error`** (`rowan`): Error types for CST operations
//!
//! # Design Philosophy
//!
//! - **Zero-cost abstractions**: Typed CST nodes are just pointers, no runtime overhead
//! - **Lossless**: All source information is preserved in the tree
//! - **Immutable**: Trees are immutable. `rowan` 0.17 removed the mutable red-tree API
//!   (`clone_for_update`, `detach`, `splice_children`), so an edit is a rebuild: read the
//!   green tree, emit a new one through `SyntaxTreeBuilder`, and re-root it.
//! - **Type-safe**: Compile-time guarantees about node types and relationships
//!
//! # See Also
//!
//! - [rowan documentation](https://docs.rs/rowan) - The underlying CST library

#[cfg(feature = "rowan")]
use core::{
  cell::{Cell, RefCell},
  marker::PhantomData,
};

#[cfg(feature = "rowan")]
use derive_more::{From, Into};
#[cfg(feature = "rowan")]
use rowan::{GreenNodeBuilder, Language, SyntaxNode, SyntaxToken};

#[cfg(feature = "rowan")]
use crate::syntax::Syntax;

pub mod event;
pub mod kinds;

// UN-GATED from `rowan` deliberately (T12). `CstProfile` and `KindValidator` are fn-pointer and
// data only — `profile.rs`'s single import is `event::TOMBSTONE`, which is itself unconditional
// — so the gate was placement, not design. A `macro_rules!` expansion cannot cfg on tokora's
// features, so a `syntax_kinds!`-style macro that names `KindValidator` in one arm was forcing
// `rowan` onto every consumer of the macro. Strictly more configurations compile; the sink half
// keeps its gate.
mod profile;

#[cfg(feature = "rowan")]
mod sink;

#[cfg(feature = "rowan")]
mod driver;

#[cfg(feature = "rowan")]
mod handle;

#[cfg(feature = "rowan")]
mod text;

pub use profile::{CstProfile, KindValidator};

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use driver::{
  parse_lossless, parse_lossless_partial, parse_lossless_partial_with_context,
  parse_lossless_with_context,
};

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use handle::Cst;

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use sink::{FinishError, Sink, TriviaPolicy};

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use text::CstText;

/// The deepest a green tree this crate materializes may nest — **the root wrapper
/// counted** — and the one thing standing between a `rowan` tree and an abort nothing
/// downstream can catch.
///
/// # What it bounds is a crash, not a resource
///
/// `rowan` releases a green tree **recursively**: a `GreenNode`'s drop glue descends into its
/// children, so a deep enough tree ends the process *in its own destructor*. That is not a
/// panic — no unwind, no destructor, no diagnostic, nothing to catch — and it is not something
/// a consumer can defend against, because by the time a consumer holds the tree its existence
/// is already the hazard: a guard placed after materialization guards a value it cannot
/// dispose of. Every recursive descent over such a tree is a second route to the same abort
/// with a fatter frame — rowan's own `Display`, a red root's `text()`, a consumer's transform
/// — and bounding those walks removes routes without removing the crash.
///
/// So the refusal happens **before the tree exists**, at every door this crate offers:
/// [`Cst::finish`], [`Cst::finish_partial`] and [`SyntaxTreeBuilder::finish`] return
/// [`FinishError::TooDeep`] rather than a value nobody can drop. What the refusing call is
/// then holding is a *bounded* tree, which is why the wall can be enforced from inside the
/// construction it is protecting.
///
/// # Which populations actually reach it
///
/// Not only a hand-rolled event stream. This crate's own Pratt CST hook
/// (`Pratt::with_cst_kinds`) mints **one** retro-wrap anchor per expression and spends it once
/// per fold, and same-target wraps nest inside-out at materialization — which is the correct
/// shape for a left-associative chain and makes the tree's depth **linear in the operator
/// count**. `1 + 1 + 1 + …` is one recursion level, one open node in the event log, and `n`
/// nested nodes in the green tree. No recursion budget bounds it, because no recursion
/// happens; nothing else did either, until this ceiling.
///
/// The consequence is worth stating plainly rather than discovering: a flat expression with
/// more than `MAX_TREE_DEPTH` operators is refused with [`FinishError::TooDeep`]. The
/// alternative is not "accepted" — it is the abort, at roughly four times this depth. Lifting
/// the ceiling needs a green tree whose release is not recursive, which is a dependency
/// decision (al8n/tokora#252) and not a number.
///
/// # Why a constant, and not a knob
///
/// The value is a property of the **runtime's stack**, not of any document. A caller who
/// raised it would be configuring the abort back in — the one thing the value exists to
/// remove — and a caller who lowered it would only refuse more trees, which no API is needed
/// to do. Between those, a knob has no setting that buys anything a caller could want, so it
/// is not offered. The two published *recursion* budgets are knobs for exactly the opposite
/// reason: what a native frame costs is a fact about the caller's grammar and build, which
/// only the caller knows. What a `GreenNode`'s drop frame costs is a fact about `rowan`.
///
/// # The value, and the measurement behind it
///
/// **1024.** One chain of nested nodes around a single token, built through
/// `rowan::GreenNodeBuilder`, finished and released on an explicitly sized 2 MiB thread — one
/// trial per process, bisected for the greatest depth that returns before the next one dies
/// with `fatal runtime error: stack overflow`. `rowan 0.17.0`, `aarch64-apple-darwin`,
/// `rustc 1.100.0-nightly (8fa1c96cf 2026-08-17)`:
///
/// | descent (2 MiB thread) | debug, last ok | release, last ok |
/// |---|---|---|
/// | green `GreenNode` release | 4389 | 26357 |
/// | red `SyntaxNode` root release | 4389 | 26355 |
/// | green `Display` | **4388** | **9410** |
/// | red root `text()` | **4388** | 26355 |
///
/// The binding cell is 4388 in debug (~477 B per level) and 9410 in release (~222 B), and
/// 1024 is the deepest power of two that clears the debug cell by this crate's own
/// `MIN_HEADROOM` of three: `1024 × 3 = 3072 < 4388`, and `2048 × 3 = 6144` is not. Both
/// halves are `const`-asserted beside the constant, so the derivation is pinned as an
/// equality rather than admitted by a band.
///
/// **One number for both profiles**, on the argument
/// [`PARSE_DEFAULT_DEPTH`](crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH)
/// already makes for holding one value across both — which is stronger here than there,
/// because the frames being bounded are `rowan`'s, so
/// they are compiled under the *dependency's* profile — a fact `cfg!(debug_assertions)` in
/// this crate cannot observe at all, not even imperfectly.
///
/// # What the ceiling clears, and the one cell it meets
///
/// Every recursion budget this crate ships or publishes fits under it —
/// [`PARSE_DEFAULT_DEPTH`](crate::state::recursion_tracker::RecursionLimiter::PARSE_DEFAULT_DEPTH)
/// is 32 and
/// [`OPTIMIZED_PARSE_DEPTH`](crate::state::recursion_tracker::RecursionLimiter::OPTIMIZED_PARSE_DEPTH)
/// is 256, against a deepest shipped consumer document of 11 and a deepest tree over this
/// crate's own CST fuzz corpus of **4** — a figure `fuzz::cst`'s `CORPUS_DEEPEST_TREE` asserts
/// live, from both sides, so this sentence cannot go stale quietly. The one cell the ceiling
/// *meets* is `SEGMENTED_PRATT_DEPTH`, which is also 1024: a caller who
/// opts into the full segmented-Pratt budget **and** a CST hook, and whose grammar opens a
/// node at every one of those levels, lands one past this ceiling once the root wrapper is
/// counted. That is not a collision to engineer away — the two numbers bound different
/// resources (64 MiB of heap stack segments there, a 2 MiB thread's drop recursion here) and
/// arrive at the same magnitude by coincidence. Where they meet, the answer is a typed
/// refusal instead of an abort, which is the trade this constant exists to make.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub const MAX_TREE_DEPTH: usize = 1024;

// ── THE MEASURED ROWS BEHIND `MAX_TREE_DEPTH` ────────────────────────────────────────────
//
// NO INTRA-DOC LINKS ON THE TWO PRIVATE CONSTANTS BELOW, DELIBERATELY. rustdoc does not
// document a private item, so it never resolves a `[link]` written on one and a dangling one
// passes `-D warnings` on all three rustdoc legs — the rule `native_stack` states, for the
// same reason. The consumer-facing half of the argument lives on `MAX_TREE_DEPTH` above,
// which is public and whose links *are* checked.

/// `rowan 0.17.0`'s recursive descent over a green tree ends a **debug** (`opt-level = 0`)
/// process at this depth, on the 2 MiB thread every stack figure in this crate is stated on.
///
/// The binding cell of four descents measured on one chain of nested nodes around a single
/// token: green release 4389, red root release 4389, green `Display` 4388, red root `text()`
/// 4388. ~477 bytes per level. One trial per process, bisected for the greatest depth that
/// returns before the next dies with `fatal runtime error: stack overflow`;
/// `aarch64-apple-darwin`, `rustc 1.100.0-nightly (8fa1c96cf 2026-08-17)`.
///
/// It is a **debug** figure and the shipped ceiling is derived from it in both profiles,
/// because the frames are `rowan`'s: their optimisation is the dependency's, which this
/// crate's own `debug_assertions` does not observe. See `MAX_TREE_DEPTH`.
#[cfg(feature = "rowan")]
const ROWAN_RECURSION_ABORTS_AT_DEBUG: usize = 4388;

/// The same instrument and the same host, **release** (`opt-level = 3`,
/// `debug-assertions = false`, `overflow-checks = false`): green release 26357, red root
/// release 26355, green `Display` 9410, red root `text()` 26355. ~222 bytes per level at the
/// binding cell.
///
/// Recorded, and deliberately **not** derived from: optimisation reorders which descent binds
/// (`Display` is the loosest debug cell and the tightest release one), which is the reordering
/// this crate's recursion figures already record about shapes. Its only readers are the
/// assertions below — that the release row is the looser of the two, and that the ceiling
/// clears it by the same margin — so a future re-measurement that inverts the relation fails
/// the build instead of quietly widening the wall.
#[cfg(feature = "rowan")]
const ROWAN_RECURSION_ABORTS_AT_RELEASE: usize = 9410;

/// The derivation of `MAX_TREE_DEPTH`, enforced by the compiler rather than by prose.
///
/// A range-shaped guard would read as though it checked a derivation while admitting every
/// value the argument rejects, so the deepest-power-of-two rule is pinned from **both** sides:
/// this depth clears the binding cell by `MIN_HEADROOM`, and the next doubling does not.
#[cfg(feature = "rowan")]
const _: () = {
  use crate::state::recursion_tracker::{RecursionLimiter, policy::MIN_HEADROOM};

  assert!(
    MAX_TREE_DEPTH.is_power_of_two(),
    "MAX_TREE_DEPTH is not a power of two. Every depth constant this crate ships is one and a \
     reader expects it; if the rule has changed, change it where it is written rather than here."
  );
  assert!(
    MAX_TREE_DEPTH * MIN_HEADROOM < ROWAN_RECURSION_ABORTS_AT_DEBUG,
    "MAX_TREE_DEPTH does not clear the measured rowan recursion ceiling by MIN_HEADROOM. The \
     wall it enforces exists to replace an uncatchable abort, so it does not get the weaker of \
     this crate's two margins."
  );
  assert!(
    MAX_TREE_DEPTH * 2 * MIN_HEADROOM >= ROWAN_RECURSION_ABORTS_AT_DEBUG,
    "MAX_TREE_DEPTH is not the DEEPEST power of two the measured row allows, so the shipped \
     value is lower than its own derivation and refuses trees rowan can release. Re-derive it \
     or re-measure the row; do not leave the two disagreeing."
  );
  assert!(
    ROWAN_RECURSION_ABORTS_AT_RELEASE > ROWAN_RECURSION_ABORTS_AT_DEBUG,
    "the release rowan row is no longer the looser of the two, so deriving the shipped ceiling \
     from the debug row is no longer conservative. Re-derive from whichever row now binds."
  );
  assert!(
    MAX_TREE_DEPTH * MIN_HEADROOM < ROWAN_RECURSION_ABORTS_AT_RELEASE,
    "MAX_TREE_DEPTH does not clear the release rowan row by MIN_HEADROOM either"
  );

  // The populations that SPEND the cell: every recursion budget this crate ships or publishes
  // must fit under the ceiling, or a parse this crate blesses cannot materialize its own tree.
  assert!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH <= MAX_TREE_DEPTH,
    "the shipped recursion default is deeper than the tree ceiling: an unconfigured parse at \
     its own budget could not materialize the tree it just built"
  );
  assert!(
    RecursionLimiter::OPTIMIZED_PARSE_DEPTH <= MAX_TREE_DEPTH,
    "the published optimized-parse budget is deeper than the tree ceiling: a caller who takes \
     the figure this crate hands them could not materialize the tree it produces"
  );
};

/// The `stacker` cell of the same population check, kept separate because the constant it
/// reads exists only under that feature.
///
/// It is an `<=` and it currently holds with **equality** — 1024 against 1024 — which is the
/// one place a published budget meets this ceiling. See `MAX_TREE_DEPTH` for why that is
/// recorded rather than engineered away.
#[cfg(all(feature = "rowan", feature = "stacker"))]
const _: () = {
  use crate::state::recursion_tracker::RecursionLimiter;

  assert!(
    RecursionLimiter::SEGMENTED_PRATT_DEPTH <= MAX_TREE_DEPTH,
    "the published segmented-Pratt budget is deeper than the tree ceiling. The two bound \
     different resources and meeting is expected; crossing is not, because a caller who takes \
     the published figure would then be unable to materialize the tree the parse built."
  );
};

/// A builder for constructing concrete syntax trees.
///
/// `SyntaxTreeBuilder` wraps rowan's [`GreenNodeBuilder`] and provides a convenient
/// interface for building syntax trees from tokens during parsing. The builder uses
/// interior mutability ([`RefCell`]) to allow sharing across parser combinators.
///
/// # Type Parameters
///
/// - `Lang`: The [`Language`] type that defines the syntax kinds for your language
///
/// # Usage Pattern
///
/// The typical usage pattern is:
///
/// 1. Create a builder with [`new()`](Self::new)
/// 2. Pass it to your parser implementation
/// 3. The parser calls [`start_node()`](Self::start_node), [`token()`](Self::token),
///    and [`finish_node()`](Self::finish_node) to build the tree
/// 4. Call [`finish()`](Self::finish) to get the final [`rowan::GreenNode`]
///
/// # Examples
///
/// ```rust,ignore
/// use tokora::cst::SyntaxTreeBuilder;
///
/// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
///
/// // Build a simple tree: Root(Identifier("hello"))
/// builder.start_node(SyntaxKind::Root);
/// builder.token(SyntaxKind::Identifier, "hello");
/// builder.finish_node();
///
/// let green_node = builder.finish();
/// ```
///
/// ## Using Checkpoints for Lookahead
///
/// Checkpoints allow you to start nodes retroactively, which is useful for
/// handling left-recursive or ambiguous grammars:
///
/// ```rust,ignore
/// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
/// let checkpoint = builder.checkpoint();
///
/// builder.token(SyntaxKind::Number, "42");
///
/// // Decide to wrap the number in a UnaryExpression
/// builder.start_node_at(checkpoint, SyntaxKind::UnaryExpression);
/// builder.token(SyntaxKind::Plus, "+");
/// builder.finish_node();
/// ```
///
/// # Interior Mutability
///
/// The builder uses [`RefCell`] internally, which means:
/// - It can be shared immutably across parser combinators
/// - Mutations are checked at runtime (will panic if you violate borrow rules)
/// - Typically safe in single-threaded parsing contexts
///
/// # The depth ceiling, and the one state this type latches
///
/// This builder has no event log behind it — it drives `rowan`'s own builder directly — so
/// nothing else can bound what it constructs. It therefore carries the ceiling itself: an open
/// that would take the tree past [`MAX_TREE_DEPTH`] is **not forwarded**, and the builder
/// latches. A latched builder records nothing further and [`finish`](Self::finish) returns
/// [`FinishError::TooDeep`], so the tree `rowan` actually holds never exceeds the ceiling and
/// what is finally dropped — with the refusal, or with the builder — is a tree it can release.
/// See [`MAX_TREE_DEPTH`] for what the ceiling is protecting and why it is not a knob.
///
/// The latch is deliberately total rather than selective. Suppressing only the over-deep opens
/// would leave a caller's tokens landing in the wrong parent, which is a *plausible* wrong
/// tree; refusing everything after the first refused open leaves a tree that is obviously
/// partial and is never handed back regardless.
///
/// ## Why a live open-count is not the ledger
///
/// The obvious shadow — count the opens `rowan` is holding and refuse past the ceiling —
/// bounds [`start_node`](Self::start_node) and is **blind to**
/// [`start_node_at`](Self::start_node_at). A retro-wrap nests on top of subtrees that are
/// already *finished*, so the live count has long since decremented past them: take a
/// checkpoint, build a chain to the ceiling, close it, then wrap — the live count is zero at
/// the wrap and the tree it produces is one level past the ceiling. That is the same shape
/// that makes the recording sink's own recorded depth the wrong counter to charge, and it is
/// held by `the_builder_refuses_a_retro_wrap_that_crosses_the_ceiling` rather than by this
/// paragraph — the shape was written as a cell first, and it was red.
///
/// So the ledger is one entry per level — the deepest *completed* child subtree at that level
/// — and a wrap is charged `level + swallowed`, which is the depth the wrap actually reaches.
/// The one approximation is *which* children a wrap swallows: `rowan::Checkpoint` is opaque,
/// so the charge uses the deepest completed child at that level rather than the deepest one
/// after the checkpoint. That over-charges only a wrap of shallow siblings sitting beside a
/// subtree already near the ceiling, and it over-charges toward refusal, which is the safe
/// direction.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
#[derive(Debug)]
pub struct SyntaxTreeBuilder<Lang> {
  builder: RefCell<GreenNodeBuilder<'static>>,
  /// One entry per open node plus a bottom entry for the builder's own top level, holding the
  /// deepest **completed** child subtree at that level, in nodes. Its length is therefore the
  /// live open count plus one, and it is what shadows `rowan`'s parent stack — which that
  /// crate's public API does not expose.
  deepest_child: RefCell<std::vec::Vec<usize>>,
  /// The latch: set by the one open this builder refused, never cleared. In [`Sink`]'s
  /// `degraded` class — a refusal cannot be un-refused, so it is deliberately outside anything
  /// that could restore it.
  too_deep: Cell<bool>,
  _marker: PhantomData<Lang>,
}

#[cfg(feature = "rowan")]
impl<Lang> Default for SyntaxTreeBuilder<Lang>
where
  Lang: Language,
{
  #[inline]
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(feature = "rowan")]
impl<Lang> SyntaxTreeBuilder<Lang>
where
  Lang: Language,
{
  /// Creates a new empty syntax tree builder.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// ```
  #[inline]
  pub fn new() -> Self {
    Self {
      builder: RefCell::new(GreenNodeBuilder::new()),
      deepest_child: RefCell::new(std::vec![0usize]),
      too_deep: Cell::new(false),
      _marker: PhantomData,
    }
  }

  /// The one door every forwarded open goes through: charges the level ledger, or latches and
  /// refuses.
  ///
  /// `wrap` says whether the new node inherits the level's already-completed children — false
  /// for a fresh [`start_node`](Self::start_node), true for a
  /// [`start_node_at`](Self::start_node_at), which nests on top of what is already there. The
  /// node sits at level `deepest_child.len()` and reaches `level + swallowed`, which is the
  /// quantity checked.
  ///
  /// `false` means the caller's call was **not** forwarded to `rowan` — either because this
  /// builder had already refused an open, or because this one would have taken the tree past
  /// [`MAX_TREE_DEPTH`]. Both leave the tree `rowan` holds at or below the ceiling, which is
  /// the invariant [`finish`](Self::finish) rests on.
  #[inline]
  fn open(&self, wrap: bool) -> bool {
    if self.too_deep.get() {
      return false;
    }
    let mut ledger = self.deepest_child.borrow_mut();
    let swallowed = if wrap {
      ledger.last().copied().unwrap_or(0)
    } else {
      0
    };
    if ledger.len() + swallowed > MAX_TREE_DEPTH {
      drop(ledger);
      self.too_deep.set(true);
      return false;
    }
    ledger.push(swallowed);
    true
  }

  /// Creates a checkpoint representing the current position in the tree.
  ///
  /// Checkpoints can be used with [`start_node_at()`](Self::start_node_at) to
  /// retroactively wrap already-added children in a new parent node. This is
  /// useful for handling left-recursive or ambiguous grammars.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// let checkpoint = builder.checkpoint();
  ///
  /// builder.token(SyntaxKind::Number, "42");
  ///
  /// // Wrap the number in an expression node
  /// builder.start_node_at(checkpoint, SyntaxKind::Expression);
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::checkpoint`]
  #[inline]
  #[must_use]
  pub fn checkpoint(&self) -> rowan::Checkpoint {
    self.builder.borrow().checkpoint()
  }

  /// Starts a new node with the given syntax kind.
  ///
  /// Must be paired with a corresponding [`finish_node()`](Self::finish_node) call.
  /// All tokens and child nodes added between `start_node()` and `finish_node()`
  /// will be children of this node.
  ///
  /// An open that would take the tree past [`MAX_TREE_DEPTH`] is **refused**: it is not
  /// forwarded to `rowan`, the builder latches, and [`finish`](Self::finish) returns
  /// [`FinishError::TooDeep`]. Nothing recorded after that point reaches the tree either.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::BinaryExpression);
  /// builder.token(SyntaxKind::Number, "1");
  /// builder.token(SyntaxKind::Plus, "+");
  /// builder.token(SyntaxKind::Number, "2");
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::start_node`]
  #[inline]
  pub fn start_node(&self, kind: Lang::Kind) {
    if !self.open(false) {
      return;
    }
    self
      .builder
      .borrow_mut()
      .start_node(Lang::kind_to_raw(kind));
  }

  /// Starts a new node at a previously created checkpoint.
  ///
  /// This allows you to retroactively wrap children that were added after the
  /// checkpoint was created. Useful for handling operator precedence and
  /// left-recursive grammars.
  ///
  /// It opens a node like [`start_node()`](Self::start_node) does and is refused on the same
  /// terms, at the same ceiling — a retro-wrap nests exactly as deep as a direct open.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// let checkpoint = builder.checkpoint();
  ///
  /// // Add a number
  /// builder.token(SyntaxKind::Number, "42");
  ///
  /// // Later, decide to wrap it in a unary expression
  /// builder.start_node_at(checkpoint, SyntaxKind::UnaryExpression);
  /// builder.token(SyntaxKind::Minus, "-");
  /// builder.finish_node();
  /// // Result: UnaryExpression(Number("42"), Minus("-"))
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::start_node_at`]
  #[inline]
  pub fn start_node_at(&self, checkpoint: rowan::Checkpoint, kind: Lang::Kind) {
    if !self.open(true) {
      return;
    }
    self
      .builder
      .borrow_mut()
      .start_node_at(checkpoint, Lang::kind_to_raw(kind));
  }

  /// Adds a token with the given kind and text to the current node.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Identifier);
  /// builder.token(SyntaxKind::IdentifierToken, "my_variable");
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::token`]
  #[inline]
  pub fn token(&self, kind: Lang::Kind, text: &str) {
    if self.too_deep.get() {
      return;
    }
    self
      .builder
      .borrow_mut()
      .token(Lang::kind_to_raw(kind), text);
  }

  /// Finishes the current node started with [`start_node()`](Self::start_node)
  /// or [`start_node_at()`](Self::start_node_at).
  ///
  /// # Panics
  ///
  /// Panics if there is no node to finish (i.e., `finish_node()` was called
  /// more times than `start_node()`).
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Root);
  /// builder.token(SyntaxKind::Identifier, "foo");
  /// builder.finish_node(); // Finishes the Root node
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::finish_node`]
  #[inline]
  pub fn finish_node(&self) {
    if self.too_deep.get() {
      return;
    }
    self.builder.borrow_mut().finish_node();
    let mut ledger = self.deepest_child.borrow_mut();
    // The bottom entry is the builder's own top level and is never popped; `rowan` has
    // already panicked on the line above if there was no node to close.
    if ledger.len() > 1 {
      let closed = 1 + ledger.pop().unwrap_or(0);
      if let Some(parent) = ledger.last_mut() {
        *parent = (*parent).max(closed);
      }
    }
  }

  /// Completes the tree building and returns the final green node.
  ///
  /// This consumes the builder and returns the root [`rowan::GreenNode`],
  /// which can be converted to a [`rowan::SyntaxNode`] for traversal.
  ///
  /// # Errors
  ///
  /// [`FinishError::TooDeep`] if any open past [`MAX_TREE_DEPTH`] was refused. That open was
  /// never forwarded to `rowan` and neither was anything after it, so what this call drops is
  /// a tree at or under the ceiling — see [`MAX_TREE_DEPTH`] for why a tree past it cannot be
  /// handed back.
  ///
  /// It is the only error this door has: an unbalanced bracketing is a panic from
  /// [`finish_node`](Self::finish_node), at the call that made it, not a value from here.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  /// use rowan::SyntaxNode;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Root);
  /// builder.token(SyntaxKind::Identifier, "foo");
  /// builder.finish_node();
  ///
  /// let green = builder.finish().expect("under the depth ceiling");
  /// let root = SyntaxNode::new_root(green);
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::finish`]
  #[inline]
  pub fn finish(self) -> Result<rowan::GreenNode, FinishError> {
    if self.too_deep.get() {
      return Err(FinishError::TooDeep {
        depth: MAX_TREE_DEPTH as u64,
      });
    }
    Ok(self.builder.into_inner().finish())
  }
}

/// Base trait for all typed CST elements (nodes and tokens).
///
/// `Element` provides the common interface shared by both CST nodes
/// ([`Node`]) and CST tokens ([`Token`]). It enables:
/// - **Type checking**: Verify if an untyped element can be cast to a specific type
/// - **Type identity**: Associate elements with their syntax kind
/// - **Polymorphism**: Write generic code that works with both nodes and tokens
///
/// # Design
///
/// This trait serves as the foundation of the typed CST hierarchy:
/// ```text
/// Element (base)
///     ├── Node (for interior nodes)
///     └── Token (for leaf tokens)
/// ```
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Element<Lang: Language>: core::fmt::Debug {
  /// The syntax kind of this CST element.
  ///
  /// For enum elements representing multiple variants, this can be a marker value
  /// that is not directly used for casting but serves as documentation.
  const KIND: Lang::Kind;

  /// Returns `true` if the given kind can be cast to this CST element.
  ///
  /// This method determines whether an untyped rowan element with a specific
  /// syntax kind can be safely converted to this typed element.
  ///
  /// # Implementation Guidelines
  ///
  /// - **Single variant**: Return `kind == Self::KIND`
  /// - **Multiple variants**: Use pattern matching to check all valid kinds
  /// - **Performance**: This method is often called frequently, keep it fast
  ///
  /// # Examples
  ///
  /// ## Simple Element (Single Kind)
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// impl Element for Comma {
  ///     const KIND: SyntaxKind = SyntaxKind::Comma;
  ///
  ///     fn castable(kind: SyntaxKind) -> bool {
  ///         kind == SyntaxKind::Comma
  ///     }
  /// }
  /// ```
  ///
  /// ## Enum Element (Multiple Kinds)
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// impl Element for BinaryOperator {
  ///     const KIND: SyntaxKind = SyntaxKind::BinaryOp; // Marker
  ///
  ///     fn castable(kind: SyntaxKind) -> bool {
  ///         matches!(
  ///             kind,
  ///             SyntaxKind::Plus
  ///             | SyntaxKind::Minus
  ///             | SyntaxKind::Star
  ///             | SyntaxKind::Slash
  ///         )
  ///     }
  /// }
  /// ```
  ///
  /// ## Usage in Type Checking
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// // Check before casting
  /// if Comma::castable(token.kind()) {
  ///     let comma = Comma::try_cast_node(token).unwrap();
  /// }
  /// ```
  fn castable(kind: Lang::Kind) -> bool
  where
    Self: Sized;
}

/// Trait for typed CST tokens (leaf elements in the syntax tree).
///
/// `Token` provides a type-safe wrapper around rowan's untyped [`SyntaxToken`],
/// representing terminal elements in the concrete syntax tree. Tokens are the leaf nodes
/// that contain actual source text (keywords, identifiers, literals, punctuation, etc.).
///
/// # Design
///
/// Tokens differ from nodes ([`Node`]) in that:
/// - **Tokens are leaves**: They contain source text directly
/// - **Nodes are interior**: They have children and structure the tree
/// - **Zero-cost**: Token wrappers have the same memory layout as [`SyntaxToken`]
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Token<Lang: Language>: Element<Lang> {
  /// Attempts to cast the given syntax token to this typed token.
  ///
  /// Returns an error if the token's kind doesn't match this type.
  ///
  /// # Errors
  ///
  /// Returns [`TokenMismatch`](error::TokenMismatch) if:
  /// - The token's kind doesn't match the expected kind for this type
  /// - For enum tokens, the kind is not one of the valid variants
  fn try_cast_token(syntax: SyntaxToken<Lang>) -> Result<Self, error::TokenMismatch<Self, Lang>>
  where
    Self: Sized;

  /// Returns a reference to the underlying syntax token.
  ///
  /// This provides access to rowan's token APIs for inspecting position,
  /// text, and tree structure.
  fn syntax(&self) -> &SyntaxToken<Lang>;

  /// Returns the source text of this token.
  ///
  /// This is a convenience method that extracts the text from the underlying
  /// [`SyntaxToken`]. The text is always valid UTF-8.
  fn text(&self) -> &str
  where
    Lang: 'static,
  {
    self.syntax().text()
  }
}

/// The main trait for typed CST nodes with zero-cost conversions.
///
/// `Node` provides a type-safe wrapper around rowan's untyped [`SyntaxNode`], allowing
/// you to work with strongly-typed CST nodes. The conversion between typed and untyped
/// nodes has **zero runtime cost** - both representations have exactly the same memory
/// layout (a pointer to the tree root and a pointer to the node itself).
///
/// # Design
///
/// The `Node` trait enables:
/// - **Type safety**: Compile-time guarantees about node types
/// - **Zero-cost**: No runtime overhead for typed wrappers
/// - **Pattern matching**: Cast nodes to specific types
/// - **Tree traversal**: Navigate the CST with type information
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
///
/// # One language, named once
///
/// The [`Syntax`] supertrait is bound at `Lang = Lang` rather than left free, and that
/// equality is load-bearing. [`Element<Lang>`](Element) says which tree this node lives in;
/// [`Syntax`] carries the `KIND` constant that says which node it *is* — and `Syntax::KIND` is
/// typed `<Self::Lang as Language>::SyntaxKind`. With `Syntax::Lang` unconstrained, a type
/// could be `Element<A> + Syntax<Lang = B>` and still satisfy `Node<A>`: a node claiming
/// membership in one language while its kind authority answers for another. Nothing would
/// reject it, and the mismatch would surface as a cast that never matches.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Node<Lang: Language>: Element<Lang> + Syntax<Lang = Lang> {
  /// Attempts to cast the given syntax node to this CST node.
  ///
  /// Returns an error if the node's kind doesn't match this type.
  fn try_cast_node(syntax: SyntaxNode<Lang>) -> Result<Self, error::SyntaxError<Self, Lang>>
  where
    Self: Sized;

  /// Returns a reference to the underlying syntax node.
  ///
  /// This provides access to rowan's tree traversal APIs.
  fn syntax(&self) -> &SyntaxNode<Lang>;

  /// Returns the source string of this CST node.
  ///
  /// This includes all text spanned by this node, including whitespace and trivia.
  fn source_string(&self) -> String {
    self.syntax().to_string()
  }

  /// Clones the subtree rooted at this CST node.
  ///
  /// This creates a deep copy of this node and all its descendants, detached
  /// from the original tree.
  fn clone_subtree(&self) -> Self
  where
    Self: Sized,
  {
    Self::try_cast_node(self.syntax().clone_subtree()).unwrap()
  }
}

/// The one capability tree *navigation* needs: turn an untyped [`SyntaxNode`] into a typed
/// node, or decline.
///
/// [`cast::child`], [`cast::children`] and [`NodeChildren`] are bound on this trait rather
/// than on [`Node`], because this is all they ever call. `child` is `find_map(N::cast_node)`
/// and nothing more — it never reads a `KIND`, a component list, or a component count.
///
/// # Why this is not [`Node`]
///
/// [`Node`] requires [`Syntax`], and [`Syntax`] is the *parser's* model of a production: a
/// `Component` enum, and `COMPONENTS`/`REQUIRED` as type-level counts, all of it in service of
/// reporting which parts of a production were missing. That is the right shape for a parser
/// and the wrong toll for a reader. A typed CST layer whose job is `field.name()` would
/// otherwise have to invent a component enum and a typenum count per node kind — once per node
/// kind, for a model it never consults.
///
/// So the navigation helpers ask for navigation. A parser-facing node still satisfies them,
/// through the blanket impl below.
///
/// # One impl per type, by construction
///
/// Every [`Node`] receives `CastNode` from a blanket impl, which is what keeps every existing
/// call site compiling and asks nothing new of existing [`Node`] implementors.
///
/// The price is coherence: because that blanket impl covers all of [`Node`], a type cannot
/// implement `CastNode` **directly** and also implement [`Node`] — the impls would overlap and
/// rustc rejects the direct one (`E0119`).
///
/// That is a commitment, not an accident. The two intended positions are disjoint, and a type
/// belongs to exactly one:
///
/// - a **parser-facing** node implements [`Node`] — hence [`Syntax`], hence the component model
///   — and gets `CastNode` for free;
/// - a **navigation-only** typed CST node implements `CastNode` directly and never names
///   [`Syntax`] at all.
///
/// A type that wants both is asking for the component model, and should implement [`Node`].
///
/// # Examples
///
/// A navigation-only node — no [`Syntax`], no components, no counts:
///
/// ```rust,ignore
/// use rowan::SyntaxNode;
/// use tokora::cst::{CastNode, cast};
///
/// struct Field(SyntaxNode<MyLang>);
/// struct Name(SyntaxNode<MyLang>);
///
/// impl CastNode<MyLang> for Name {
///     fn cast_node(syntax: SyntaxNode<MyLang>) -> Option<Self> {
///         (syntax.kind() == MyKind::Name).then_some(Self(syntax))
///     }
/// }
///
/// impl Field {
///     fn name(&self) -> Option<Name> {
///         cast::child(&self.0)
///     }
/// }
/// ```
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait CastNode<Lang: Language>: Sized {
  /// Casts an untyped syntax node into this typed node, or returns `None` if it is not one.
  ///
  /// This is the whole trait. Implementations should be a kind check and a wrap; they must not
  /// panic, because the navigation helpers call this once per child and read `None` as "not
  /// this type, keep looking".
  fn cast_node(syntax: SyntaxNode<Lang>) -> Option<Self>;
}

#[cfg(feature = "rowan")]
impl<N, Lang> CastNode<Lang> for N
where
  N: Node<Lang>,
  Lang: Language,
{
  #[inline]
  fn cast_node(syntax: SyntaxNode<Lang>) -> Option<Self> {
    Self::try_cast_node(syntax).ok()
  }
}

/// An iterator over typed CST children of a particular node type.
///
/// `NodeChildren` filters and casts child nodes to a specific typed node type,
/// skipping any children that cannot be cast to the target type.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
#[derive(Debug, From, Into)]
#[repr(transparent)]
pub struct NodeChildren<N, Lang: Language> {
  inner: rowan::SyntaxNodeChildren<Lang>,
  _m: PhantomData<N>,
}

#[cfg(feature = "rowan")]
impl<N, Lang: Language> Clone for NodeChildren<N, Lang> {
  #[inline]
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      _m: PhantomData,
    }
  }
}

#[cfg(feature = "rowan")]
impl<N, Lang: Language> NodeChildren<N, Lang> {
  #[inline]
  fn new(parent: &SyntaxNode<Lang>) -> Self {
    Self {
      inner: parent.children(),
      _m: PhantomData,
    }
  }

  /// Returns an iterator over syntax node children matching a kind predicate.
  ///
  /// This allows further filtering of children based on their syntax kind,
  /// returning the underlying [`SyntaxNode`] instead of typed nodes.
  ///
  /// `rowan` 0.17 removed `SyntaxNodeChildren::by_kind`, so the filter is applied here
  /// rather than delegated. The predicate is the same one the removed method took, and
  /// `SyntaxNode::kind` is what that method compared against, so the yielded sequence is
  /// unchanged — only the crate that runs the comparison moved.
  pub fn by_kind<F>(self, f: F) -> impl Iterator<Item = SyntaxNode<Lang>>
  where
    F: Fn(Lang::Kind) -> bool,
  {
    self.inner.filter(move |node| f(node.kind()))
  }
}

#[cfg(feature = "rowan")]
impl<N, Lang> Iterator for NodeChildren<N, Lang>
where
  N: CastNode<Lang>,
  Lang: Language,
{
  type Item = N;

  #[inline]
  fn next(&mut self) -> Option<N> {
    self.inner.find_map(N::cast_node)
  }
}

/// Utility functions for casting and accessing CST nodes.
///
/// This module provides convenient functions for working with typed CST nodes,
/// including finding children, accessing tokens, and casting between types.
///
/// The node-typed helpers — [`child`](cast::child) and [`children`](cast::children) — are bound
/// on [`CastNode`], not on [`Node`]. Casting a child is the only thing they do with `N`, so it
/// is the only thing they ask of it, and a navigation-only typed CST layer can use them without
/// ever naming the parser's [`Syntax`] model. Every [`Node`] still qualifies, through the
/// blanket impl. [`token`](cast::token) is not node-typed at all — it matches on a
/// [`Lang::Kind`](Language::Kind) value and needs no bound beyond the language, and neither do
/// its multi-kind and plural forms, [`token_any`](cast::token_any) and [`tokens`](cast::tokens).
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub mod cast;

/// Error types for CST operations.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub mod error;

#[cfg(all(test, feature = "rowan"))]
mod tests;
