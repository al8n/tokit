# 15. Walkthrough: C expressions

Prerequisites: chapters 5 and 11; chapter 12 is helpful.

This walkthrough builds the maintained
[`c_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/c_expression.rs)
end to end, inline. Where [chapter 12](super::ch12_calculator_example) folded tokens straight to an
`f64` with the *token-level* Pratt engine, this is the **AST-level** engine
([`pratt`](crate::parser::pratt)): `parse_lhs` and `parse_rhs` are full parser *functions*
with the [`InputRef`](crate::InputRef) in hand, and the folds build a typed `Expr` tree over your
own node type. Three things here are unique to this chapter, worth watching for as they appear
below:

- **The classifiers are parsers, not a token trait.** `parse_lhs`/`parse_rhs` return
  [`PrattLHS`](crate::parser::PrattLHS)/[`PrattRHS`](crate::parser::PrattRHS) values; a
  non-operator is [`PrattRHS::End`](crate::parser::PrattRHS), not a `None`.
- **Postfix operators that consume more input.** `fold_postfix` receives the `InputRef` *first*, so
  `[i]`, `(args...)`, and `? t : f` read the tokens they need — the token-level postfix fold could
  not.
- **C's deep precedence ladder**, with the ternary as a *low-precedence* postfix.

Every part — lexer, AST types, precedence ladder, the two classifiers, the three folds, and the
one-call entry point — is shown as a compiling doctest, so you can follow the whole parser without
leaving the page.

| Maintained program | Symbols to follow |
| --- | --- |
| [`c_expression.rs`](https://github.com/al8n/tokora/blob/main/tokora/examples/c_expression.rs) | `parse_lhs`, `parse_rhs`, `fold_prefix`, `fold_infix`, `fold_postfix`, `parse_cexpr` |

This chapter's public surface is [`parser::pratt`](crate::parser::pratt),
[`parser::PrattLHS`](crate::parser::PrattLHS), [`parser::PrattRHS`](crate::parser::PrattRHS),
[`parser::PrattInfix`](crate::parser::PrattInfix),
[`parser::Precedenced`](crate::parser::Precedenced),
[`parser::PrattPower`](crate::parser::PrattPower), [`InputRef::next`](crate::InputRef::next),
[`InputRef::try_expect`](crate::InputRef::try_expect),
[`ParseInput::parse_input`](crate::ParseInput::parse_input), and
[`Parse::parse_str`](crate::Parse::parse_str). The [Pratt reference](super::ref_pratt) catalogs the
whole surface, token-level and AST-level, side by side.

## Define the lexer, token kinds, and `CExprError`

The Logos lexer carries payloads in `Token::Num(i64)` and `Token::Ident(String)`; the remaining
variants are punctuation. Multi-character operators (`==`, `<<`, `++`, …) are listed *before* their
single-character prefixes so Logos' longest-match rule tokenizes `==` as one token, not two. As in
the calculator, classification lives on a separate fieldless `TokenKind` (so it can be
`Copy + Eq + Hash`, as [`Token`](crate::Token) requires of its `Kind`), and `Display` on the kind
doubles as the diagnostic name.

The error family is *simpler* than the calculator's: AST-level Pratt does not route
expression-end errors through a `PrattEmitter` (`parse_lhs` reports "ran out of operand" itself, as
you will see) — an ordinary `From` impl is enough, where the token-level engine needs the extra
capability trait. That does not make `From<UnexpectedEoLhs>` and `From<UnexpectedEoRhs>` optional,
though: the Pratt engine raises them itself, marked terminal, when `parse_lhs`/`parse_rhs` breaks its
own contract — reports an operator and consumes nothing, a grammar bug rather than the ordinary end
of input `parse_lhs` already reports as `UnexpectedEot`. `CExprError` carries both conversions below,
alongside the lexical error and the unexpected token.

```rust
use tokora::{
  Token as TokenT,
  error::{
    NonAssociativeChain, RecursionLimitReached, UnexpectedEoLhs, UnexpectedEoRhs,
    token::UnexpectedTokenOf,
  },
  logos::{self, Logos},
};

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;
impl From<()> for LexError { fn from(_: ()) -> Self { Self } }

#[derive(Clone, Debug, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Token {
  #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Num(i64),
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())]
  Ident(String),
  // Multi-character operators — before the single-char variants (longest match wins).
  #[token("++")] PlusPlus,
  #[token("--")] MinusMinus,
  #[token("==")] EqEq,
  #[token("!=")] BangEq,
  #[token("<=")] LtEq,
  #[token(">=")] GtEq,
  #[token("&&")] AmpAmp,
  #[token("||")] PipePipe,
  #[token("<<")] Shl,
  #[token(">>")] Shr,
  // Single-character operators.
  #[token("+")] Plus,
  #[token("-")] Minus,
  #[token("*")] Star,
  #[token("/")] Slash,
  #[token("%")] Percent,
  #[token("&")] Amp,
  #[token("|")] Pipe,
  #[token("^")] Caret,
  #[token("~")] Tilde,
  #[token("!")] Bang,
  #[token("?")] Question,
  #[token(":")] Colon,
  #[token("<")] Lt,
  #[token(">")] Gt,
  #[token(",")] Comma,
  // Delimiters.
  #[token("(")] LParen,
  #[token(")")] RParen,
  #[token("[")] LBracket,
  #[token("]")] RBracket,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum TokenKind {
  Num, Ident, PlusPlus, MinusMinus, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Shl, Shr, Plus,
  Minus, Star, Slash, Percent, Amp, Pipe, Caret, Tilde, Bang, Question, Colon, Lt, Gt, Comma,
  LParen, RParen, LBracket, RBracket,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Self::Num => "number", Self::Ident => "identifier", Self::PlusPlus => "++",
      Self::MinusMinus => "--", Self::EqEq => "==", Self::BangEq => "!=", Self::LtEq => "<=",
      Self::GtEq => ">=", Self::AmpAmp => "&&", Self::PipePipe => "||", Self::Shl => "<<",
      Self::Shr => ">>", Self::Plus => "+", Self::Minus => "-", Self::Star => "*",
      Self::Slash => "/", Self::Percent => "%", Self::Amp => "&", Self::Pipe => "|",
      Self::Caret => "^", Self::Tilde => "~", Self::Bang => "!", Self::Question => "?",
      Self::Colon => ":", Self::Lt => "<", Self::Gt => ">", Self::Comma => ",",
      Self::LParen => "(", Self::RParen => ")", Self::LBracket => "[", Self::RBracket => "]",
    })
  }
}
impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    core::fmt::Display::fmt(&self.kind(), f)
  }
}

impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Num(_) => Self::Num, Token::Ident(_) => Self::Ident, Token::PlusPlus => Self::PlusPlus,
      Token::MinusMinus => Self::MinusMinus, Token::EqEq => Self::EqEq, Token::BangEq => Self::BangEq,
      Token::LtEq => Self::LtEq, Token::GtEq => Self::GtEq, Token::AmpAmp => Self::AmpAmp,
      Token::PipePipe => Self::PipePipe, Token::Shl => Self::Shl, Token::Shr => Self::Shr,
      Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star,
      Token::Slash => Self::Slash, Token::Percent => Self::Percent, Token::Amp => Self::Amp,
      Token::Pipe => Self::Pipe, Token::Caret => Self::Caret, Token::Tilde => Self::Tilde,
      Token::Bang => Self::Bang, Token::Question => Self::Question, Token::Colon => Self::Colon,
      Token::Lt => Self::Lt, Token::Gt => Self::Gt, Token::Comma => Self::Comma,
      Token::LParen => Self::LParen, Token::RParen => Self::RParen, Token::LBracket => Self::LBracket,
      Token::RBracket => Self::RBracket,
    }
  }
}

impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = LexError;
  fn kind(&self) -> TokenKind { TokenKind::from(self) }
  fn is_trivia(&self) -> bool { false }
}

type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

#[derive(Debug)]
enum CExprError { Lex(LexError), UnexpectedToken, UnexpectedEot }

impl From<LexError> for CExprError { fn from(e: LexError) -> Self { Self::Lex(e) } }
impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError {
  fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self { Self::UnexpectedToken }
}
// The Pratt engine's own terminal exits — required by `Pratt`'s `ParseInput` impl regardless of
// whether this grammar ever trips them. Both name a contract violation in `parse_lhs`/`parse_rhs`
// (an operator reported but not consumed), never ordinary operand exhaustion, so both fold into
// the same `UnexpectedEot` this parser already reports for that.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoLhs<O, Lang, Set>> for CExprError {
  fn from(_: UnexpectedEoLhs<O, Lang, Set>) -> Self { Self::UnexpectedEot }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEoRhs<O, Lang, Set>> for CExprError {
  fn from(_: UnexpectedEoRhs<O, Lang, Set>) -> Self { Self::UnexpectedEot }
}
// The two the engines RETURN rather than emit: a recursion-limit trip at a frame prologue, and a
// second same-power non-associative operator. No `emit_*` hook sees either, so neither is part of
// the `FromPrattError` bundle — the pratt entry points name these two `From`s themselves.
impl<O, Lang: ?Sized> From<RecursionLimitReached<O, Lang>> for CExprError {
  fn from(_: RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot }
}
impl<O, Lang: ?Sized> From<NonAssociativeChain<O, Lang>> for CExprError {
  fn from(_: NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot }
}

assert_eq!(Token::Star.kind(), TokenKind::Star);
assert_eq!(Token::PlusPlus.kind(), TokenKind::PlusPlus);
assert_eq!(Token::Star.to_string(), "*");
```

## Define `UnaryOp`, `BinOp`, `PostfixOp`, and `Expr`

The AST has typed variants for prefix, binary, postfix increment/decrement, index, call, and
ternary expressions. Separating operator tags from tree nodes lets the folds stay small and keeps
the `Display` implementation useful as an assertion oracle: every node prints fully parenthesised,
so `to_string()` is a compact, exact check on the tree's *shape*. `PostfixOp` is the tag
`parse_rhs` hands to `fold_postfix`; its `Index`/`Call`/`Ternary` variants are instructions to the
fold to consume more input. There is no "not an operator" tag: that answer is `PrattRHS::End`.

```rust
#[derive(Clone, Copy, Debug)]
enum UnaryOp { Neg, Pos, Not, BNot, PreInc, PreDec }

#[derive(Clone, Copy, Debug)]
enum BinOp {
  Add, Sub, Mul, Div, Mod, Or, And, BOr, BXor, BAnd, Eq, Neq, Lt, Gt, Lte, Gte, Shl, Shr,
}

// The tag `parse_rhs` passes to `fold_postfix`. `Index`/`Call`/`Ternary` tell the fold to consume
// more input. Every variant here is a real operator.
#[derive(Clone, Copy, Debug)]
enum PostfixOp { Inc, Dec, Index, Call, Ternary }

#[derive(Clone, Debug)]
enum Expr {
  Num(i64),
  Var(String),
  Prefix { op: UnaryOp, operand: Box<Expr> },
  Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> },
  PostfixInc(Box<Expr>),
  PostfixDec(Box<Expr>),
  Index { base: Box<Expr>, index: Box<Expr> },
  Call { func: Box<Expr>, args: Vec<Expr> },
  Ternary { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> },
}

impl core::fmt::Display for UnaryOp {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      UnaryOp::Neg => "-", UnaryOp::Pos => "+", UnaryOp::Not => "!", UnaryOp::BNot => "~",
      UnaryOp::PreInc => "++", UnaryOp::PreDec => "--",
    })
  }
}
impl core::fmt::Display for BinOp {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%",
      BinOp::Or => "||", BinOp::And => "&&", BinOp::BOr => "|", BinOp::BXor => "^", BinOp::BAnd => "&",
      BinOp::Eq => "==", BinOp::Neq => "!=", BinOp::Lt => "<", BinOp::Gt => ">", BinOp::Lte => "<=",
      BinOp::Gte => ">=", BinOp::Shl => "<<", BinOp::Shr => ">>",
    })
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
      Expr::Ternary { cond, then, otherwise } => write!(f, "({cond} ? {then} : {otherwise})"),
      Expr::Call { func, args } => {
        write!(f, "{func}(")?;
        for (i, a) in args.iter().enumerate() {
          if i > 0 { write!(f, ", ")?; }
          write!(f, "{a}")?;
        }
        write!(f, ")")
      }
    }
  }
}

// The Display impl is the assertion oracle used throughout this chapter.
let two_times_three = Expr::Binary {
  op: BinOp::Mul,
  left: Box::new(Expr::Num(2)),
  right: Box::new(Expr::Num(3)),
};
assert_eq!(two_times_three.to_string(), "(2 * 3)");
assert_eq!(
  Expr::Prefix { op: UnaryOp::Neg, operand: Box::new(Expr::Var("a".to_string())) }.to_string(),
  "(-a)",
);
assert_eq!(
  Expr::Call {
    func: Box::new(Expr::Var("f".to_string())),
    args: vec![Expr::Num(1), Expr::Num(2)],
  }
  .to_string(),
  "f(1, 2)",
);
```

## Define the precedence ladder

The ladder runs from the ternary through the logical and bitwise operators, comparison, shifts,
arithmetic, prefix, and the high-power postfix forms. The precise numeric values matter only
relative to one another; named constants make that relationship auditable. One row carries
C-specific character: the **ternary is a postfix operator, but a very low-precedence one**
(`a || b ? c : d` parses as `(a || b) ? c : d`). Every constant here is a real operator — "not an
operator" is `PrattRHS::End`, which is not a power and needs no room below the floor.

```rust
use tokora::parser::PrattPower;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Power(i32);
// Nothing to implement: a binding power is only ever compared, never stepped.
impl PrattPower for Power {}

const PREC_TERNARY: Power = Power(2);  // ?:        (postfix, low precedence)
const PREC_OR: Power = Power(3);       // ||
const PREC_AND: Power = Power(4);      // &&
const PREC_BOR: Power = Power(5);      // |
const PREC_BXOR: Power = Power(6);     // ^
const PREC_BAND: Power = Power(7);     // &
const PREC_EQ: Power = Power(8);       // == !=
const PREC_CMP: Power = Power(9);      // < > <= >=
const PREC_SHIFT: Power = Power(10);   // << >>
const PREC_ADD: Power = Power(11);     // + -
const PREC_MUL: Power = Power(12);     // * / %
const PREC_PREFIX: Power = Power(13);  // unary - + ! ~ ++ --
const PREC_POSTFIX: Power = Power(14); // ++ -- [] ()  (postfix)

