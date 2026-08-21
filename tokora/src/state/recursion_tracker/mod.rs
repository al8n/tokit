use crate::State;

/// Error returned when recursion depth exceeds the configured limit.
///
/// This error provides context about both the actual recursion depth reached
/// and the maximum depth allowed, making it easy to diagnose whether the limit
/// needs adjustment or if there's a genuine infinite recursion bug.
///
/// # Example
///
/// ```rust
/// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
///
/// let mut limiter = RecursionLimiter::with_limitation(10);
///
/// // Simulate deep recursion
/// for _ in 0..15 {
///     limiter.increase();
/// }
///
/// match limiter.check() {
///     Err(error) => {
///         eprintln!("Recursion limit exceeded!");
///         eprintln!("Current depth: {}", error.depth());
///         eprintln!("Maximum allowed: {}", error.limitation());
///     }
///     Ok(_) => unreachable!(),
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[error("recursion limit exceeded: depth {}, maximum {}", .0.depth(), .0.limitation())]
pub struct RecursionLimitExceeded(RecursionLimiter);

impl RecursionLimitExceeded {
  /// Returns the actual recursion depth that triggered the error.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
  ///
  /// let mut limiter = RecursionLimiter::with_limitation(3);
  /// limiter.increase();
  /// assert_eq!(limiter.depth(), 1);
  /// ```
  #[inline(always)]
  pub const fn depth(&self) -> usize {
    self.0.depth()
  }

  /// Returns the maximum recursion depth that was configured.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::{RecursionLimiter, RecursionLimitExceeded};
  ///
  /// let mut limiter = RecursionLimiter::with_limitation(3);
  /// assert_eq!(limiter.limitation(), 3);
  /// ```
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.0.limitation()
  }
}

/// A recursion depth tracker that prevents stack overflow in recursive parsers.
///
/// `RecursionLimiter` helps protect against infinite recursion by tracking the current
/// recursion depth and enforcing a maximum depth limit. This is essential for parsers
/// that use recursive descent, as deeply nested or circular grammar rules can easily
/// cause stack overflow.
///
/// # Two Defaults, Two Subjects
///
/// [`new`](Self::new) and [`Default`] give this type its own general-purpose depth, **500**: a
/// generous, unmeasured ceiling that assumes nothing about what one level of "recursion" costs.
/// That is what a caller reaching for this type directly gets — including a lexer's `State`/
/// `Extras` nesting tracker (see `Integration with tokora` below), where a level costs no native
/// stack at all, so no stack-safety math applies to it.
///
/// **tokora's own Pratt-parser wiring does not use that default.**
/// [`ParserContext`](crate::ParserContext) and the input layer each request
/// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) explicitly instead of inheriting
/// [`new`](Self::new)'s, because on that path a level IS a live native-stack frame, and 500 of
/// those is not safe everywhere a parse runs. The combined
/// [`Limiter`](crate::state::tracker::Limiter) is not part of that wiring — tokora's own parser
/// never builds its recursion budget through it — so its constructors inherit this same
/// general-purpose 500 rather than requesting the parser's own.
///
/// That constant is **32** in a debug build and **256** in a release one, under every feature, and
/// the whole derivation — including why it was 16, why 16 was measured at the wrong door, why 64
/// was wrong before that, and why no feature is allowed to move it — is on
/// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) rather than repeated here. What belongs here
/// is the measurement table it is *not* derived from, because that table is this type's own and
/// keeps being mistaken for the answer:
///
/// Measured on this tree, one pratt frame per level of nesting, bisected to the last depth that
/// completes on an explicitly sized **2 MiB** thread — the stack Rust gives every
/// `std::thread::spawn` and every libtest harness thread, and so the smallest stack a parse is
/// likely to get — before the native stack aborts the process:
///
/// | build | typed driver | token driver |
/// |---|---|---|
/// | release (`opt-level = 3`), 2 MiB thread | **3871** frames, ~0.53 KiB each | **4247** frames, ~0.48 KiB each |
/// | debug (`opt-level = 0`), 2 MiB thread | **384** frames, ~5.3 KiB each | **125** frames, ~16.4 KiB each |
///
/// The binding cell of *this* table is the **debug token driver at 125**, not the release figures
/// above it, and sizing the parser's default against the release ceiling was one mistake it
/// corrected: an unconfigured parse in a debug build — which is what every test suite runs —
/// reached the native stack before the limiter. **Sizing it against this table at all was the
/// second mistake**, because these are tokora's frames and a consumer's grammar runs *inside*
/// them; that is what `PARSE_DEFAULT_DEPTH` records, and it is why the shipped figure is derived
/// from a consumer measurement of **51** rather than from the 125 here.
///
/// **And the 51 is a reading of that consumer's *syntactic* door, which spends no level of this
/// counter at all** — the door the budget is enforced on has since been measured separately and is
/// roughly an order of magnitude cheaper per level. The shipped default is no longer derived from
/// either row alone: it takes headroom against the populations that *spend* the cell and a bare fit
/// against the heaviest one that is only *modelled*, which is what 51 now prices.
/// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) carries the derivation and this module's
/// private `measured` carries every row.
///
/// The two failure modes are not symmetric, and that is what settles the direction to err in. A
/// limit that is too *low* returns a clean, catchable, documented
/// [`RecursionLimitReached`](crate::error::RecursionLimitReached) telling the caller to raise it.
/// A limit that is too *high* aborts the process with no diagnostic at all and takes the whole
/// suite with it. Only one of those can be recovered from, so the parser's own default is set
/// where **every** measured configuration survives.
///
/// This type's own 500 was never sized against anything, and that is not an oversight: nothing
/// about tallying lexer nesting, or an arbitrary caller's own notion of "depth", implies a
/// native-stack cost for this type to protect against, so there is no stack ceiling for 500 to
/// clear. It only has to stay out of the way of ordinary use while still refusing a runaway
/// count eventually, and it did exactly that for this type before the measurement above existed.
///
/// **A grammar that parses untrusted, deeply nested input should still set its own limit** with
/// [`with_limitation`](Self::with_limitation) rather than inherit either default — especially on
/// a worker thread, where the stack is 2 MiB and not the main thread's 8 MiB. The figures above
/// are this crate's pratt frames on one platform and one toolchain; a grammar whose own recursive
/// combinators sit between them spends more per level, and a build with overflow checks, extra
/// debuginfo or a different codegen unit count spends more again. Pick the limit against the
/// stack the parse will actually run on.
///
/// # Use Cases
///
/// - **Recursive descent parsers**: Track depth through grammar rules
/// - **AST traversal**: Prevent stack overflow on deeply nested trees
/// - **Expression evaluation**: Limit nesting in arithmetic/boolean expressions
/// - **Stateful lexers**: Track depth in the lexer's `Extras` state
///
/// # Integration with tokora
///
/// **This is the cell the parse itself descends through.** Every parse input carries one,
/// configured through
/// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
/// (or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter))
/// and defaulting to [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) — requested explicitly by
/// that wiring, and NOT [`new`](Self::new)'s own 500.
/// Both Pratt engines enter one level per live frame through
/// [`InputRef::descend`](crate::InputRef::descend), whose [`Descent`](crate::input::Descent)
/// guard releases the level on every exit including an unwind; exceeding the limit fails the
/// parse with the always-terminal [`RecursionLimitReached`](crate::error::RecursionLimitReached).
/// The budget is *per input*, not per parser, so two pratt parsers composed into one grammar
/// share one depth budget. A hand-written recursive combinator draws on the same cell through
/// [`InputRef::descending`](crate::InputRef::descending), which is the guard's scoped form and
/// the one to prefer.
///
/// It is deliberately **not** part of the checkpoint set: depth is a fact about the control
/// stack, and a rollback happens at the same frame depth as the save it returns to.
///
/// Separately, `RecursionLimiter` can also be used as part of a Logos lexer's `Extras` state by
/// implementing the [`State`] trait, allowing you to track nesting during *lexing*. That is a
/// different cell with a different subject: the lexer's tally is monotone in the input and its
/// trip latches the poison boundary, while the parse's descent unwinds — and it is exactly why a
/// tracker built through [`new`](Self::new) (or `#[derive(Default)]`, as the example below does)
/// gets 500 rather than [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH): lexing spends no
/// native-stack frame per nesting level, so the stack-safety derivation has nothing to say about
/// it, and this type does not presume a caller reaching for it directly is on the parser's path.
///
/// # Examples
///
/// ## Basic Usage
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let mut limiter = RecursionLimiter::new();
///
/// limiter.increase(); // Enter recursion
/// assert_eq!(limiter.depth(), 1);
///
/// limiter.increase(); // Go deeper
/// assert_eq!(limiter.depth(), 2);
///
/// limiter.decrease(); // Return from recursion
/// assert_eq!(limiter.depth(), 1);
///
/// limiter.decrease();
/// assert_eq!(limiter.depth(), 0);
/// ```
///
/// ## Custom Limit
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// // Allow deeper nesting for complex grammars
/// let mut limiter = RecursionLimiter::with_limitation(1000);
///
/// assert_eq!(limiter.limitation(), 1000);
/// ```
///
/// ## Checking Limits
///
/// ```rust
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// let mut limiter = RecursionLimiter::with_limitation(5);
///
/// for _ in 0..5 {
///     limiter.increase();
///     assert!(limiter.check().is_ok()); // Still within limit
/// }
///
/// limiter.increase(); // One too many
/// assert!(limiter.check().is_err()); // Limit exceeded!
/// ```
///
/// ## Recursive Parser Example
///
/// ```rust,ignore
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// fn parse_expr(input: &str, limiter: &mut RecursionLimiter) -> Result<Expr, Error> {
///     limiter.increase();
///     limiter.check()?; // Fail fast if too deep
///
///     let result = match input.chars().next() {
///         Some('(') => {
///             // Recursively parse nested expression
///             let nested = parse_expr(&input[1..], limiter)?;
///             Expr::Paren(Box::new(nested))
///         }
///         Some(c) if c.is_numeric() => Expr::Number(c.to_digit(10).unwrap()),
///         _ => return Err(Error::Unexpected),
///     };
///
///     limiter.decrease(); // Return from recursion
///     Ok(result)
/// }
/// ```
///
/// ## With Logos Lexer State
///
/// ```rust,ignore
/// use logos::Logos;
/// use tokora::state::recursion_tracker::RecursionLimiter;
///
/// #[derive(Default)]
/// struct LexerState {
///     recursion: RecursionLimiter,
/// }
///
/// #[derive(Logos, Debug)]
/// #[logos(extras = LexerState)]
/// enum Token {
///     #[regex(r"\(", |lex| {
///         lex.extras.recursion.increase();
///         lex.extras.recursion.check().ok()
///     })]
///     LParen(()),
///
///     #[regex(r"\)", |lex| {
///         lex.extras.recursion.decrease();
///         Some(())
///     })]
///     RParen,
/// }
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecursionLimiter {
  max: usize,
  current: usize,
}

