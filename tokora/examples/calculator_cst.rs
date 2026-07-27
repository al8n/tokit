//! The lossless-CST twin of `calculator.rs`.
//!
//! The twin evaluates arithmetic with the **token-level** Pratt API
//! ([`InputRef::pratt`](tokora::InputRef::pratt)), which folds into synthetic tokens and is
//! documented CST-unsupported. So this twin uses the **typed** Pratt driver
//! ([`pratt`](tokora::parser::pratt)) and its CST seam,
//! [`with_cst_kinds`](tokora::parser::Pratt::with_cst_kinds): the driver holds one mark per
//! expression and wraps each fold in a node whose kind a classifier picks from the operator.
//! No evaluation happens — the lossless tree is the whole product, and the fold functions do
//! nothing but satisfy the driver.
//!
//! `1 + 2 * 3` materializes as `BinExpr[1, +, BinExpr[2, *, 3]]`: precedence and
//! associativity fall out of the driver, and every byte (whitespace included) round-trips.
//!
//! Run: `cargo run --example calculator_cst --features "std,logos,rowan"`

use rowan::{Language, NodeOrToken, SyntaxNode};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser, Token as TokenT,
  cache::DefaultCache,
  cst::Sink,
  emitter::{CstEmitter, Fatal},
  error::token::UnexpectedTokenOf,
  logos::{self, Logos},
  parser::{PrattFoldOp, PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced, pratt},
};

// ── Lossless lexer ──────────────────────────────────────────────────────────────
//
// No `skip` rule: whitespace is a real trivia token. Tokens are fieldless — the tree keeps
// each number's source text, so there is nothing to parse into an `f64` here.

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Logos)]
#[logos(crate = logos, error = LexError)]
enum Token {
  #[regex(r"[ \t\r\n]+")]
  Whitespace,
  #[regex(r"[0-9]+(\.[0-9]+)?")]
  Num,
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("^")]
  Caret,
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
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
      Token::Caret => "^",
      Token::LParen => "(",
      Token::RParen => ")",
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

type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Error ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CalcError {
  Lex,
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for CalcError {
  fn from(_: LexError) -> Self {
    CalcError::Lex
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, CalcLexer<'inp>>> for CalcError {
  fn from(_: UnexpectedTokenOf<'inp, CalcLexer<'inp>>) -> Self {
    CalcError::UnexpectedToken
  }
}

// The typed pratt driver's stalled-report exits: a report the driver acted on that consumed
// nothing is a grammar bug, surfaced as the terminal end-of-expression error for the channel
// that broke the contract — the LHS one for a prefix report, the RHS one for an infix/postfix
// report — rather than ending the expression with a silent `Ok`.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoLhs<O, Lang, Set>>
  for CalcError
{
  fn from(_: tokora::error::UnexpectedEoLhs<O, Lang, Set>) -> Self {
    CalcError::UnexpectedEot
  }
}

impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEoRhs<O, Lang, Set>>
  for CalcError
{
  fn from(_: tokora::error::UnexpectedEoRhs<O, Lang, Set>) -> Self {
    CalcError::UnexpectedEot
  }
}

// ── Unified syntax-kind space ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum SyntaxKind {
  // Token images.
  Whitespace,
  Num,
  Plus,
  Minus,
  Star,
  Slash,
  Caret,
  LParen,
  RParen,
  // Node kinds.
  BinExpr,
  PrefixExpr,
  // Bookkeeping.
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
    Token::Caret => K::Caret,
    Token::LParen => K::LParen,
    Token::RParen => K::RParen,
  }) as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CalcLang {}

impl Language for CalcLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    const KINDS: [SyntaxKind; 14] = [
      K::Whitespace,
      K::Num,
      K::Plus,
      K::Minus,
      K::Star,
      K::Slash,
      K::Caret,
      K::LParen,
      K::RParen,
      K::BinExpr,
      K::PrefixExpr,
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

// ── Binding powers ────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Power(i32);

impl PrattPower for Power {}

const PREC_SUM: Power = Power(1); // + -
const PREC_PROD: Power = Power(2); // * /
const PREC_NEG: Power = Power(3); // unary -
const PREC_EXP: Power = Power(4); // ^ (right-assoc)

// ── Pratt parse functions ─────────────────────────────────────────────────────────
//
// Named functions (not closures) satisfy the higher-rank lifetime bounds the driver
// requires. `O = ()` throughout: no value is computed, so operands and folds carry the unit
// type and the tree — built by the driver's CST seam — is the sole result.

type CalcIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, CalcLexer<'inp>, Ctx>;

/// Left-hand side: an operand (number, or a parenthesized sub-expression) or a prefix `-`.
/// Skips leading trivia first, so the whitespace is committed and kept.
fn parse_lhs<'inp, Ctx>(
  inp: &mut CalcIn<'inp, '_, Ctx>,
) -> Result<PrattLHS<(), (), Power>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CalcLexer<'inp>> + Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    None => Err(CalcError::UnexpectedEot),
    Some(tok) => match tok.into_data() {
      Token::Num => Ok(PrattLHS::Operand(())),
      Token::Minus => Ok(PrattLHS::Prefix(Precedenced::new((), PREC_NEG))),
      // Grouping: `(` expr `)`. The parens land as bare tokens inside whatever node later
      // wraps this operand; the inner expression parses with a fresh precedence floor.
      Token::LParen => {
        parse_expr(inp)?;
        inp.skip_while(|t| t.is_trivia())?;
        match inp.try_expect(|t| matches!(t.data().kind(), Token::RParen))? {
          Some(_) => Ok(PrattLHS::Operand(())),
          None => Err(CalcError::UnexpectedToken),
        }
      }
      _ => Err(CalcError::UnexpectedToken),
    },
  }
}

