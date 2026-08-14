//! The lossless-CST twin of `c_expression.rs`.
//!
//! Same C-style expression grammar as its twin, built into a **lossless** rowan tree instead
//! of an [`Expr`] AST. Like the twin it uses the high-level [`pratt`](tokora::parser::pratt)
//! driver — here with its CST seam, [`with_cst_kinds`](tokora::parser::Pratt::with_cst_kinds):
//! each fold's operator picks the node kind that wraps the folded region. The interesting part
//! is the **postfix folds that consume tokens**: `e[i]`, `f(args)`, and `e ? t : f` each parse
//! extra tokens inside the fold, and the driver still wraps the whole region — the wrap runs
//! *after* the fold — so a call materializes as `CallExpr[f ( arg , arg )]`.
//!
//! No AST or value is produced; the tree is the product, and folds carry the unit type.
//!
//! The payoff, shown in `main`: every twin input round-trips; then a tree dump and one typed
//! traversal over `f(a + b, c * d)`.
//!
//! Run: `cargo run --example c_expression_cst --features "std,logos,rowan"`

use rowan::{Language, NodeOrToken, SyntaxNode};
use tokora::{
  Emitter, InputRef, ParseContext, ParseInput, Token as TokenT,
  cache::DefaultCache,
  cst::{CstProfile, KindValidator, parse_lossless},
  emitter::{CstEmitter, Fatal},
  error::token::UnexpectedTokenOf,
  logos::{self, Logos},
  parser::{PrattFoldOp, PrattInfix, PrattLHS, PrattPower, PrattRHS, Precedenced, pratt},
};

// ── Lossless lexer ──────────────────────────────────────────────────────────────
//
// No `skip` rule: whitespace and `//` line comments are real trivia tokens. Tokens are
// fieldless — the tree keeps each token's text, so there is no number or identifier body to
// parse out. Multi-character operators precede their single-char prefixes for longest-match.

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
  #[regex(r"//[^\n]*", allow_greedy = true)]
  Comment,

  #[regex(r"[0-9]+")]
  Num,
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Ident,

  // Multi-character operators (before single-char variants).
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

  // Single-character operators.
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

  // Delimiters.
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  #[token("[")]
  LBracket,
  #[token("]")]
  RBracket,
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Token::Whitespace => "whitespace",
      Token::Comment => "comment",
      Token::Num => "number",
      Token::Ident => "identifier",
      Token::PlusPlus => "++",
      Token::MinusMinus => "--",
      Token::EqEq => "==",
      Token::BangEq => "!=",
      Token::LtEq => "<=",
      Token::GtEq => ">=",
      Token::AmpAmp => "&&",
      Token::PipePipe => "||",
      Token::Shl => "<<",
      Token::Shr => ">>",
      Token::Plus => "+",
      Token::Minus => "-",
      Token::Star => "*",
      Token::Slash => "/",
      Token::Percent => "%",
      Token::Amp => "&",
      Token::Pipe => "|",
      Token::Caret => "^",
      Token::Tilde => "~",
      Token::Bang => "!",
      Token::Question => "?",
      Token::Colon => ":",
      Token::Lt => "<",
      Token::Gt => ">",
      Token::Comma => ",",
      Token::LParen => "(",
      Token::RParen => ")",
      Token::LBracket => "[",
      Token::RBracket => "]",
    })
  }
}

impl TokenT<'_> for Token {
  type Kind = Token;
  type Error = LexError;

  const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;

  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> Token {
    *self
  }
  fn is_trivia(&self) -> bool {
    matches!(self, Token::Whitespace | Token::Comment)
  }
}

type CExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Error ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum CExprError {
  Lex,
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for CExprError {
  fn from(_: LexError) -> Self {
    CExprError::Lex
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, CExprLexer<'inp>>> for CExprError {
  fn from(_: UnexpectedTokenOf<'inp, CExprLexer<'inp>>) -> Self {
    CExprError::UnexpectedToken
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

// ── Unified syntax-kind space ─────────────────────────────────────────────────────
//
// The token-image section mirrors the `Token` enum's declaration order; `map_token` bridges
// the two 1:1, and the `KINDS` array is the inverse `kind_from_raw`.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum SyntaxKind {
  // Token images.
  Whitespace,
  Comment,
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
  // Node kinds.
  PrefixExpr,
  BinExpr,
  PostExpr,
  IndexExpr,
  CallExpr,
  TernaryExpr,
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
    Token::Comment => K::Comment,
    Token::Num => K::Num,
    Token::Ident => K::Ident,
    Token::PlusPlus => K::PlusPlus,
    Token::MinusMinus => K::MinusMinus,
    Token::EqEq => K::EqEq,
    Token::BangEq => K::BangEq,
    Token::LtEq => K::LtEq,
    Token::GtEq => K::GtEq,
    Token::AmpAmp => K::AmpAmp,
    Token::PipePipe => K::PipePipe,
    Token::Shl => K::Shl,
    Token::Shr => K::Shr,
    Token::Plus => K::Plus,
    Token::Minus => K::Minus,
    Token::Star => K::Star,
    Token::Slash => K::Slash,
    Token::Percent => K::Percent,
    Token::Amp => K::Amp,
    Token::Pipe => K::Pipe,
    Token::Caret => K::Caret,
    Token::Tilde => K::Tilde,
    Token::Bang => K::Bang,
    Token::Question => K::Question,
    Token::Colon => K::Colon,
    Token::Lt => K::Lt,
    Token::Gt => K::Gt,
    Token::Comma => K::Comma,
    Token::LParen => K::LParen,
    Token::RParen => K::RParen,
    Token::LBracket => K::LBracket,
    Token::RBracket => K::RBracket,
  }) as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CExprLang {}

impl Language for CExprLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    const KINDS: [SyntaxKind; 42] = [
      K::Whitespace,
      K::Comment,
      K::Num,
      K::Ident,
      K::PlusPlus,
      K::MinusMinus,
      K::EqEq,
      K::BangEq,
      K::LtEq,
      K::GtEq,
      K::AmpAmp,
      K::PipePipe,
      K::Shl,
      K::Shr,
      K::Plus,
      K::Minus,
      K::Star,
      K::Slash,
      K::Percent,
      K::Amp,
      K::Pipe,
      K::Caret,
      K::Tilde,
      K::Bang,
      K::Question,
      K::Colon,
      K::Lt,
      K::Gt,
      K::Comma,
      K::LParen,
      K::RParen,
      K::LBracket,
      K::RBracket,
      K::PrefixExpr,
      K::BinExpr,
      K::PostExpr,
      K::IndexExpr,
      K::CallExpr,
      K::TernaryExpr,
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

const PREC_TERNARY: Power = Power(2); // ?:
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
const PREC_PREFIX: Power = Power(13); // unary prefix
const PREC_POSTFIX: Power = Power(14); // postfix ++ -- [] ()

/// The postfix tag the driver passes to `fold_postfix` and the CST classifier. Its `Index`,
/// `Call`, and `Ternary` variants tell the fold to consume the tokens that follow.
#[derive(Debug, Clone, Copy)]
enum PostfixOp {
  Inc,
  Dec,
  Index,
  Call,
  Ternary,
}

// ── Pratt parse functions ─────────────────────────────────────────────────────────
//
// `O = ()`: no value is built. Prefix and infix operator payloads are `()` too — the operator
// token is committed into the wrapping node, so the tree already records which operator it
// was. Only the postfix tag is carried, because the fold behavior and node kind differ by it.

type CExprIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, CExprLexer<'inp>, Ctx>;

fn parse_lhs<'inp, Ctx>(
  inp: &mut CExprIn<'inp, '_, Ctx>,
) -> Result<PrattLHS<(), (), Power>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CExprLexer<'inp>> + Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    None => Err(CExprError::UnexpectedEot),
    Some(tok) => match tok.into_data() {
      Token::Num | Token::Ident => Ok(PrattLHS::Operand(())),
      // Grouping: `(` expr `)` — parens land as bare tokens around the inner expression.
      Token::LParen => {
        parse_expr(inp)?;
        expect(inp, Token::RParen)?;
        Ok(PrattLHS::Operand(()))
      }
      // Prefix operators.
      Token::Minus
      | Token::Plus
      | Token::Bang
      | Token::Tilde
      | Token::PlusPlus
      | Token::MinusMinus => Ok(PrattLHS::Prefix(Precedenced::new((), PREC_PREFIX))),
      _ => Err(CExprError::UnexpectedToken),
    },
  }
}

