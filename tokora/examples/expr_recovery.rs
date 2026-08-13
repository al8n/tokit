//! Error **recovery** with the typed Pratt driver — the rust-analyzer posture, in tokora.
//!
//! Every other example in this repository aborts on the first error. This one does not: it
//! parses malformed input to completion, records one diagnostic per problem, and hands back
//! *both* an AST whose holes are explicit `Expr::Error` nodes and a lossless rowan tree whose
//! holes are explicit `ErrorExpr` nodes. That is what an IDE needs — a tree for every
//! keystroke, not a tree for the inputs that happen to be valid.
//!
//! The precise claim is **no input makes [`parse`] fail**, which is not quite the same as
//! "everything recovers": one class of error *ends* a parse rather than holing it, and saying
//! where that line falls is half of what this file is for. See
//! *What does not recover* below.
//!
//! Run: `cargo run --example expr_recovery --features "std,logos,rowan"`
//!
//! # The reference posture
//!
//! rust-analyzer, not rustc. rustc propagates a `PResult` upward and gives up on the
//! enclosing production; r-a completes the node anyway and keeps looping, which is the
//! behaviour that serves an editor. Its two rules, both reproduced here:
//!
//! * **Missing left-hand side** → `err_recover("expected expression", EXPR_RECOVERY_SET)`.
//!   If the offending token belongs to the *recovery set* — something an enclosing construct
//!   can still use — report it and consume **nothing**, leaving a zero-width error node. If
//!   it belongs to nobody, report it and swallow exactly one token into a one-token error
//!   node, so the parse is guaranteed to move.
//! * **Missing right-hand side** → the recursive operand parse errors *internally*, the
//!   binary node is completed anyway, **and the loop continues**. `+ 1 +` therefore folds
//!   two `+` operators and reports two problems, rather than reporting one and stopping.
//!
//! # What makes it work here: a zero-width `Operand` is legal
//!
//! The whole posture rests on one property of [`ParsePrattLHS`](tokora::parser::ParsePrattLHS):
//! **[`Operand`](tokora::parser::PrattLHS::Operand) is not held to "consume what you report".**
//! Only [`Prefix`](tokora::parser::PrattLHS::Prefix) is stall-checked, because only a prefix
//! report makes the driver re-enter the expression at the same position. An operand costs the
//! driver no recursion and no fold, so a report that consumed nothing costs it nothing — which
//! is exactly the licence an error node needs.
//!
//! The other three properties this file leans on:
//!
//! * an ordinary `Err` out of a channel or a fold **keeps** the expression's emissions, so a
//!   parse can carry several diagnostics;
//! * the RHS channel's [`End`](tokora::parser::PrattRHS::End) rolls the deciding read back, so
//!   the recovery set can be tested by *peeking* and the token is left for whoever wants it;
//! * the driver's own terminal errors (`UnexpectedEoLhs` / `UnexpectedEoRhs`) fire only on a
//!   grammar bug — a report that consumed nothing where the driver required progress — never
//!   on malformed *input*. Recovery never has to distinguish them from user error.
//!
//! # Where the error node comes from
//!
//! Not from the driver. The LHS channel writes it itself, through the raw CST transport:
//! [`cst_mark`](tokora::InputRef::cst_mark) before anything is consumed,
//! [`cst_start_at`](tokora::InputRef::cst_start_at) +
//! [`cst_finish`](tokora::InputRef::cst_finish) after. The retro-wrap shape is used for both
//! widths on purpose: a `?` between the mark and the wrap leaves *no* node rather than an
//! unclosed one, which is the same posture the driver's own fold wrap has.
//!
//! # What does **not** recover, and why that is correct
//!
//! Read this part. An example that showed only the recoveries would teach that everything
//! recovers, and that is not true — the useful shape of the truth is:
//!
//! * a **grammar** error becomes a hole and the parse continues;
//! * a **resource** error ends the parse, and no recovery point can spend it.
//!
//! There is exactly one of the second kind here: the **recursion budget**. Every nested group
//! and every prefix `-` enters the driver's descent, tokora's parser-facing default is
//! `RecursionLimiter::PARSE_DEFAULT_DEPTH` (16 — and `parse_with_depth` is this file's
//! demonstration of choosing otherwise), and a trip raises `RecursionLimitReached`. That
//! error is *terminal*, and every
//! recovery combinator — [`inplace_recover`](tokora::ParseInput::inplace_recover) included —
//! re-raises a terminal error rather than spending it. Deliberately. A budget whose whole
//! purpose is to stop a parse before it exhausts the native stack would be worthless if a
//! recovery point could swallow it and carry on descending.
//!
//! **Terminal for the attempt, not a switch thrown on the parse.** The trip is counted on a
//! monotone cell of the input session — so a grammar error type that discards the payload
//! cannot lose the stop — but every recovery point reads that cell *relative to the attempt it
//! is judging*, by snapshotting it beforehand and asking whether it moved. So a budget charges
//! the failure it actually stopped, and nothing else. Grammar code is free to catch a trip
//! itself and parse on; the recoveries after it behave exactly as they would in a parse that
//! never tripped (`tests/pratt_limit_unit_sink.rs`,
//! `a_caught_trip_does_not_disable_a_later_recovery`). An absolute reading would instead let one
//! deep expression early in a file suppress every diagnostic after it, which is the opposite of
//! what an editor wants. This grammar never catches the trip, so here the stop simply travels
//! out — but the mechanism is the per-attempt comparison, not a latch.
//!
//! So `recover_expr` never runs for it, no error node is synthesized, and there is nothing to
//! resume from. What [`parse`] owes the caller is therefore *not* "recover", it is **"do not
//! panic"**: record the trip, hand back an `Expr::Error` root, and set
//! [`Parsed::terminated`]. `parse` used to `expect()` here, which meant 64 nested parentheses —
//! lexically valid, grammatically valid, and shorter than this sentence — took the process down
//! in a file whose stated posture is that no input fails.
//!
//! The lossless tree still round-trips through a terminal trip, but only because the tree comes
//! out of a different door: see the note in [`parse`] on `finish` versus `finish_partial`.
//!
//! The trip gets its own diagnostic, [`Diag::DepthExceeded`], kept apart from
//! [`Diag::DriverContract`]. One is a resource bound working as designed and reachable from
//! ordinary input; the other is a bug that should never fire. A single variant covering both
//! would blunt the corpus tripwire that asserts the bug arm is unreachable.
//!
//! **On not choosing the number.** [`parse`] takes tokora's default rather than a figure of this
//! example's own, and the pins key off that same constant so they go red if it moves. That is a
//! choice now and not a limitation: a lossless parse *can* name its own budget, through
//! [`parse_lossless_with_context`](tokora::cst::parse_lossless_with_context), and [`parse_with_depth`]
//! is this file's one-line demonstration of it. `parse` stays defaulted because a recovery example
//! showing an unconfigured parse is the honest thing for a reader to copy — a deeper budget is a
//! claim about the stack the parse runs on, and only the caller can make it.

