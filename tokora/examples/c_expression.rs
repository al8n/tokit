//! A C-style expression parser using [`pratt`](tokora::parser::pratt).
//!
//! Demonstrates the high-level `pratt` combinator API where `parse_lhs` and
//! `parse_rhs` are full parser functions with access to [`InputRef`](tokora::InputRef),
//! and fold functions receive typed AST nodes rather than raw tokens.
//!
//! The postfix fold functions use `inp` to consume additional tokens needed for
//! complex postfix forms: `[index]`, `(call args...)`, and `? true : false`.
//!
//! Operator table (ascending precedence):
//!
//! | Operator(s)                  | Kind    | Assoc  | Prec |
//! |------------------------------|---------|--------|------|
//! | `\|\|`                       | infix   | left   | 3    |
//! | `&&`                         | infix   | left   | 4    |
//! | `\|`                         | infix   | left   | 5    |
//! | `^`                          | infix   | left   | 6    |
//! | `&`                          | infix   | left   | 7    |
//! | `==` `!=`                    | infix   | left   | 8    |
//! | `<` `>` `<=` `>=`            | infix   | left   | 9    |
//! | `<<` `>>`                    | infix   | left   | 10   |
//! | `+` `-`                      | infix   | left   | 11   |
//! | `*` `/` `%`                  | infix   | left   | 12   |
//! | `-` `+` `!` `~` `++` `--`   | prefix  | —      | 13   |
//! | `?:` (ternary)               | postfix | —      | 2    |
//! | `++` `--` `[i]` `(args)`     | postfix | left   | 14   |

use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, Token as TokenT,
  error::token::UnexpectedTokenOf,
  logos::{self, Logos},
  parser::{PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced, pratt},
};

// ── Lexer ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

/// Token enum.  Multi-character operators are listed before their single-char
/// prefixes so that Logos' longest-match rule applies correctly.
#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Token {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Num(i64),

  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
  Ident(String),

  // Multi-character operators (must be before single-char variants)
  #[token("++")]
  PlusPlus,
  #[token("--")]
  MinusMinus,
  #[token("==")]
  EqEq,
  #[token("!=")]
  BangEq,
  #[token("<=")]
  LtEq,
  #[token(">=")]
  GtEq,
  #[token("&&")]
  AmpAmp,
  #[token("||")]
  PipePipe,
  #[token("<<")]
  Shl,
  #[token(">>")]
  Shr,

  // Single-character operators
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("%")]
  Percent,
  #[token("&")]
  Amp,
  #[token("|")]
  Pipe,
  #[token("^")]
  Caret,
  #[token("~")]
  Tilde,
  #[token("!")]
  Bang,
  #[token("?")]
  Question,
  #[token(":")]
  Colon,
  #[token("<")]
  Lt,
  #[token(">")]
  Gt,
  #[token(",")]
  Comma,

  // Delimiters
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  #[token("[")]
  LBracket,
  #[token("]")]
  RBracket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Num,
  Ident,
  PlusPlus,
  MinusMinus,
  EqEq,
  BangEq,
  LtEq,
  GtEq,
  AmpAmp,
  PipePipe,
  Shl,
  Shr,
  Plus,
  Minus,
  Star,
  Slash,
  Percent,
  Amp,
  Pipe,
  Caret,
  Tilde,
  Bang,
  Question,
  Colon,
  Lt,
  Gt,
  Comma,
  LParen,
  RParen,
  LBracket,
  RBracket,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let s = match self {
      TokenKind::Num => "number",
      TokenKind::Ident => "identifier",
      TokenKind::PlusPlus => "++",
      TokenKind::MinusMinus => "--",
      TokenKind::EqEq => "==",
      TokenKind::BangEq => "!=",
      TokenKind::LtEq => "<=",
      TokenKind::GtEq => ">=",
      TokenKind::AmpAmp => "&&",
      TokenKind::PipePipe => "||",
      TokenKind::Shl => "<<",
      TokenKind::Shr => ">>",
      TokenKind::Plus => "+",
      TokenKind::Minus => "-",
      TokenKind::Star => "*",
      TokenKind::Slash => "/",
      TokenKind::Percent => "%",
      TokenKind::Amp => "&",
      TokenKind::Pipe => "|",
      TokenKind::Caret => "^",
      TokenKind::Tilde => "~",
      TokenKind::Bang => "!",
      TokenKind::Question => "?",
      TokenKind::Colon => ":",
      TokenKind::Lt => "<",
      TokenKind::Gt => ">",
      TokenKind::Comma => ",",
      TokenKind::LParen => "(",
      TokenKind::RParen => ")",
      TokenKind::LBracket => "[",
      TokenKind::RBracket => "]",
    };
    write!(f, "{s}")
  }
}

impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Num(_) => TokenKind::Num,
      Token::Ident(_) => TokenKind::Ident,
      Token::PlusPlus => TokenKind::PlusPlus,
      Token::MinusMinus => TokenKind::MinusMinus,
      Token::EqEq => TokenKind::EqEq,
      Token::BangEq => TokenKind::BangEq,
      Token::LtEq => TokenKind::LtEq,
      Token::GtEq => TokenKind::GtEq,
      Token::AmpAmp => TokenKind::AmpAmp,
      Token::PipePipe => TokenKind::PipePipe,
      Token::Shl => TokenKind::Shl,
      Token::Shr => TokenKind::Shr,
      Token::Plus => TokenKind::Plus,
      Token::Minus => TokenKind::Minus,
      Token::Star => TokenKind::Star,
      Token::Slash => TokenKind::Slash,
      Token::Percent => TokenKind::Percent,
      Token::Amp => TokenKind::Amp,
      Token::Pipe => TokenKind::Pipe,
      Token::Caret => TokenKind::Caret,
      Token::Tilde => TokenKind::Tilde,
      Token::Bang => TokenKind::Bang,
      Token::Question => TokenKind::Question,
      Token::Colon => TokenKind::Colon,
      Token::Lt => TokenKind::Lt,
      Token::Gt => TokenKind::Gt,
      Token::Comma => TokenKind::Comma,
      Token::LParen => TokenKind::LParen,
      Token::RParen => TokenKind::RParen,
      Token::LBracket => TokenKind::LBracket,
      Token::RBracket => TokenKind::RBracket,
    }
  }
}

impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = LexError;

  fn kind(&self) -> TokenKind {
    TokenKind::from(self)
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CExprError {
  Lex(LexError),
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for CExprError {
  fn from(e: LexError) -> Self {
    CExprError::Lex(e)
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError {
  fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self {
    CExprError::UnexpectedToken
  }
}

// The token-level bundle the entry points require: one `Set`-generic end-of-input impl covers
// both the default expected-set spelling and the `Kind`-table one the committed dispatch
// drivers raise, and one generic `FromUnclosed` covers every delimiter pair.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for CExprError
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    CExprError::UnexpectedEot
  }
}

// The typed pratt driver's stalled-report exits: a report the driver acted on that consumed
// nothing is a grammar bug, surfaced as the terminal end-of-expression error for the channel
// that broke the contract — the LHS one for a prefix report, the RHS one for an infix/postfix
// report — rather than ending the expression with a silent `Ok`.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>>
  for CExprError
{
  fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self {
    CExprError::UnexpectedEot
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>>
  for CExprError
{
  fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self {
    CExprError::UnexpectedEot
  }
}

impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CExprError {
  fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self {
    CExprError::UnexpectedEot
  }
}

impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CExprError {
  fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self {
    CExprError::UnexpectedEot
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CExprError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    CExprError::UnexpectedEot
  }
}

// ── Binding powers ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Power(i32);

impl PrattPower for Power {}

const PREC_TERNARY: Power = Power(2); // ?: postfix (low precedence)
const PREC_OR: Power = Power(3); // ||
const PREC_AND: Power = Power(4); // &&
const PREC_BOR: Power = Power(5); // |
const PREC_BXOR: Power = Power(6); // ^
const PREC_BAND: Power = Power(7); // &
const PREC_EQ: Power = Power(8); // == !=
const PREC_CMP: Power = Power(9); // < > <= >=
const PREC_SHIFT: Power = Power(10); // << >>
const PREC_ADD: Power = Power(11); // + -
const PREC_MUL: Power = Power(12); // * / %
const PREC_PREFIX: Power = Power(13); // unary prefix operators
const PREC_POSTFIX: Power = Power(14); // postfix ++/--, [], ()

#[derive(Debug, Clone, Copy)]
enum UnaryOp {
  Neg,
  Pos,
  Not,
  BNot,
  PreInc,
  PreDec,
}

#[derive(Debug, Clone, Copy)]
enum BinOp {
  Add,
  Sub,
  Mul,
  Div,
  Mod,
  Or,
  And,
  BOr,
  BXor,
  BAnd,
  Eq,
  Neq,
  Lt,
  Gt,
  Lte,
  Gte,
  Shl,
  Shr,
}

/// Tag passed from `parse_rhs` to `fold_postfix`.
#[derive(Debug, Clone, Copy)]
enum PostfixOp {
  Inc,     // e++
  Dec,     // e--
  Index,   // e[i]  — fold_postfix will consume the index expr and `]`
  Call,    // e(…)  — fold_postfix will consume args and `)`
  Ternary, // e ? t : f — fold_postfix will consume `t`, `:`, `f`
}

#[derive(Debug, Clone)]
enum Expr {
  Num(i64),
  Var(String),
  Prefix {
    op: UnaryOp,
    operand: Box<Expr>,
  },
  Binary {
    op: BinOp,
    left: Box<Expr>,
    right: Box<Expr>,
  },
  PostfixInc(Box<Expr>),
  PostfixDec(Box<Expr>),
  Index {
    base: Box<Expr>,
    index: Box<Expr>,
  },
  Call {
    func: Box<Expr>,
    args: Vec<Expr>,
  },
  Ternary {
    cond: Box<Expr>,
    then: Box<Expr>,
    otherwise: Box<Expr>,
  },
}

impl core::fmt::Display for UnaryOp {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      UnaryOp::Neg => write!(f, "-"),
      UnaryOp::Pos => write!(f, "+"),
      UnaryOp::Not => write!(f, "!"),
      UnaryOp::BNot => write!(f, "~"),
      UnaryOp::PreInc => write!(f, "++"),
      UnaryOp::PreDec => write!(f, "--"),
    }
  }
}

impl core::fmt::Display for BinOp {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    let s = match self {
      BinOp::Add => "+",
      BinOp::Sub => "-",
      BinOp::Mul => "*",
      BinOp::Div => "/",
      BinOp::Mod => "%",
      BinOp::Or => "||",
      BinOp::And => "&&",
      BinOp::BOr => "|",
      BinOp::BXor => "^",
      BinOp::BAnd => "&",
      BinOp::Eq => "==",
      BinOp::Neq => "!=",
      BinOp::Lt => "<",
      BinOp::Gt => ">",
      BinOp::Lte => "<=",
      BinOp::Gte => ">=",
      BinOp::Shl => "<<",
      BinOp::Shr => ">>",
    };
    write!(f, "{s}")
  }
}

impl core::fmt::Display for Expr {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Expr::Num(n) => write!(f, "{n}"),
      Expr::Var(s) => write!(f, "{s}"),
      Expr::Prefix { op, operand } => write!(f, "({op}{operand})"),
      Expr::Binary { op, left, right } => write!(f, "({left} {op} {right})"),
      Expr::PostfixInc(e) => write!(f, "({e}++)"),
      Expr::PostfixDec(e) => write!(f, "({e}--)"),
      Expr::Index { base, index } => write!(f, "({base}[{index}])"),
      Expr::Ternary {
        cond,
        then,
        otherwise,
      } => write!(f, "({cond} ? {then} : {otherwise})"),
      Expr::Call { func, args } => {
        write!(f, "{func}(")?;
        for (i, a) in args.iter().enumerate() {
          if i > 0 {
            write!(f, ", ")?;
          }
          write!(f, "{a}")?;
        }
        write!(f, ")")
      }
    }
  }
}

// ── Pratt parse functions ─────────────────────────────────────────────────────
//
// Named functions (not closures) are required here for the same reason as in
// calculator.rs: they satisfy the higher-rank lifetime bound automatically,
// whereas closures cannot.