fn parse_rhs<'inp, Ctx>(
  inp: &mut CExprIn<'inp, '_, Ctx>,
) -> Result<PrattRHS<(), (), (), PostfixOp, Power>, CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  macro_rules! infix_l {
    ($prec:expr) => {
      PrattRHS::Infix(Precedenced::new(PrattInfix::Left(()), $prec))
    };
  }
  inp.skip_while(|t| t.is_trivia())?;
  match inp.next()? {
    None => Ok(PrattRHS::End),
    Some(tok) => Ok(match tok.into_data() {
      // Infix (all left-associative).
      Token::PipePipe => infix_l!(PREC_OR),
      Token::AmpAmp => infix_l!(PREC_AND),
      Token::Pipe => infix_l!(PREC_BOR),
      Token::Caret => infix_l!(PREC_BXOR),
      Token::Amp => infix_l!(PREC_BAND),
      Token::EqEq | Token::BangEq => infix_l!(PREC_EQ),
      Token::Lt | Token::Gt | Token::LtEq | Token::GtEq => infix_l!(PREC_CMP),
      Token::Shl | Token::Shr => infix_l!(PREC_SHIFT),
      Token::Plus | Token::Minus => infix_l!(PREC_ADD),
      Token::Star | Token::Slash | Token::Percent => infix_l!(PREC_MUL),
      // Postfix.
      Token::PlusPlus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Inc, PREC_POSTFIX)),
      Token::MinusMinus => PrattRHS::Postfix(Precedenced::new(PostfixOp::Dec, PREC_POSTFIX)),
      Token::LBracket => PrattRHS::Postfix(Precedenced::new(PostfixOp::Index, PREC_POSTFIX)),
      Token::LParen => PrattRHS::Postfix(Precedenced::new(PostfixOp::Call, PREC_POSTFIX)),
      Token::Question => PrattRHS::Postfix(Precedenced::new(PostfixOp::Ternary, PREC_TERNARY)),
      // Not an operator: end the expression; the driver rolls the token back.
      _ => PrattRHS::End,
    }),
  }
}

fn fold_prefix<'inp, Ctx>(
  _inp: &mut CExprIn<'inp, '_, Ctx>,
  _operand: (),
  _op: Precedenced<(), Power>,
) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  Ok(())
}

fn fold_infix<'inp, Ctx>(
  _inp: &mut CExprIn<'inp, '_, Ctx>,
  _left: (),
  _right: (),
  _op: Precedenced<PrattInfix<(), (), ()>, Power>,
) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  Ok(())
}

/// The postfix fold. For `[i]`, `(args)`, and `? t : f` it consumes the tokens that follow
/// the trigger the driver already read — and the driver wraps the whole region afterward.
fn fold_postfix<'inp, Ctx>(
  inp: &mut CExprIn<'inp, '_, Ctx>,
  _operand: (),
  op: Precedenced<PostfixOp, Power>,
) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CExprLexer<'inp>> + Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  match op.into_data() {
    PostfixOp::Inc | PostfixOp::Dec => Ok(()),

    // e[i]
    PostfixOp::Index => {
      parse_expr(inp)?;
      expect(inp, Token::RBracket)
    }

    // e(arg, arg, ...)
    PostfixOp::Call => {
      inp.skip_while(|t| t.is_trivia())?;
      if inp
        .try_expect(|t| matches!(t.data().kind(), Token::RParen))?
        .is_some()
      {
        return Ok(()); // empty call: f()
      }
      parse_expr(inp)?; // first argument
      loop {
        inp.skip_while(|t| t.is_trivia())?;
        if inp
          .try_expect(|t| matches!(t.data().kind(), Token::RParen))?
          .is_some()
        {
          return Ok(());
        }
        if inp
          .try_expect(|t| matches!(t.data().kind(), Token::Comma))?
          .is_none()
        {
          return Err(CExprError::UnexpectedToken);
        }
        parse_expr(inp)?; // subsequent argument
      }
    }

    // c ? t : f
    PostfixOp::Ternary => {
      parse_expr(inp)?; // then-branch (stops before `:`)
      expect(inp, Token::Colon)?;
      parse_expr(inp) // else-branch
    }
  }
}

/// The CST seam: prefix and infix map to one kind each; the postfix tag fans out to four.
fn cexpr_kinds(op: PrattFoldOp<'_, (), (), (), (), PostfixOp>) -> Option<u16> {
  match op {
    PrattFoldOp::Prefix(_) => Some(K::PrefixExpr.raw()),
    PrattFoldOp::Infix(_) => Some(K::BinExpr.raw()),
    PrattFoldOp::Postfix(p) => Some(match p {
      PostfixOp::Inc | PostfixOp::Dec => K::PostExpr.raw(),
      PostfixOp::Index => K::IndexExpr.raw(),
      PostfixOp::Call => K::CallExpr.raw(),
      PostfixOp::Ternary => K::TernaryExpr.raw(),
    }),
  }
}