use rowan::{Language, NodeOrToken, SyntaxNode};
use tokora::{
  Emitter, InputRef, ParseContext, ParseInput, SimpleSpan, Token as TokenT,
  cache::DefaultCache,
  cst::{CstProfile, KindValidator, parse_lossless_with_context},
  emitter::{CstEmitter, DiagnosticKind, Verbose},
  error::{
    MaybeIncomplete, MaybeTerminal, NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs,
    UnexpectedEoRhs, UnexpectedEot, token::UnexpectedTokenOf,
  },
  input::{Cursor, InputContext},
  logos::{self, Logos},
  parser::{PrattFoldOp, PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced, pratt},
  span::Spanned,
  state::recursion_tracker::RecursionLimiter,
};

// ── Lossless lexer ──────────────────────────────────────────────────────────────
//
// No `skip` rule: whitespace is a real trivia token, so the tree round-trips byte for byte
// even when the parse recovered.

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
#[logos(crate = logos, error = LexError)]
pub(crate) enum Token {
  #[regex(r"[ \t\r\n]+")]
  Whitespace,
  #[regex(r"[0-9]+")]
  Num,
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  /// Lexes fine, parses nowhere: the token that exercises r-a's *bump* branch. No production
  /// in this grammar can consume an `@`, so leaving it in place would stall the parse.
  #[token("@")]
  At,
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Token::Whitespace => "whitespace",
      Token::Num => "number",
      Token::Plus => "+",
      Token::Minus => "-",
      Token::Star => "*",
      Token::Slash => "/",
      Token::LParen => "(",
      Token::RParen => ")",
      Token::At => "@",
    })
  }
}

impl TokenT<'_> for Token {
  type Kind = Token;
  type Error = LexError;

  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> Token {
    *self
  }
  fn is_trivia(&self) -> bool {
    matches!(self, Token::Whitespace)
  }
}

pub(crate) type ExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Diagnostics ─────────────────────────────────────────────────────────────────
//
// The emitter is `Verbose`, so these are *collected* rather than returned: every one of them
// is emitted through `InputRef::emit_error`, which returns `Ok(())` under this emitter and lets
// the grammar keep going. The `Err` channel is reserved for the one condition the production
// that finds it cannot repair — an unclosed group — and even that one is *reported* on this
// channel first, then propagated to the enclosing recovery point.

/// One recorded problem, plus the driver-contract arms no valid grammar reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Diag {
  /// The lexer rejected the bytes at this position.
  Lex,
  /// A token appeared where an operand had to start.
  ExpectedExpression,
  /// The digits lexed, and the value they spell does not fit in `i64`.
  ///
  /// The only problem in this grammar raised by input the *lexer* approved: `[0-9]+` bounds the
  /// shape of a literal and says nothing about its magnitude. It recovers like everything else,
  /// which is the whole claim — a parser with no failing input cannot have an operand arm that
  /// panics on a well-formed token.
  NumberOutOfRange,
  /// A parenthesized group ran out before its `)`.
  ExpectedRParen,
  /// The expression finished but the input did not.
  TrailingInput,
  /// The recursion budget tripped: the expression nests deeper than the parse allows.
  ///
  /// **Terminal, and the only terminal error a user can cause.** It is kept apart from
  /// [`DriverContract`](Self::DriverContract) precisely because the two are opposites — this one
  /// is a resource bound working as designed and reachable from ordinary input, that one is a
  /// bug. Folding them together would blunt the corpus tripwire that asserts the bug arm is
  /// never reached.
  DepthExceeded,
  /// A driver contract violation. Unreachable in this grammar — present because the driver's
  /// error types must be convertible, and folded to one variant so a regression that *does*
  /// reach it is loud rather than plausible.
  ///
  /// Note this no longer covers the depth budget; see [`DepthExceeded`](Self::DepthExceeded).
  DriverContract,
}