impl Default for RecursionLimiter {
  #[inline(always)]
  fn default() -> Self {
    Self::new()
  }
}

/// **The measured figures every stack-safety number in this crate is derived from**, named once so
/// that a constant and its derivation cannot drift apart.
///
/// They live here rather than in `native_stack`, which is the module that reads most of them,
/// because that module is gated on the `pratt` feature and
/// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`] is not: a build without `pratt` still has to be one
/// whose default is justified. The full tables, the platform, and the bisection method are on
/// `native_stack`'s module docs and on the constant below.
/// # EVERY FIGURE HERE IS A **DEBUG (`opt-level = 0`)** ONE
///
/// That is not incidental, and it is why the constants derived from them are keyed on
/// `debug_assertions`. A release build's frames are far cheaper — tokora's own table is 125 debug
/// frames against 3 871 / 4 247 release ones on the same thread, ~16.4 KiB against ~0.48–0.53 KiB,
/// a **31× spread covered by one number** if a default is derived from this module and shipped
/// unconditionally. Roughly 90% of the debug figure is `opt-level = 0` stack-slot non-colouring
/// rather than code size, so it does not shrink with a smaller grammar.
///
/// The consumer side has one release reading (~3.6 KiB/level against ~41 KiB debug) and it is
/// **deliberately not a constant here**, because it was taken on one of the five axes the debug
/// bisection covered — not on architecture, the second dialect, its generic angle brackets, or the
/// alternative source representation. A derivation reading it would be exactly the
/// derived-not-measured figure this module's assertions exist to refuse. `policy` therefore ships
/// the release cells as *floors* equal to the debug cells until that sweep is run.
///
/// # EVERY FIGURE IS ALSO OF ONE **DOOR**, AND THE NAMES NOW SAY WHICH
///
/// The consumer's five axes were varied over architecture, dialect, shape and source backing —
/// and held one thing fixed that nobody wrote down: they are all readings of that consumer's
/// **syntactic** door, which spends **no** [`RecursionLimiter`] level at all. Its cost per level
/// is a native frame the budget never sees. The budget is spent on the consumer's **lossless**
/// door, one [`InputRef::descend`](crate::InputRef::descend) per nesting delimiter, and a level
/// there is roughly an order of magnitude cheaper. So a default derived from the syntactic row
/// bounds one population with another's frame cost, which is the same
/// derived-over-the-wrong-population defect that made 64 wrong one layer further in.
///
/// Both rows are therefore named for their door, and neither name is `CONSUMER_ABORTS_AT` any
/// more: a bare "consumer" figure is the thing that got read as though it described the door the
/// budget is enforced on.
pub(crate) mod measured {
  /// A real **consumer** grammar's debug build aborts at this depth on an explicitly sized 2 MiB
  /// thread, on the tightest of its five measured axes (the others are 52, 53, 57 and 60) — at
  /// its **syntactic** door.
  ///
  /// **That door spends no recursion level.** Its cost is a native frame per level of nesting and
  /// nothing the budget can count, cross-checked three ways: a sweep of that consumer's syntactic
  /// tree finds no `descend`/`descending` call; [`RecursionLimitReached`] has exactly one
  /// construction site in this crate ([`InputRef::descend`](crate::InputRef::descend)); and that
  /// site's only in-crate callers are the two Pratt engines, which the consumer does not use.
  /// Confirmed at runtime: under the shipped budget of 16 with the consumer's own lexer tally
  /// lifted, its syntactic door parses 64 nested braces clean while its lossless door refuses at
  /// exactly 16.
  ///
  /// So this figure is **not** the one [`PARSE_DEFAULT_DEBUG`](super::policy::PARSE_DEFAULT_DEBUG)
  /// should be derived from, and today it is. See
  /// [`CONSUMER_LOSSLESS_ABORTS_AT`] for the row that is, and
  /// [`PARSE_DEFAULT_DEBUG`](super::policy::PARSE_DEFAULT_DEBUG) for why the repoint is a decision
  /// rather than an edit.
  ///
  /// What it *is* still the right row for is a **memory** policy over heavy frames — see
  /// [`CONSUMER_SYNTACTIC_BYTES_PER_LEVEL`].
  pub(crate) const CONSUMER_SYNTACTIC_ABORTS_AT: usize = 51;

  /// **The same consumer, the same five axes, pointed at the door the budget is actually spent
  /// on** — one [`InputRef::descend`](crate::InputRef::descend) per nesting delimiter — and the
  /// figure is in *descents*, which is the unit
  /// [`RecursionLimiter::limitation`](super::RecursionLimiter::limitation) is compared against.
  ///
  /// # The bisection
  ///
  /// One parse per **process**, on an explicitly sized 2 MiB thread, greatest descent count that
  /// returns before the next one aborts with `fatal runtime error: stack overflow`. The document
  /// depth a boundary is found at is converted to descents by an in-process oracle: the smallest
  /// ceiling under which that same document parses without a diagnostic, which is exactly how many
  /// levels it spends. Measured `aarch64-apple-darwin`, `opt-level = 0` with `debug-assertions`
  /// and `overflow-checks` on, rustc 1.99.0-nightly (771916f90 2026-08-08):
  ///
  /// | cell (lossless door, 2 MiB, debug) | last descent count that returns | first that aborts |
  /// |---|---|---|
  /// | GraphQL, inline fragments, `str`, aarch64 | 717 | 718 |
  /// | GraphQL, inline fragments, `str`, x86_64 | 805 | 806 |
  /// | GraphQLx, inline fragments, `str`, aarch64 | 715 | 716 |
  /// | GraphQLx, generic angle brackets, `str`, aarch64 | 843 | 844 |
  /// | GraphQL, the recovery bypass `{ ) f {`, `str`, aarch64 | 669 | 670 |
  /// | GraphQLx, the same with `)` and with `>` alike, `str`, aarch64 | **667** | 668 |
  /// | GraphQL, the object-value cycle, `str`, aarch64 | 875 | 876 |
  /// | GraphQL, the list-value cycle, `str`, aarch64 | 1294 | 1295 |
  ///
  /// **The fifth axis has no cell, and that is a fact about the consumer rather than an omission.**
  /// The syntactic row's tightest cell was a `bytes::Bytes` source backing; every one of that
  /// consumer's twelve shipped lossless entry points takes `&str`, so there is no lossless door to
  /// point the backing axis at. If the syntactic row's 0.89x for that backing carried across, the
  /// binding cell would extrapolate to about 593 — which is an extrapolation, is **not** what this
  /// constant records, and is exactly the shape this module refuses.
  ///
  /// **The binding cell is a shape the syntactic five never had.** The recovery bypass costs two
  /// native frames per level (`field` and `selection_set`) while spending one descent, so it is
  /// the tightest cell in the budget's own unit even though it is not the tightest in document
  /// depth. It is the shape that walked past the consumer's first nesting fix, and the consumer's
  /// suite pins it.
  ///
  /// The figures are one host's, and the control says how far one host moves them. Re-running the
  /// same instrument against the *syntactic* door here reproduces that row's recorded 57 / 53 / 52
  /// / 60 at 64 / 60 / 60 / 68 — 12-15% looser, on a newer toolchain and a newer branch of the
  /// consumer, with the same architecture relation (x86_64 the looser of the two) and the same
  /// ordering up to a tie between the two GraphQLx cells. It also reproduces that row's recorded
  /// release reading, "500 levels of the syntactic door need about 1.95 MiB of the 2 MiB
  /// available", at 505. **So each figure here is an expectation to re-check on another host
  /// rather than a constant**, which is what the headroom policy is there to absorb.
  pub(crate) const CONSUMER_LOSSLESS_ABORTS_AT: usize = 667;

  /// The same bisection, the same door, the same host, **release**: `opt-level = 3`,
  /// `debug-assertions = false`, `overflow-checks = false`.
  ///
  /// | cell (lossless door, 2 MiB, release, aarch64) | last that returns | first that aborts |
  /// |---|---|---|
  /// | GraphQL, inline fragments, `str` | 4238 | 4239 |
  /// | GraphQLx, inline fragments, `str` | 3861 | 3862 |
  /// | GraphQLx, generic angle brackets, `str` | **3282** | 3283 |
  /// | GraphQL, the recovery bypass `{ ) f {`, `str` | 5472 | 5473 |
  /// | GraphQLx, the same with `)` and with `>` alike, `str` | 4863 | 4864 |
  /// | GraphQL, the object-value cycle, `str` | 3455 | 3456 |
  /// | GraphQL, the list-value cycle, `str` | 3455 | 3456 |
  ///
  /// **The binding cell is not the debug row's.** Optimisation reorders which shape is tightest:
  /// the generic angle brackets are the *loosest* debug cell and the *tightest* release one. A
  /// release figure extrapolated from debug's ratio would have named the wrong shape as well as
  /// the wrong number, which is the whole reason
  /// [`PARSE_DEFAULT_RELEASE`](super::policy::PARSE_DEFAULT_RELEASE) refused to be raised from one.
  pub(crate) const CONSUMER_LOSSLESS_RELEASE_ABORTS_AT: usize = 3282;

  /// The tightest cell of **tokora's own** table — the debug token driver on the same thread.
  ///
  /// Kept beside the consumer figures precisely because it is the one the old default was sized
  /// against. Note that it is no longer comparable with the row the budget is enforced on: 125 is
  /// a *Pratt frame* figure at ~16.4 KiB, and a lossless descent is ~3.1 KiB, so tokora's own row
  /// being the looser of the two is what a cheaper frame means rather than evidence of an edit.
  pub(crate) const TOKORA_ABORTS_AT: usize = 125;

