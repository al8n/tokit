use core::{fmt, hash::Hash};

use derive_more::{IsVariant, TryUnwrap, Unwrap};

use crate::span::{Span, Spanned};

use super::{Source, State, token::Token};

/// A module containing integrations with the `logos` lexer library.
#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
#[cfg_attr(
  docsrs,
  doc(cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")))
)]
mod logos;

#[cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14"))]
#[cfg_attr(
  docsrs,
  doc(cfg(any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")))
)]
pub use logos::*;

/// The result of lexing a single token: either a successful token or an error.
///
/// `Lexed` represents the output of the lexing process for a single position in the input.
/// It can either be a successfully recognized [`Token`] with its span information, or a
/// lexing error that occurred at that position.
///
/// # Error Handling Strategy
///
/// `Lexed` enables **error recovery** during lexing by keeping errors in the token stream
/// rather than immediately aborting. This allows you to:
///
/// - Continue lexing after an error to find multiple issues in one pass
/// - Collect all lexing errors before reporting them to the user
/// - Implement "best-effort" parsing that tolerates some malformed input
/// - Provide better diagnostics by showing multiple errors at once
///
/// # Convenience Methods
///
/// Thanks to the `derive_more` macros, `Lexed` provides several utility methods:
///
/// - `is_token()` / `is_error()`: Check which variant this is
/// - `unwrap_token()` / `unwrap_error()`: Unwrap to get the inner value (panics if wrong variant)
/// - `try_unwrap_token()` / `try_unwrap_error()`: Safe unwrapping that returns `Option`
/// - `unwrap_token_ref()` / `unwrap_error_ref()`: Get a reference to the inner value
///
/// ## Examples
///
/// ## Basic Usage
///
/// ```rust,ignore
/// use tokora::{Lexed, TokenExt};
///
/// let mut lexer = logos::Lexer::<MyTokens>::new(input);
///
/// while let Some(lexed) = MyToken::lex(&mut lexer) {
///     match lexed {
///         Lexed::Token(spanned_token) => {
///             println!("Token: {:?} at {:?}", spanned_token.data(), spanned_token.span());
///         }
///         Lexed::Error(err) => {
///             eprintln!("Lexing error: {:?}", err);
///         }
///     }
/// }
/// ```
///
/// ## Error Collection
///
/// ```rust,ignore
/// let mut tokens = Vec::new();
/// let mut errors = Vec::new();
///
/// let mut lexer = logos::Lexer::<MyTokens>::new(input);
/// while let Some(lexed) = MyToken::lex(&mut lexer) {
///     match lexed {
///         Lexed::Token(tok) => tokens.push(tok),
///         Lexed::Error(err) => errors.push(err),
///     }
/// }
///
/// if !errors.is_empty() {
///     report_lexing_errors(&errors);
/// }
/// ```
///
/// ## Using Convenience Methods
///
/// ```rust,ignore
/// if lexed.is_token() {
///     let token = lexed.unwrap_token_ref();
///     process_token(token);
/// }
///
/// // Safe unwrapping
/// if let Some(token) = lexed.try_unwrap_token() {
///     use_token(token);
/// }
/// ```
#[derive(Debug, PartialEq, IsVariant, Unwrap, TryUnwrap)]
#[unwrap(ref, ref_mut)]
#[try_unwrap(ref, ref_mut)]
pub enum Lexed<'a, T: Token<'a>> {
  /// A successfully recognized token with its span information.
  Token(T),

  /// A lexing error that occurred during tokenization.
  ///
  /// The error type is determined by the Logos lexer's error type. It typically
  /// contains information about what went wrong and where in the input it occurred.
  Error(T::Error),
}

impl<'a, T> Clone for Lexed<'a, T>
where
  T: Token<'a>,
{
  #[inline(always)]
  fn clone(&self) -> Self {
    match self {
      Self::Token(tok) => Self::Token(tok.clone()),
      Self::Error(err) => Self::Error(err.clone()),
    }
  }
}

impl<'a, T> Copy for Lexed<'a, T>
where
  T: Token<'a> + Copy,
  T::Error: Copy,
{
}

