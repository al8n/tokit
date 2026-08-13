//! The resumable partial-parse **session**: the cross-attempt half of the streaming story.
//!
//! [`parse_partial`](crate::input::parse_partial) is a one-shot: it builds a fresh input over the
//! caller's whole buffer, drives the parser, and returns a value that carries no committed
//! offset, no evolved lexer state and no budget. Every refill therefore re-drives the entire
//! prefix, and — the part that matters — **no work budget can live anywhere**. A fresh
//! per-attempt limiter admits `L(L+1)/2` work under its nominal bound; a shared cumulative
//! limiter counts replay as input and trips on valid documents. Both live in `L::State`,
//! which `parse_partial` consumes and discards.
//!
//! A [`PartialSession`] is the value that survives between attempts. It carries the base
//! lexer state, the committed frontier, and a [`Budget`] over the one quantity an adversarial
//! chunker actually controls: **Σ attempt-lexable bytes**.
//!
//! # What a session costs, stated honestly
//!
//! [`RedriveFromBase`] re-drives the whole buffer every attempt, so cumulative lexer work is
//! **Θ(Σ attempt lengths)** — the honest quadratic under one-token refills. That is not a
//! defect being hidden: it is the price of a continuation-free design, and it is exactly why
//! the budget is a *required* constructor argument rather than an option. A resuming mode
//! that lexes only `[committed, len)` — O(total bytes) for parsers that commit incrementally
//! — is designed and prototype-verified, and ships in a later wave;
//! [`ReplayMode`] is sealed and additive precisely so that arrival costs nothing.
//!
//! # Nothing here is omittable
//!
//! Both the budget and the replay mode are **required constructor arguments**.
//! [`Budget::Unbounded`] is expressible but must be *written*, so it surfaces in review as a
//! decision; there is no `Default for Budget` and no constructor without it. The same rule
//! governs the mode: a session with no stated replay mode does not exist, and because the
//! mode is a type parameter, "this shape re-drives from base" is a compile fact rather than
//! a sentence.

use core::fmt;

use crate::{Emitter, Lexer, ParseContext, ParseInput, Source, error::MaybeTerminal, span::Span};

use super::{Input, Partial, SurfaceIncomplete};

mod sealed {
  /// Seals [`ReplayMode`](super::ReplayMode) against downstream implementations.
  pub trait Sealed {}
}

/// How a session's attempt relates to the frontier its predecessor committed.
///
/// **Sealed**: downstream cannot add a mode, and new modes ride additively — a later minor
/// can introduce one without breaking any consumer, because a mode is only ever named by a
/// value the caller writes at construction.
pub trait ReplayMode: sealed::Sealed {}

/// Each attempt re-drives the whole buffer from the session's base.
///
/// Cumulative lexer work stays Θ(Σ attempt lengths) — the honest quadratic — but the attempt
/// is a *pure function of the whole prefix*, so the final attempt's emitter and the final
/// attempt's sink are each complete by construction. That is what makes this the mode a
/// streaming CST must use: materialization needs every event, and only a from-base attempt
/// produces every event.
///
/// Under this mode the [`Budget`] is the only thing standing between an adversarial chunker
/// and Θ(L²), which is why [`Budget::Unbounded`] paired with `RedriveFromBase` is the one
/// combination worth calling out by name: it is a deliberate declaration that the stream is
/// trusted or bounded somewhere else.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RedriveFromBase;

impl sealed::Sealed for RedriveFromBase {}
impl ReplayMode for RedriveFromBase {}

/// The cross-attempt work budget: a cap on Σ **attempt-lexable bytes** over a session's life.
///
/// Bytes, not attempts. An attempt count cannot bound work — *n* attempts over a growing
/// buffer is Θ(n · len) with `len` unbounded, which is the very quantity under attack —
/// whereas Σ attempt-lexable bytes is an exact upper bound on cross-attempt lexer work,
/// enforceable with no per-token instrumentation and no lexer cooperation.
///
/// There is deliberately **no `Default`**: see the [module docs](crate::input).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Budget {
  /// The caller has decided the stream is trusted, or bounded somewhere else. Visible in
  /// review because it had to be written.
  Unbounded,
  /// Refuse the attempt that would push Σ attempt-lexable bytes past this cap.
  Bytes(u64),
}

