use crate::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput,
  emitter::EmitterView,
  input::{Complete, Completeness},
  span::Spanned,
};

pub use cst::*;
pub use expr::*;
mod cst;
mod expr;

/// PRATT_CENSUS — source-census tests over `include_str!` snapshots. The census greps the
/// same source bytes in every configuration, so one allocator-enabled run locks it for all of
/// them; the string-building the checks use needs `format!`, which the allocator-less build
/// lacks.
#[cfg(all(test, any(feature = "std", feature = "alloc")))]
mod census_tests;

/// The power level of an operator, used to determine the order of operations in Pratt parsing.
///
/// A binding power is *ordered*, nothing more: the drivers only ever **compare** two powers.
/// They never compute a neighbouring level, so an implementation owes no `next`/`prev` pair
/// and no saturation discipline. Associativity is expressed by the *strictness* of the
/// recursion bound on the operator's own power — a left- or non-associative operator recurses
/// on a strictly-greater bound, a right-associative one on a greater-or-equal bound — which is
/// exact at the extremes of the ladder, where stepping one level was not.
///
/// Implemented by tokora for every standard integer type, so a plain `i64` — the default
/// `Power` of [`Precedenced`] — works out of the box. Custom implementations (a
/// domain-specific precedence ladder, a newtype with named levels) remain first-class; the
/// integer impls only cover the common numeric case that would otherwise force every consumer
/// into a newtype (the orphan rule bars downstream `impl PrattPower for i64`).
pub trait PrattPower: Default + Clone + Ord {}

macro_rules! impl_pratt_power_for_int {
  ($($int:ty),+ $(,)?) => {
    $(
      /// Integer binding power: the ladder is the type's own total order.
      impl PrattPower for $int {}
    )+
  };
}

impl_pratt_power_for_int!(
  i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize
);

/// The recursion bound a pratt driver descends with: an operator power plus the strictness
/// with which it admits the next operator.
///
/// This is the whole of what used to be power *arithmetic*. A left- or non-associative
/// operator at `p` recurses with `Exclusive(p)` so an equal-power operator to its right folds
/// into the outer call; a right-associative operator recurses with `Inclusive(p)` so the
/// equal-power operator clears the bound and is consumed by the inner call. Entry
/// (`min_precedence`) and prefix operands are `Inclusive`.
///
/// Stepping the ladder instead (`p + 1` / `p - 1`) is wrong at its extremes — the step
/// saturates and the strictness silently inverts — and it obliges every custom power type to
/// supply a dense successor function the drivers can no longer ask for.
#[derive(Debug, Clone)]
pub(crate) enum PrattFloor<P> {
  /// Admits a power `q` iff `q >= m`.
  Inclusive(P),
  /// Admits a power `q` iff `q > m`.
  Exclusive(P),
}

impl<P: Ord> PrattFloor<P> {
  /// Does this bound admit an operator of power `q`?
  #[inline(always)]
  pub(crate) fn admits(&self, q: &P) -> bool {
    match self {
      Self::Inclusive(m) => q >= m,
      Self::Exclusive(m) => q > m,
    }
  }
}

/// A type with an associated precedence level, used in Pratt parsing.
#[derive(Debug, Clone, Copy)]
pub struct Precedenced<T, Power = i64> {
  token: T,
  precedence: Power,
}

impl<T, Power> Precedenced<T, Power> {
  /// Creates a new `Precedenced` with the given token and precedence.
  #[inline(always)]
  pub const fn new(token: T, precedence: Power) -> Self {
    Self { token, precedence }
  }

  /// Returns a new `Precedenced` with the given token but a different precedence level.
  #[inline(always)]
  pub const fn with_precedence(token: T, precedence: Power) -> Self {
    Self { token, precedence }
  }

  /// Returns the reference to the token contained in this `Precedenced`.
  #[inline(always)]
  pub const fn token_ref(&self) -> &T {
    &self.token
  }

  /// Returns the mutable reference to the token contained in this `Precedenced`.
  #[inline(always)]
  pub const fn token_mut(&mut self) -> &mut T {
    &mut self.token
  }

  /// Returns the precedence level of this `Precedenced`.
  #[inline(always)]
  pub const fn precedence(&self) -> &Power {
    &self.precedence
  }