/// Skips leading trivia, then consumes exactly `want`.
fn expect<'inp, Ctx>(inp: &mut CExprIn<'inp, '_, Ctx>, want: Token) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.try_expect(|t| t.data().kind() == want)? {
    Some(_) => Ok(()),
    None => Err(CExprError::UnexpectedToken),
  }
}

/// One full expression through the typed Pratt driver, with the CST seam configured.
fn parse_expr<'inp, Ctx>(inp: &mut CExprIn<'inp, '_, Ctx>) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CExprLexer<'inp>> + Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  pratt(parse_lhs, parse_rhs, fold_prefix, fold_infix, fold_postfix)
    .with_cst_kinds(cexpr_kinds)
    .parse_input(inp)
}

/// Top-level: leading trivia, one expression, trailing trivia — every byte committed.
fn program<'inp, Ctx>(inp: &mut CExprIn<'inp, '_, Ctx>) -> Result<(), CExprError>
where
  Ctx: ParseContext<'inp, CExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, CExprLexer<'inp>> + Emitter<'inp, CExprLexer<'inp>, Error = CExprError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  parse_expr(inp)?;
  inp.skip_while(|t| t.is_trivia())?;
  Ok(())
}

// ── Tree building + dump ────────────────────────────────────────────────────────

fn parse_to_tree(src: &str) -> SyntaxNode<CExprLang> {
  // The dialect's kind space, stated once: the mapper, the predicate that decides which raw
  // u16s this language can name (every discriminant up to and including `Root`), and the two
  // kinds the sink synthesizes on its own behalf.
  let profile = CstProfile::new(
    map_token,
    KindValidator::new(|kind| kind <= K::Root.raw()),
    K::Error.raw(),
    K::Gap.raw(),
  );

  // `parse_lossless` mints the sink from `src` itself: the buffer the parse reads and the
  // buffer the tree's text is sliced out of are the same argument of the same call. The second
  // argument is the lexer's `State`: `LogosLexer` inherits logos' `Extras`, which this grammar
  // leaves at `()`. Spell the unit literal rather than `Default::default()` — the latter is a
  // `clippy::unit_arg` and reads as though a state were being configured.
  let (cst, parsed) = parse_lossless(
    src,
    (),
    Fatal::<CExprError>::new(),
    profile,
    DefaultCache::<CExprLexer<'_>>::default(),
    program,
  );
  parsed.expect("parse succeeds");
  let (green, _emitter) = cst.finish(K::Root.raw());
  SyntaxNode::<CExprLang>::new_root(green.expect("well-formed tree"))
}

fn dump(node: &SyntaxNode<CExprLang>, depth: usize, out: &mut String) {
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
    "a + b + c",
    "-a",
    "!flag",
    "~bits",
    "++x",
    "x++",
    "x++ + ++y",
    "a ? b : c",
    "a ? b : c ? d : e",
    "arr[0]",
    "f()",
    "f(1, 2)",
    "a == b && c != d",
    "~bits | flags",
    "arr[i + 1]",
    "f(a + b, c * d)",
    "x << 2 | y >> 1",
  ];
  for src in cases {
    let tree = parse_to_tree(src);
    assert_eq!(tree.text().to_string(), src, "round trip for `{src}`");
  }
  println!("round-trip ok for all {} inputs  ✓", cases.len());

  // A detailed look at one input: a call whose two arguments are themselves binary nodes.
  let src = "f(a + b, c * d)";
  let tree = parse_to_tree(src);
  println!("\n{src}\n");
  let mut out = String::new();
  dump(&tree, 0, &mut out);
  print!("{out}");

  // One typed traversal: locate the call, confirm its kind, and count the binary args.
  let call = tree
    .descendants()
    .find(|n| n.kind() == SyntaxKind::CallExpr)
    .expect("a call expression");
  let bin_args = call
    .children()
    .filter(|n| n.kind() == SyntaxKind::BinExpr)
    .count();
  println!(
    "\ncall node text: {:?};  {bin_args} binary argument(s)",
    call.text().to_string()
  );

  assert_eq!(call.text().to_string(), "f(a + b, c * d)");
  assert_eq!(bin_args, 2); // (a + b) and (c * d)
  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