/// A session's own refusal — decided **before any attempt work**, and always terminal.
///
/// One type for both legs on purpose. Two separate `From` bounds would be exactly the bound
/// sprawl the context bundle exists to collapse, and both legs are the same kind of fact:
/// *no amount of further input will fix this*. More input only raises the spend, and a
/// latched terminal trip never unlatches.
///
/// # The coherence law
///
/// A consumer's `From<SessionRefusal>` impl **must** yield a value whose
/// [`is_terminal()`](MaybeTerminal::is_terminal) is `true`. An impl that reports
/// `is_incomplete()` instead gives the caller a **zero-work infinite refill loop**: the
/// caller retries, the gate refuses before any work, and the identical converted error comes
/// back forever. The law is not left as prose, and not left to debug builds either: the one
/// conversion site inside [`PartialSession::parse`] carries an **unconditional** assertion,
/// so a violating impl is a loud panic at the cause in every build — including the release
/// builds that actually face untrusted input.
///
/// This is one of the three terminal sources this crate builds and a consumer's error type has to
/// answer for, and the only one that is not itself a [`MaybeTerminal`] implementor: there is
/// nothing to delegate to, so the arm is written `true`. It is also the only one whose wrong
/// answer is a panic rather than a silent spend. See the trait's own
/// [table of terminal sources](crate::error::MaybeTerminal#the-terminal-sources) for the other
/// two — and, since those three are what the crate knows it produces rather than a closed set of
/// terminal conditions,
/// [where the set stops being closed](crate::error::MaybeTerminal#where-the-set-stops-being-closed)
/// for the rule covering an arm the table does not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
#[non_exhaustive]
pub enum SessionRefusal {
  /// The attempt would have pushed Σ attempt-lexable bytes past the session's budget, so it
  /// was refused before any lexing.
  #[error(
    "the parse session's budget is exhausted: this attempt would reach {spent} lexable \
     bytes against a budget of {budget}"
  )]
  BudgetExhausted {
    /// The Σ attempt-lexable bytes this attempt **would have** brought the session to. The
    /// session's own spend is unchanged — a refused attempt does no work.
    spent: u64,
    /// The cap that refused it.
    budget: u64,
  },
  /// An earlier attempt returned a terminal error, so the session is latched and refuses
  /// every subsequent attempt before any work.
  #[error("the parse session is latched: an earlier attempt ended in a terminal error")]
  TerminalLatched,
}