impl<'a, T: Token<'a>> Lexed<'a, T> {
  /// Lexes the next token from the given lexer, returning `None` if the input is exhausted.
  #[inline(always)]
  pub fn lex<L>(lexer: &mut L) -> Option<Self>
  where
    L: super::Lexer<'a, Token = T>,
  {
    lexer.lex().map(|res| res.into())
  }

  /// Lexes the next token from the given lexer, returning `None` if the input is exhausted.
  #[inline(always)]
  pub fn lex_spanned<L>(lexer: &mut L) -> Option<Spanned<Self, L::Span>>
  where
    L: super::Lexer<'a, Token = T>,
  {
    lexer
      .lex()
      .map(|res| Spanned::new(lexer.span(), res.into()))
  }

  /// Returns the contained [`Lexed::Token`] value, consuming the `self` value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Error`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_token(self, msg: &str) -> T {
    match self {
      Self::Token(tok) => tok,
      Self::Error(_) => panic!("{msg}"),
    }
  }

  /// Returns the contained [`Lexed::Error`] value, consuming the `self` value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Token`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_error(self, msg: &str) -> T::Error {
    match self {
      Self::Token(_) => panic!("{msg}"),
      Self::Error(err) => err,
    }
  }

  /// Returns the reference of the contained [`Lexed::Token`] value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Error`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_token_ref(&self, msg: &str) -> &T {
    match self {
      Self::Token(tok) => tok,
      Self::Error(_) => panic!("{msg}"),
    }
  }

  /// Returns the reference of the contained [`Lexed::Error`] value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Token`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_error_ref(&self, msg: &str) -> &T::Error {
    match self {
      Self::Token(_) => panic!("{msg}"),
      Self::Error(err) => err,
    }
  }

  /// Returns the mutable reference of the contained [`Lexed::Token`] value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Error`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_token_mut(&mut self, msg: &str) -> &mut T {
    match self {
      Self::Token(tok) => tok,
      Self::Error(_) => panic!("{msg}"),
    }
  }

  /// Returns the mutable reference of the contained [`Lexed::Error`] value.
  ///
  /// # Panics
  ///
  /// Panics if the value is a [`Lexed::Token`] with a custom panic message provided by
  /// `msg`.
  #[inline(always)]
  #[track_caller]
  pub fn expect_error_mut(&mut self, msg: &str) -> &mut T::Error {
    match self {
      Self::Token(_) => panic!("{msg}"),
      Self::Error(err) => err,
    }
  }
}

impl<'a, T: 'a> core::fmt::Display for Lexed<'a, T>
where
  T: Token<'a> + core::fmt::Display,
  T::Error: core::fmt::Display,
{
  #[inline(always)]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Token(tok) => ::core::fmt::Display::fmt(tok, f),
      Self::Error(err) => err.fmt(f),
    }
  }
}

impl<'a, T: Token<'a>> From<Result<T, T::Error>> for Lexed<'a, T> {
  #[inline(always)]
  fn from(value: Result<T, T::Error>) -> Self {
    match value {
      Ok(tok) => Self::Token(tok),
      Err(err) => Self::Error(err),
    }
  }
}

impl<'a, T: Token<'a>> From<Lexed<'a, T>> for Result<T, T::Error> {
  #[inline(always)]
  fn from(value: Lexed<'a, T>) -> Self {
    match value {
      Lexed::Token(tok) => Ok(tok),
      Lexed::Error(err) => Err(err),
    }
  }
}

/// A trait to convert a type into a lexer.
pub trait IntoLexer<'inp, T: ?Sized> {
  /// The lexer type.
  type Lexer;

  /// Converts `self` into a lexer.
  fn into_lexer(self) -> Self::Lexer
  where
    Self: 'inp,
    T: Token<'inp>;
}

impl<'inp, T, L> IntoLexer<'inp, T> for L
where
  T: Token<'inp>,
  L: Lexer<'inp, Token = T>,
{
  type Lexer = L;

  #[inline(always)]
  fn into_lexer(self) -> Self::Lexer {
    self
  }
}