  /// Decomposes this `Precedenced` into its precedence.
  #[inline(always)]
  pub fn into_precedence(self) -> Power {
    self.precedence
  }

  /// Decomposes this `Precedenced` into its data.
  #[inline(always)]
  pub fn into_data(self) -> T {
    self.token
  }

  /// Decomposes this `Precedenced` into its token and precedence components.
  #[inline(always)]
  pub fn into_components(self) -> (T, Power) {
    (self.token, self.precedence)
  }
}

/// A left-hand side for Pratt parsing, which can be either an operand or a prefix operator with its precedence level.
#[derive(Debug, Clone, Copy)]
pub enum PrattLHS<Op, Pre, Power = i64> {
  /// A left-hand side that is an operand (not an operator).
  Operand(Op),
  /// A left-hand side that is a prefix operator with its precedence level.
  Prefix(Precedenced<Pre, Power>),
}

/// An infix operator for Pratt parsing, which can be left-associative, right-associative, or non-associative with its precedence level.
#[derive(Debug, Clone, Copy)]
pub enum PrattInfix<L, R, N> {
  /// A left-associative infix operator with its precedence level and operator type.
  Left(L),
  /// A right-associative infix operator with its precedence level and operator type.
  Right(R),
  /// A non-associative infix operator with its precedence level and operator type.
  ///
  /// **Non-associative in the conventional sense: chaining is a syntax error.** After a frame
  /// folds one of these at power `p`, a second *infix* operator at exactly `p` — whatever its own
  /// associativity — fails the parse with
  /// [`NonAssociativeChain`](crate::error::NonAssociativeChain), with the offending operator left
  /// on the input. The latch is cleared by folding an infix at a different power and is untouched
  /// by a postfix fold. The contract is per-operator rather than whole-chain fixity resolution:
  /// `a == b < c` (this variant, then `Left` at the same power) is rejected, while `a < b == c` is
  /// accepted — see the associativity table in the pratt reference.
  Neither(N),
}

/// A right-hand side for Pratt parsing: an infix operator with its precedence level and
/// associativity, a zero-token infix continuation, a postfix operator with its precedence level,
/// or the end of the expression.
#[derive(Debug, Clone, Copy)]
pub enum PrattRHS<L, R, N, Post, Power = i64> {
  /// An infix operator with its precedence level and associativity.
  Infix(Precedenced<PrattInfix<L, R, N>, Power>),
  /// A **zero-token** infix continuation: the expression continues here, and the operator that
  /// continues it is spelled with no token at all.
  ///
  /// This is juxtaposition as an *operator* rather than as a repetition — LogQL's
  /// `labelFilter labelFilter`, where the connective competing with `and`, `,` and `|` at the
  /// same power is the empty string. A repetition dissolves into a `while` loop at the
  /// production and needs nothing from this enum; an operator does not, because its right
  /// operand's floor has to be the **driver's** and not the production's. Reporting the
  /// already-parsed right operand as a [`Postfix`](Self::Postfix) is the workaround this variant
  /// replaces, and what it gives up is exactly that: the floor moves to the production, where no
  /// law of this driver reaches it.
  ///
  /// # It is left-associative, and there is no associativity to choose
  ///
  /// The payload is the [`Infix`](Self::Infix) arm's **left**-associative operator type, and the
  /// driver descends the right operand on `> power` — the left-associative bound — with no
  /// alternative offered. That is not a default; it is the depth bound. A right-associative bound
  /// admits the operator's *own* power into the right operand, so an inner frame handed a
  /// zero-token continuation at that power reports the same continuation again and descends
  /// **having consumed nothing**: one native frame per level, none of them paid for by the
  /// document. On the left-associative bound the inner frame declines it, the chain iterates in
  /// one frame, and that frame's progress guard bounds it. Zero-token continuations can therefore
  /// nest only by *strictly increasing* power, which is a property of the grammar's ladder rather
  /// than of the input.
  ///
  /// Every other driver posture that reads an infix report reads this one identically: the floor
  /// decides whether the continuation is this expression's, and a second same-power operator
  /// after a [`Neither`](PrattInfix::Neither) fold is refused here exactly as it is there. An
  /// adjacency never *arms* that latch, because it folds as [`Left`](PrattInfix::Left) — which is
  /// also why it needs no fold hook and no CST kind of its own. Tell an adjacency from a spelled
  /// left-associative operator in the payload type, as LogQL must anyway to tell `and` from `,`
  /// from `|`.
  ///
  /// # The obligation moves from the report to the operand
  ///
  /// **"Consume what you report" has nothing to bite on here**, so this report is exempt from it.
  /// Consuming nothing is the point; consuming *something* is legal too — a CST-shaped classifier
  /// that skips trivia before deciding two operands are adjacent has consumed that trivia. Neither
  /// is a violation, and no report boundary is applied.
  ///
  /// **The right operand carries the obligation instead.** The driver reads committed consumption
  /// when the operand parse returns — before the fold and before the CST wrap, since both are the
  /// work this charge exists to price — and refuses a continuation whose cycle advanced nothing:
  /// [`UnexpectedEoRhs`](crate::error::UnexpectedEoRhs), terminal, with a debug build additionally
  /// tripping an assertion naming this rule. Without that charge a document buys a fold, a wrap
  /// and another cycle for no input at all, and the next cycle reports the same continuation over
  /// the same bytes.
  Adjacent(Precedenced<L, Power>),
  /// Postfix operator with its precedence level and operator type.
  Postfix(Precedenced<Post, Power>),
  /// The expression does not continue at this position: the next token is not an infix or
  /// postfix operator of this expression, or the input is exhausted. The driver restores
  /// whatever the deciding read consumed and ends the RHS loop.
  ///
  /// Return this rather than a below-minimum "sentinel" operator. A sentinel is a real
  /// report and is subject to the floor like any other: whether it binds depends on
  /// grammar-wide power discipline that no signature can check, and it is not expressible at
  /// all over an unsigned `Power`, whose minimum representable value *is* the default floor.
  /// `End` is not a power, so no floor admits it.
  End,
}