/// A resumable partial-parse session: the value that survives between refills.
///
/// See the [module docs](crate::input) for what a session is for and what it costs. Not to be
#[cfg_attr(
  any(feature = "std", feature = "alloc"),
  doc = " confused with [`SessionPointId`](crate::input::SessionPointId), which names a savepoint *inside*"
)]
#[cfg_attr(
  not(any(feature = "std", feature = "alloc")),
  doc = " confused with `SessionPointId`, which names a savepoint *inside*"
)]
/// one parse.
///
/// # Example
///
/// ```
/// use core::convert::Infallible;
/// use tokora::{InputRef, Lexer, Partial, SimpleSpan, Token};
/// use tokora::cache::DefaultCache;
/// use tokora::emitter::Fatal;
/// use tokora::error::{Incomplete, MaybeIncomplete, MaybeTerminal};
/// use tokora::input::{Budget, PartialSession, RedriveFromBase, SessionRefusal};
/// #
/// # #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
/// # struct WordKind;
/// # impl core::fmt::Display for WordKind {
/// #   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("word") }
/// # }
/// # #[derive(Clone, Debug)]
/// # struct Word;
/// # impl Token<'_> for Word {
/// #   type Kind = WordKind;
/// #   type Error = Infallible;
/// #   fn kind(&self) -> WordKind { WordKind }
/// #   fn is_trivia(&self) -> bool { false }
/// # }
/// # struct WordLexer<'a> { src: &'a str, start: usize, end: usize, state: () }
/// # impl<'a> Lexer<'a> for WordLexer<'a> {
/// #   type State = (); type Source = str; type Token = Word;
/// #   type Span = SimpleSpan; type Offset = usize;
/// #   fn new(src: &'a str) -> Self { Self { src, start: 0, end: 0, state: () } }
/// #   fn with_state(src: &'a str, state: ()) -> Self { Self { src, start: 0, end: 0, state } }
/// #   fn check(&self) -> Result<(), Infallible> { Ok(()) }
/// #   fn state(&self) -> &() { &self.state }
/// #   fn state_mut(&mut self) -> &mut () { &mut self.state }
/// #   fn into_state(self) {}
/// #   fn source(&self) -> &'a str { self.src }
/// #   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.start, self.end) }
/// #   fn slice(&self) -> &'a str { &self.src[self.start..self.end] }
/// #   fn lex(&mut self) -> Option<Result<Word, Infallible>> {
/// #     let b = self.src.as_bytes();
/// #     self.start = self.end;
/// #     while self.start < b.len() && b[self.start] == b' ' { self.start += 1; }
/// #     if self.start >= b.len() { self.end = self.start; return None; }
/// #     let mut e = self.start;
/// #     while e < b.len() && b[e] != b' ' { e += 1; }
/// #     self.end = e;
/// #     Some(Ok(Word))
/// #   }
/// #   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
/// #   fn bump(&mut self, n: &usize) { self.end += *n; self.start = self.end; }
/// # }
/// # #[derive(Debug, PartialEq)]
/// # enum MyErr { Incomplete, Other, Refused(SessionRefusal) }
/// # impl From<Infallible> for MyErr { fn from(x: Infallible) -> Self { match x {} } }
/// # impl From<Incomplete<usize>> for MyErr {
/// #   fn from(_: Incomplete<usize>) -> Self { MyErr::Incomplete }
/// # }
/// # impl From<SessionRefusal> for MyErr {
/// #   fn from(r: SessionRefusal) -> Self { MyErr::Refused(r) }
/// # }
/// # impl MaybeIncomplete for MyErr {
/// #   fn is_incomplete(&self) -> bool { matches!(self, MyErr::Incomplete) }
/// # }
/// # impl MaybeTerminal for MyErr {
/// #   fn is_terminal(&self) -> bool { matches!(self, MyErr::Refused(_)) }
/// # }
/// # impl<'a, T, K: Clone, S, Lg: ?Sized> From<tokora::error::token::UnexpectedToken<'a, T, K, S, Lg>>
/// #   for MyErr { fn from(_: tokora::error::token::UnexpectedToken<'a, T, K, S, Lg>) -> Self { MyErr::Other } }
/// #
/// # type Lex<'a> = WordLexer<'a>;
/// # type Ctx<'a> = (Fatal<MyErr>, DefaultCache<'a, Lex<'a>>);
/// # fn ctx<'a>() -> Ctx<'a> { (Fatal::of(), DefaultCache::<'a, Lex<'a>>::default()) }
/// #
/// # fn count<'inp>(
/// #   inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, (), Partial>,
/// # ) -> Result<usize, MyErr> {
/// #   let mut n = 0;
/// #   while inp.next()?.is_some() { n += 1; }
/// #   Ok(n)
/// # }
/// // Both arguments are written down: no silent default in either direction.
/// let mut session = PartialSession::new((), Budget::Bytes(8), RedriveFromBase);
///
/// assert_eq!(session.parse(ctx(), "a b", true, count), Ok(2));
/// assert_eq!(*session.committed(), 3);
/// assert_eq!(session.spent(), 3);
///
/// // 3 + 7 = 10 lexable bytes would pass a budget of 8: refused before any work.
/// let refused = session.parse(ctx(), "a b c d", true, count);
/// assert_eq!(
///   refused,
///   Err(MyErr::Refused(SessionRefusal::BudgetExhausted { spent: 10, budget: 8 })),
/// );
/// // A refused attempt does no work, so neither the frontier nor the spend moved.
/// assert_eq!(*session.committed(), 3);
/// assert_eq!(session.spent(), 3);
/// ```
pub struct PartialSession<St, Off, M> {
  /// The **base** lexer state, retained unchanged for the session's life.
  ///
  /// Under `RedriveFromBase` this field is a seed source and nothing else. It is never
  /// overwritten from a finished attempt: one field cannot be both "the base state the next
  /// seed needs" and "whatever the last attempt evolved to", and writing the latter is
  /// precisely the shared-cumulative-limiter defect the session exists to fix — the next
  /// attempt would re-drive from byte 0 with an already-evolved tally and trip on a valid
  /// document.
  state: St,
  /// The absolute committed frontier, harvested after every attempt.
  committed: Off,
  /// Required at construction.
  budget: Budget,
  /// Σ attempt-lexable bytes actually spent.
  spent: u64,
  /// The sticky cross-attempt terminal latch.
  terminal: bool,
  /// Required at construction, and a type parameter so the mode is visible at the type.
  mode: M,
}