  /// The tightest cell of **tokora's own release table** — the typed driver, 3 871 frames at
  /// ~0.53 KiB against the token driver's 4 247 at ~0.48 KiB.
  ///
  /// A constant rather than only a row in [`RecursionLimiter`]'s own doc table, because a release
  /// derivation has to read it and a figure that exists only in prose is the thing this module
  /// stopped doing. The two now cannot drift.
  ///
  /// **Note which driver binds, and that it is the other one.** Debug's tightest cell is the
  /// *token* driver at 125; release's is the *typed* driver at 3 871. Optimisation reorders them,
  /// which is the same lesson [`CONSUMER_LOSSLESS_RELEASE_ABORTS_AT`] records about shapes.
  pub(crate) const TOKORA_RELEASE_ABORTS_AT: usize = 3871;

  /// The thread every figure is stated on, and the one `std::thread::spawn` hands out.
  pub(crate) const STACK: usize = 2 * 1024 * 1024;

  /// Bytes of native stack one level of the heaviest measured **syntactic** grammar spends —
  /// **derived** from that bisection rather than written down separately, so the two cannot
  /// disagree. ~41 KiB.
  ///
  /// This is the row a **memory** policy over segmented Pratt frames must be priced at, and that
  /// is why the repoint below does not touch it: a Pratt frame is the heavy kind, and pricing a
  /// segment budget at the cheap lossless descent would multiply
  /// [`SEGMENTED_PRATT_DEBUG`](super::policy::SEGMENTED_PRATT_DEBUG) by 16 as a side effect of
  /// fixing an unrelated default.
  pub(crate) const CONSUMER_SYNTACTIC_BYTES_PER_LEVEL: usize = STACK / CONSUMER_SYNTACTIC_ABORTS_AT;

  /// **The release sweep of the syntactic row**, which this module recorded as owed for four
  /// revisions and which never existed until now — the binding cell of four of its five axes.
  ///
  /// Same instrument, same 2 MiB thread, same one-parse-per-process bisection;
  /// `opt-level = 3`, `debug-assertions = false`, `overflow-checks = false`. Last depth that
  /// returns / first that aborts:
  ///
  /// | cell (syntactic door, 2 MiB, release) | returns | aborts |
  /// |---|---|---|
  /// | GraphQL, inline fragments, `str`, aarch64 | 504 | 505 |
  /// | GraphQLx, inline fragments, `str`, aarch64 | **471** | 472 |
  /// | GraphQLx, generic angle brackets, `str`, aarch64 | 567 | 568 |
  /// | GraphQL, inline fragments, `str`, x86_64 | 513 | 514 |
  /// | GraphQLx, inline fragments, `str`, x86_64 | 490 | 491 |
  ///
  /// The fifth axis — a `bytes::Bytes` source backing — is **not** measured here. On the debug
  /// table that backing cost 0.89x and was the binding cell; the same discount over 471 would be
  /// about 419. That is an extrapolation and is deliberately not this constant.
  ///
  /// **What the missing axis can and cannot change.** This row is read only to price a *fit*, and
  /// the fit is what fixes [`PARSE_DEFAULT_RELEASE`](super::policy::PARSE_DEFAULT_RELEASE) at 256:
  /// the next power of two up needs this cell above 512, and the loosest axis measured is 505. So
  /// the missing axis cannot raise the answer, and to lower it the true binding cell would have to
  /// fall below 256 — a 1.8x miss, twice the spread the whole debug table showed. The answer is
  /// robust to the axis that is absent, which is the only reason recording four is honest.
  pub(crate) const CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT: usize = 471;

  /// Bytes of native stack one level of the heaviest measured syntactic grammar spends in a
  /// **release** build — derived from [`CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT`]. ~4.3 KiB, against
  /// ~41 KiB debug, so a release level of the heavy population is 9.2x cheaper.
  ///
  /// It prices the release half of the *fit* tier. It deliberately does **not** reprice
  /// [`SEGMENTED_PRATT_RELEASE`](super::policy::SEGMENTED_PRATT_RELEASE), which is a memory policy
  /// over segments rather than a thread-stack bound and whose value is its own decision.
  pub(crate) const CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL: usize =
    STACK / CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT;

  /// Bytes of native stack one **descent** of that consumer's lossless door spends, debug —
  /// derived from [`CONSUMER_LOSSLESS_ABORTS_AT`] the same way. ~3.1 KiB.
  pub(crate) const CONSUMER_LOSSLESS_BYTES_PER_LEVEL: usize = STACK / CONSUMER_LOSSLESS_ABORTS_AT;

  /// The same, release — derived from [`CONSUMER_LOSSLESS_RELEASE_ABORTS_AT`]. ~0.6 KiB.
  ///
  /// **This is the figure that unblocks the release arm.** The provisional assertion that keeps
  /// [`PARSE_DEFAULT_RELEASE`](super::policy::PARSE_DEFAULT_RELEASE) at a floor prices the release
  /// budget at the *debug* per-level cost, because until now that was the only cost there was; a
  /// release budget large enough to be worth having does not fit a 2 MiB thread at a debug price
  /// and never could.
  pub(crate) const CONSUMER_LOSSLESS_RELEASE_BYTES_PER_LEVEL: usize =
    STACK / CONSUMER_LOSSLESS_RELEASE_ABORTS_AT;

  /// The descents the **deepest document that consumer actually ships** spends at the door the
  /// budget is enforced on, over the 472 fixtures in its tree.
  ///
  /// Every other figure here is about where a stack dies. This one is about what a real document
  /// needs, and it is the half no arithmetic over the rows above can reach: it is what decides
  /// whether a derived ceiling is comfortable or tight. Measured with the same in-process oracle
  /// the descent conversion uses — the smallest ceiling under which that fixture parses clean —
  /// and it comes out equal to its bracket count, which is what one-descent-per-nesting-delimiter
  /// predicts.
  ///
  /// For scale beside it, and deliberately not constants because they are illustrations rather
  /// than bounds: an ordinary filter query reaches 10 or 11, and `f(where: {a: {b: [1]}})` spends
  /// 5 for what reads as three-deep, because an argument list's `(` is a one-off toll on top of
  /// the object nesting.
  pub(crate) const CONSUMER_DEEPEST_SHIPPED_DOCUMENT: usize = 11;
}

/// **What this crate CHOSE**, given what [`measured`] records — kept apart from it because the two
/// go wrong in different ways and only one of them can be re-measured.
///
/// A number in [`measured`] is answerable to a bisection: if it is wrong, run the bisection again.
/// A number here is answerable to an argument, and the argument is written on it. The reason for
/// the split is the defect this module's assertions exist to catch: a range-shaped guard reads as
/// though it were checking a derivation while in fact admitting every value the argument would
/// have rejected. So each shipped constant below is *derived* — computed from a measurement and a
/// policy — and the published constant is then asserted **equal** to its derivation, rather than
/// merely inside a band around it.
///
/// # A matrix of four cells, not two numbers
///
/// Two axes, and each one is a population whose frames carry a different guarantee:
///
/// | | `stacker` off | `stacker` on |
/// |---|---|---|
/// | debug | [`PARSE_DEFAULT_DEBUG`] — derived | [`SEGMENTED_PRATT_DEBUG`] — derived |
/// | release | [`PARSE_DEFAULT_RELEASE`] — **floor** | [`SEGMENTED_PRATT_RELEASE`] — **floor** |
///
/// Every cell is named and pinned by an `==` of its own. The two release cells are **floors set
/// equal to their debug siblings, and are not derivations**: [`measured`] contains no release
/// bisection *of the population they describe* to derive them from, and inventing one from debug's
/// ratio is the defect. What the split buys today is that filling them in later is a value change
/// and not a refactor — and that no assertion in this file claims a release derivation nobody
/// performed.
///
/// # The two tiers, and why the file has an asymmetry in it
///
/// The two `PARSE_DEFAULT_*` cells are derived by [`policy::two_tier`], which applies
/// [`MIN_HEADROOM`](policy::MIN_HEADROOM) to the tightest population that **spends** a level of the
/// cell and a bare fit — no multiplier — to the heaviest population that is merely **modelled**.
/// That asymmetry is a decision and not a convenience: applying the multiplier to the modelled row
/// as well gives 16, and applying neither tier's opposite gives 128, a value **above** the depth at
/// which tokora's own Pratt driver aborts on the thread every figure here is stated on. 32 is
/// precisely the width of that distinction, and 256 is its release twin.
///
/// The set each tier ranges over is written out at [`policy::SPENDING_MIN_DEBUG`] and asserted
/// beside the derivation, so a third population that starts spending the cell has an obvious place
/// to be added — and so that the syntactic row's exclusion from the headroom tier stays visible
/// rather than inferable. That row spends zero levels; its productions take `descend` nowhere.
pub(crate) mod policy {
  use super::measured;

  /// How many times the shipped parse-side default must fit under the binding measured cell.
  ///
  /// Three, and the three is the whole of what makes 16 rather than 32 the answer: 32 leaves
  /// 1.59× under the consumer row's 51, which is *below* the 1.9× that made 64 look safe on
  /// tokora's own row, and well inside the range another platform's codegen moves.
  pub(crate) const MIN_HEADROOM: usize = 3;

  /// The margin the old default was rejected for not having, in tenths.
  ///
  /// `100` under `125` is 1.25× and was called too thin to survive another platform's codegen;
  /// `64` under `125` is 1.9× and was accepted. Any default this crate ships must clear its
  /// binding cell by at least the accepted figure, and stating it as a constant is what makes the
  /// next person to move a default move this line too.
  ///
  /// It is the *weaker* of the two requirements — [`MIN_HEADROOM`] is 30 tenths — and it is kept
  /// beside it rather than folded into it because it is the figure the rejected default was judged
  /// against, so it is what makes "and it still clears the bar 64 was held to" checkable.
  pub(crate) const MIN_MARGIN_TENTHS: usize = 19;

  /// **The two-tier rule both parse-side defaults are derived by**, and the asymmetry between the
  /// tiers is the decision rather than a convenience.
  ///
  /// - **Tier one — full [`MIN_HEADROOM`] against every population that actually SPENDS the
  ///   cell.** `spending_min` is the tightest of those; see [`SPENDING_MIN_DEBUG`] for who is in
  ///   the set and who is not.
  /// - **Tier two — a bare fit, with no multiplier, at the heaviest MODELLED per-level price.**
  ///   Nothing shipped spends the cell at that price today; the syntactic consumer's ~41 KiB level
  ///   is what a caller's own hand-written [`descending`](crate::InputRef::descending) *could*
  ///   cost, and `descent_tests.rs` models exactly that. A population that is modelled rather than
  ///   observed earns a fit, not a margin.
  ///
  /// **Applying `MIN_HEADROOM` to the modelled row instead gives 16, and applying only the fit
  /// gives what ships. 32 is precisely the width of that distinction.** A future reader who
  /// collapses the two tiers into one will move the default in one direction or the other without
  /// noticing, which is how this constant reached the wrong door in the first place.
  const fn two_tier(spending_min: usize, heaviest_modelled_bytes: usize) -> usize {
    let mut depth = 1;
    while depth * 2 * MIN_HEADROOM < spending_min
      && depth * 2 * heaviest_modelled_bytes < measured::STACK
    {
      depth *= 2;
    }
    depth
  }