/// A trait for lexers.
///
/// # The lexer contract
///
/// tokora's input layer ([`InputRef`](crate::InputRef)) pulls tokens from a lexer on
/// demand and, to support lookahead and backtracking, **rebuilds a fresh lexer per
/// operation and re-lexes on demand any region it needs again** — the prefix a
/// checkpoint restore rewinds, and any cached token an abandoned branch dropped or a
/// cache truncation discarded. That reconstruction is always the same two steps
/// ([`InputRef::lexer`](crate::InputRef::lexer)): `L::with_state(src, saved_state)`
/// followed by [`bump`](Lexer::bump) to the committed offset. The clauses below are
/// the properties that make that machinery correct.
///
/// They are a *contract*, not a set of `unsafe` invariants: breaking one is neither
/// undefined behavior nor memory-unsafe, but it makes the input layer's observable
/// behavior unspecified (see [*Violation posture*](#violation-posture)). The bundled
/// Logos backend upholds every clause; a hand-written lexer must uphold them itself,
/// and the `conformance` kit checks a lexer against them.
///
/// ## Determinism: scanning is a pure function of source, position, and state
///
/// Every scan-visible result — which token [`lex`](Lexer::lex) decides, its span,
/// whether the item is a token or an [`Error`](Lexed::Error), and any limit
/// accounting [`check`](Lexer::check) reports — must derive **entirely** from the
/// source, the offset being lexed at, and the lexer [`State`](Lexer::State). Nothing
/// may route through state a checkpoint's `State` snapshot cannot capture and a
/// restore therefore cannot rewind: a shared counter, an ambient global, an
/// allocator address. Determinism is what makes replay observationally identical to
/// the run it rewound, and two concrete mechanisms depend on it:
///
/// - **prefix replay after a checkpoint restore** —
///   [`restore`](crate::InputRef::restore) copies the saved [`State`](Lexer::State),
///   span, poison boundary, and lexer-error dedup watermark back, and drops the cache
///   entries pushed since the save; the region they covered is re-lexed on demand and
///   must reproduce the identical items;
/// - **cache truncation** — a peek lexes ahead and caches tokens; when a branch is
///   abandoned or the cache is drained, those tokens are dropped and re-lexed on the
///   next read.
///
/// Re-lexing the same source from the same offset and state must reproduce the
/// identical item and the identical accounting. (Scan *counting* held **outside**
/// `State` — e.g. a test probe — legitimately observes the extra re-lex scans; that
/// is instrumentation, not a scan-visible result.)
///
/// ## State faithfulness and cheapness
///
/// A resume point is the pair (**[`State`](Lexer::State)**, **offset**):
/// [`with_state`](Lexer::with_state) carries the *mode* — every scanning regime the
/// lexer can be in (nesting depth, string-vs-normal mode, a resource limiter's tally)
/// — and [`bump`](Lexer::bump) carries the *position*. Position is deliberately **not**
/// encoded in `State`; the input layer threads it separately through `bump`. Together
/// the pair must be sufficient: rebuilding with `L::with_state(src, saved_state)` and
/// bumping to the saved offset must reproduce **exactly** the suffix the original
/// lexer would have produced from the moment that state was captured. The state that
/// pairs with a given item is the one observed *right after* [`lex`](Lexer::lex)
/// returned it, and the offset it pairs with is that item's span end.
///
/// `State` is cloned on **every** checkpoint ([`save`](crate::InputRef::save) clones
/// it, and every cached token stores a clone), so cloning must be cheap. Keep it
/// `Copy` or small; if it must own heavy data, store a handle (e.g. `Arc`) inside it
/// so clones stay inexpensive.
///
/// ## Monotone progress: spans never move backward, every scan advances
///
/// Over a run, span starts are **non-decreasing**, and every produced item — a token
/// or an [`Error`](Lexed::Error) — has a **nonempty** span: its [`span`](Lexer::span)
/// end is strictly greater than its start. A zero-width span (end equal to start) is a
/// contract violation. The input layer reasons about the stream *positionally* —
/// cached spans, the lexer-error dedup watermark, and the poison boundary past which
/// lexing stops are all offsets that assume every lexed item advances the position —
/// so a zero-width item is at once excluded (it starts at the boundary) and
/// non-advancing (it consumes nothing), which silently degrades replay and, worse,
/// *termination*: a lexer that never advances yields an unbounded zero-width run.
/// Debug builds assert the nonempty span at the input layer's single lexing chokepoint
/// (`lex_within_boundary`); release builds omit it. [`lex`](Lexer::lex) restates this
/// clause on the method itself.
///
/// ## Exhaustion is sticky
///
/// Once [`lex`](Lexer::lex) returns `None`, every subsequent [`lex`](Lexer::lex) on
/// that same instance must keep returning `None` — exhaustion never "un-exhausts".
/// The input layer never re-polls a single instance past its first `None` (both
/// scanning loops stop there), but it relies on the *positional* face of the same
/// property: on end of input a scan commits no progress, so the committed offset stays
/// put and the **next** operation rebuilds a fresh lexer at that same offset and
/// re-lexes it — which, by determinism, must return `None` again. Sticky `None` is
/// what lets a parse terminate at end of input instead of looping. (The bundled Logos
/// backend additionally latches a *limit* trip to sticky `None`; that is a stronger
/// backend guarantee documented on its `LogosLexer`, not a requirement on every
/// lexer.)
///
/// ## The span survives exhaustion
///
/// After [`lex`](Lexer::lex) first returns `None`, [`span`](Lexer::span) must keep returning a
/// **well-formed** span (`start <= end`) whose `end` is the lexer's final position in the
/// source: at least the end of the last yielded item, at most the source length. Bytes the
/// lexer consumed and skipped after the last item — trailing trivia — are included, which is
/// exactly what makes the value useful. Repeated calls must agree.
///
/// The input layer reads this at two sites, so an unspecified value is not academic: a
/// `to`-shaped scan commits at `span().end` when it reaches end of input, and the partial-input
/// frontier reports `Incomplete(offset)` from it — the offset a refill driver resumes at. The
/// frontier reader floors and clamps as defence in depth against a non-conforming lexer; that
/// clamp is not the specification, this clause is.
///
/// ## The settle path's operations must not panic
///
/// The input layer settles a consumed token by computing the committed position **from that
/// token's own span** and then installing it. Those steps therefore run after the token has left
/// the front of the stream and before the position has moved — a window nothing can reorder away,
/// because the position is a function of the token. The operations in it are:
///
/// 1. [`Source::len`](crate::Source);
/// 2. [`Span::end_ref`](crate::Span) and the `Ord` comparison on [`Offset`](Lexer::Offset)
///    (the input layer clamps a span to the source);
/// 3. [`Span::clone`](crate::Span), on the common branch where no clamping is needed;
/// 4. on the clamping branch only, [`Span::start_ref`](crate::Span) plus `Offset::clone` plus
///    `Span::new`.
///
/// That is the whole list, and it is deliberately shorter than it was. Two operations used to be
/// in it and are not: the `Drop` of the span and state a commit replaces, and the `Drop` of the
/// `Source::len` temporary. Both are now handed out of the window and run after the settle, so an
/// implementor does not have to reason about them at all. Nothing else was removable — every
/// operation above is computed *from the token's own span*, which does not exist until the token
/// has been taken, so no ordering can lift them out.
///
/// **None of these may panic.** A panic there leaves the token gone from the stream with the
/// position still behind it, and a host that catches it resumes past a token it never saw — the
/// crate cannot detect that, and for a stateful lexer it is silent stream corruption. This is the
/// same posture the crate already takes on
/// [`Emitter::release`](crate::Emitter::release)/[`rewind`](crate::Emitter::rewind) and the
/// [`Cache`](crate::cache::Cache) restore-path operations, and for the same reason: the operation
/// runs at a point where nothing else can repair it.
///
/// Every span type the crate ships satisfies this trivially (`SimpleSpan` is `Copy`, and `usize`
/// offsets neither allocate nor drop). The bound stays [`Clone`] — `Copy` is **not** required, and
/// a span that allocates is still welcome; what the clause asks is that the allocation not panic
/// on this path. Like the emitter and cache clauses, this is an obligation the crate states and
/// cannot check: a violation surfaces as a lost token after a caught panic, not as a compile
/// error.
///
/// ## Truncation faithfulness (partial-input mode only)
///
/// When the input layer is driven in [`Partial`](crate::input::Partial) (Sans-I/O) mode, one
/// further property is assumed — it is inert for a [`Complete`](crate::input::Complete) parse. A
/// produced item (token or [`Error`](Lexed::Error)) whose span ends **strictly before the buffer
/// end** must be **stable under appended input**: its decision — token-vs-error, kind, and span —
/// must derive only from source bytes up to its own span end, never from bytes further ahead. The
/// frontier holdback withholds exactly the one item whose span *reaches* the buffer end (the
/// maximal-munch one-boundary-byte lookahead is safe, because that boundary byte is present
/// precisely when the item is yielded), so a lexer that decides an item from bytes *beyond* its
/// span breaks the chunked-equivalence guarantee — a prefix parse would produce a different item
/// than the whole. A maximal-munch lexer (the Logos backend, and every hand-written lexer that
/// commits each item from its own bytes) satisfies this; the `conformance` kit's `run_partial`
/// check exercises it over every split point.
///
/// ## Span / slice coherence
///
/// [`slice`](Lexer::slice) must equal the source content at [`span`](Lexer::span):
/// slicing the source over `span().start..span().end` yields exactly `slice()`, and
/// every span lies within source bounds (`0 <= start <= end <= source.len()`). The
/// input layer builds its own slices from spans over the source
/// ([`InputRef::slice`](crate::InputRef::slice)), so a span that disagrees with the
/// slice, or points outside the source, yields wrong text or a panic.
///
/// ## Composite tokens own their contents (token-level nesting)
///
/// A composite token — a block string, a raw/heredoc literal, a comment — is **one**
/// token whose span covers the whole construct; the lexer must **swallow every
/// delimiter character inside it** and emit no nested delimiter tokens for them. The
/// recovery scanner [`sync_balanced`](crate::InputRef::sync_balanced) counts delimiter
/// nesting **token by token**, so a `{` or `}` buried inside a block string must never
/// reach it as a separate token — otherwise a brace inside a string literal would
/// perturb the depth counter and mis-place a recovery sync point. See
/// [`sync_balanced`](crate::InputRef::sync_balanced) for the counter this leans on.
///
/// ## Trivia surfacing
///
/// A lexer either **surfaces** every trivia byte as a real token or **skips** it at the
/// lexer level. [`SURFACES_TRIVIA`](Self::SURFACES_TRIVIA) declares which — defaulting to
/// the token vocabulary's own [`Token::SURFACES_TRIVIA`], the totality half of the trivia
/// concept whose per-token identity half is [`Token::is_trivia`]. Declaring `true` promises
/// *totality*: every source byte is covered by either an emitted token (trivia included) or
/// a reported lexer error, none silently discarded. The lossless (`gap_kind`)
/// `cst::Sink` requires it at **compile time**; declaring it while a
/// lexer-level `skip` rule discards bytes anyway is the unspecified-but-bounded violation
/// class below — surfaced by materialization as
/// `cst::FinishError::UncoveredGap`, never UB or a panic from this
/// crate.
///
/// ## Violation posture
///
/// A lexer that breaks a clause above is **not** undefined behavior and **not**
/// memory-unsafe. The input layer's behavior under a violating lexer is
/// **unspecified but bounded**, exactly the posture a misused checkpoint gets (see
/// [`restore`](crate::InputRef::restore)'s release-build section): no undefined
/// behavior, no leak, no panic originating in this crate, and — because a resource
/// limiter's tally travels inside the checkpointed `State` — every scan still
/// terminates. What is *not* guaranteed is that a replay matches the original run:
/// diagnostics may be missing or misattributed, and the replayed token stream may
/// differ. Debug builds turn the one locally-checkable clause (nonempty spans) into an
/// assertion at the single lexing chokepoint.
pub trait Lexer<'inp>: 'inp {
  /// The state of the lexer.
  type State: State;
  /// The source type of the lexer.
  type Source: super::Source<Self::Offset> + ?Sized;
  /// The token type produced by the lexer.
  type Token: Token<'inp>;