impl core::fmt::Display for Diag {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Diag::Lex => "invalid token",
      Diag::ExpectedExpression => "expected expression",
      Diag::NumberOutOfRange => "number does not fit in i64",
      Diag::ExpectedRParen => "expected `)`",
      Diag::TrailingInput => "unexpected trailing input",
      Diag::DepthExceeded => "expression nests too deeply",
      Diag::DriverContract => "parser contract violation",
    })
  }
}

// The recovery gate's two classification traits. Both defaults are `false`, and that is the
// right answer for every variant here: none of them is a partial-input sentinel, and none is a
// terminal scanner stop. Opting in is not optional — `inplace_recover` will not compile without
// them, which is the gate refusing to guess whether an error is spendable.
impl MaybeIncomplete for Diag {}
impl MaybeTerminal for Diag {}

impl From<LexError> for Diag {
  fn from(_: LexError) -> Self {
    Diag::Lex
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, ExprLexer<'inp>>> for Diag {
  fn from(_: UnexpectedTokenOf<'inp, ExprLexer<'inp>>) -> Self {
    Diag::ExpectedExpression
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for Diag {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    // Raised by the terminal-aware reads (`peek_kind`) on a *scanner* stop, never on genuine
    // end of input — that is `Ok(None)`, which this grammar treats as a recovery-set member.
    Diag::DriverContract
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoLhs<O, Lang, Set>> for Diag {
  fn from(_: UnexpectedEoLhs<O, Lang, Set>) -> Self {
    Diag::DriverContract
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoRhs<O, Lang, Set>> for Diag {
  fn from(_: UnexpectedEoRhs<O, Lang, Set>) -> Self {
    Diag::DriverContract
  }
}

impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for Diag {
  fn from(_: RecursionLimitReached<O, Lang>) -> Self {
    // NOT `DriverContract`. A depth trip is the one terminal error ordinary input can cause,
    // and the corpus cell's tripwire depends on the bug arm staying unreachable.
    Diag::DepthExceeded
  }
}

impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for Diag {
  fn from(_: NonAssociativeChain<O, Lang>) -> Self {
    Diag::DriverContract
  }
}

// ── AST ─────────────────────────────────────────────────────────────────────────

/// A binary operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BinOp {
  /// `+`
  Add,
  /// `-`
  Sub,
  /// `*`
  Mul,
  /// `/`
  Div,
}

/// The AST. `Error` is the recovery node — a real value the folds may combine, not an
/// absence, which is why a malformed parse still produces a complete tree.
///
/// Parentheses are **not** in the AST: the CST keeps them, the AST drops them. So `(1+)` and
/// `1 + ` share an AST and differ only in the tree — which is what makes the tree pin
/// load-bearing rather than decorative.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Expr {
  /// An integer literal.
  Num(i64),
  /// `- operand`
  Neg(Box<Expr>),
  /// `left op right`
  Bin(BinOp, Box<Expr>, Box<Expr>),
  /// The hole a recovery left behind.
  Error,
}

impl core::fmt::Display for Expr {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Expr::Num(n) => write!(f, "{n}"),
      Expr::Neg(e) => write!(f, "(-{e})"),
      Expr::Bin(op, l, r) => {
        let op = match op {
          BinOp::Add => '+',
          BinOp::Sub => '-',
          BinOp::Mul => '*',
          BinOp::Div => '/',
        };
        write!(f, "({l} {op} {r})")
      }
      Expr::Error => f.write_str("<error>"),
    }
  }
}

// ── Unified syntax-kind space ───────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
pub(crate) enum SyntaxKind {
  // Token images.
  Whitespace,
  Num,
  Plus,
  Minus,
  Star,
  Slash,
  LParen,
  RParen,
  At,
  // Node kinds.
  BinExpr,
  PrefixExpr,
  ParenExpr,
  /// The recovery node — zero-width where nothing was swallowed, one token wide where the
  /// bump branch ran.
  ErrorExpr,
  // Bookkeeping the sink synthesizes on its own behalf.
  Error,
  Gap,
  Root,
}
type K = SyntaxKind;

impl SyntaxKind {
  const fn raw(self) -> u16 {
    self as u16
  }
}

fn map_token(tok: &Token) -> u16 {
  (match tok {
    Token::Whitespace => K::Whitespace,
    Token::Num => K::Num,
    Token::Plus => K::Plus,
    Token::Minus => K::Minus,
    Token::Star => K::Star,
    Token::Slash => K::Slash,
    Token::LParen => K::LParen,
    Token::RParen => K::RParen,
    Token::At => K::At,
  }) as u16
}

/// The rowan language marker for this grammar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum ExprLang {}

impl Language for ExprLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    const KINDS: [SyntaxKind; 16] = [
      K::Whitespace,
      K::Num,
      K::Plus,
      K::Minus,
      K::Star,
      K::Slash,
      K::LParen,
      K::RParen,
      K::At,
      K::BinExpr,
      K::PrefixExpr,
      K::ParenExpr,
      K::ErrorExpr,
      K::Error,
      K::Gap,
      K::Root,
    ];
    KINDS[raw.0 as usize]
  }