  /// The tightest population that **spends** a level of this cell in a debug build, named rather
  /// than minimised so the assertion beside it has something to be wrong about.
  ///
  /// The set is two rows and the exclusion is the point:
  ///
  /// - [`measured::TOKORA_ABORTS_AT`] — tokora's own Pratt engines, which take
  ///   [`descend`](crate::InputRef::descend) at their two prologues;
  /// - [`measured::CONSUMER_LOSSLESS_ABORTS_AT`] — the measured consumer's lossless door, one
  ///   level per nesting delimiter;
  /// - **and NOT [`measured::CONSUMER_SYNTACTIC_ABORTS_AT`]**, because that door spends zero
  ///   levels. Its productions call `descend` nowhere, which is why 51 sizes the *fit* tier and
  ///   never the headroom tier.
  ///
  /// tokora's own row is the tighter of the two in debug. It is not in release — see
  /// [`SPENDING_MIN_RELEASE`], where the lossless row binds instead.
  pub(crate) const SPENDING_MIN_DEBUG: usize = measured::TOKORA_ABORTS_AT;

  /// [`SPENDING_MIN_DEBUG`]'s release twin, over the release halves of the same two rows — and it
  /// is the **other** row that binds.
  ///
  /// tokora's release Pratt row is 3 871 and the consumer's release lossless row is 3 282, so the
  /// consumer binds here where tokora binds in debug. Which member is the minimum is therefore not
  /// a fact about the crate, and a guard that assumed it would be reading one profile's accident.
  pub(crate) const SPENDING_MIN_RELEASE: usize = measured::CONSUMER_LOSSLESS_RELEASE_ABORTS_AT;

  /// **The parse-side default in a debug build, derived** by [`two_tier`] over
  /// [`SPENDING_MIN_DEBUG`] and [`measured::CONSUMER_SYNTACTIC_BYTES_PER_LEVEL`].
  ///
  /// **32.** Tier one: `32 × 3 = 96 < 125`, and `64 × 3 = 192` is not. Tier two:
  /// `32 × 41 120 B = 1.31 MiB < 2 MiB`, and `64 × 41 120 B = 2.51 MiB` is not. **Both tiers refuse
  /// the next doubling here**, which is why the debug arm is the one where the rule is easy to
  /// mistake for a single criterion. The release arm is refused by tier two alone.
  ///
  /// A power of two because every default this crate has shipped has been one and a reader expects
  /// it, and the *deepest* one because the asymmetry the constant's docs argue says to err low,
  /// not to err small.
  ///
  /// # Why it is 32 and was 16, and why 128 was refused
  ///
  /// 16 came from applying `MIN_HEADROOM` to `CONSUMER_SYNTACTIC_ABORTS_AT`, a reading of a door
  /// that spends nothing here — the defect #297 recorded. Pointing that same single-tier formula
  /// at the row the budget *is* spent on gives 128, and 128 is worse than wrong: it is **above
  /// [`measured::TOKORA_ABORTS_AT`]**, so an unconfigured Pratt parse in a debug build would reach
  /// the native stack before the limiter — `128 × 16 112 B` measured is 98.3% of a 2 MiB thread —
  /// which is the abort the 500 → 64 → 16 sequence exists to delete, restated with a bigger number.
  ///
  /// What both of those got wrong is the same thing in opposite directions: **this cell is shared,
  /// and no single row bounds it.** The two tiers are what say so.
  pub(crate) const PARSE_DEFAULT_DEBUG: usize = two_tier(
    SPENDING_MIN_DEBUG,
    measured::CONSUMER_SYNTACTIC_BYTES_PER_LEVEL,
  );

  /// **The parse-side default in a release build — now a DERIVATION, not a floor.**
  ///
  /// **256**, by the same [`two_tier`] rule over [`SPENDING_MIN_RELEASE`] and
  /// [`measured::CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL`]. Tier one: `256 × 3 = 768 < 3282`
  /// — and so is `512 × 3 = 1536`, so **tier one does not refuse the next doubling**. Tier two
  /// does: `256 × 4 452 B = 1.09 MiB < 2 MiB`, `512 × 4 452 B = 2.17 MiB` is not.
  ///
  /// So the release arm is fixed by the fit and the debug arm by the headroom, and each tier is
  /// the binding one somewhere. Deleting either changes a shipped number.
  ///
  /// It is a floor no longer because the sweep it owed exists: see
  /// [`measured::CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT`] and
  /// [`measured::CONSUMER_LOSSLESS_RELEASE_ABORTS_AT`]. The provisional `==` assertion that held it
  /// equal to the debug cell is deleted with it.
  ///
  /// **1024 was refused, and by a guard rather than by an argument.** The single-tier derivation
  /// over the release lossless row gives 1024, which collides exactly with
  #[cfg_attr(
    feature = "stacker",
    doc = "[`SEGMENTED_PRATT_RELEASE`] — so `stacker` would have bought zero extra depth in a"
  )]
  #[cfg_attr(
    not(feature = "stacker"),
    doc = "`SEGMENTED_PRATT_RELEASE` (`stacker` only) — so that feature would have bought zero \
           extra depth in a"
  )]
  /// release build, which is the one thing the segmented constant exists to be.
  pub(crate) const PARSE_DEFAULT_RELEASE: usize = two_tier(
    SPENDING_MIN_RELEASE,
    measured::CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL,
  );

  /// The cell this build selects, and the value
  /// [`RecursionLimiter::PARSE_DEFAULT_DEPTH`](super::RecursionLimiter::PARSE_DEFAULT_DEPTH) is
  /// pinned to.
  ///
  /// # `debug_assertions` is a **proxy** for the profile, and the caveat is live
  ///
  /// It is the only profile signal a constant can read, and it is not `opt-level`: a build with
  /// `debug-assertions = false` at `opt-level = 0`, or the reverse, gets the other arm. It is also
  /// *this crate's* flag — a per-package profile override that compiles tokora differently from the
  /// consumer's grammar makes the two disagree about what a frame costs.
  ///
  /// **Both arms are currently the same number, so neither can be observed today.** Whoever raises
  /// [`PARSE_DEFAULT_RELEASE`] makes both live in the same commit, and owes this paragraph a
  /// decision rather than a re-reading.
  pub(crate) const PARSE_DEFAULT: usize = if cfg!(debug_assertions) {
    PARSE_DEFAULT_DEBUG
  } else {
    PARSE_DEFAULT_RELEASE
  };

  /// **The one number the `stacker` feature actually gets to choose**: how much stack-segment
  /// memory an unconfigured segmented parse may reach before the budget refuses it.
  ///
  /// With segments there is no native ceiling left for a depth to clear, so the bound stops being
  /// a hardware fact and becomes a memory policy — and this is that policy, stated in bytes,
  /// which is the unit the policy is actually about. The depth follows from it and the binding
  /// per-level cost; it is not chosen separately.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENT_MEMORY_CEILING: usize = 64 * 1024 * 1024;

  /// **The segmented-Pratt budget in a debug build, derived**: the deepest power of two whose
  /// segments, at the binding per-level cost, still fit inside [`SEGMENT_MEMORY_CEILING`].
  ///
  /// 1024, from 64 MiB and ~41 KiB — `1024 × 41 KiB ≈ 40 MiB`, and `2048` is ≈80 MiB, past it.
  ///
  /// Same job as [`PARSE_DEFAULT_DEBUG`] and for the same reason: the guard this replaces accepted
  /// anything from 501 to 1632, so 512 passed while contradicting the documented figure.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT_DEBUG: usize = {
    let mut depth = 1;
    while depth * 2 * measured::CONSUMER_SYNTACTIC_BYTES_PER_LEVEL <= SEGMENT_MEMORY_CEILING {
      depth *= 2;
    }
    depth
  };

  /// **The segmented-Pratt budget in a release build — still a FLOOR, and no longer for the reason
  /// [`PARSE_DEFAULT_RELEASE`] used to give.**
  ///
  /// That one was a floor because the sweep it needed had never been run. It has been, so that arm
  /// is a derivation now and this one is the last floor in the file — but not because a figure is
  /// missing. [`measured::CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL`] exists, and deriving this
  /// cell from it gives **8192**.
  ///
  /// It stays a floor because that would be a different decision from the one the release
  /// measurement licensed. This budget is a **memory** policy over heap segments, not a
  /// thread-stack bound: a cheaper release level does not buy headroom under a wall, it buys more
  /// segment memory reached before the refusal. 8192 levels at the release per-level cost is a
  /// parse allowed to touch roughly half a gigabyte of segments, and
  /// [`SEGMENT_MEMORY_CEILING`] — 64 MiB, stated in bytes precisely because bytes are what this
  /// policy is about — is the number that would have to move for that to be intended.
  ///
  /// So the derivation is one line and it is deliberately not written. The `const` assertion below
  /// says the same thing where a reader who skips docs will meet it.
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT_RELEASE: usize = SEGMENTED_PRATT_DEBUG;

  /// The cell this build selects, and the value
  /// [`RecursionLimiter::SEGMENTED_PRATT_DEPTH`](super::RecursionLimiter::SEGMENTED_PRATT_DEPTH)
  /// is pinned to. Same `debug_assertions` caveat as [`PARSE_DEFAULT`].
  #[cfg(feature = "stacker")]
  pub(crate) const SEGMENTED_PRATT: usize = if cfg!(debug_assertions) {
    SEGMENTED_PRATT_DEBUG
  } else {
    SEGMENTED_PRATT_RELEASE
  };
}