  /// The span type of the lexer.
  type Span: fmt::Debug + Span<Offset = Self::Offset> + Ord + Clone + Hash;
  /// The offset type of the source.
  type Offset: Default + fmt::Debug + Ord + Clone + Hash;

  /// Whether this lexer **surfaces trivia as real tokens** instead of silently skipping
  /// the bytes. Defaults to the token vocabulary's own declaration
  /// ([`Token::SURFACES_TRIVIA`]), which is where a `LogosLexer`-backed dialect
  /// declares it (the logos adapter is one blanket impl for every token type, so the
  /// per-dialect site is the `Token` impl). A hand-written lexer whose skipping behavior
  /// differs from its vocabulary's declaration overrides it here.
  ///
  /// See [`Token::SURFACES_TRIVIA`] for the contract this declares and the compile-time
  /// wall (`Sink::new`) that consumes it.
  const SURFACES_TRIVIA: bool = <Self::Token as Token<'inp>>::SURFACES_TRIVIA;

  /// Lexes the input source and returns a tokenizer.
  fn new(src: &'inp Self::Source) -> Self;

  /// Lexes the input source with the given initial state and returns a tokenizer.
  ///
  /// This is the **resume** constructor: the input layer rebuilds a lexer with a saved
  /// [`State`](Self::State) here and then [`bump`](Self::bump)s to the resume offset.
  /// See *State faithfulness* in the [trait contract](Self#the-lexer-contract) for what
  /// the (state, offset) pair must reproduce.
  fn with_state(src: &'inp Self::Source, state: Self::State) -> Self;

  /// Checks the current state of the lexer for errors.
  ///
  /// If the state is valid, returns `Ok(())`, otherwise returns an error.
  ///
  /// Not to be confused with [`Check::check`](crate::Check::check) (the parser-side value
  /// predicate) or [`State::check`] (the state's own validity probe, which this method
  /// typically wraps into the token's error type).
  fn check(&self) -> Result<(), <Self::Token as Token<'inp>>::Error>;

  /// Returns a reference to the current state of the lexer.
  fn state(&self) -> &Self::State;

  /// Returns a mutable reference to the current state of the lexer.
  fn state_mut(&mut self) -> &mut Self::State;

  /// Consumes the lexer and returns the current state.
  fn into_state(self) -> Self::State;

  /// Returns a reference to the source being lexed.
  fn source(&self) -> &'inp Self::Source;

  /// Get the range for the current token in `Source`.
  ///
  /// After exhaustion this must still answer, well-formed, with the lexer's final position —
  /// see [the span-survives-exhaustion clause](Self#the-span-survives-exhaustion).
  fn span(&self) -> Self::Span;

  /// Returns the slice of the current token in the source.
  fn slice(&self) -> <Self::Source as Source<Self::Offset>>::Slice<'inp>;

  /// Lexes the next token from the input source.
  ///
  /// Returns `None` at end of input; once it returns `None` it must keep returning
  /// `None` (exhaustion is sticky). Every produced token and error must have a nonempty
  /// span. See the [trait contract](Self#the-lexer-contract) for both clauses.
  fn lex(&mut self) -> Option<Result<Self::Token, <Self::Token as Token<'inp>>::Error>>;

  /// Bumps the end of currently lexed token by `n` offsets.
  ///
  /// # Panics
  ///
  /// Panics if adding `n` to current offset would place the `Lexer` beyond the last byte,
  /// or in the middle of an UTF-8 code point (does not apply when lexing raw `&[u8]`).
  fn bump(&mut self, n: &Self::Offset);
}

/// The slice type lexer `L` yields from its source.
///
/// Generic parser code reaches for this projection constantly — the payload of an
/// identifier, the raw text of a literal — and spelling it out means chaining two
/// associated types through [`Source`]. `SliceOf` names that path once, so a bound
/// like `SliceOf<'inp, L>: Clone` or a return type carrying the slice stays legible.
///
/// # Examples
///
/// The alias is definitionally the nested projection — this identity function
/// compiles precisely because the two spellings are the same type:
///
/// ```rust
/// use tokora::{Lexer, SliceOf, Source};
///
/// fn same_type<'inp, L: Lexer<'inp>>(
///   slice: <L::Source as Source<L::Offset>>::Slice<'inp>,
/// ) -> SliceOf<'inp, L> {
///   slice
/// }
/// ```
pub type SliceOf<'inp, L> =
  <<L as Lexer<'inp>>::Source as Source<<L as Lexer<'inp>>::Offset>>::Slice<'inp>;

/// A trait for types that can be lexed from the input.
///
/// This trait provides a standardized way to lex (tokenize) an entire input
/// into a structured type. It's useful for types that represent complete
/// lexical structures that can be built from an input source.
///
/// # Type Parameters
///
/// - `I`: The input type to lex from (e.g., `&str`, `&[u8]`)
/// - `Error`: The error type returned when lexing fails
///
/// # Example
///
/// ```rust,ignore
/// use tokora::Lexable;
///
/// struct Document {
///     tokens: Vec<Token>,
/// }
///
/// impl Lexable<&str, LexError> for Document {
///     fn lex(input: &str) -> Result<Self, LexError> {
///         // Lex the entire input into a Document
///         let tokens = tokenize(input)?;
///         Ok(Document { tokens })
///     }
/// }
/// ```
pub trait Lexable<I, Error> {
  /// Lexes `Self` from the given input.
  ///
  /// This method consumes the input and attempts to construct `Self` by
  /// lexing the entire input. It returns an error if the input cannot be
  /// successfully lexed.
  ///
  /// # Errors
  ///
  /// Returns an error if the input is malformed or cannot be lexed according
  /// to the rules of the implementing type.
  fn lex(input: I) -> Result<Self, Error>
  where
    Self: Sized;
}

#[cfg(test)]
#[allow(warnings)]
mod tests;

#[cfg(test)]
pub(crate) struct DummyLexer;

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, derive_more::Display)]
#[display("DummyToken")]
pub(crate) struct DummyToken;