/// A trait for parsing left-hand sides in Pratt parsing, which can be either operands or operators with precedence levels.
///
/// # The contract
///
/// **Consume what you report.** A [`Prefix`](PrattLHS::Prefix) report means "I have taken this
/// operator": the driver then parses its operand by re-entering the expression at the position
/// the report left behind, folds the two, and wraps the result. A report that consumed nothing
/// hands that recursion the very input it was given, which reports again and descends until the
/// stack runs out — and short of that, folds and records an operator that is not in the source.
/// The typed driver checks this the instant the report crosses back, before the recursion,
/// before the fold and before the CST wrap, since all three can move input the report did not.
/// A `Prefix` report that consumed nothing is a grammar bug: the driver raises the end-of-LHS
/// error ([`UnexpectedEoLhs`](crate::error::UnexpectedEoLhs)) marked terminal, rather than
/// descending or folding a phantom operator, and a debug build additionally trips an assertion
/// naming this rule. That assertion is raised **after** the driver has settled its own guards, so
/// the two profiles leave the surrounding grammar the same input position and the same emission
/// log — see the emission contract on [`Pratt`] — and differ only in whether the violation
/// arrives as a panic or as that error.
///
/// **Every `Prefix` is held to it.** There is no admitted/declined distinction on this channel:
/// `PrattLHS` has no decline arm, no floor applies to a prefix power, and the driver acts on the
/// report the moment it is made. [`ParsePrattRHS`]'s exemption — a report the floor declines is
/// an ending in another spelling, and may consume nothing — has no counterpart here.
///
/// An [`Operand`](PrattLHS::Operand) report is not held to it. The driver neither recurses nor
/// folds for an operand, and the RHS loop that follows takes its own watermark, so an operand
/// that consumed nothing costs the driver nothing.
pub trait ParsePrattLHS<'inp, Power, Op, Pre, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Try to parse a pratt lhs from the lexer input, returning with its precedence level if successful.
  fn parse_pratt_lhs(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<PrattLHS<Op, Pre, Power>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

// STAYS COMPLETE-ONLY: the position-test hazard is gone — neither driver asks where the
// buffer ends any more — but the other S-class hazard remains: RHS operator detection is a
// peek, and under a partial input a frontier-cut window reads as "no operator", ending the
// expression where more input would have continued it. That needs the deferred
// frontier-window rule; the vocabulary traits reserve the `Cmpl` position, the impls stay at
// the default.
impl<'inp, P, Power, L, Op, Pre, Ctx, Lang: ?Sized>
  ParsePrattLHS<'inp, Power, Op, Pre, L, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, PrattLHS<Op, Pre, Power>, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_pratt_lhs(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<PrattLHS<Op, Pre, Power>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parse_input(input)
  }
}