fn parse_lhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
) -> Result<PrattLHS<Box<Expr>, UnaryOp, Power>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  match inp.next()? {
    None => Err(CExprError::UnexpectedEot),
    Some(tok) => match tok.into_data() {
      // Operands
      Token::Num(n) => Ok(PrattLHS::Operand(Box::new(Expr::Num(n)))),
      Token::Ident(s) => Ok(PrattLHS::Operand(Box::new(Expr::Var(s)))),

      // Grouping: ( expr ) — calls the full pratt parser recursively
      Token::LParen => {
        let e = parse_cexpr(inp)?;
        if inp
          .try_expect(|t| matches!(t.data, Token::RParen))?
          .is_none()
        {
          return Err(CExprError::UnexpectedToken);
        }
        Ok(PrattLHS::Operand(e))
      }

      // Prefix operators
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::Neg,
        PREC_PREFIX,
      ))),
      Token::Plus => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::Pos,
        PREC_PREFIX,
      ))),
      Token::Bang => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::Not,
        PREC_PREFIX,
      ))),
      Token::Tilde => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::BNot,
        PREC_PREFIX,
      ))),
      Token::PlusPlus => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::PreInc,
        PREC_PREFIX,
      ))),
      Token::MinusMinus => Ok(PrattLHS::Prefix(Precedenced::new(
        UnaryOp::PreDec,
        PREC_PREFIX,
      ))),

      _ => Err(CExprError::UnexpectedToken),
    },
  }
}

fn parse_rhs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
) -> Result<PrattRHS<BinOp, BinOp, BinOp, PostfixOp, Power>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  // Helper: infix left-associative
  macro_rules! infix_l {
    ($op:expr, $prec:expr) => {
      Ok(PrattRHS::Infix(Precedenced::new(
        PrattInfix::Left($op),
        $prec,
      )))
    };
  }
  // Helper: postfix
  macro_rules! postfix {
    ($op:expr, $prec:expr) => {
      Ok(PrattRHS::Postfix(Precedenced::new($op, $prec)))
    };
  }

  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => match tok.into_data() {
      // Infix operators (left-associative)
      Token::PipePipe => infix_l!(BinOp::Or, PREC_OR),
      Token::AmpAmp => infix_l!(BinOp::And, PREC_AND),
      Token::Pipe => infix_l!(BinOp::BOr, PREC_BOR),
      Token::Caret => infix_l!(BinOp::BXor, PREC_BXOR),
      Token::Amp => infix_l!(BinOp::BAnd, PREC_BAND),
      Token::EqEq => infix_l!(BinOp::Eq, PREC_EQ),
      Token::BangEq => infix_l!(BinOp::Neq, PREC_EQ),
      Token::Lt => infix_l!(BinOp::Lt, PREC_CMP),
      Token::Gt => infix_l!(BinOp::Gt, PREC_CMP),
      Token::LtEq => infix_l!(BinOp::Lte, PREC_CMP),
      Token::GtEq => infix_l!(BinOp::Gte, PREC_CMP),
      Token::Shl => infix_l!(BinOp::Shl, PREC_SHIFT),
      Token::Shr => infix_l!(BinOp::Shr, PREC_SHIFT),
      Token::Plus => infix_l!(BinOp::Add, PREC_ADD),
      Token::Minus => infix_l!(BinOp::Sub, PREC_ADD),
      Token::Star => infix_l!(BinOp::Mul, PREC_MUL),
      Token::Slash => infix_l!(BinOp::Div, PREC_MUL),
      Token::Percent => infix_l!(BinOp::Mod, PREC_MUL),

      // Postfix operators
      Token::PlusPlus => postfix!(PostfixOp::Inc, PREC_POSTFIX),
      Token::MinusMinus => postfix!(PostfixOp::Dec, PREC_POSTFIX),
      Token::LBracket => postfix!(PostfixOp::Index, PREC_POSTFIX),
      Token::LParen => postfix!(PostfixOp::Call, PREC_POSTFIX),
      Token::Question => postfix!(PostfixOp::Ternary, PREC_TERNARY),

      // Anything else is not an operator of this expression. The Pratt loop restores
      // whatever this call consumed, putting the token back into the stream.
      _ => Ok(PrattRHS::End),
    },
  }
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
  operand: Box<Expr>,
  op: Precedenced<UnaryOp, Power>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  Ok(Box::new(Expr::Prefix {
    op: op.into_data(),
    operand,
  }))
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
  left: Box<Expr>,
  right: Box<Expr>,
  op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  let bin_op = match op.into_data() {
    PrattInfix::Left(o) | PrattInfix::Right(o) | PrattInfix::Neither(o) => o,
  };
  Ok(Box::new(Expr::Binary {
    op: bin_op,
    left,
    right,
  }))
}