/// The derivation of [`RecursionLimiter::PARSE_DEFAULT_DEPTH`], enforced by the compiler.
///
/// These were `#[test]`s for one revision and clippy was right to refuse them: every operand is a
/// constant, so the question is settled at compile time and a *test* is the weaker place to settle
/// it. As `const` assertions a tree whose default contradicts its own measurement does not build,
/// which is a stronger guarantee than one whose test suite reddens — and it holds in every leg,
/// including the ones that never run a test.
///
/// # Equality first, arithmetic second
///
/// **The first assertion of each pair is an `==`.** For one revision they were all inequalities,
/// and an inequality that reads like a derivation is not one: the guards here imposed only *upper*
/// bounds on the unsegmented default, so a default of **1** satisfied every predicate, and the
/// segmented block accepted **anything from 501 to 1632**, so 512 passed while contradicting the
/// figure the docs published. A claim that "putting the default back to 64 fails the build" was
/// true of 64 and false as the general statement it was offered as.
///
/// So each published constant is now asserted equal to a value in [`policy`] that is *computed*
/// from a measurement and a stated policy, and the arithmetic those derivations rest on is
/// asserted separately, on the named values. Both halves are needed: the equality is what pins the
/// number, and the arithmetic is what stops the derivation being "repaired" by moving a policy
/// figure until it produces whatever the constant already said.
///
/// # Every cell of the matrix, by name
///
/// A single `==` against "whichever arm this build selected" is not a pin, it is the same
/// range-shaped guard with one member: the *other* arm is then unchecked in this build and
/// unchecked in every other, because no build compiles both. So the cells are asserted
/// individually — [`policy::PARSE_DEFAULT_DEBUG`] and [`policy::PARSE_DEFAULT_RELEASE`] each on
/// their own line, in every build, whichever one `debug_assertions` selects — and the selection
/// itself is asserted separately, against the arm the flag names.
///
/// The two release cells get **two** assertions and the pair is deliberate: a standing `>=` that
/// survives the measurement (a release frame is cheaper, so a release budget may never be the
/// *smaller* of the two) and a provisional `==` that records that no release derivation has been
/// performed. The second is the line to delete when it has been.
///
/// The cost is the message: `panic!` in a const context takes a string literal and no arguments, so
/// these cannot print the numbers they compared. They name what to go and read instead.
const _: () = {
  use measured::{
    CONSUMER_LOSSLESS_ABORTS_AT, CONSUMER_LOSSLESS_RELEASE_ABORTS_AT,
    CONSUMER_SYNTACTIC_BYTES_PER_LEVEL, CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL, STACK,
    TOKORA_ABORTS_AT, TOKORA_RELEASE_ABORTS_AT,
  };
  use policy::{
    MIN_HEADROOM, MIN_MARGIN_TENTHS, PARSE_DEFAULT, PARSE_DEFAULT_DEBUG, PARSE_DEFAULT_RELEASE,
    SPENDING_MIN_DEBUG, SPENDING_MIN_RELEASE,
  };

  // THE NUMBER, PINNED — both arms, and by BOTH tiers. Not "inside a band the derivation would
  // tolerate": within each tier, and blocked from the next doubling by at least one of them. The
  // third clause is what makes it a derivation rather than a bound satisfied by 1, and it is a
  // disjunction because which tier refuses the doubling differs by profile: tier one for debug,
  // tier two for release. 16, 64 and 128 all fail the debug line; 1024 fails the release one.
  assert!(
    PARSE_DEFAULT_DEBUG * MIN_HEADROOM < SPENDING_MIN_DEBUG
      && PARSE_DEFAULT_DEBUG * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL < STACK
      && (PARSE_DEFAULT_DEBUG * 2 * MIN_HEADROOM >= SPENDING_MIN_DEBUG
        || PARSE_DEFAULT_DEBUG * 2 * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL >= STACK),
    "PARSE_DEFAULT_DEBUG is not the number its own two-tier derivation produces — MIN_HEADROOM \
     under the tightest population that SPENDS the cell, a bare fit at the heaviest MODELLED \
     per-level price, and the deepest power of two meeting both. See `policy::two_tier`: the two \
     tiers are not interchangeable and collapsing them moves this number."
  );
  assert!(
    PARSE_DEFAULT_RELEASE * MIN_HEADROOM < SPENDING_MIN_RELEASE
      && PARSE_DEFAULT_RELEASE * CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL < STACK
      && (PARSE_DEFAULT_RELEASE * 2 * MIN_HEADROOM >= SPENDING_MIN_RELEASE
        || PARSE_DEFAULT_RELEASE * 2 * CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL >= STACK),
    "PARSE_DEFAULT_RELEASE is not the number the same two-tier derivation produces over the \
     release rows. It is a derivation now rather than a floor, so it has the same obligation the \
     debug arm has."
  );

  // THE STANDING ORDER between the two profiles, and the one that survives the release sweep. A
  // release frame is cheaper than a debug one, so a release budget may be larger and may never be
  // smaller; getting this backwards ships debug-sized frames against a release-sized budget.
  assert!(
    PARSE_DEFAULT_RELEASE >= PARSE_DEFAULT_DEBUG,
    "the release parse default is below the debug one, which inverts the only thing known about \
     the two profiles: a release frame is cheaper, so its budget can only be the larger"
  );
  // THE PROVISIONAL LINE IS GONE. It held the release arm equal to the debug one and said so:
  // "raising it means running the five-axis bisection, not extrapolating from debug's ratio."
  // The bisection was run — `measured::CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT` and its lossless
  // sibling — so the arm is a derivation and the two now differ. What survives is the standing
  // `>=` above, which is the half that was never provisional.

  // And the selection, so that the published constant is the arm this build's flag names rather
  // than whichever one somebody typed.
  assert!(
    PARSE_DEFAULT
      == if cfg!(debug_assertions) {
        PARSE_DEFAULT_DEBUG
      } else {
        PARSE_DEFAULT_RELEASE
      },
    "PARSE_DEFAULT does not select the cell `debug_assertions` names"
  );
  assert!(
    RecursionLimiter::PARSE_DEFAULT_DEPTH == PARSE_DEFAULT,
    "PARSE_DEFAULT_DEPTH is not the value `policy` derives for this build. If the shipped default \
     should move, move the measurement or the policy it is derived from; editing the published \
     constant alone is the defect this assertion exists for."
  );

  // THE WHOLE ORIGINAL DEFECT, AS ONE INEQUALITY, now stated over the derived value and over the
  // row each arm is actually held to. `PARSE_DEFAULT_DEPTH` was 64 against a binding cell of 51,
  // so the native stack got there before the limiter could and the refusal the budget exists to
  // produce was unreachable. Stated on SPENDING_MIN rather than on a fixed row, because which
  // population is tightest is a per-profile fact.
  assert!(
    PARSE_DEFAULT_DEBUG * MIN_MARGIN_TENTHS <= SPENDING_MIN_DEBUG * 10
      && PARSE_DEFAULT_RELEASE * MIN_MARGIN_TENTHS <= SPENDING_MIN_RELEASE * 10,
    "a derived default does not clear the tightest population that spends the cell by \
     MIN_MARGIN_TENTHS, the margin this crate already requires of such a number. See \
     PARSE_DEFAULT_DEPTH's derivation."
  );
  assert!(
    MIN_HEADROOM * 10 > MIN_MARGIN_TENTHS,
    "MIN_HEADROOM has been loosened below MIN_MARGIN_TENTHS, the margin the rejected default was \
     already held to — which would let this crate ship a default it had itself refused"
  );
  // A8, RE-DERIVED RATHER THAN FLIPPED.
  //
  // The old form asserted `TOKORA_ABORTS_AT > CONSUMER_ABORTS_AT` — "the consumer row is the
  // tighter of the two" — and that was never the property worth keeping. It held only because the
  // consumer row was read at a door whose levels are heavier than tokora's own Pratt frames; on
  // the door the budget is actually spent at, the relation inverts for a correct reason (a
  // lossless descent is ~3.1 KiB against tokora's ~16.4 KiB). An assertion kept by flipping its
  // comparison would have been a gate that had stopped gating.
  //
  // What it was protecting is that the headroom tier is taken against the population that BINDS
  // the cell. So: SPENDING_MIN must be a member of the spending set, and no member may be below
  // it. False in both directions — too high fails the first pair of clauses, and a value that is
  // no row at all fails the second.
  //
  // **The syntactic row is deliberately not in this set**, and that is the whole content of #297:
  // that door spends ZERO levels, its productions call `descend` nowhere, so it can price the fit
  // tier and must never price this one. **A third spender goes here**, as another `<=` clause and
  // another disjunct — which is the obvious place precisely because the set is written out.
  assert!(
    SPENDING_MIN_DEBUG <= TOKORA_ABORTS_AT
      && SPENDING_MIN_DEBUG <= CONSUMER_LOSSLESS_ABORTS_AT
      && (SPENDING_MIN_DEBUG == TOKORA_ABORTS_AT
        || SPENDING_MIN_DEBUG == CONSUMER_LOSSLESS_ABORTS_AT),
    "SPENDING_MIN_DEBUG is not the minimum of the populations that spend a level of this cell. \
     The set is tokora's own Pratt engines and the measured consumer's lossless door, and it \
     excludes the syntactic row because that door takes no `descend` at all. Adding a spender \
     means adding a clause here, not widening the constant."
  );
  assert!(
    SPENDING_MIN_RELEASE <= TOKORA_RELEASE_ABORTS_AT
      && SPENDING_MIN_RELEASE <= CONSUMER_LOSSLESS_RELEASE_ABORTS_AT
      && (SPENDING_MIN_RELEASE == TOKORA_RELEASE_ABORTS_AT
        || SPENDING_MIN_RELEASE == CONSUMER_LOSSLESS_RELEASE_ABORTS_AT),
    "SPENDING_MIN_RELEASE is not the minimum of the release halves of the same two rows. Note it \
     is the OTHER member than in debug: a guard that hardcoded which one binds would be reading \
     one profile's accident."
  );

  // **NO `stacker` ARM, and that is the point of this round.** This constant seeds every
  // `InputContext`, and it is read by hand-written public descent through `InputRef::descend` /
  // `descending` — paths `native_stack::maybe_grow` never touches, because its only two callers
  // are the Pratt prologues. So it has to be safe for a consumer who segmented nothing, whatever
  // tokora compiled for itself: re-introducing a `cfg!(feature = "stacker")` arm here fails this
  // line, because 1024 levels of the heaviest measured grammar is ≈41 MiB and the thread is 2.
  //
  // The debug cell is what it is priced against, because `CONSUMER_SYNTACTIC_BYTES_PER_LEVEL` is
  // a debug figure and pricing a release budget with a debug cost is the conservative direction.
  assert!(
    PARSE_DEFAULT_DEBUG * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL < STACK,
    "an unconfigured parse can reach more native stack than the 2 MiB the derivation is stated \
     on. A budget that only a segmented path could survive does not belong on the cell every \
     unsegmented `descend` reads; see RecursionLimiter::SEGMENTED_PRATT_DEPTH."
  );
  // The release arm, priced at the RELEASE heaviest-modelled cost — which is the re-pricing this
  // assertion's own text asked for and which `measured::CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT` now
  // supplies. It is the tier that fixes the release number: 512 fails it where tier one would
  // have allowed 1024.
  assert!(
    PARSE_DEFAULT_RELEASE * CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL < STACK,
    "the release parse default, priced at the heaviest modelled RELEASE per-level cost, no longer \
     fits the 2 MiB thread the derivation is stated on."
  );
  // **AND THE RESIDUE, NAMED BECAUSE NOTHING CAN CHECK IT.** The two arms now differ, so
  // `debug_assertions` is observable for the first time — and it is a proxy for the profile, not
  // for `opt-level`. A build with `debug-assertions = false` at `opt-level = 0` takes the release
  // arm while paying debug frame prices: 256 levels of the heaviest modelled DEBUG level is 10 MiB
  // against a 2 MiB thread. Nothing refuses that. There is no `cfg(opt_level)`; `native_stack`'s
  // red zone is `stacker`-only and sits on the two Pratt prologues, not on the cell this bounds;
  // and `ci/stack_probe.sh` asserts no threshold by design. The available closure is a `build.rs`
  // cfg off cargo's `OPT_LEVEL`, which this crate's build script is already shaped to emit — it is
  // a decision, not an oversight, and this comment is where it is recorded rather than lost.
  //
  // This line states the part that IS checkable: the debug arm remains safe at the debug price,
  // so the residue is confined to the profile combination above and does not reach an ordinary
  // debug or an ordinary release build.
  assert!(
    PARSE_DEFAULT_DEBUG * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL < STACK
      && PARSE_DEFAULT_RELEASE * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL >= STACK,
    "either the debug arm has stopped fitting the 2 MiB thread at the debug price, or the release \
     arm has started fitting it — the second would retire the `debug-assertions = false at \
     opt-level = 0` residue recorded above, and that is a paragraph to delete rather than a line."
  );
};