// Every operator outranks the default floor a top-level parse starts at.
assert!(PREC_TERNARY > Power::default());
// The whole ladder is one strictly-increasing chain; only the relative order matters.
assert!(
  PREC_TERNARY < PREC_OR && PREC_OR < PREC_AND && PREC_AND < PREC_BOR && PREC_BOR < PREC_BXOR
    && PREC_BXOR < PREC_BAND && PREC_BAND < PREC_EQ && PREC_EQ < PREC_CMP && PREC_CMP < PREC_SHIFT
    && PREC_SHIFT < PREC_ADD && PREC_ADD < PREC_MUL && PREC_MUL < PREC_PREFIX
    && PREC_PREFIX < PREC_POSTFIX
);
// Left-associativity means the engine recurses admitting only powers strictly above the
// operator's own, so `*` binds inside a `+`'s right operand and a second `+` does not.
assert!(PREC_MUL > PREC_ADD);
```

## Implement `parse_lhs` and `parse_rhs`

`parse_lhs` reads the left edge of a (sub-)expression: an operand (number or identifier), a
parenthesised group, or a prefix operator (`-`, `+`, `!`, `~`, `++`, `--`). Unlike the calculator's
[`PrattToken::try_pratt_lhs`](crate::token::PrattToken) — a pure classifier *on the token* — this
is a full parser function with the `InputRef` in hand, which is exactly what lets the `(` arm
recurse into `parse_cexpr` and then consume its own `)` with [`try_expect`](crate::InputRef::try_expect).
Running out of input here is *this function's* error to report (`CExprError::UnexpectedEot`); the
AST engine does not synthesize one.

`parse_rhs` classifies what follows an operand: an infix operator (mapped to a left-associative
[`PrattInfix`](crate::parser::PrattInfix)), a postfix trigger (`++`, `--`, `[`, `(`, `?`), or — for
anything else — [`PrattRHS::End`](crate::parser::PrattRHS). On `End` the Pratt engine restores
whatever `parse_rhs` consumed, leaving that token on the input for the surrounding grammar (a `)`,
`]`, `:`, or `,` closing an enclosing form) instead of losing it. This is the AST-level counterpart
of a token-level `try_pratt_rhs` returning `None`: `parse_rhs` must always return a *value*, and
`End` is the value that means "the expression stops here".

```rust
# use tokora::{Token as TokenT, ParseInput, error::token::UnexpectedTokenOf, logos::{self, Logos}, parser::{PrattPower, pratt}};
# #[derive(Clone, Debug, Default, PartialEq)] struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))] Num(i64),
#   #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())] Ident(String),
#   #[token("++")] PlusPlus, #[token("--")] MinusMinus, #[token("==")] EqEq, #[token("!=")] BangEq,
#   #[token("<=")] LtEq, #[token(">=")] GtEq, #[token("&&")] AmpAmp, #[token("||")] PipePipe,
#   #[token("<<")] Shl, #[token(">>")] Shr, #[token("+")] Plus, #[token("-")] Minus,
#   #[token("*")] Star, #[token("/")] Slash, #[token("%")] Percent, #[token("&")] Amp,
#   #[token("|")] Pipe, #[token("^")] Caret, #[token("~")] Tilde, #[token("!")] Bang,
#   #[token("?")] Question, #[token(":")] Colon, #[token("<")] Lt, #[token(">")] Gt,
#   #[token(",")] Comma, #[token("(")] LParen, #[token(")")] RParen, #[token("[")] LBracket,
#   #[token("]")] RBracket,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Ident, PlusPlus, MinusMinus, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Shl, Shr, Plus, Minus, Star, Slash, Percent, Amp, Pipe, Caret, Tilde, Bang, Question, Colon, Lt, Gt, Comma, LParen, RParen, LBracket, RBracket }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Ident => "identifier", Self::PlusPlus => "++", Self::MinusMinus => "--", Self::EqEq => "==", Self::BangEq => "!=", Self::LtEq => "<=", Self::GtEq => ">=", Self::AmpAmp => "&&", Self::PipePipe => "||", Self::Shl => "<<", Self::Shr => ">>", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Percent => "%", Self::Amp => "&", Self::Pipe => "|", Self::Caret => "^", Self::Tilde => "~", Self::Bang => "!", Self::Question => "?", Self::Colon => ":", Self::Lt => "<", Self::Gt => ">", Self::Comma => ",", Self::LParen => "(", Self::RParen => ")", Self::LBracket => "[", Self::RBracket => "]" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Ident(_) => Self::Ident, Token::PlusPlus => Self::PlusPlus, Token::MinusMinus => Self::MinusMinus, Token::EqEq => Self::EqEq, Token::BangEq => Self::BangEq, Token::LtEq => Self::LtEq, Token::GtEq => Self::GtEq, Token::AmpAmp => Self::AmpAmp, Token::PipePipe => Self::PipePipe, Token::Shl => Self::Shl, Token::Shr => Self::Shr, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Percent => Self::Percent, Token::Amp => Self::Amp, Token::Pipe => Self::Pipe, Token::Caret => Self::Caret, Token::Tilde => Self::Tilde, Token::Bang => Self::Bang, Token::Question => Self::Question, Token::Colon => Self::Colon, Token::Lt => Self::Lt, Token::Gt => Self::Gt, Token::Comma => Self::Comma, Token::LParen => Self::LParen, Token::RParen => Self::RParen, Token::LBracket => Self::LBracket, Token::RBracket => Self::RBracket } }
# }
# impl TokenT<'_> for Token { type Kind = TokenKind; type Error = LexError; fn kind(&self) -> TokenKind { TokenKind::from(self) } fn is_trivia(&self) -> bool { false } }
# type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug)] enum CExprError { Lex(LexError), UnexpectedToken, UnexpectedEot }
# impl From<LexError> for CExprError { fn from(e: LexError) -> Self { Self::Lex(e) } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CExprError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CExprError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CExprError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError { fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self { Self::UnexpectedToken } }
# #[derive(Clone, Copy, Debug)] enum UnaryOp { Neg, Pos, Not, BNot, PreInc, PreDec }
# #[derive(Clone, Copy, Debug)] enum BinOp { Add, Sub, Mul, Div, Mod, Or, And, BOr, BXor, BAnd, Eq, Neq, Lt, Gt, Lte, Gte, Shl, Shr }
# #[derive(Clone, Copy, Debug)] enum PostfixOp { Inc, Dec, Index, Call, Ternary }
# #[derive(Clone, Debug)] enum Expr { Num(i64), Var(String), Prefix { op: UnaryOp, operand: Box<Expr> }, Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> }, PostfixInc(Box<Expr>), PostfixDec(Box<Expr>), Index { base: Box<Expr>, index: Box<Expr> }, Call { func: Box<Expr>, args: Vec<Expr> }, Ternary { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> } }
# impl core::fmt::Display for UnaryOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { UnaryOp::Neg => "-", UnaryOp::Pos => "+", UnaryOp::Not => "!", UnaryOp::BNot => "~", UnaryOp::PreInc => "++", UnaryOp::PreDec => "--" }) } }
# impl core::fmt::Display for BinOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%", BinOp::Or => "||", BinOp::And => "&&", BinOp::BOr => "|", BinOp::BXor => "^", BinOp::BAnd => "&", BinOp::Eq => "==", BinOp::Neq => "!=", BinOp::Lt => "<", BinOp::Gt => ">", BinOp::Lte => "<=", BinOp::Gte => ">=", BinOp::Shl => "<<", BinOp::Shr => ">>" }) } }
# impl core::fmt::Display for Expr { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { match self { Expr::Num(n) => write!(f, "{n}"), Expr::Var(s) => write!(f, "{s}"), Expr::Prefix { op, operand } => write!(f, "({op}{operand})"), Expr::Binary { op, left, right } => write!(f, "({left} {op} {right})"), Expr::PostfixInc(e) => write!(f, "({e}++)"), Expr::PostfixDec(e) => write!(f, "({e}--)"), Expr::Index { base, index } => write!(f, "({base}[{index}])"), Expr::Ternary { cond, then, otherwise } => write!(f, "({cond} ? {then} : {otherwise})"), Expr::Call { func, args } => { write!(f, "{func}(")?; for (i, a) in args.iter().enumerate() { if i > 0 { write!(f, ", ")?; } write!(f, "{a}")?; } write!(f, ")") } } } }
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)] struct Power(i32);
# impl PrattPower for Power {}
# const PREC_TERNARY: Power = Power(2);
# const PREC_OR: Power = Power(3);
# const PREC_AND: Power = Power(4);
# const PREC_BOR: Power = Power(5);
# const PREC_BXOR: Power = Power(6);
# const PREC_BAND: Power = Power(7);
# const PREC_EQ: Power = Power(8);
# const PREC_CMP: Power = Power(9);
# const PREC_SHIFT: Power = Power(10);
# const PREC_ADD: Power = Power(11);
# const PREC_MUL: Power = Power(12);
# const PREC_PREFIX: Power = Power(13);
# const PREC_POSTFIX: Power = Power(14);
# fn fold_prefix<'inp, Ctx>(_inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, operand: Box<Expr>, op: Precedenced<UnaryOp, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { Ok(Box::new(Expr::Prefix { op: op.into_data(), operand })) }
# fn fold_infix<'inp, Ctx>(_inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, left: Box<Expr>, right: Box<Expr>, op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = op.into_data(); Ok(Box::new(Expr::Binary { op, left, right })) }
# fn fold_postfix<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, operand: Box<Expr>, op: Precedenced<PostfixOp, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match op.into_data() {
#     PostfixOp::Inc => Ok(Box::new(Expr::PostfixInc(operand))),
#     PostfixOp::Dec => Ok(Box::new(Expr::PostfixDec(operand))),
#     PostfixOp::Index => { let index = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::RBracket))?.is_none() { return Err(CExprError::UnexpectedToken); } Ok(Box::new(Expr::Index { base: operand, index })) }
#     PostfixOp::Call => { let mut args: Vec<Expr> = Vec::new(); if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() { return Ok(Box::new(Expr::Call { func: operand, args })); } args.push(*parse_cexpr(inp)?); loop { if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() { break; } if inp.try_expect(|t| matches!(t.data, Token::Comma))?.is_none() { return Err(CExprError::UnexpectedToken); } args.push(*parse_cexpr(inp)?); } Ok(Box::new(Expr::Call { func: operand, args })) }
#     PostfixOp::Ternary => { let then = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::Colon))?.is_none() { return Err(CExprError::UnexpectedToken); } let otherwise = parse_cexpr(inp)?; Ok(Box::new(Expr::Ternary { cond: operand, then, otherwise })) }
#   }
# }
# fn parse_cexpr<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser,
  parser::{PrattInfix, PrattLHS, PrattRHS, Precedenced},
};

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
      Token::Num(n) => Ok(PrattLHS::Operand(Box::new(Expr::Num(n)))),
      Token::Ident(s) => Ok(PrattLHS::Operand(Box::new(Expr::Var(s)))),
      // Grouping: recurse, then require the matching `)`. The parser owns the delimiter.
      Token::LParen => {
        let e = parse_cexpr(inp)?;
        if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_none() {
          return Err(CExprError::UnexpectedToken);
        }
        Ok(PrattLHS::Operand(e))
      }
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Neg, PREC_PREFIX))),
      Token::Plus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Pos, PREC_PREFIX))),
      Token::Bang => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Not, PREC_PREFIX))),
      Token::Tilde => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::BNot, PREC_PREFIX))),
      Token::PlusPlus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreInc, PREC_PREFIX))),
      Token::MinusMinus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreDec, PREC_PREFIX))),
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
  macro_rules! infix_l {
    ($op:expr, $prec:expr) => { PrattRHS::Infix(Precedenced::new(PrattInfix::Left($op), $prec)) };
  }
  match inp.next()? {
    // Nothing left: the expression stops here.
    None => Ok(PrattRHS::End),
    Some(tok) => Ok(match tok.into_data() {
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
      // Postfix triggers. `parse_rhs` consumes only the trigger token; `fold_postfix` reads the
      // rest. `?` binds at the low ternary level; the rest at the high postfix level.
      Token::PlusPlus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Inc, PREC_POSTFIX)),
      Token::MinusMinus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Dec, PREC_POSTFIX)),
      Token::LBracket => PrattRHS::Postfix(Precedenced::new(PostfixOp::Index, PREC_POSTFIX)),
      Token::LParen => PrattRHS::Postfix(Precedenced::new(PostfixOp::Call, PREC_POSTFIX)),
      Token::Question => PrattRHS::Postfix(Precedenced::new(PostfixOp::Ternary, PREC_TERNARY)),
      // Anything else is not an operator here; the engine restores what this call consumed and
      // leaves the token for the surrounding grammar.
      _ => PrattRHS::End,
    }),
  }
}