  fn kind_to_raw(kind: SyntaxKind) -> rowan::SyntaxKind {
    rowan::SyntaxKind(kind as u16)
  }
}

// ── Binding powers ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Power(i32);

impl PrattPower for Power {}

const PREC_SUM: Power = Power(1); // + -   (left)
const PREC_PROD: Power = Power(2); // * /  (left)
const PREC_NEG: Power = Power(3); // unary -

// ── The recovery set ────────────────────────────────────────────────────────────

/// r-a's `EXPR_RECOVERY_SET`, translated to this grammar.
///
/// A token is in the set when **some enclosing construct can still consume it**: the closing
/// paren of a group, any infix operator (an enclosing Pratt frame will fold it), and the end
/// of input. For those, recovery reports and consumes nothing — a zero-width error node — so
/// the enclosing frame is handed the token it was waiting for.
///
/// Everything else is in nobody's follow set. Leaving it in place would hand the next cycle
/// the same input, so recovery swallows exactly one token: r-a's `err_and_bump`, and the
/// reason this grammar always terminates.
const fn in_recovery_set(kind: Option<Token>) -> bool {
  match kind {
    // Genuine end of input. (A *terminal* scanner stop never arrives here as `None` — the
    // terminal-aware read raises instead — so this arm cannot mistake a truncated view for
    // the end of the source.)
    None => true,
    Some(Token::RParen | Token::Plus | Token::Minus | Token::Star | Token::Slash) => true,
    Some(Token::Num | Token::LParen | Token::At | Token::Whitespace) => false,
  }
}

// ── Pratt channels ──────────────────────────────────────────────────────────────

type ExprIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, ExprLexer<'inp>, Ctx>;

/// Every bound the recovering grammar needs, in one alias: the CST seam plus a `Verbose`-shaped
/// error channel.
trait RecoveringCtx<'inp>:
  ParseContext<'inp, ExprLexer<'inp>, Emitter = <Self as RecoveringCtx<'inp>>::Emit>
{
  type Emit: CstEmitter<'inp, ExprLexer<'inp>> + Emitter<'inp, ExprLexer<'inp>, Error = Diag>;
}

impl<'inp, C> RecoveringCtx<'inp> for C
where
  C: ParseContext<'inp, ExprLexer<'inp>>,
  C::Emitter: CstEmitter<'inp, ExprLexer<'inp>> + Emitter<'inp, ExprLexer<'inp>, Error = Diag>,
{
  type Emit = C::Emitter;
}

/// A zero-width span at the committed frontier — where the *next* token would start.
fn here<'inp, Ctx>(inp: &ExprIn<'inp, '_, Ctx>) -> SimpleSpan
where
  Ctx: ParseContext<'inp, ExprLexer<'inp>>,
{
  let at = *inp.span().end_ref();
  SimpleSpan::new(at, at)
}