/// Right-hand side: an infix operator, or the end of the expression.
fn parse_rhs<'inp, Ctx>(
  inp: &mut CalcIn<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), (), Power>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => Ok(match tok.into_data() {
      Token::Plus | Token::Minus => {
        PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_SUM))
      }
      Token::Star | Token::Slash => {
        PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), PREC_PROD))
      }
      Token::Caret => PrattRHS::Infix(Precedenced::new(PrattInfix::Right(()), PREC_EXP)),
      // Not an operator (a `)`, or an operand): end here; the driver rolls it back.
      _ => PrattRHS::End,
    }),
  }
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut CalcIn<'inp, '_, Ctx>,
  _operand: (),
  _op: Precedenced<(), Power>,
) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  Ok(()) // the tree records the fold; there is no value to build
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut CalcIn<'inp, '_, Ctx>,
  _left: (),
  _right: (),
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  Ok(())
}

fn fold_postfix<'inp, Ctx>(
  _inp: &mut CalcIn<'inp, '_, Ctx>,
  _operand: (),
  _op: Precedenced<(), Power>,
) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  Ok(()) // this grammar declares no postfix operator, so nothing folds here
}

/// The CST seam: each fold's operator picks the node kind that wraps the folded region. This
/// grammar has no postfix operator, so that arm records nothing.
fn calc_kinds(op: PrattFoldOp<'_, (), (), (), (), ()>) -> Option<u16> {
  match op {
    PrattFoldOp::Prefix(_) => Some(K::PrefixExpr.raw()),
    PrattFoldOp::Infix(_) => Some(K::BinExpr.raw()),
    PrattFoldOp::Postfix(_) => None,
  }
}

/// One full expression through the typed Pratt driver, with the CST seam configured.
fn parse_expr<'inp, Ctx>(inp: &mut CalcIn<'inp, '_, Ctx>) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CalcLexer<'inp>> + Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
    .with_cst_kinds(calc_kinds)
    .parse_input(inp)
}

/// Top-level: leading trivia, one expression, trailing trivia — every byte committed, so the
/// round trip holds. The trivia at the edges commits at depth 0, under the synthetic `Root`.
fn program<'inp, Ctx>(inp: &mut CalcIn<'inp, '_, Ctx>) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CalcLexer<'inp>> + Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  parse_expr(inp)?;
  inp.skip_while(|t| t.is_trivia())?;
  Ok(())
}

// ── Tree building + dump ────────────────────────────────────────────────────────

fn parse_to_tree(src: &str) -> SyntaxNode<CalcLang> {
  let mut sink: Sink<'_, CalcLexer<'_>, _> = Sink::new(
    Fatal::<CalcError>::new(),
    map_token,
    K::Error.raw(),
    K::Gap.raw(),
  );
  Parser::with_context((&mut sink, DefaultCache::<CalcLexer<'_>>::default()))
    .apply(program)
    .parse_str(src)
    .expect("parse succeeds");
  let (green, _emitter) = sink.finish(K::Root.raw(), src);
  SyntaxNode::<CalcLang>::new_root(green.expect("well-formed tree"))
}

fn dump(node: &SyntaxNode<CalcLang>, depth: usize, out: &mut String) {
  use core::fmt::Write as _;
  let _ = writeln!(out, "{:indent$}{:?}", "", node.kind(), indent = depth * 2);
  for child in node.children_with_tokens() {
    match child {
      NodeOrToken::Node(n) => dump(&n, depth + 1, out),
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

// ── Main ────────────────────────────────────────────────────────────────────────

fn main() {
  // The same inputs as the twin — parsed to a tree, not evaluated. Each round-trips.
  let cases = [
    "1 + 2 * 3",
    "(1 + 2) * 3",
    "2 ^ 3 ^ 2",
    "-2 ^ 2",
    "10 / 2 / 5",
  ];
  for src in cases {
    let tree = parse_to_tree(src);
    assert_eq!(tree.text().to_string(), src, "round trip for `{src}`");
    println!(
      "round-trip ok: {src:12} -> {} bin/prefix nodes",
      tree.descendants().filter(is_expr_node).count()
    );
  }

  // A detailed look at one input: the precedence nesting is the tree's shape.
  let src = "1 + 2 * 3";
  let tree = parse_to_tree(src);
  println!("\n{src}\n");
  let mut out = String::new();
  dump(&tree, 0, &mut out);
  print!("{out}");

  // One typed traversal: the outermost BinExpr and its operator token; then a node count.
  let outer = tree
    .children()
    .find(|n| n.kind() == SyntaxKind::BinExpr)
    .expect("a top-level binary expression");
  let op = outer
    .children_with_tokens()
    .filter_map(|el| el.into_token())
    .find(|t| {
      matches!(
        t.kind(),
        SyntaxKind::Plus
          | SyntaxKind::Minus
          | SyntaxKind::Star
          | SyntaxKind::Slash
          | SyntaxKind::Caret
      )
    })
    .expect("the operator token");
  let bins = tree
    .descendants()
    .filter(|n| n.kind() == SyntaxKind::BinExpr)
    .count();
  println!(
    "\noutermost operator: {:?};  {bins} BinExpr node(s)",
    op.text()
  );

  assert_eq!(op.text(), "+");
  assert_eq!(bins, 2); // (1 + …) and (2 * 3)
  println!("All assertions passed.");
}

fn is_expr_node(n: &SyntaxNode<CalcLang>) -> bool {
  matches!(n.kind(), SyntaxKind::BinExpr | SyntaxKind::PrefixExpr)
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