// Precedence, associativity, grouping, and prefix operators are all decided by these two
// classifiers plus the engine. (The folds — hidden here — build the tree; they are revealed next.)
let parse = |src| Parser::new().apply(parse_cexpr).parse_str(src).map(|e: Box<Expr>| e.to_string());
assert_eq!(parse("1 + 2 * 3").unwrap(), "(1 + (2 * 3))");    // `*` outranks `+`
assert_eq!(parse("(1 + 2) * 3").unwrap(), "((1 + 2) * 3)");  // grouping overrides
assert_eq!(parse("a + b + c").unwrap(), "((a + b) + c)");    // `+` is left-associative
assert_eq!(parse("-a").unwrap(), "(-a)");                    // prefix
assert_eq!(parse("~bits | flags").unwrap(), "((~bits) | flags)"); // prefix binds tighter than `|`
```

## Implement the three folds

Use named `fn`s, not closures: the fold traits carry a higher-ranked lifetime bound (here on the
`InputRef`'s inner lifetime) that a monomorphic closure cannot satisfy but a generic `fn` item
satisfies for free. `fold_prefix` wraps an operand in `Expr::Prefix`; `fold_infix` extracts the
operator from [`PrattInfix`](crate::parser::PrattInfix) (the engine has already applied
associativity) and builds `Expr::Binary`. Both are pure tree builders — they never touch the input.

`fold_postfix` is where the AST-level API earns its keep. It receives the `InputRef` **first** (the
calculator's token-level postfix fold gets no input at all), so a postfix trigger can go on to
consume the tokens it needs. `parse_rhs` has already eaten the trigger (`[`, `(`, or `?`); the fold
reads the rest:

- **index** parses an expression, then requires `]`;
- **call** accepts `)` for an empty argument list or loops over comma-separated expressions;
- **ternary** parses the then-expression, requires `:`, and parses the otherwise-expression.

Crucially, each enclosed sub-expression is just a recursive `parse_cexpr` call, and each of those
stops before the delimiter this fold then consumes itself — the parser stays in control of every
delimiter.

```rust
# use tokora::{Token as TokenT, ParseInput, error::token::UnexpectedTokenOf, logos::{self, Logos}, parser::{PrattLHS, PrattPower, PrattRHS, pratt}};
# #[derive(Clone, Debug, Default, PartialEq)] struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))] Num(i64),
#   #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())] Ident(String),
#   #[token("++")] PlusPlus, #[token("--")] MinusMinus, #[token("==")] EqEq, #[token("!=")] BangEq,
#   #[token("<=")] LtEq, #[token(">=")] GtEq, #[token("&&")] AmpAmp, #[token("||")] PipePipe,
#   #[token("<<")] Shl, #[token(">>")] Shr, #[token("+")] Plus, #[token("-")] Minus,
#   #[token("*")] Star, #[token("/")] Slash, #[token("%")] Percent, #[token("&")] Amp,
#   #[token("|")] Pipe, #[token("^")] Caret, #[token("~")] Tilde, #[token("!")] Bang,
#   #[token("?")] Question, #[token(":")] Colon, #[token("<")] Lt, #[token(">")] Gt,
#   #[token(",")] Comma, #[token("(")] LParen, #[token(")")] RParen, #[token("[")] LBracket,
#   #[token("]")] RBracket,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Ident, PlusPlus, MinusMinus, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Shl, Shr, Plus, Minus, Star, Slash, Percent, Amp, Pipe, Caret, Tilde, Bang, Question, Colon, Lt, Gt, Comma, LParen, RParen, LBracket, RBracket }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Ident => "identifier", Self::PlusPlus => "++", Self::MinusMinus => "--", Self::EqEq => "==", Self::BangEq => "!=", Self::LtEq => "<=", Self::GtEq => ">=", Self::AmpAmp => "&&", Self::PipePipe => "||", Self::Shl => "<<", Self::Shr => ">>", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Percent => "%", Self::Amp => "&", Self::Pipe => "|", Self::Caret => "^", Self::Tilde => "~", Self::Bang => "!", Self::Question => "?", Self::Colon => ":", Self::Lt => "<", Self::Gt => ">", Self::Comma => ",", Self::LParen => "(", Self::RParen => ")", Self::LBracket => "[", Self::RBracket => "]" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Ident(_) => Self::Ident, Token::PlusPlus => Self::PlusPlus, Token::MinusMinus => Self::MinusMinus, Token::EqEq => Self::EqEq, Token::BangEq => Self::BangEq, Token::LtEq => Self::LtEq, Token::GtEq => Self::GtEq, Token::AmpAmp => Self::AmpAmp, Token::PipePipe => Self::PipePipe, Token::Shl => Self::Shl, Token::Shr => Self::Shr, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Percent => Self::Percent, Token::Amp => Self::Amp, Token::Pipe => Self::Pipe, Token::Caret => Self::Caret, Token::Tilde => Self::Tilde, Token::Bang => Self::Bang, Token::Question => Self::Question, Token::Colon => Self::Colon, Token::Lt => Self::Lt, Token::Gt => Self::Gt, Token::Comma => Self::Comma, Token::LParen => Self::LParen, Token::RParen => Self::RParen, Token::LBracket => Self::LBracket, Token::RBracket => Self::RBracket } }
# }
# impl TokenT<'_> for Token { type Kind = TokenKind; type Error = LexError; fn kind(&self) -> TokenKind { TokenKind::from(self) } fn is_trivia(&self) -> bool { false } }
# type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug)] enum CExprError { Lex(LexError), UnexpectedToken, UnexpectedEot }
# impl From<LexError> for CExprError { fn from(e: LexError) -> Self { Self::Lex(e) } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CExprError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CExprError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CExprError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError { fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self { Self::UnexpectedToken } }
# #[derive(Clone, Copy, Debug)] enum UnaryOp { Neg, Pos, Not, BNot, PreInc, PreDec }
# #[derive(Clone, Copy, Debug)] enum BinOp { Add, Sub, Mul, Div, Mod, Or, And, BOr, BXor, BAnd, Eq, Neq, Lt, Gt, Lte, Gte, Shl, Shr }
# #[derive(Clone, Copy, Debug)] enum PostfixOp { Inc, Dec, Index, Call, Ternary }
# #[derive(Clone, Debug)] enum Expr { Num(i64), Var(String), Prefix { op: UnaryOp, operand: Box<Expr> }, Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> }, PostfixInc(Box<Expr>), PostfixDec(Box<Expr>), Index { base: Box<Expr>, index: Box<Expr> }, Call { func: Box<Expr>, args: Vec<Expr> }, Ternary { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> } }
# impl core::fmt::Display for UnaryOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { UnaryOp::Neg => "-", UnaryOp::Pos => "+", UnaryOp::Not => "!", UnaryOp::BNot => "~", UnaryOp::PreInc => "++", UnaryOp::PreDec => "--" }) } }
# impl core::fmt::Display for BinOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%", BinOp::Or => "||", BinOp::And => "&&", BinOp::BOr => "|", BinOp::BXor => "^", BinOp::BAnd => "&", BinOp::Eq => "==", BinOp::Neq => "!=", BinOp::Lt => "<", BinOp::Gt => ">", BinOp::Lte => "<=", BinOp::Gte => ">=", BinOp::Shl => "<<", BinOp::Shr => ">>" }) } }
# impl core::fmt::Display for Expr { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { match self { Expr::Num(n) => write!(f, "{n}"), Expr::Var(s) => write!(f, "{s}"), Expr::Prefix { op, operand } => write!(f, "({op}{operand})"), Expr::Binary { op, left, right } => write!(f, "({left} {op} {right})"), Expr::PostfixInc(e) => write!(f, "({e}++)"), Expr::PostfixDec(e) => write!(f, "({e}--)"), Expr::Index { base, index } => write!(f, "({base}[{index}])"), Expr::Ternary { cond, then, otherwise } => write!(f, "({cond} ? {then} : {otherwise})"), Expr::Call { func, args } => { write!(f, "{func}(")?; for (i, a) in args.iter().enumerate() { if i > 0 { write!(f, ", ")?; } write!(f, "{a}")?; } write!(f, ")") } } } }
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)] struct Power(i32);
# impl PrattPower for Power {}
# const PREC_TERNARY: Power = Power(2);
# const PREC_OR: Power = Power(3);
# const PREC_AND: Power = Power(4);
# const PREC_BOR: Power = Power(5);
# const PREC_BXOR: Power = Power(6);
# const PREC_BAND: Power = Power(7);
# const PREC_EQ: Power = Power(8);
# const PREC_CMP: Power = Power(9);
# const PREC_SHIFT: Power = Power(10);
# const PREC_ADD: Power = Power(11);
# const PREC_MUL: Power = Power(12);
# const PREC_PREFIX: Power = Power(13);
# const PREC_POSTFIX: Power = Power(14);
# fn parse_lhs<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<PrattLHS<Box<Expr>, UnaryOp, Power>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match inp.next()? {
#     None => Err(CExprError::UnexpectedEot),
#     Some(tok) => match tok.into_data() {
#       Token::Num(n) => Ok(PrattLHS::Operand(Box::new(Expr::Num(n)))),
#       Token::Ident(s) => Ok(PrattLHS::Operand(Box::new(Expr::Var(s)))),
#       Token::LParen => { let e = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_none() { return Err(CExprError::UnexpectedToken); } Ok(PrattLHS::Operand(e)) }
#       Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Neg, PREC_PREFIX))),
#       Token::Plus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Pos, PREC_PREFIX))),
#       Token::Bang => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Not, PREC_PREFIX))),
#       Token::Tilde => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::BNot, PREC_PREFIX))),
#       Token::PlusPlus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreInc, PREC_PREFIX))),
#       Token::MinusMinus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreDec, PREC_PREFIX))),
#       _ => Err(CExprError::UnexpectedToken),
#     },
#   }
# }
# fn parse_rhs<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<PrattRHS<BinOp, BinOp, BinOp, PostfixOp, Power>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match inp.next()? {
#     None => Ok(PrattRHS::End),
#     Some(tok) => Ok(match tok.into_data() {
#       Token::PipePipe => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Or), PREC_OR)),
#       Token::AmpAmp => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::And), PREC_AND)),
#       Token::Pipe => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BOr), PREC_BOR)),
#       Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BXor), PREC_BXOR)),
#       Token::Amp => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BAnd), PREC_BAND)),
#       Token::EqEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Eq), PREC_EQ)),
#       Token::BangEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Neq), PREC_EQ)),
#       Token::Lt => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Lt), PREC_CMP)),
#       Token::Gt => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Gt), PREC_CMP)),
#       Token::LtEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Lte), PREC_CMP)),
#       Token::GtEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Gte), PREC_CMP)),
#       Token::Shl => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Shl), PREC_SHIFT)),
#       Token::Shr => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Shr), PREC_SHIFT)),
#       Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Add), PREC_ADD)),
#       Token::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Sub), PREC_ADD)),
#       Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Mul), PREC_MUL)),
#       Token::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Div), PREC_MUL)),
#       Token::Percent => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Mod), PREC_MUL)),
#       Token::PlusPlus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Inc, PREC_POSTFIX)),
#       Token::MinusMinus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Dec, PREC_POSTFIX)),
#       Token::LBracket => PrattRHS::Postfix(Precedenced::new(PostfixOp::Index, PREC_POSTFIX)),
#       Token::LParen => PrattRHS::Postfix(Precedenced::new(PostfixOp::Call, PREC_POSTFIX)),
#       Token::Question => PrattRHS::Postfix(Precedenced::new(PostfixOp::Ternary, PREC_TERNARY)),
#       _ => PrattRHS::End,
#     }),
#   }
# }
# fn parse_cexpr<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp) }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser,
  parser::{PrattInfix, Precedenced},
};