/// The left-hand side: an operand, a prefix `-`, or — when neither is there — a recovery.
///
/// The recovery arm is the whole point of the file, and it is a plain `Ok`: the driver is told
/// "here is your operand", the operand happens to be an error node, and nothing about the parse
/// is aborted. Note it returns [`PrattLHS::Operand`], never
/// [`Prefix`](tokora::parser::PrattLHS::Prefix) — the zero-width report is legal precisely
/// because it is an operand.
fn parse_lhs<'inp, Ctx>(inp: &mut ExprIn<'inp, '_, Ctx>) -> Result<PrattLHS<Expr, (), Power>, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  inp.skip_while(|t| t.is_trivia())?;

  match inp.peek_kind()? {
    // A literal — and the one arm where a token the *lexer* accepted can still be
    // unrepresentable. `[0-9]+` bounds the shape of the run, not its magnitude, so the value
    // conversion is fallible on input nothing upstream can reject. The posture leaves exactly
    // one answer available: recover, the same way a missing operand does. A panic here would
    // make "this parser has no failing input" false for an input a user can type.
    //
    // The mark is minted *before* the token is consumed, because on the failing path the
    // digits have to end up inside the error node — they are real text, and the tree owes the
    // source every byte either way.
    Some(Token::Num) => {
      let mark = inp.cst_mark();
      let tok = inp.next()?.expect("peeked");
      let span = tok.span();
      let value = inp.source()[*span.start_ref()..*span.end_ref()].parse::<i64>();

      match value {
        Ok(n) => Ok(PrattLHS::Operand(Expr::Num(n))),
        // A third error-node width, and the only one whose span the *input* chose: the node
        // covers the literal the user actually typed rather than sitting at a point. The
        // diagnostic is spanned the same way, so a caret under it underlines the digits.
        Err(_) => {
          inp.emit_error(Spanned::new(span, Diag::NumberOutOfRange))?;
          inp.cst_start_at(mark, K::ErrorExpr.raw());
          inp.cst_finish(K::ErrorExpr.raw());
          Ok(PrattLHS::Operand(Expr::Error))
        }
      }
    }

    Some(Token::Minus) => {
      inp.next()?;
      Ok(PrattLHS::Prefix(Precedenced::new((), PREC_NEG)))
    }

    // `( expr )`. The group gets its own CST node through a retro-wrap: the mark is minted
    // before the `(` is consumed, so the node covers the whole group.
    Some(Token::LParen) => {
      let mark = inp.cst_mark();
      inp.next()?;
      let inner = parse_expr(inp)?;
      inp.skip_while(|t| t.is_trivia())?;
      if inp
        .try_expect(|t| matches!(t.data().kind(), Token::RParen))?
        .is_none()
      {
        // THE OTHER HALF OF THE POSTURE: recover where the production knows what to do,
        // **propagate** where it does not. An operand parser cannot guess how far an unclosed
        // group was meant to reach, so this one reports and gets out of the way — the `Err`
        // travels up through the driver as `Fault::Keep`, which *commits*: the position stays
        // where this left it and every diagnostic the expression has already made survives,
        // this one included. The nearest enclosing recovery point (`recover_expr`, installed
        // in `program`) then resumes from exactly here.
        inp.emit_error(Spanned::new(here(inp), Diag::ExpectedRParen))?;
        return Err(Diag::ExpectedRParen);
      }
      inp.cst_start_at(mark, K::ParenExpr.raw());
      inp.cst_finish(K::ParenExpr.raw());
      Ok(PrattLHS::Operand(inner))
    }

    // ── The recovery ────────────────────────────────────────────────────────────
    other => {
      inp.emit_error(Spanned::new(here(inp), Diag::ExpectedExpression))?;
      let mark = inp.cst_mark();
      if !in_recovery_set(other) {
        // `err_and_bump`: the token belongs to nobody, so it becomes the error node's body
        // and the parse is guaranteed to have moved.
        inp.next()?;
      }
      inp.cst_start_at(mark, K::ErrorExpr.raw());
      inp.cst_finish(K::ErrorExpr.raw());
      Ok(PrattLHS::Operand(Expr::Error))
    }
  }
}

/// The right-hand side: an infix operator, or the end of this expression.
///
/// Nothing here knows about recovery. It does not have to: a missing operand is the *other*
/// channel's problem, and by the time this runs the fold has already produced a complete node.
fn parse_rhs<'inp, Ctx>(
  inp: &mut ExprIn<'inp, '_, Ctx>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, (), Power>, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => Ok(match tok.into_data() {
      Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Add), PREC_SUM)),
      Token::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Sub), PREC_SUM)),
      Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Mul), PREC_PROD)),
      Token::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Div), PREC_PROD)),
      // Not an operator of this expression. The driver restores the deciding read, so the
      // token is handed back untouched — which is also how the recovery set above gets to
      // *peek* at `)` and leave it alone.
      _ => PrattRHS::End,
    }),
  }
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut ExprIn<'inp, '_, Ctx>,
  operand: Expr,
  _op: Precedenced<(), Power>,
) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  Ok(Expr::Neg(Box::new(operand)))
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut ExprIn<'inp, '_, Ctx>,
  left: Expr,
  right: Expr,
  op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>,
) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  // The node completes whether or not either side is an error node. That is the r-a rule
  // "`m.complete(BIN_EXPR)` runs anyway", and it is why the RHS loop has something to keep
  // folding onto.
  let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = op.into_data();
  Ok(Expr::Bin(op, Box::new(left), Box::new(right)))
}

fn fold_postfix<'inp, Ctx>(
  _inp: &mut ExprIn<'inp, '_, Ctx>,
  operand: Expr,
  _op: Precedenced<(), Power>,
) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  Ok(operand) // this grammar declares no postfix operator
}

/// The CST seam: one node kind per fold shape.
fn expr_kinds(op: PrattFoldOp<'_, (), BinOp, BinOp, BinOp, ()>) -> Option<u16> {
  match op {
    PrattFoldOp::Prefix(_) => Some(K::PrefixExpr.raw()),
    PrattFoldOp::Infix(_) => Some(K::BinExpr.raw()),
    PrattFoldOp::Postfix(_) => None,
  }
}

/// One expression through the typed driver — the bare parser, no recovery point of its own.
///
/// This is what a *nested* expression (the body of a group) runs, and it is deliberately
/// unwrapped: a recovery point inside the group would swallow the unclosed-group error the
/// group's own caller has to see.
fn parse_expr<'inp, Ctx>(inp: &mut ExprIn<'inp, '_, Ctx>) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
    .with_cst_kinds(expr_kinds)
    .parse_input(inp)
}