#[cfg(test)]
const _: () = {
  use crate::token::{LitToken, PunctuatorToken};

  impl Token<'_> for DummyToken {
    type Kind = Self;
    type Error = ();

    #[inline(always)]
    fn kind(&self) -> Self::Kind {
      *self
    }

    #[inline(always)]
    fn is_trivia(&self) -> bool {
      true
    }
  }

  impl PunctuatorToken<'_> for DummyToken {}

  impl LitToken<'_> for DummyToken {}

  impl<'inp> Lexer<'inp> for DummyLexer {
    type State = ();

    type Source = str;

    type Token = DummyToken;

    type Span = crate::span::SimpleSpan;

    type Offset = usize;

    fn new(_: &'inp Self::Source) -> Self {
      todo!()
    }

    fn with_state(_: &'inp Self::Source, _: Self::State) -> Self {
      todo!()
    }

    fn check(&self) -> Result<(), <Self::Token as Token<'inp>>::Error> {
      todo!()
    }

    fn state(&self) -> &Self::State {
      todo!()
    }

    fn state_mut(&mut self) -> &mut Self::State {
      todo!()
    }

    fn into_state(self) -> Self::State {
      todo!()
    }

    fn source(&self) -> &'inp Self::Source {
      todo!()
    }

    fn span(&self) -> Self::Span {
      todo!()
    }

    fn slice(&self) -> <Self::Source as Source<Self::Offset>>::Slice<'inp> {
      todo!()
    }

    fn lex(&mut self) -> Option<Result<Self::Token, <Self::Token as Token<'inp>>::Error>> {
      todo!()
    }

    fn bump(&mut self, _: &Self::Offset) {
      todo!()
    }
  }
};