fn fold_prefix<'inp, Ctx>(
  _inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
  operand: Box<Expr>,
  op: Precedenced<UnaryOp, Power>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  Ok(Box::new(Expr::Prefix { op: op.into_data(), operand }))
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
  // Associativity has already done its job in the engine; the fold just wants the operator.
  let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = op.into_data();
  Ok(Box::new(Expr::Binary { op, left, right }))
}

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
    // e[index]: parse the index (stops before `]`), then require `]`.
    PostfixOp::Index => {
      let index = parse_cexpr(inp)?;
      if inp.try_expect(|t| matches!(t.data, Token::RBracket))?.is_none() {
        return Err(CExprError::UnexpectedToken);
      }
      Ok(Box::new(Expr::Index { base: operand, index }))
    }
    // e(args): an empty `)`, or a comma-separated argument loop.
    PostfixOp::Call => {
      let mut args: Vec<Expr> = Vec::new();
      if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() {
        return Ok(Box::new(Expr::Call { func: operand, args }));
      }
      args.push(*parse_cexpr(inp)?);
      loop {
        if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() {
          break;
        }
        if inp.try_expect(|t| matches!(t.data, Token::Comma))?.is_none() {
          return Err(CExprError::UnexpectedToken);
        }
        args.push(*parse_cexpr(inp)?);
      }
      Ok(Box::new(Expr::Call { func: operand, args }))
    }
    // cond ? then : otherwise — parse `then` (stops before `:`), require `:`, parse `otherwise`.
    PostfixOp::Ternary => {
      let then = parse_cexpr(inp)?;
      if inp.try_expect(|t| matches!(t.data, Token::Colon))?.is_none() {
        return Err(CExprError::UnexpectedToken);
      }
      let otherwise = parse_cexpr(inp)?;
      Ok(Box::new(Expr::Ternary { cond: operand, then, otherwise }))
    }
  }
}