/// The enclosing recovery point: swallow whatever is left into one error node and hand back a
/// hole.
///
/// [`inplace_recover`](tokora::ParseInput::inplace_recover), not
/// [`recover`](tokora::ParseInput::recover) — the input must stay where the driver handed it
/// back, since that position is what the error is *about*. `recover` would restore to the
/// attempt origin and re-parse the same bytes; this resumes.
///
/// It emits nothing. The diagnostic was already made at the point that knew what was wrong,
/// and it survived the `Err` because a `Fault::Keep` commits.
fn recover_expr<'inp, Ctx>(
  inp: &mut ExprIn<'inp, '_, Ctx>,
  _from: Cursor<'inp, '_, ExprLexer<'inp>>,
  _err: Diag,
) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  swallow_rest(inp)?;
  Ok(Expr::Error)
}

/// Consumes everything remaining and brackets it in one `ErrorExpr` node, so the tree still
/// covers every byte. Zero-width when there is nothing left.
fn swallow_rest<'inp, Ctx>(inp: &mut ExprIn<'inp, '_, Ctx>) -> Result<SimpleSpan, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  let start = *inp.span().end_ref();
  let mark = inp.cst_mark();
  while inp.next()?.is_some() {}
  let end = *inp.span().end_ref();
  inp.cst_start_at(mark, K::ErrorExpr.raw());
  inp.cst_finish(K::ErrorExpr.raw());
  Ok(SimpleSpan::new(start, end))
}

/// Top level: one expression behind a recovery point, then everything left over swallowed into
/// a final error node so the tree still covers every byte.
fn program<'inp, Ctx>(inp: &mut ExprIn<'inp, '_, Ctx>) -> Result<Expr, Diag>
where
  Ctx: RecoveringCtx<'inp>,
{
  inp.skip_while(|t| t.is_trivia())?;
  let expr = pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
    .with_cst_kinds(expr_kinds)
    .inplace_recover(recover_expr)
    .parse_input(inp)?;
  inp.skip_while(|t| t.is_trivia())?;

  if inp.peek_kind()?.is_some() {
    let span = swallow_rest(inp)?;
    inp.emit_error(Spanned::new(span, Diag::TrailingInput))?;
  }
  Ok(expr)
}

// ── Driving it ──────────────────────────────────────────────────────────────────

/// Everything one recovering parse produces.
pub(crate) struct Parsed {
  /// The AST — always present, holes and all.
  pub(crate) ast: Expr,
  /// Every diagnostic, in emission order. A terminal stop, if there was one, is the last entry.
  pub(crate) diagnostics: Vec<(SimpleSpan, Diag)>,
  /// The lossless tree — `tree.text() == src`, always, terminal stops included.
  pub(crate) tree: SyntaxNode<ExprLang>,
  /// `Some` when the parse **ended** instead of recovering.
  ///
  /// The distinction this whole file is about, in one field. A *grammar* error becomes a hole
  /// and the parse continues; a *resource* error — here, only the recursion budget — is
  /// terminal, so the recovery point judging the attempt that tripped re-raises it instead of
  /// spending it, and this grammar catches it nowhere, so there is nothing left to parse. The
  /// caller that wants "is this AST a description of the input, or of how far we got" reads
  /// this rather than scanning the diagnostics.
  pub(crate) terminated: Option<Diag>,
}

/// Parses `src`, recovering from every error it meets — and recording the one it cannot.
///
/// There is no `Result` here, and that is the point: **no input makes this function fail**.
/// That is a weaker claim than "everything recovers", and the difference is
/// [`Parsed::terminated`].
///
/// The recursion budget is tokora's default. [`parse_with_depth`] is the same parse with a budget
/// this example chose.
pub(crate) fn parse(src: &str) -> Parsed {
  parse_in(src, InputContext::new(Verbose::<Diag>::new(), cache()))
}

/// [`parse`] with a **recursion budget of this example's own choosing**.
///
/// The module header used to say a lossless parse "inherits the default and cannot currently
/// override it". That is what [`parse_lossless_with_context`] fixed, and this function is the
/// whole of what using it costs: build an
/// [`InputContext`](tokora::input::InputContext) — the same emitter and cache [`parse`] passes
/// positionally — and hand it a [`RecursionLimiter`].
///
/// **A deeper budget is not free**, which is why the default is low and this is opt-in: one level
/// of nesting is a live native stack frame, so a limit above what the running thread can hold
/// trades a catchable [`Diag::DepthExceeded`] for a process abort. Pick it against the stack the
/// parse will actually run on; see `RecursionLimiter::PARSE_DEFAULT_DEPTH` for the measurements.
pub(crate) fn parse_with_depth(src: &str, depth: usize) -> Parsed {
  parse_in(
    src,
    InputContext::new(Verbose::<Diag>::new(), cache())
      .with_recursion_limiter(RecursionLimiter::with_limitation(depth)),
  )
}

/// The lookahead cache both doors pass, named once so the two cannot differ in anything but the
/// budget.
fn cache<'inp>() -> DefaultCache<'inp, ExprLexer<'inp>> {
  DefaultCache::<ExprLexer<'_>>::default()
}