/// **The measured rows themselves**, and the consumer-side fact no arithmetic over them reaches.
///
/// The block above pins what this crate *chose*. These pin what it is choosing *between*: that the
/// two doors are far enough apart to be two populations rather than one row edited twice, that
/// tokora's own row is not comparable with the lossless one, and that the shipped default still
/// clears a document the measured consumer actually ships.
///
/// The two `PARSE_DEFAULT_*_ON_THE_LOSSLESS_ROW` constants that used to live here are **gone**.
/// They existed to publish what the derivation would give if repointed, while the shipped cells
/// were still derived from the syntactic row. The shipped cells now take their headroom tier from
/// the spending set — which the lossless row is a member of — so a separate "on the lossless row"
/// derivation would be a second derivation of a number this file already derives, and one that no
/// longer describes what ships. Two derivations of one number is how they drift.
const _: () = {
  use measured::{
    CONSUMER_DEEPEST_SHIPPED_DOCUMENT, CONSUMER_LOSSLESS_ABORTS_AT,
    CONSUMER_LOSSLESS_BYTES_PER_LEVEL, CONSUMER_LOSSLESS_RELEASE_ABORTS_AT,
    CONSUMER_LOSSLESS_RELEASE_BYTES_PER_LEVEL, CONSUMER_SYNTACTIC_ABORTS_AT,
    CONSUMER_SYNTACTIC_BYTES_PER_LEVEL, CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT,
    CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL, STACK, TOKORA_ABORTS_AT, TOKORA_RELEASE_ABORTS_AT,
  };
  use policy::{PARSE_DEFAULT_DEBUG, PARSE_DEFAULT_RELEASE};

  // THE DOOR MISMATCH, AS A COMPILED FACT. Measured at ~13x per level; four is a floor loose
  // enough to survive another platform's codegen and tight enough that the two rows converging
  // means one of them was edited to match the other rather than re-measured. If they ever do
  // converge, the argument for two rows has gone with them and this file needs re-reading rather
  // than a wider constant.
  assert!(
    CONSUMER_LOSSLESS_ABORTS_AT > CONSUMER_SYNTACTIC_ABORTS_AT * 4,
    "the syntactic and lossless consumer rows have converged. They are readings of two different \
     doors whose per-level costs differ by roughly an order of magnitude, so convergence means a \
     row was edited rather than re-measured — see `measured`'s header for what each door spends."
  );
  // And the relation that makes tokora's row incomparable with the lossless one, stated so a
  // reader does not reinstate the ordering the old A8 asserted. A Pratt frame is the heavier kind;
  // the lossless row is looser BECAUSE its levels are cheaper, which is the opposite of evidence
  // that something was edited.
  assert!(
    CONSUMER_LOSSLESS_ABORTS_AT > TOKORA_ABORTS_AT,
    "the lossless consumer row has fallen to tokora's own Pratt-frame row, which would mean a \
     lossless descent had become as expensive as a Pratt frame"
  );
  // A release level is cheaper than a debug one, so more of them fit the same thread. Stated once
  // per row now that all three have both halves measured.
  assert!(
    CONSUMER_LOSSLESS_RELEASE_ABORTS_AT > CONSUMER_LOSSLESS_ABORTS_AT,
    "the release lossless row is at or below the debug one, which inverts the only thing known \
     about the two profiles"
  );
  assert!(
    TOKORA_RELEASE_ABORTS_AT > TOKORA_ABORTS_AT,
    "tokora's own release row is at or below its debug one, which inverts the only thing known \
     about the two profiles"
  );
  assert!(
    CONSUMER_SYNTACTIC_RELEASE_ABORTS_AT > CONSUMER_SYNTACTIC_ABORTS_AT
      && CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL < CONSUMER_SYNTACTIC_BYTES_PER_LEVEL,
    "the release syntactic row is at or below the debug one, or its derived per-level cost is not \
     the cheaper of the two — the second cannot disagree with the first unless one was written \
     down rather than derived"
  );

  // **THE REAL FLOOR, replacing the tripwire this file carried for one revision.** That one
  // asserted the shipped default did NOT clear the deepest document the measured consumer ships,
  // and it was right to: it recorded that the case for repointing still stood, and it fired the
  // moment the repoint happened. With the repoint taken, the obligation inverts — the shipped
  // default must now stay clear of the corpus by the same 2x that consumer enforces on its own
  // nesting ceiling at compile time. 32 over 11 is 2.9x; the release arm has far more.
  assert!(
    PARSE_DEFAULT_DEBUG >= CONSUMER_DEEPEST_SHIPPED_DOCUMENT * 2
      && PARSE_DEFAULT_RELEASE >= CONSUMER_DEEPEST_SHIPPED_DOCUMENT * 2,
    "a shipped default no longer clears the deepest document the measured consumer ships by 2x — \
     the floor that consumer holds its OWN ceiling to. Either the default was lowered or the \
     corpus grew; `measured::CONSUMER_DEEPEST_SHIPPED_DOCUMENT` says which."
  );
  // And the budget still fits the 2 MiB thread at the price of the door that actually spends it,
  // in both profiles — the cheap side of the same pair the fit tier states on the heavy side.
  assert!(
    PARSE_DEFAULT_DEBUG * CONSUMER_LOSSLESS_BYTES_PER_LEVEL < STACK
      && PARSE_DEFAULT_RELEASE * CONSUMER_LOSSLESS_RELEASE_BYTES_PER_LEVEL < STACK,
    "a shipped default does not fit the 2 MiB thread even at the lossless per-level cost, which \
     is the cheapest population that spends it"
  );
};

/// The derivation of [`RecursionLimiter::SEGMENTED_PRATT_DEPTH`], on the same terms as the block
/// above: every cell of the profile matrix pinned by name, arithmetic second.
#[cfg(feature = "stacker")]
const _: () = {
  use measured::{
    CONSUMER_SYNTACTIC_ABORTS_AT, CONSUMER_SYNTACTIC_BYTES_PER_LEVEL, TOKORA_ABORTS_AT,
  };
  use policy::{
    SEGMENT_MEMORY_CEILING, SEGMENTED_PRATT, SEGMENTED_PRATT_DEBUG, SEGMENTED_PRATT_RELEASE,
  };

  // 512 fails this, and so did every other value in the 501..=1632 band the inequality-only
  // guards accepted. Both halves again: within the ceiling, and the deepest such power of two.
  assert!(
    SEGMENTED_PRATT_DEBUG * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL <= SEGMENT_MEMORY_CEILING
      && SEGMENTED_PRATT_DEBUG * 2 * CONSUMER_SYNTACTIC_BYTES_PER_LEVEL > SEGMENT_MEMORY_CEILING,
    "SEGMENTED_PRATT_DEBUG is not the number its own derivation produces — the DEEPEST power of \
     two whose segments fit inside SEGMENT_MEMORY_CEILING at the binding per-level cost. Move the \
     memory policy, not the published depth."
  );
  // The same standing order and the same provisional line as the unsegmented pair, for the same
  // two reasons: a cheaper release level fits MORE of itself in the same memory ceiling, so the
  // release cell can only be the larger — and no release per-level figure exists to derive it.
  assert!(
    SEGMENTED_PRATT_RELEASE >= SEGMENTED_PRATT_DEBUG,
    "the release segmented budget is below the debug one, which inverts what a cheaper level does \
     to a fixed memory ceiling"
  );
  // **THE PROVISIONAL LINE THAT STAYS**, and its reason has changed, so read it rather than
  // pattern-matching it against the unsegmented one that was just deleted.
  //
  // That one went because the measurement it was waiting for arrived. This one is NOT waiting on a
  // missing figure any more either — `measured::CONSUMER_SYNTACTIC_RELEASE_BYTES_PER_LEVEL` exists
  // now, and deriving this cell from it would give 8192. It stays because that would be a
  // different decision from the one taken: this budget is a MEMORY policy over heap segments, not
  // a thread-stack bound, so what a cheaper release level buys here is 8x more segment memory
  // reached before the refusal rather than 8x more headroom under a wall. Nobody has decided that
  // 64 MiB should become the effective 512 MiB an 8192-deep segmented parse could touch.
  //
  // So: the figure is available, the derivation is one line, and the line is deliberately not
  // written. Deleting this assertion is how that decision gets taken, and it should be taken
  // knowingly.
  assert!(
    SEGMENTED_PRATT_RELEASE == SEGMENTED_PRATT_DEBUG,
    "the release segmented budget has been raised above the debug one. A release per-level figure \
     now exists to derive it from, so this is no longer blocked on a measurement — it is blocked \
     on a decision about how much segment memory an unconfigured release parse may reach. See \
     SEGMENTED_PRATT_RELEASE and policy::SEGMENT_MEMORY_CEILING."
  );
  assert!(
    SEGMENTED_PRATT
      == if cfg!(debug_assertions) {
        SEGMENTED_PRATT_DEBUG
      } else {
        SEGMENTED_PRATT_RELEASE
      },
    "SEGMENTED_PRATT does not select the cell `debug_assertions` names"
  );
  assert!(
    RecursionLimiter::SEGMENTED_PRATT_DEPTH == SEGMENTED_PRATT,
    "SEGMENTED_PRATT_DEPTH is not the value `policy` derives for this build"
  );

  // With the feature on, the ceiling is a policy choice about memory, so the arithmetic is about
  // memory: the budget must buy a depth the thread stack could not have — otherwise the feature is
  // paying for a ceiling it did not raise.
  assert!(
    SEGMENTED_PRATT_DEBUG > CONSUMER_SYNTACTIC_ABORTS_AT * 4,
    "SEGMENTED_PRATT_DEPTH is not meaningfully past what a 2 MiB thread already gave that \
     grammar, so the feature raised no ceiling"
  );
  // Past BOTH measured rows, not just the consumer one. The consumer row is what the
  // stack-bounded default is derived from, but the claim this feature makes is that neither
  // row bounds it any more, and tokora's own is the larger of the two.
  assert!(
    SEGMENTED_PRATT_DEBUG > TOKORA_ABORTS_AT * 4,
    "SEGMENTED_PRATT_DEPTH does not clear tokora's own measured ceiling by enough to be a policy \
     figure rather than a stack one"
  );
  // And the separation itself: the segmented figure has to be strictly the larger of the two, or
  // there was no reason to name it apart from the default. Stated per profile, because the two
  // pairs move independently once the release sweep lands.
  assert!(
    SEGMENTED_PRATT_DEBUG > policy::PARSE_DEFAULT_DEBUG
      && SEGMENTED_PRATT_RELEASE > policy::PARSE_DEFAULT_RELEASE,
    "SEGMENTED_PRATT_DEPTH is not above PARSE_DEFAULT_DEPTH in one of the two profiles, so the \
     constants have collapsed into one there and the segmented policy buys nothing"
  );
};