/// Fold postfix operators.
///
/// For complex forms (`[index]`, `(args)`, `? t : f`) this function uses `inp`
/// to consume the additional tokens that follow the trigger token already
/// consumed by `parse_rhs`.
fn fold_postfix<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
  operand: Box<Expr>,
  op: Precedenced<PostfixOp, Power>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  match op.into_data() {
    PostfixOp::Inc => Ok(Box::new(Expr::PostfixInc(operand))),
    PostfixOp::Dec => Ok(Box::new(Expr::PostfixDec(operand))),

    // e[index_expr]
    PostfixOp::Index => {
      let index = parse_cexpr(inp)?; // stops before `]`
      if inp
        .try_expect(|t| matches!(t.data, Token::RBracket))?
        .is_none()
      {
        return Err(CExprError::UnexpectedToken);
      }
      Ok(Box::new(Expr::Index {
        base: operand,
        index,
      }))
    }

    // e(arg, arg, ...)
    PostfixOp::Call => {
      let mut args: Vec<_> = Vec::new();
      // Empty call: f()
      if inp
        .try_expect(|t| matches!(t.data, Token::RParen))?
        .is_some()
      {
        return Ok(Box::new(Expr::Call {
          func: operand,
          args,
        }));
      }
      // First arg
      args.push(*parse_cexpr(inp)?);
      // Subsequent args, comma-separated
      loop {
        if inp
          .try_expect(|t| matches!(t.data, Token::RParen))?
          .is_some()
        {
          break;
        }
        if inp
          .try_expect(|t| matches!(t.data, Token::Comma))?
          .is_none()
        {
          return Err(CExprError::UnexpectedToken);
        }
        args.push(*parse_cexpr(inp)?);
      }
      Ok(Box::new(Expr::Call {
        func: operand,
        args,
      }))
    }

    // cond ? then_expr : else_expr
    PostfixOp::Ternary => {
      let then = parse_cexpr(inp)?; // stops before `:`
      if inp
        .try_expect(|t| matches!(t.data, Token::Colon))?
        .is_none()
      {
        return Err(CExprError::UnexpectedToken);
      }
      let otherwise = parse_cexpr(inp)?;
      Ok(Box::new(Expr::Ternary {
        cond: operand,
        then,
        otherwise,
      }))
    }
  }
}

/// Parses a complete C-style expression using `pratt`.
///
/// Mutual recursion with `parse_lhs` (for grouping) and `fold_postfix`
/// (for index, call, ternary) is achieved through named functions — there
/// are no recursive types involved, only recursive call-stack frames.
fn parse_cexpr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
}

fn main() {
  let cases: &[(&str, &str)] = &[
    // Precedence: * before +
    ("1 + 2 * 3", "(1 + (2 * 3))"),
    // Grouping overrides precedence
    ("(1 + 2) * 3", "((1 + 2) * 3)"),
    // Left-associativity
    ("a + b + c", "((a + b) + c)"),
    // Unary operators
    ("-a", "(-a)"),
    ("!flag", "(!flag)"),
    ("~bits", "(~bits)"),
    // Prefix / postfix increment
    ("++x", "(++x)"),
    ("x++", "(x++)"),
    // Ternary
    ("a ? b : c", "(a ? b : c)"),
    // Array index
    ("arr[0]", "(arr[0])"),
    // Function calls
    ("f()", "f()"),
    ("f(1, 2)", "f(1, 2)"),
    // Mixed precedence chains
    ("a == b && c != d", "((a == b) && (c != d))"),
    ("~bits | flags", "((~bits) | flags)"),
    // Chained calls and index
    ("arr[i + 1]", "(arr[(i + 1)])"),
    ("f(a + b, c * d)", "f((a + b), (c * d))"),
    // Shift and bitwise
    ("x << 2 | y >> 1", "((x << 2) | (y >> 1))"),
  ];

  for (src, expected) in cases {
    let expr: Box<Expr> = Parser::new().apply(parse_cexpr).parse_str(src).unwrap();
    println!("{src:35}  ==>  {expr}");
    assert_eq!(expr.to_string(), *expected, "mismatch for `{src}`");
  }

  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