/// Hand-written, and the rendered field list is **pinned**: `committed`, `budget`, `spent`,
/// `terminal`, then `finish_non_exhaustive()`.
///
/// `state` and `mode` are deliberately omitted, following the precedent that the sink's own
/// `Debug` omits its mapper. A derive would additionally demand `St: Debug` and `M: Debug`
/// from every consumer, and — because a `Debug` rendering of a public type is part of this
/// crate's observable surface — the list is pinned so that a later mode cannot quietly widen
/// it and break a consumer that pattern-matches on the render.
impl<St, Off, M> fmt::Debug for PartialSession<St, Off, M>
where
  Off: fmt::Debug,
{
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    f.debug_struct("PartialSession")
      .field("committed", &self.committed)
      .field("budget", &self.budget)
      .field("spent", &self.spent)
      .field("terminal", &self.terminal)
      .finish_non_exhaustive()
  }
}

impl<St, Off, M> PartialSession<St, Off, M>
where
  Off: Default,
  M: ReplayMode,
{
  /// Opens a session over `state`, with `budget` and `mode` both **written down**.
  ///
  /// There is no constructor that omits either, and no `Default` for either: the unsafe
  /// choice must be impossible to make by omission (see the [module docs](crate::input)).
  #[inline]
  pub fn new(state: St, budget: Budget, mode: M) -> Self {
    Self {
      state,
      committed: Off::default(),
      budget,
      spent: 0,
      terminal: false,
      mode,
    }
  }

  /// The absolute committed frontier: how far the last attempt's parser actually got.
  ///
  /// Observability only under [`RedriveFromBase`]. The session reports the frontier, and
  /// deliberately ships no operation that *consumes* it: a re-driving session cannot drain,
  /// because it cannot re-drive a prefix the caller has thrown away.
  #[inline]
  pub const fn committed(&self) -> &Off {
    &self.committed
  }

  /// The session's budget, as written at construction.
  #[inline]
  pub const fn budget(&self) -> Budget {
    self.budget
  }

  /// Σ attempt-lexable bytes spent so far. A refused attempt spends nothing.
  #[inline]
  pub const fn spent(&self) -> u64 {
    self.spent
  }

  /// Whether a terminal error has latched the session shut.
  #[inline]
  pub const fn is_latched(&self) -> bool {
    self.terminal
  }

  /// The replay mode witness, by shared reference.
  #[inline]
  pub const fn mode(&self) -> &M {
    &self.mode
  }
}

/// Converts a session refusal into the consumer's error, checking the coherence law at the
/// one site where the conversion happens.
#[inline]
fn refuse<E>(refusal: SessionRefusal) -> E
where
  E: From<SessionRefusal> + MaybeTerminal,
{
  let converted = E::from(refusal);
  // Unconditional, and it has to be. A refusal that reports itself as incomplete gives the
  // caller a zero-work infinite refill loop — refill, refuse before any work, identical error,
  // forever — which is the denial of service the budget exists to prevent. Gating this on
  // `debug_assertions` would remove the wall from exactly the builds that face untrusted
  // input. It is also free where it matters: `refuse` runs only on the refusal path, at most a
  // few times in a session's life and never on the success path, so it costs one
  // `is_terminal()` call per refused attempt and nothing per token or per event.
  assert!(
    converted.is_terminal(),
    "From<SessionRefusal> must yield a terminal error: a refusal that reports itself as \
     incomplete gives the caller a zero-work infinite refill loop (the gate refuses before \
     any work, so the identical error comes back forever)"
  );
  converted
}