/// A trait for parsing right-hand sides in Pratt parsing: infix operators with precedence
/// levels and associativity, postfix operators with precedence levels, or the end of the
/// expression.
///
/// # The contract
///
/// **Invoked at any position, including exhaustion.** The driver does not pre-screen the
/// position before asking; where the expression ends is this parser's answer, so it is asked
/// with no input left just as it is asked mid-stream.
///
/// **Say [`End`](PrattRHS::End) when the expression does not continue here** — at exhaustion,
/// and on any token that is not an operator of this expression. The driver rolls back
/// whatever the deciding read consumed, so the token is left for the surrounding grammar.
///
/// **Consume what you report.** An [`Infix`](PrattRHS::Infix) or [`Postfix`](PrattRHS::Postfix)
/// report means "I have taken this operator": the next cycle must see fresh input. The typed
/// driver checks this the instant the report crosses back — before it recurses and before any
/// fold runs, since both of those can move input the report did not. A report the floor
/// **admitted** that consumed nothing is a grammar bug: the driver rolls **the cycle** back — not
/// the expression — and raises the end-of-RHS error
/// ([`UnexpectedEoRhs`](crate::error::UnexpectedEoRhs)) marked terminal, rather than folding a
/// phantom operator or ending the expression with a silent `Ok`, and a debug build additionally
/// trips an assertion naming this rule. That assertion is raised **after** the cycle rollback and
/// after the expression-scoped guard has committed, so the two profiles leave the surrounding
/// grammar the same input position and the same emission log — see the emission contract on
/// [`Pratt`] — and differ only in whether the violation arrives as a panic or as that error. Only
/// admitted reports are held to it — a report the floor declines is the [`End`](PrattRHS::End)
/// answer in another spelling, is rolled back, and may consume nothing.
///
/// **[`Adjacent`](PrattRHS::Adjacent) is exempt, and its obligation is discharged elsewhere.** A
/// zero-token continuation has no operator to have consumed, so there is nothing for the report
/// boundary to measure; the driver charges its **right operand** instead, refusing a continuation
/// whose cycle advanced nothing. That variant states the rule in full.
///
/// **A terminal stop is an `Err`, never an `End`.** A tripped scanner limit is not the end of
/// an expression; surfacing it as one lets a truncated view masquerade as a complete parse.
///
/// **A second same-power operator after a [`Neither`](PrattInfix::Neither) fold is not yours to
/// answer.** Report it as the ordinary [`Infix`](PrattRHS::Infix) it is; the driver holds the
/// chain latch and refuses it, restoring the deciding read and returning
/// [`NonAssociativeChain`](crate::error::NonAssociativeChain) — not terminal, so an explicit
/// [`recover`](crate::ParseInput::recover) may spend it. Answering [`End`](PrattRHS::End) instead
/// would hide the constraint: in a nested position the enclosing frame folds the operator by its
/// own rules and the chain re-associates across it with nothing left over to reject.
///
/// The offset that error carries is the position the driver **hands the input back at** — its own
/// restore target, which is the committed frontier this cycle started from and not a position
/// measured anywhere near your operator. An implementation that skips leading trivia tokens before
/// reading its operator (the CST-style shape, entirely permitted: this function holds the whole
/// [`InputRef`] and decides for itself where its operator begins) is therefore reported *before*
/// the skipped trivia; so is one whose operator is spelled with several tokens, and so is any
/// operator a whitespace skip or a reported lexer error sits in front of. The driver cannot do
/// better without running this function, and it must decide the repeat before it may. What the
/// offset always is, is the byte the input was left at — which is where a grammar that catches the
/// `Err`, or recovers with [`inplace_recover`](crate::ParseInput::inplace_recover), resumes;
/// [`recover`](crate::ParseInput::recover) and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) roll back to their own attempt origin
/// first. See [`NonAssociativeChain`](crate::error::NonAssociativeChain) for the full statement.
///
/// ```
/// use tokora::parser::{PrattInfix, PrattRHS, Precedenced};
///
/// # #[derive(Debug, PartialEq)] enum Tok { Plus, Semi }
/// fn classify(tok: Option<&Tok>) -> PrattRHS<(), (), (), (), i64> {
///   match tok {
///     Some(Tok::Plus) => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), 1)),
///     // Not an operator of this expression, and end of input: the same answer.
///     Some(_) | None => PrattRHS::End,
///   }
/// }
///
/// assert!(matches!(classify(Some(&Tok::Plus)), PrattRHS::Infix(_)));
/// assert!(matches!(classify(Some(&Tok::Semi)), PrattRHS::End));
/// assert!(matches!(classify(None), PrattRHS::End));
/// ```
pub trait ParsePrattRHS<
  'inp,
  Power,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  Post,
  L,
  Ctx,
  Lang: ?Sized = (),
  Cmpl = Complete,