// The postfix forms — increment/decrement, indexing, calls, and the ternary — each proven by the
// fold consuming exactly the input it needs after the trigger token.
let parse = |src| Parser::new().apply(parse_cexpr).parse_str(src).map(|e: Box<Expr>| e.to_string());
assert_eq!(parse("x++").unwrap(), "(x++)");                       // postfix ++ (same token as prefix)
assert_eq!(parse("arr[i + 1]").unwrap(), "(arr[(i + 1)])");       // index consumes an expr and `]`
assert_eq!(parse("f()").unwrap(), "f()");                         // empty argument list
assert_eq!(parse("f(a + b, c * d)").unwrap(), "f((a + b), (c * d))"); // comma-separated args
assert_eq!(parse("a ? b : c").unwrap(), "(a ? b : c)");           // ternary consumes `t`, `:`, `f`
```

## Close the recursion in `parse_cexpr`

`parse_cexpr` calls [`pratt`](crate::parser::pratt) with the two classifiers and three folds,
then drives it with [`parse_input`](crate::ParseInput::parse_input). Note what is *absent* from its
bounds: there is no [`PrattEmitter`](crate::emitter::PrattEmitter). The AST engine keeps
error-reporting inside the folds and `parse_lhs`, so the ordinary [`Emitter`](crate::Emitter) bound
is all it needs — a [`FatalContext`](crate::FatalContext) satisfies it with no extra work. The
mutual recursion (grouping in `parse_lhs`, index/call/ternary in `fold_postfix`) rides ordinary
call-stack frames through these named `fn`s; no recursive parser *type* is involved.

The five assertions below extend to the maintained binary's full assertion table — precedence,
grouping, associativity, unary operators, increment, ternary, indexing, calls, shifts, and bitwise
operators — now executable inline:

```rust
# use tokora::{Token as TokenT, error::token::UnexpectedTokenOf, logos::{self, Logos}, parser::{PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced}};
# #[derive(Clone, Debug, Default, PartialEq)] struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { Self } }
# #[derive(Clone, Debug, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Token {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))] Num(i64),
#   #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice().to_string())] Ident(String),
#   #[token("++")] PlusPlus, #[token("--")] MinusMinus, #[token("==")] EqEq, #[token("!=")] BangEq,
#   #[token("<=")] LtEq, #[token(">=")] GtEq, #[token("&&")] AmpAmp, #[token("||")] PipePipe,
#   #[token("<<")] Shl, #[token(">>")] Shr, #[token("+")] Plus, #[token("-")] Minus,
#   #[token("*")] Star, #[token("/")] Slash, #[token("%")] Percent, #[token("&")] Amp,
#   #[token("|")] Pipe, #[token("^")] Caret, #[token("~")] Tilde, #[token("!")] Bang,
#   #[token("?")] Question, #[token(":")] Colon, #[token("<")] Lt, #[token(">")] Gt,
#   #[token(",")] Comma, #[token("(")] LParen, #[token(")")] RParen, #[token("[")] LBracket,
#   #[token("]")] RBracket,
# }
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# enum TokenKind { Num, Ident, PlusPlus, MinusMinus, EqEq, BangEq, LtEq, GtEq, AmpAmp, PipePipe, Shl, Shr, Plus, Minus, Star, Slash, Percent, Amp, Pipe, Caret, Tilde, Bang, Question, Colon, Lt, Gt, Comma, LParen, RParen, LBracket, RBracket }
# impl core::fmt::Display for TokenKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self { Self::Num => "number", Self::Ident => "identifier", Self::PlusPlus => "++", Self::MinusMinus => "--", Self::EqEq => "==", Self::BangEq => "!=", Self::LtEq => "<=", Self::GtEq => ">=", Self::AmpAmp => "&&", Self::PipePipe => "||", Self::Shl => "<<", Self::Shr => ">>", Self::Plus => "+", Self::Minus => "-", Self::Star => "*", Self::Slash => "/", Self::Percent => "%", Self::Amp => "&", Self::Pipe => "|", Self::Caret => "^", Self::Tilde => "~", Self::Bang => "!", Self::Question => "?", Self::Colon => ":", Self::Lt => "<", Self::Gt => ">", Self::Comma => ",", Self::LParen => "(", Self::RParen => ")", Self::LBracket => "[", Self::RBracket => "]" })
#   }
# }
# impl core::fmt::Display for Token { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { core::fmt::Display::fmt(&self.kind(), f) } }
# impl From<&Token> for TokenKind {
#   fn from(t: &Token) -> Self { match t { Token::Num(_) => Self::Num, Token::Ident(_) => Self::Ident, Token::PlusPlus => Self::PlusPlus, Token::MinusMinus => Self::MinusMinus, Token::EqEq => Self::EqEq, Token::BangEq => Self::BangEq, Token::LtEq => Self::LtEq, Token::GtEq => Self::GtEq, Token::AmpAmp => Self::AmpAmp, Token::PipePipe => Self::PipePipe, Token::Shl => Self::Shl, Token::Shr => Self::Shr, Token::Plus => Self::Plus, Token::Minus => Self::Minus, Token::Star => Self::Star, Token::Slash => Self::Slash, Token::Percent => Self::Percent, Token::Amp => Self::Amp, Token::Pipe => Self::Pipe, Token::Caret => Self::Caret, Token::Tilde => Self::Tilde, Token::Bang => Self::Bang, Token::Question => Self::Question, Token::Colon => Self::Colon, Token::Lt => Self::Lt, Token::Gt => Self::Gt, Token::Comma => Self::Comma, Token::LParen => Self::LParen, Token::RParen => Self::RParen, Token::LBracket => Self::LBracket, Token::RBracket => Self::RBracket } }
# }
# impl TokenT<'_> for Token { type Kind = TokenKind; type Error = LexError; fn kind(&self) -> TokenKind { TokenKind::from(self) } fn is_trivia(&self) -> bool { false } }
# type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;
# #[derive(Debug)] enum CExprError { Lex(LexError), UnexpectedToken, UnexpectedEot }
# impl From<LexError> for CExprError { fn from(e: LexError) -> Self { Self::Lex(e) } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>> for CExprError { fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::RecursionLimitReached<O, Lang>> for CExprError { fn from(_: tokora::error::RecursionLimitReached<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<O, Lang: ?Sized> From<tokora::error::NonAssociativeChain<O, Lang>> for CExprError { fn from(_: tokora::error::NonAssociativeChain<O, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CExprError { fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { Self::UnexpectedEot } }
# impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError { fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self { Self::UnexpectedToken } }
# #[derive(Clone, Copy, Debug)] enum UnaryOp { Neg, Pos, Not, BNot, PreInc, PreDec }
# #[derive(Clone, Copy, Debug)] enum BinOp { Add, Sub, Mul, Div, Mod, Or, And, BOr, BXor, BAnd, Eq, Neq, Lt, Gt, Lte, Gte, Shl, Shr }
# #[derive(Clone, Copy, Debug)] enum PostfixOp { Inc, Dec, Index, Call, Ternary }
# #[derive(Clone, Debug)] enum Expr { Num(i64), Var(String), Prefix { op: UnaryOp, operand: Box<Expr> }, Binary { op: BinOp, left: Box<Expr>, right: Box<Expr> }, PostfixInc(Box<Expr>), PostfixDec(Box<Expr>), Index { base: Box<Expr>, index: Box<Expr> }, Call { func: Box<Expr>, args: Vec<Expr> }, Ternary { cond: Box<Expr>, then: Box<Expr>, otherwise: Box<Expr> } }
# impl core::fmt::Display for UnaryOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { UnaryOp::Neg => "-", UnaryOp::Pos => "+", UnaryOp::Not => "!", UnaryOp::BNot => "~", UnaryOp::PreInc => "++", UnaryOp::PreDec => "--" }) } }
# impl core::fmt::Display for BinOp { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str(match self { BinOp::Add => "+", BinOp::Sub => "-", BinOp::Mul => "*", BinOp::Div => "/", BinOp::Mod => "%", BinOp::Or => "||", BinOp::And => "&&", BinOp::BOr => "|", BinOp::BXor => "^", BinOp::BAnd => "&", BinOp::Eq => "==", BinOp::Neq => "!=", BinOp::Lt => "<", BinOp::Gt => ">", BinOp::Lte => "<=", BinOp::Gte => ">=", BinOp::Shl => "<<", BinOp::Shr => ">>" }) } }
# impl core::fmt::Display for Expr { fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { match self { Expr::Num(n) => write!(f, "{n}"), Expr::Var(s) => write!(f, "{s}"), Expr::Prefix { op, operand } => write!(f, "({op}{operand})"), Expr::Binary { op, left, right } => write!(f, "({left} {op} {right})"), Expr::PostfixInc(e) => write!(f, "({e}++)"), Expr::PostfixDec(e) => write!(f, "({e}--)"), Expr::Index { base, index } => write!(f, "({base}[{index}])"), Expr::Ternary { cond, then, otherwise } => write!(f, "({cond} ? {then} : {otherwise})"), Expr::Call { func, args } => { write!(f, "{func}(")?; for (i, a) in args.iter().enumerate() { if i > 0 { write!(f, ", ")?; } write!(f, "{a}")?; } write!(f, ")") } } } }
# #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)] struct Power(i32);
# impl PrattPower for Power {}
# const PREC_TERNARY: Power = Power(2);
# const PREC_OR: Power = Power(3);
# const PREC_AND: Power = Power(4);
# const PREC_BOR: Power = Power(5);
# const PREC_BXOR: Power = Power(6);
# const PREC_BAND: Power = Power(7);
# const PREC_EQ: Power = Power(8);
# const PREC_CMP: Power = Power(9);
# const PREC_SHIFT: Power = Power(10);
# const PREC_ADD: Power = Power(11);
# const PREC_MUL: Power = Power(12);
# const PREC_PREFIX: Power = Power(13);
# const PREC_POSTFIX: Power = Power(14);
# fn parse_lhs<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<PrattLHS<Box<Expr>, UnaryOp, Power>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match inp.next()? {
#     None => Err(CExprError::UnexpectedEot),
#     Some(tok) => match tok.into_data() {
#       Token::Num(n) => Ok(PrattLHS::Operand(Box::new(Expr::Num(n)))),
#       Token::Ident(s) => Ok(PrattLHS::Operand(Box::new(Expr::Var(s)))),
#       Token::LParen => { let e = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_none() { return Err(CExprError::UnexpectedToken); } Ok(PrattLHS::Operand(e)) }
#       Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Neg, PREC_PREFIX))),
#       Token::Plus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Pos, PREC_PREFIX))),
#       Token::Bang => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::Not, PREC_PREFIX))),
#       Token::Tilde => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::BNot, PREC_PREFIX))),
#       Token::PlusPlus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreInc, PREC_PREFIX))),
#       Token::MinusMinus => Ok(PrattLHS::Prefix(Precedenced::new(UnaryOp::PreDec, PREC_PREFIX))),
#       _ => Err(CExprError::UnexpectedToken),
#     },
#   }
# }
# fn parse_rhs<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>) -> Result<PrattRHS<BinOp, BinOp, BinOp, PostfixOp, Power>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match inp.next()? {
#     None => Ok(PrattRHS::End),
#     Some(tok) => Ok(match tok.into_data() {
#       Token::PipePipe => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Or), PREC_OR)),
#       Token::AmpAmp => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::And), PREC_AND)),
#       Token::Pipe => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BOr), PREC_BOR)),
#       Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BXor), PREC_BXOR)),
#       Token::Amp => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::BAnd), PREC_BAND)),
#       Token::EqEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Eq), PREC_EQ)),
#       Token::BangEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Neq), PREC_EQ)),
#       Token::Lt => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Lt), PREC_CMP)),
#       Token::Gt => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Gt), PREC_CMP)),
#       Token::LtEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Lte), PREC_CMP)),
#       Token::GtEq => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Gte), PREC_CMP)),
#       Token::Shl => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Shl), PREC_SHIFT)),
#       Token::Shr => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Shr), PREC_SHIFT)),
#       Token::Plus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Add), PREC_ADD)),
#       Token::Minus => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Sub), PREC_ADD)),
#       Token::Star => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Mul), PREC_MUL)),
#       Token::Slash => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Div), PREC_MUL)),
#       Token::Percent => PrattRHS::Infix(Precedenced::new(PrattInfix::Left(BinOp::Mod), PREC_MUL)),
#       Token::PlusPlus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Inc, PREC_POSTFIX)),
#       Token::MinusMinus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Dec, PREC_POSTFIX)),
#       Token::LBracket => PrattRHS::Postfix(Precedenced::new(PostfixOp::Index, PREC_POSTFIX)),
#       Token::LParen => PrattRHS::Postfix(Precedenced::new(PostfixOp::Call, PREC_POSTFIX)),
#       Token::Question => PrattRHS::Postfix(Precedenced::new(PostfixOp::Ternary, PREC_TERNARY)),
#       _ => PrattRHS::End,
#     }),
#   }
# }
# fn fold_prefix<'inp, Ctx>(_inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, operand: Box<Expr>, op: Precedenced<UnaryOp, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { Ok(Box::new(Expr::Prefix { op: op.into_data(), operand })) }
# fn fold_infix<'inp, Ctx>(_inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, left: Box<Expr>, right: Box<Expr>, op: Precedenced<PrattInfix<BinOp, BinOp, BinOp>, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> { let (PrattInfix::Left(op) | PrattInfix::Right(op) | PrattInfix::Neither(op)) = op.into_data(); Ok(Box::new(Expr::Binary { op, left, right })) }
# fn fold_postfix<'inp, Ctx>(inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>, operand: Box<Expr>, op: Precedenced<PostfixOp, Power>) -> Result<Box<Expr>, CExprError> where Ctx: ParseContext<'inp, CExprLexer<'inp>>, Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError> {
#   match op.into_data() {
#     PostfixOp::Inc => Ok(Box::new(Expr::PostfixInc(operand))),
#     PostfixOp::Dec => Ok(Box::new(Expr::PostfixDec(operand))),
#     PostfixOp::Index => { let index = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::RBracket))?.is_none() { return Err(CExprError::UnexpectedToken); } Ok(Box::new(Expr::Index { base: operand, index })) }
#     PostfixOp::Call => { let mut args: Vec<Expr> = Vec::new(); if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() { return Ok(Box::new(Expr::Call { func: operand, args })); } args.push(*parse_cexpr(inp)?); loop { if inp.try_expect(|t| matches!(t.data, Token::RParen))?.is_some() { break; } if inp.try_expect(|t| matches!(t.data, Token::Comma))?.is_none() { return Err(CExprError::UnexpectedToken); } args.push(*parse_cexpr(inp)?); } Ok(Box::new(Expr::Call { func: operand, args })) }
#     PostfixOp::Ternary => { let then = parse_cexpr(inp)?; if inp.try_expect(|t| matches!(t.data, Token::Colon))?.is_none() { return Err(CExprError::UnexpectedToken); } let otherwise = parse_cexpr(inp)?; Ok(Box::new(Expr::Ternary { cond: operand, then, otherwise })) }
#   }
# }
use tokora::{Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, parser::pratt};