/// The one body behind [`parse`] and [`parse_with_depth`].
fn parse_in<'inp>(
  src: &'inp str,
  context: InputContext<Verbose<Diag>, DefaultCache<'inp, ExprLexer<'inp>>>,
) -> Parsed {
  let profile = CstProfile::new(
    map_token,
    KindValidator::new(|kind| kind <= K::Root.raw()),
    K::Error.raw(),
    K::Gap.raw(),
  );

  let (cst, parsed) = parse_lossless_with_context(
    src,
    // The initial `L::State`. This lexer declares no logos `extras`, so its state is `()`;
    // spelled literally rather than as `Default::default()`, which clippy reads as passing a
    // unit value to a function.
    (),
    context,
    profile,
    program,
  );

  // WHICH DOOR THE TREE COMES OUT OF depends on whether the parse ran to the end.
  //
  // A recovering parse covers every byte itself — that is what the error nodes and
  // `swallow_rest` are for — so `finish` and its strict gap-coverage guarantee is the right
  // door, and an uncovered gap coming out of *that* door would be a bug in this grammar.
  //
  // A terminal trip is the other case. The parse stops where it stood, the bytes after it are
  // never lexed, and nothing covers them; `finish` refuses with `UncoveredGap` and is correct
  // to. `finish_partial` is the door tokora documents for precisely this — an aborted or
  // truncated parse — and it tiles the un-parsed tail as one `Gap` run. That is what keeps
  // `tree.text() == src` true even for an input that ended the parse, and it is the only
  // reason the round trip survives here at all.
  let (green, emitter) = if parsed.is_ok() {
    cst.finish(K::Root.raw())
  } else {
    cst.finish_partial(K::Root.raw())
  };

  let mut diagnostics: Vec<(SimpleSpan, Diag)> = emitter
    .diagnostics()
    .filter_map(|d| match d.kind() {
      DiagnosticKind::Error(e) => Some((*d.span(), *e)),
      _ => None,
    })
    .collect();

  // THE LIMIT OF THE POSTURE. Everything above recovers because the *grammar* knew what to do
  // with what it found. A **resource** trip is not like that: `RecursionLimitReached` is
  // terminal, and every recovery combinator — `inplace_recover` included — re-raises a terminal
  // error rather than spending it, deliberately, because the whole point of the budget is that
  // the parse must stop. What is scoped to the *attempt* is which failure gets charged: each
  // recovery point compares the session's monotone trip counter against a baseline it took
  // before its own attempt. Nothing here catches the trip, so it travels all the way out — so
  // there is no error node to synthesize and no position to resume from; the parse *ended*.
  //
  // What must not happen is a panic. `parse` used to unwrap this, which made a deeply nested
  // but otherwise valid input — 64 nested groups is enough — take the process down, in a file
  // whose whole claim is that no input fails. The trip is recorded like any other problem and
  // the root becomes a hole.
  let (ast, terminated) = match parsed {
    Ok(ast) => (ast, None),
    Err(fatal) => {
      // Spanned over the whole source on purpose: a budget trip is a property of the parse, not
      // of a byte. Pointing at the token that happened to be current would read as blame.
      diagnostics.push((SimpleSpan::new(0, src.len()), fatal));
      (Expr::Error, Some(fatal))
    }
  };

  Parsed {
    ast,
    diagnostics,
    terminated,
    tree: SyntaxNode::<ExprLang>::new_root(green.expect("well-formed tree")),
  }
}

/// An indented `kind "text"` dump of a tree, the shape the pins compare against.
pub(crate) fn dump(node: &SyntaxNode<ExprLang>) -> String {
  fn go(node: &SyntaxNode<ExprLang>, depth: usize, out: &mut String) {
    use core::fmt::Write as _;
    let _ = writeln!(out, "{:indent$}{:?}", "", node.kind(), indent = depth * 2);
    for child in node.children_with_tokens() {
      match child {
        NodeOrToken::Node(n) => go(&n, depth + 1, out),
        NodeOrToken::Token(t) => {
          let _ = writeln!(
            out,
            "{:indent$}{:?} {:?}",
            "",
            t.kind(),
            t.text(),
            indent = (depth + 1) * 2
          );
        }
      }
    }
  }
  let mut out = String::new();
  go(node, 0, &mut out);
  out
}

// ── Main ────────────────────────────────────────────────────────────────────────