>
{
  /// Try to parse a pratt rhs from the lexer input, returning with its precedence level if successful.
  fn parse_pratt_rhs(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<
    PrattRHS<LeftAssoc, RightAssoc, NeitherAssoc, Post, Power>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, P, Power, LeftAssoc, RightAssoc, NeitherAssoc, Post, L, Ctx, Lang: ?Sized>
  ParsePrattRHS<'inp, Power, LeftAssoc, RightAssoc, NeitherAssoc, Post, L, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: ParseInput<'inp, L, PrattRHS<LeftAssoc, RightAssoc, NeitherAssoc, Post, Power>, Ctx, Lang>,
{
  #[inline(always)]
  fn parse_pratt_rhs(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
  ) -> Result<
    PrattRHS<LeftAssoc, RightAssoc, NeitherAssoc, Post, Power>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self.parse_input(input)
  }
}

/// A trait for postfix fold dispatch
pub trait PrattFoldPostfix<'inp, Power, Operator, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Apply the postfix fold to the operand.
  fn fold_postfix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    operand: O,
    operator: Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, P, Power, Operator, L, O, Ctx, Lang: ?Sized>
  PrattFoldPostfix<'inp, Power, Operator, L, O, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang>,
    O,
    Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_postfix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    operand: O,
    operator: Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(input, operand, operator)
  }
}

/// A trait for infix fold dispatch
///
/// # Diagnosing a chained comparison here, and the two things you must arrange first
///
/// `a < b < c` is well-formed to this driver whenever `<` is [`Left`](PrattInfix::Left), and
/// rustc diagnoses it in the parser rather than in the grammar. The fold is the right place to do
/// the same — [`NonAssociativeChain`](crate::error::NonAssociativeChain) carries a single `at`
/// offset and a two-more-offsets variant of it does not fit the driver's per-frame error budget,
/// so enriching that error is not the route. But the fold is not handed what such a diagnostic
/// needs, and this is the list of what it is missing rather than a claim that it has it:
///
/// * **no span for the operator being folded.** `operator` is a [`Precedenced`], which carries
///   the payload you chose and a power — nothing positional — and `input.span()` at fold time is
///   the committed frontier *past* the right operand. Put the span in the payload type; that is
///   legal, because payload types are yours, but it is an instruction and not a given.
/// * **no previous operator at all.** It has to be recovered from the shape of `left`, rustc's
///   way, which requires `O` to be a node you can inspect. **That excludes the `O = ()` CST
///   shape**, where the fold is handed nothing to look at — and CST-shaped grammars are what this
///   crate is built for. Such a grammar either gives up `O = ()` for a marker type carrying the
///   last comparison's power and span, or spells the comparison family
///   [`Neither`](PrattInfix::Neither) and takes the driver's own refusal instead.
pub trait PrattFoldInfix<
  'inp,
  Power,
  LeftAssoc,
  RightAssoc,
  NeitherAssoc,
  L,
  O,
  Ctx,
  Lang: ?Sized = (),
  Cmpl = Complete,
>
{
  /// Apply the infix fold to the operand.
  fn fold_infix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    left: O,
    right: O,
    operator: Precedenced<PrattInfix<LeftAssoc, RightAssoc, NeitherAssoc>, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, P, Power, LO, RO, NO, L, O, Ctx, Lang: ?Sized>
  PrattFoldInfix<'inp, Power, LO, RO, NO, L, O, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang>,
    O,
    O,
    Precedenced<PrattInfix<LO, RO, NO>, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_infix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    left: O,
    right: O,
    operator: Precedenced<PrattInfix<LO, RO, NO>, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(input, left, right, operator)
  }
}