impl RecursionLimiter {
  /// tokora's own recursion budget for a parse — **32** in a debug build and **256** in a release
  /// one — requested explicitly by
  /// [`ParserContext`](crate::ParserContext) and the input layer instead of inherited from
  /// [`new`](Self::new).
  ///
  /// # It is one number, and no feature moves it
  ///
  /// For one revision it was `16`, or `1024` with the `stacker` feature, and that was the same
  /// defect as the one below wearing a different hat. This constant seeds **every**
  /// [`InputContext`](crate::input::InputContext), and the cell it seeds is read by hand-written
  /// public descent — [`InputRef::descend`](crate::InputRef::descend) and
  /// [`descending`](crate::InputRef::descending) — from a consumer's *own* productions. Those
  /// frames sit on the ordinary native stack: the segmented prologue is inside tokora's two Pratt
  /// engines and nowhere else, so a feature that segments tokora's frames says nothing whatever
  /// about a caller's. Moving a shared budget to a figure only the segmented path can survive is a
  /// number derived over one population and applied to a wider one, which is precisely what made
  /// 64 wrong.
  ///
  /// The figure that *is* defensible over segmented Pratt frames is published separately, as
  #[cfg_attr(
    feature = "stacker",
    doc = "[`SEGMENTED_PRATT_DEPTH`](Self::SEGMENTED_PRATT_DEPTH), which a caller whose whole"
  )]
  #[cfg_attr(
    not(feature = "stacker"),
    doc = "`SEGMENTED_PRATT_DEPTH` (`stacker` only), which a caller whose whole"
  )]
  /// descent is Pratt frames opts into by hand. Opting in is the point: only the caller knows
  /// whether that precondition holds of their grammar.
  ///
  /// # Why the default is 32, and why it was 16, 64 and 500 before that
  ///
  /// **64 was wrong, and it was wrong in the direction that aborts.** It was derived from the
  /// table in `Two Defaults, Two Subjects` — tokora's own worst cell, the debug token driver's
  /// **125** frames at ~16.4 KiB on a 2 MiB thread — with roughly 1.9× of margin. But that table
  /// measures *this crate's* frames, and a consumer's grammar does not sit beside tokora's
  /// recursion, it sits **inside** it: the productions the pratt driver calls are the consumer's,
  /// so one level of nesting pays for a tokora frame *and* a consumer frame, and the number to
  /// size against is the sum.
  ///
  /// **16 was wrong in the other direction, and for a subtler reason: it was measured at the wrong
  /// door.** It came from a real consumer grammar aborting at **51** levels, debug, on the same
  /// 2 MiB thread — the tightest of five axes over architecture, dialect, shape and source
  /// backing. Those five held one thing fixed that nobody wrote down: every reading is of that
  /// consumer's **syntactic** door, which takes no [`descend`](crate::InputRef::descend) and
  /// therefore spends none of this budget. What spends it is that consumer's **lossless** door, one
  /// level per nesting delimiter, where a level costs about eleven times less. So the shipped
  /// default bounded one population with a different, far heavier one's frame cost.
  ///
  /// # The rule that produced 32, and why it has two tiers
  ///
  /// **Pointing the old single-tier formula at the right row gives 128, and 128 aborts.** It is
  /// *above* tokora's own 125, so an unconfigured Pratt parse in a debug build would reach the
  /// native stack before the limiter — measured, 128 × 16 112 B is 98.3% of a 2 MiB thread — which
  /// is the failure the whole 500 → 64 → 16 sequence exists to delete, restated with a bigger
  /// number. **This cell is shared, and no single row bounds it.**
  ///
  /// Three populations read it, and their per-level costs span 68×: tokora's two Pratt engines
  /// (~16.4 KiB), a caller's own hand-written [`descending`](crate::InputRef::descending)
  /// (modelled at the measured heaviest, ~41 KiB), and a lossless consumer's descent (~3.1 KiB).
  /// The derivation therefore applies **full headroom to every population that actually spends the
  /// cell**, and **a bare fit, with no multiplier, to the heaviest population that is only
  /// modelled**. That asymmetry is the decision: give the modelled row headroom too and the answer
  /// is 16; give the spending rows only a fit and it is 128.
  ///
  /// **32** is the deepest power of two meeting both — `32 × 3 = 96 < 125`, and
  /// `32 × 41 120 B = 1.31 MiB < 2 MiB`, with 64 refused by both. It is double what 0.10.0 shipped
  /// before this change, it clears every population that spends it by at least 3×, and it is the
  /// first value of this constant derived from the door the budget is actually enforced on.
  ///
  /// **The release arm is 256 and is now a derivation rather than a floor.** The same rule over the
  /// release rows: the fit refuses 512 at ~4.3 KiB per modelled level, where headroom alone would
  /// have allowed 1024 — a value that collides exactly with the segmented budget and would have
  /// made `stacker` buy nothing in a release build. So each tier is the binding one in one profile.
  ///
  /// # What it costs a real document
  ///
  /// Measured on the same consumer: its deepest shipped fixture spends **11** of these levels over
  /// 472 fixtures, and an ordinary filter query 10 or 11 — an argument list's `(` is a one-off
  /// toll, so `f(where: {a: {b: [1]}})` spends 5 for what reads as three-deep. 32 leaves **2.9×**
  /// over the deepest document that exists, which clears the 2× floor that consumer enforces on its
  /// *own* nesting ceiling at compile time; 16 left 1.45× and escaped that floor only because the
  /// binding number was this one rather than the consumer's.
  ///
  /// **A grammar that legitimately nests deeper must still say so** — with
  /// [`with_limitation`](Self::with_limitation) through
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter),
  /// [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter), or —
  /// for a lossless parse, whose driver builds its own context —
  #[cfg_attr(
    feature = "rowan",
    doc = "[`parse_lossless_with_context`](crate::cst::parse_lossless_with_context)."
  )]
  #[cfg_attr(
    not(feature = "rowan"),
    doc = "`cst::parse_lossless_with_context` (`rowan` only)."
  )]
  /// That asymmetry is
  /// deliberate and is the same one the type's docs argue: too low returns a clean, catchable
  /// [`RecursionLimitReached`](crate::error::RecursionLimitReached) naming the knob that raises it;
  /// too high aborts the process and takes the caller's whole program with it. Only one of the two
  /// can be recovered from.
  ///
  /// **`stacker` is not the answer to "I need more depth" either**, and that is a change from the
  /// revision that introduced it: enabling the feature no longer moves this number. It moves what
  /// *tokora's own Pratt frames* cost, and publishes the budget that fact justifies for a caller
  /// to request explicitly:
  #[cfg_attr(
    feature = "stacker",
    doc = "[`SEGMENTED_PRATT_DEPTH`](Self::SEGMENTED_PRATT_DEPTH)."
  )]
  #[cfg_attr(
    not(feature = "stacker"),
    doc = "`SEGMENTED_PRATT_DEPTH`, published only when that feature is on."
  )]
  ///
  /// # It is keyed on the build profile, and for the first time the two cells differ
  ///
  /// A debug frame and a release frame are not the same size — tokora's own table is ~16.4 KiB
  /// against ~0.48–0.53 KiB, a 31× spread — so the constant selects on `debug_assertions`: **32**
  /// in a debug build, **256** in a release one. For every previous revision both arms held the
  /// same number and the selection could not be observed. It can now, and that has a cost worth
  /// stating plainly.
  ///
  /// ## The residue: `debug-assertions = false` at `opt-level = 0`
  ///
  /// **`debug_assertions` is a proxy for the profile, and it is not `opt-level`.** A build that
  /// turns debug assertions off while leaving optimisation at zero takes the **release** arm — 256
  /// — while its frames still cost debug prices. 256 levels of the heaviest modelled debug level is
  /// ≈10 MiB against a 2 MiB thread. **Nothing refuses that**, and the list of things that were
  /// checked and cannot is short: there is no `cfg(opt_level)` for a constant to read;
  /// `native_stack`'s red zone is `stacker`-only and sits on the two Pratt prologues rather than on
  /// the cell this bounds; and `ci/stack_probe.sh` asserts no threshold by design. While both arms
  /// held one number this combination was refused at compile time by the assertion that priced the
  /// release arm at the debug cost; that refusal is what diverging spends.
  ///
  /// It is confined: an ordinary debug build and an ordinary release build are both safe at their
  /// own prices, and a `const` assertion says so. The same caveat also covers a **per-package
  /// profile override** that compiles tokora differently from the consumer's grammar, which no
  /// signal available to this crate can see.
  ///
  /// **The closure exists and has not been taken.** Cargo passes `OPT_LEVEL` to build scripts, and
  /// this crate already has one that emits cfgs; reading it and selecting on that instead of on
  /// `debug_assertions` would make the proxy exact. That is a decision rather than an oversight,
  /// and it is recorded here so it does not have to be rediscovered.
  ///
  /// **A caller who cannot accept the residue sets the budget rather than inheriting it** — the
  /// knobs are the ones listed above, and passing an explicit
  /// [`with_limitation`](Self::with_limitation) makes the profile irrelevant.
  ///
  pub const PARSE_DEFAULT_DEPTH: usize = policy::PARSE_DEFAULT;

  /// The recursion budget that is defensible **when every level of the descent is a segmented
  /// Pratt frame** — 1024, and `stacker`-only because that is the feature that makes the sentence
  /// true.
  ///
  /// Nothing installs it. It is a figure to hand to
  /// [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter)
  /// or [`ParserContext::with_recursion_limiter`](crate::ParserContext::with_recursion_limiter),
  /// and the reason it is opt-in rather than the default is the whole content of the constant:
  /// **only the caller knows whether the precondition holds of their grammar.**
  ///
  /// # The precondition, stated as narrowly as it actually is
  ///
  /// `stacker` puts `stacker::maybe_grow` on the frame prologue of tokora's **two Pratt engines**,
  /// and on nothing else. A frame that enters within 256 KiB of the end of its stack
  /// continues on a fresh 2 MiB heap segment, so for those frames the ceiling stops being a
  /// hardware constant.
  ///
  /// It is **not** on [`InputRef::descend`](crate::InputRef::descend) or
  /// [`descending`](crate::InputRef::descending) as a consumer calls them from their own
  /// productions. Those frames are ordinary native frames on an ordinary thread stack, they draw
  /// on the *same* shared budget cell, and 1024 of them at the heaviest measured per-level cost is
  /// ≈41 MiB against a 2 MiB thread — the native abort this crate's whole recursion story exists
  /// to delete. So: pass this constant when the deep part of your grammar is
  /// [`pratt`](crate::parser::pratt) or [`InputRef::pratt`](crate::InputRef::pratt) frames,
  /// and do not pass it because a build happens to have the feature on.
  ///
  /// # Why 1024
  ///
  /// With segments there is no native ceiling left to clear, so the bound is a **policy choice
  /// about memory** and is derived as one: 64 MiB of stack segments, at the binding ~41 KiB per
  /// level from the consumer row of the measurement table, is 1024 levels (2048 would be ≈80 MiB).
  /// That is generous for an unconfigured parse, bounded, reached by no realistic document, and
  /// 20× the depth at which that same grammar aborts *without* the feature — which is the whole of
  /// what the feature buys.
  ///
  /// It is keyed on `debug_assertions` for the reason
  /// [`PARSE_DEFAULT_DEPTH`](Self::PARSE_DEFAULT_DEPTH) is — a cheaper release level fits more of
  /// itself inside the same 64 MiB — and the release cell is likewise a floor equal to the debug
  /// one until the per-level table exists to derive it from.
  ///
  /// # What it is NOT
  ///
  /// **It does not make depth unlimited, and it is not decoration under a feature that already
  /// solved the problem.** A segment is a heap allocation; stacker's allocator is `mmap` plus an
  /// assertion, not a fallible reserve; so a deep enough input still ends the process, with
  /// nothing on any `Result` channel for a caller to catch and nothing for
  /// [`MaybeTerminal`](crate::error::MaybeTerminal) to report. Measured, with the feature on and
  /// the limiter set to [`unlimited`](Self::unlimited): the parse returns `Ok` past every depth a
  /// thread stack could have held — 1 000 000 levels, 5 588 MB, no refusal at any depth — and the
  /// run ends when the machine's memory does.
  ///
  /// So **the budget is what makes a too-deep input refusable, and `stacker` is not a substitute
  /// for it.** A future reader concluding the budget is now redundant, and deleting the
  /// [`InputRef::descend`](crate::InputRef::descend) that spends it, restores the abort both
  /// numbers exist to delete.
  #[cfg(feature = "stacker")]
  #[cfg_attr(docsrs, doc(cfg(feature = "stacker")))]
  pub const SEGMENTED_PRATT_DEPTH: usize = policy::SEGMENTED_PRATT;

  /// Creates a new recursion tracker.
  ///
  /// Defaults to a maximum depth of 500 — this type's own general-purpose ceiling, with no
  /// assumption about what one level costs. tokora's Pratt-parser wiring does not inherit this
  /// default; see the type's `Two Defaults, Two Subjects` docs.
  #[inline(always)]
  pub const fn new() -> Self {
    Self {
      max: 500,
      current: 0,
    }
  }

  /// Creates a new recursion tracker with the given maximum depth.
  #[inline(always)]
  pub const fn with_limitation(max: usize) -> Self {
    Self { max, current: 0 }
  }

  /// Creates a tracker that never trips — `with_limitation(usize::MAX)`.
  ///
  /// "No limit" is spelled rather than implied: a parse configured with this one still counts
  /// its descent, so [`InputRef::recursion`](crate::InputRef::recursion) stays readable, but
  /// [`check`](Self::check) can never fail. Use it to opt a parse out of the default budget
  /// (see [`InputContext::with_recursion_limiter`](crate::input::InputContext::with_recursion_limiter))
  /// when the grammar's depth is bounded by something other than the input.
  ///
  /// # Example
  ///
  /// ```rust
  /// use tokora::state::recursion_tracker::RecursionLimiter;
  ///
  /// let mut limiter = RecursionLimiter::unlimited();
  /// assert_eq!(limiter.limitation(), usize::MAX);
  ///
  /// for _ in 0..10_000 {
  ///   limiter.increase();
  /// }
  /// assert!(limiter.check().is_ok());
  /// ```
  #[inline(always)]
  pub const fn unlimited() -> Self {
    Self::with_limitation(usize::MAX)
  }

  /// Returns the current depth of the recursion.
  #[inline(always)]
  pub const fn depth(&self) -> usize {
    self.current
  }

  /// Returns the maximum depth of the recursion.
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.max
  }

  /// Increase the current depth of the recursion.
  ///
  /// Saturates at `usize::MAX`, mirroring the saturating [`decrease`](Self::decrease).
  #[inline(always)]
  pub const fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  /// Decrease the current depth of the recursion.
  #[inline(always)]
  pub const fn decrease(&mut self) {
    self.current = self.current.saturating_sub(1);
  }

  /// Increases the recursion depth.
  #[inline(always)]
  pub const fn increase_recursion(&mut self) {
    self.increase();
  }

  /// Decrease the current depth of the recursion.
  #[inline(always)]
  pub const fn decrease_recursion(&mut self) {
    self.decrease();
  }

  /// Checks if the recursion limit has been exceeded.
  #[inline(always)]
  pub const fn check(&self) -> Result<(), RecursionLimitExceeded> {
    if self.depth() > self.limitation() {
      Err(RecursionLimitExceeded(*self))
    } else {
      Ok(())
    }
  }
}