impl<St, Off> PartialSession<St, Off, RedriveFromBase> {
  /// Drives one attempt over the caller's current buffer, re-driving from the session's base.
  ///
  /// Every bound this method needs is stated **here**, on its own where-clause, and not on
  /// the shared impl block above. That is deliberate and it is measured: hoisting them
  /// costs eighteen downstream errors on a perfectly legal offset type, taking out
  /// [`committed`](Self::committed) and this method together. And they cannot be left
  /// unstated either — a projection equality (`L: Lexer<'inp, Offset = Off>`) does **not**
  /// reverse-normalise onto a free type parameter, so `Off` inherits nothing from
  /// `Lexer::Offset`'s own bound list.
  ///
  /// # The refill loop
  ///
  /// The caller owns the buffer, exactly as with
  /// [`parse_partial`](crate::input::parse_partial): on an incomplete result it appends the next
  /// chunk to *its own* buffer and calls this again over the larger slice. What the session
  /// adds is the cross-attempt budget, the terminal latch, and the harvested frontier.
  ///
  /// # Cost, and diagnostics
  ///
  /// The `ctx` you pass is consumed per attempt, so the diagnostics of the attempt that
  /// finally succeeds (or finally fails non-incompletely) are the **complete** set — earlier
  /// attempts' emitters are redundant, not partial. Threading one collector through the ctx
  /// (`(&mut shared, cache)`) is legal and **duplicates** the prefix's diagnostics once per
  /// attempt: complete but noisy, never lossy. Like [`Budget::Unbounded`], that is a choice
  /// you write.
  ///
  /// **That reasoning is about the diagnostic channel, and it does not carry to an emitter
  /// with an event channel.** A recording `Sink` (`rowan`) threaded across attempts
  /// records the *replayed prefix* again on every attempt, and duplicated committed tokens
  /// are corruption rather than noise.
  ///
  /// Materialization catches **some** of it — duplicated non-zero-width token spans trip the
  /// monotone wall (`FinishError::OverlappingSpans`) — and this doc previously claimed that
  /// closed the shape. It does not, and the claim is withdrawn rather than narrowed: an
  /// attempt can commit no token and still emit structure, leaving a node in the log that a
  /// later redrive's token then makes materializable, and two zero-width spans never trip the
  /// wall at all. Either resulting event log is byte-identical to one a legal single parse
  /// could produce, so no materialization check can separate them. Both escapes are pinned as
  /// open defects in the sink's own suite.
  ///
  /// So carrying a recording sink across attempts is **corruption**, and it is refused at
  /// compile time by the [`ValueKeyedEmitter`](crate::emitter::ValueKeyedEmitter) bound below,
  /// which a recording `Sink` deliberately does not satisfy. The escapes above are why a
  /// *runtime* check could not have substituted for that bound. The supported streaming-CST
  /// shape is a **fresh sink per attempt** over the whole never-drained buffer, materialized on
  /// the final attempt.
  ///
  /// **Identity is delivered per attempt.** The attempt's input *owns* the emitter
  /// `ctx.provide()` yields, for exactly that attempt's life, so the suppression watermarks the
  /// attempt raises can only ever describe that attempt's own log — a property of the type
  /// rather than of this method's discipline. The caveat that survives is the collector one
  /// above and nothing wider: the binding is to whatever `provide()` hands over, so a `&mut` to
  /// a collector shared across attempts still observes the replayed prefix once per attempt —
  /// noisy, never lossy, and a choice you write.
  ///
  /// Cumulative lexer work is Θ(Σ attempt lengths). The budget is the wall.
  ///
  /// # Errors
  ///
  /// Returns the emitter's error type. Two of its values come from the session itself,
  /// converted through `From<SessionRefusal>` and always terminal: the budget refusal and
  /// the terminal latch. Both are decided **before any work**.
  ///
  /// # Panics
  ///
  /// Panics if `src` is shorter than the frontier the session has already committed. A
  /// caller that hands back a *shrunken* buffer has made a mistake no return value can
  /// repair — silently regressing the frontier would produce a wrong parse — so it is a
  /// deliberate, named panic at the cause, in every build.
  pub fn parse<'inp, L, Ctx, Lang, O, P>(
    &mut self,
    ctx: Ctx,
    src: &'inp L::Source,
    is_final: bool,
    mut f: P,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp, State = St, Offset = Off>,
    St: Clone,
    Off: Ord + TryInto<u64>,
    Lang: ?Sized,
    Ctx: ParseContext<'inp, L, Lang>,
    // A session re-drives from base every attempt, so an emitter that RECORDS what it is told
    // accumulates the replayed prefix instead of observing it once. This bound states the fact
    // the crate already believes: a recording `Sink` is deliberately not a `ValueKeyedEmitter`
    // — its marks are positions in a log, not a function of emission state, which is why a
    // sink cannot wrap a sink — so pairing one with a session is refused here, at compile
    // time, rather than documented as unsupported. The diagnostics-only emitters (`Fatal`,
    // `Verbose`, `Silent`, `Ignored`, and `&mut` of each) all satisfy it, so streaming
    // diagnostics is untouched.
    Ctx::Emitter: crate::emitter::ValueKeyedEmitter,
    Partial: SurfaceIncomplete<'inp, L, Ctx, Lang>,
    P: ParseInput<'inp, L, O, Ctx, Lang, Partial>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: MaybeTerminal + From<SessionRefusal>,
  {
    // The shrunken-buffer wall, before anything reads the frontier.
    let attempt_len = <L::Source as Source<Off>>::len(src);
    assert!(
      attempt_len >= self.committed,
      "src is shorter than the committed frontier: a re-driving session re-lexes the whole \
       buffer, so a caller may only append to it"
    );

    // Step 1 — the budget gate, before any work. `saturating_add` can only ever
    // over-refuse, and only in ranges no real buffer reaches; under `Unbounded` the
    // comparison is skipped entirely, so saturation can never refuse a legal session.
    let lexable: u64 = attempt_len.try_into().unwrap_or(u64::MAX);
    let projected = self.spent.saturating_add(lexable);
    if let Budget::Bytes(cap) = self.budget
      && projected > cap
    {
      return Err(refuse(SessionRefusal::BudgetExhausted {
        spent: projected,
        budget: cap,
      }));
    }

    // Step 2 — the terminal latch, also before any work. A latched session never unlatches:
    // more input cannot un-trip a monotone limiter.
    if self.terminal {
      return Err(refuse(SessionRefusal::TerminalLatched));
    }

    self.spent = projected;

    // Step 3 — seed the attempt from the **base**, byte for byte what the one-shot does.
    // The base state is cloned, never moved out and never written back (see the field's own
    // note).
    let mut input = Input::<L, Ctx, Lang, Partial>::with_state_and_context(
      src,
      self.state.clone(),
      ctx.provide(),
    );
    if is_final {
      input.seal();
    }

    // Step 4 — drive `f` exactly as the one-shot does. No transaction wrapping: the caller
    // owns transaction discipline, and that ownership is what makes a session possible.
    let outcome = {
      let mut input_ref = input.as_ref();
      f.parse_input(&mut input_ref)
    };

    // Step 5 — harvest, and harvest exactly two things. `committed` so the caller can see
    // progress, and the terminal latch so a trip sticks. The evolved `L::State` is
    // deliberately **not** harvested: nothing in this mode ever seeds from it, and writing
    // it would destroy the base state the next attempt needs.
    self.committed = input.span.end();
    if let Err(e) = &outcome
      && e.is_terminal()
    {
      self.terminal = true;
    }
    outcome
  }
}

#[cfg(test)]
#[cfg(any(feature = "std", feature = "alloc"))]
mod tests;