fn main() {
  // One row per recovery shape. Every one of them round-trips, and none of them fails.
  let cases: &[(&str, usize, &str)] = &[
    // clean: prefix binds tighter than `*`
    ("-2 * 3", 0, "((-2) * 3)"),
    // missing RHS at end of input: the node completes anyway
    ("1 + ", 1, "(1 + <error>)"),
    // missing operand before an operator the *enclosing* frame can use — and then the loop
    // continues and folds that operator, which is the second `+` appearing in the output
    ("1 + + 2", 1, "((1 + <error>) + 2)"),
    // the recovery set contains `)`, so the group still closes
    ("(1+)", 1, "(1 + <error>)"),
    // TWO diagnostics in one parse, and the fold depth proves the loop kept going
    ("+ 1 +", 2, "((<error> + 1) + <error>)"),
    // the bump branch: `@` belongs to nobody, so it becomes the error node's body
    ("1 + @", 1, "(1 + <error>)"),
    // a token the lexer accepted and `i64` cannot hold — the only fallible conversion in the
    // grammar, and the third error-node width: as wide as the literal
    ("1 + 9223372036854775808", 1, "(1 + <error>)"),
    // one digit less is an ordinary operand, so the recovery is about the value and not the
    // length
    ("1 + 9223372036854775807", 0, "(1 + 9223372036854775807)"),
    // a complete expression followed by input no production wants
    ("1 2", 1, "1"),
    // the propagating half: an unclosed group is reported, the `Err` travels through the
    // driver keeping both diagnostics, and the enclosing recovery point resumes in place
    ("(1 + ", 2, "<error>"),
    ("(1 + 2 3", 1, "<error>"),
  ];

  for (src, expected_diags, expected_ast) in cases {
    let p = parse(src);
    assert_eq!(
      p.tree.text().to_string(),
      *src,
      "the tree must round-trip `{src}`"
    );
    assert_eq!(
      p.diagnostics.len(),
      *expected_diags,
      "diagnostics for `{src}`"
    );
    assert_eq!(p.ast.to_string(), *expected_ast, "ast for `{src}`");
    println!(
      "{src:10} -> {:22} {} diagnostic(s)",
      p.ast.to_string(),
      p.diagnostics.len()
    );
    for (span, diag) in &p.diagnostics {
      println!("{:12}{}..{}: {diag}", "", span.start_ref(), span.end_ref());
    }
  }

  // The tree an IDE gets for a half-typed line. The error node is a real, zero-width node
  // sitting exactly where the operand was owed.
  let src = "1 + ";
  println!("\n{src:?}\n{}", dump(&parse(src).tree));

  // And the one-token-wide variant, where the bump branch swallowed the offending token.
  let src = "1 + @";
  println!("{src:?}\n{}", dump(&parse(src).tree));

  // The third width: a literal the lexer accepted and `i64` cannot hold. The node is as wide
  // as the offending text, and the digits stay in the tree — the round trip does not care that
  // the value was unrepresentable.
  let src = "1 + 9223372036854775808";
  println!("{src:?}\n{}", dump(&parse(src).tree));

  // A group that closed, so the CST keeps the parens the AST dropped.
  let src = "(1+)";
  println!("{src:?}\n{}", dump(&parse(src).tree));

  // A group that did not: the error travelled out of the driver, and the error node the
  // enclosing recovery point wrote starts at exactly the byte the driver handed back.
  let src = "(1 + 2 3";
  println!("{src:?}\n{}", dump(&parse(src).tree));

  // ── The boundary: the one thing that does not recover ──
  //
  // The parser-facing recursion budget. A trip is terminal — `inplace_recover` re-raises the
  // attempt it stopped rather than spending it, and there is no position at which to synthesize
  // an error node. So this is the one input class the file cannot turn into a hole *in the tree*.
  // What it can do, and does, is not panic.
  //
  // The two depths are read off the constant rather than written out: the budget is a library
  // figure derived from a stack measurement, and a literal here would state a number this file
  // does not own.
  let budget = RecursionLimiter::PARSE_DEFAULT_DEPTH;
  for depth in [budget - 1, budget] {
    let src = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
    let p = parse(&src);
    assert_eq!(
      p.tree.text().to_string(),
      src,
      "even a terminated parse round-trips"
    );
    match p.terminated {
      None => {
        assert_eq!(p.diagnostics.len(), 0);
        println!("\n{depth} nested groups -> parsed, {} diagnostic(s)", 0);
      }
      Some(d) => {
        assert_eq!(p.ast, Expr::Error);
        assert_eq!(p.diagnostics.len(), 1);
        println!(
          "{depth} nested groups -> TERMINATED ({d}), tree still covers {} bytes",
          src.len()
        );
      }
    }
  }

  // ── And the knob, because "terminated" above is a *default*, not a ceiling ──
  //
  // The same input the default refuses, parsed cleanly against a budget this example chose. That
  // is the whole of what `parse_lossless_with_context` buys a lossless consumer: before it, the
  // line above was the end of the story for a grammar whose documents legitimately nest deeper
  // than tokora's conservative default, because the door built its own context.
  //
  // The raised figure is still bounded, and deliberately: `unlimited()` would hand the input back
  // control of the native stack, which is the abort the budget exists to delete.
  let src = format!("{}1{}", "(".repeat(budget), ")".repeat(budget));
  let raised = parse_with_depth(&src, budget * 4);
  assert_eq!(raised.terminated, None, "the raised budget must carry it");
  assert_eq!(raised.diagnostics.len(), 0);
  assert_eq!(raised.tree.text().to_string(), src);
  println!(
    "{budget} nested groups at a budget of {} -> {}",
    budget * 4,
    raised.ast
  );

  println!("\nAll assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