impl State for RecursionLimiter {
  type Error = RecursionLimitExceeded;

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    <Self as RecursionTracker>::check(self)
  }
}

/// A recursion tracker trait.
pub trait RecursionTracker {
  /// The error type returned when the recursion limit is exceeded.
  type Error;

  /// Increases the recursion depth.
  fn increase(&mut self);

  /// Decreases the recursion depth.
  fn decrease(&mut self);

  /// Checks if the recursion limit has been exceeded.
  fn check(&self) -> Result<(), Self::Error>;

  /// Increases the recursion depth and checks the limit.
  #[inline(always)]
  fn increase_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase();
    self.check()
  }
}

impl RecursionTracker for RecursionLimiter {
  type Error = RecursionLimitExceeded;

  #[inline(always)]
  fn increase(&mut self) {
    self.current = self.current.saturating_add(1);
  }

  #[inline(always)]
  fn decrease(&mut self) {
    self.current = self.current.saturating_sub(1);
  }

  #[inline(always)]
  fn check(&self) -> Result<(), Self::Error> {
    if self.depth() > self.limitation() {
      Err(RecursionLimitExceeded(*self))
    } else {
      Ok(())
    }
  }
}

const _: () = {
  #[allow(dead_code, unused_macros)]
  macro_rules! bail {
    ($lib:ident) => {
      use crate::lexer::$lib::{FromLogos, LogosLexer};
      use $lib::{Lexer, Logos};

      impl<'a, T> RecursionTracker for Lexer<'a, T>
      where
        T: Logos<'a>,
        T::Extras: RecursionTracker,
      {
        type Error = <T::Extras as RecursionTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.extras.increase();
        }

        #[inline(always)]
        fn decrease(&mut self) {
          self.extras.decrease();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.extras.check()
        }

        #[inline(always)]
        fn increase_and_check(&mut self) -> Result<(), Self::Error> {
          self.extras.increase_and_check()
        }
      }

      impl<'a, T> RecursionTracker for LogosLexer<'a, T>
      where
        T: FromLogos<'a>,
        <T::Logos as Logos<'a>>::Extras: RecursionTracker,
      {
        type Error = <<T::Logos as Logos<'a>>::Extras as RecursionTracker>::Error;

        #[inline(always)]
        fn increase(&mut self) {
          self.inner_mut().increase();
        }

        #[inline(always)]
        fn decrease(&mut self) {
          self.inner_mut().decrease();
        }

        #[inline(always)]
        fn check(&self) -> Result<(), Self::Error> {
          self.inner().check()
        }

        #[inline(always)]
        fn increase_and_check(&mut self) -> Result<(), Self::Error> {
          self.inner_mut().increase_and_check()
        }
      }
    };
  }

  #[cfg(feature = "logos_0_16")]
  #[cfg_attr(docsrs, doc(cfg(feature = "logos_0_16")))]
  const _: () = {
    bail!(logos_0_16);
  };
};

#[cfg(test)]
mod tests;