fn parse_cexpr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CExprLexer<'inp>, Ctx>,
) -> Result<Box<Expr>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix).parse_input(inp)
}

// The maintained binary's assertion table — the parser's behavior contract — now inline. Each row
// parses a C expression and checks its fully-parenthesised `Display`.
let parse = |src| Parser::new().apply(parse_cexpr).parse_str(src).map(|e: Box<Expr>| e.to_string());
assert_eq!(parse("1 + 2 * 3").unwrap(), "(1 + (2 * 3))");
assert_eq!(parse("(1 + 2) * 3").unwrap(), "((1 + 2) * 3)");
assert_eq!(parse("a + b + c").unwrap(), "((a + b) + c)");
assert_eq!(parse("-a").unwrap(), "(-a)");
assert_eq!(parse("!flag").unwrap(), "(!flag)");
assert_eq!(parse("~bits").unwrap(), "(~bits)");
assert_eq!(parse("++x").unwrap(), "(++x)");
assert_eq!(parse("x++").unwrap(), "(x++)");
assert_eq!(parse("a ? b : c").unwrap(), "(a ? b : c)");
assert_eq!(parse("arr[0]").unwrap(), "(arr[0])");
assert_eq!(parse("f()").unwrap(), "f()");
assert_eq!(parse("f(1, 2)").unwrap(), "f(1, 2)");
assert_eq!(parse("a == b && c != d").unwrap(), "((a == b) && (c != d))");
assert_eq!(parse("~bits | flags").unwrap(), "((~bits) | flags)");
assert_eq!(parse("arr[i + 1]").unwrap(), "(arr[(i + 1)])");
assert_eq!(parse("f(a + b, c * d)").unwrap(), "f((a + b), (c * d))");
assert_eq!(parse("x << 2 | y >> 1").unwrap(), "((x << 2) | (y >> 1))");
```

## Reproduce the maintained assertion table

The assertions above *are* the maintained binary's assertion table: precedence, grouping,
left-associativity, unary operators, prefix and postfix increment, the ternary, indexing, calls,
mixed precedence chains, and C's low-precedence bitwise and shift operators. They are the behavior
contract for the parser. For the full runnable program — the same code driven from a `main` that
prints each parsed expression — run:

```sh
cargo run -p tokora --example c_expression --features logos
```

You have now followed the complete C-expression parser inline: a Logos lexer, a typed `Expr` AST,
C's precedence ladder, two classifier *functions*, and three folds — the postfix one consuming its
own delimiters for indexing, calls, and the ternary — closed into a one-call `parse_cexpr`. The
[Pratt reference](super::ref_pratt) catalogs both Pratt surfaces side by side.

The optional chapter 16, Lossless CSTs with Rowan, takes a different route: it records source
tokens rather than reducing them to an AST. It requires the `rowan` feature, so it is named here
without a rustdoc link.