/// A trait for prefix fold dispatch
pub trait PrattFoldPrefix<'inp, Power, Operator, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Apply the prefix fold to the operand.
  fn fold_prefix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    operand: O,
    operator: Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, P, Power, Operator, L, O, Ctx, Lang: ?Sized>
  PrattFoldPrefix<'inp, Power, Operator, L, O, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang>,
    O,
    Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_prefix(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang>,
    operand: O,
    operator: Precedenced<Operator, Power>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(input, operand, operator)
  }
}

/// A trait for postfix fold dispatch
///
/// # The emitter's operations, not the emitter
///
/// The trailing parameter is an [`EmitterView`] rather than `&mut Ctx::Emitter`, for the reason
/// [`Decision`](crate::Decision) gives: `&mut Ctx::Emitter` is itself an emitter, and a fold
/// handed one could install it as the context of a second parse over a foreign buffer. Every
/// emitting operation a fold could reach before, it reaches now under the same name.
pub trait PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang: ?Sized = ()> {
  /// Apply the postfix fold to the operand.
  fn fold_postfix(
    &mut self,
    operand: Spanned<L::Token, L::Span>,
    operator: Spanned<L::Token, L::Span>,
    emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

impl<'inp, P, Power, L, Ctx, Lang: ?Sized> PrattFoldTokenPostfix<'inp, Power, L, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    Spanned<L::Token, L::Span>,
    Spanned<L::Token, L::Span>,
    EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  )
    -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_postfix(
    &mut self,
    operand: Spanned<L::Token, L::Span>,
    operator: Spanned<L::Token, L::Span>,
    emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(operand, operator, emitter)
  }
}

/// A trait for infix fold dispatch
///
/// The trailing parameter is an [`EmitterView`] rather than `&mut Ctx::Emitter` — see
/// [`PrattFoldTokenPostfix`] for why.
pub trait PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang: ?Sized = ()> {
  /// Apply the infix fold to the operand.
  fn fold_infix(
    &mut self,
    left: Spanned<L::Token, L::Span>,
    right: Spanned<L::Token, L::Span>,
    infix: Spanned<PrattInfix<L::Token, L::Token, L::Token>, L::Span>,
    emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

impl<'inp, P, Power, L, Ctx, Lang: ?Sized> PrattFoldTokenInfix<'inp, Power, L, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    Spanned<L::Token, L::Span>,
    Spanned<L::Token, L::Span>,
    Spanned<PrattInfix<L::Token, L::Token, L::Token>, L::Span>,
    EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  )
    -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_infix(
    &mut self,
    left: Spanned<L::Token, L::Span>,
    right: Spanned<L::Token, L::Span>,
    infix: Spanned<PrattInfix<L::Token, L::Token, L::Token>, L::Span>,
    emitter: EmitterView<'_, 'inp, L, <Ctx>::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <<Ctx>::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(left, right, infix, emitter)
  }
}

/// A trait for prefix fold dispatch
///
/// The trailing parameter is an [`EmitterView`] rather than `&mut Ctx::Emitter` — see
/// [`PrattFoldTokenPostfix`] for why.
pub trait PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang: ?Sized = ()> {
  /// Apply the prefix fold to the operand.
  fn fold_prefix(
    &mut self,
    operator: Spanned<L::Token, L::Span>,
    operand: Spanned<L::Token, L::Span>,
    emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>;
}

impl<'inp, P, Power, L, Ctx, Lang: ?Sized> PrattFoldTokenPrefix<'inp, Power, L, Ctx, Lang> for P
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  P: FnMut(
    Spanned<L::Token, L::Span>,
    Spanned<L::Token, L::Span>,
    EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  )
    -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn fold_prefix(
    &mut self,
    operator: Spanned<L::Token, L::Span>,
    operand: Spanned<L::Token, L::Span>,
    emitter: EmitterView<'_, 'inp, L, Ctx::Emitter, Lang>,
  ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
  {
    self(operator, operand, emitter)
  }
}
