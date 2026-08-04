//! The lossless-CST twin of `s_expression.rs`.
//!
//! Same Lisp grammar as its twin, but instead of reducing to an [`Atom`] value it builds a
//! **lossless** rowan concrete syntax tree through a [`cst::Sink`](tokora::cst::Sink) — every
//! byte of the source, whitespace and `;` comments included, survives into the tree. This is
//! the canonical [`node`](tokora::parser::node) pattern from the guide's chapter 16: a
//! recursive-descent parser whose structure is declared by wrapping sub-parses in `node(...)`
//! brackets; committed tokens (and trivia) flow to the sink on their own.
//!
//! The payoff, shown in `main`:
//!
//! - the tree round-trips: `tree.text() == source`, exactly;
//! - the tree structure is printed as an indented dump;
//! - one typed traversal walks the outer list and reads its head atom.
//!
//! Run: `cargo run --example s_expression_cst --features "std,logos,rowan"`

use rowan::{Language, NodeOrToken, SyntaxNode};
use tokora::{
  Emitter, InputRef, ParseContext, ParseInput, Token as TokenT,
  cache::DefaultCache,
  cst::{CstProfile, KindValidator, parse_lossless},
  emitter::{CstEmitter, Fatal},
  error::token::UnexpectedTokenOf,
  logos::{self, Logos},
  parser::node,
};

// ── Lossless lexer ──────────────────────────────────────────────────────────────
//
// Unlike the twin's lexer, this one has NO `skip` rule: whitespace and comments are real
// tokens, marked as trivia. `SURFACES_TRIVIA = true` is the compile-time promise the sink
// requires — a trivia-skipping lexer would leave gaps indistinguishable from dropped tokens.
// The tokens are fieldless: the tree keeps each token's source text, so there is nothing to
// parse out of a number or keyword.

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
  #[regex(r";[^\n]*", allow_greedy = true)]
  Comment,

  #[regex(r"-?[0-9]+")]
  Int,
  #[token("#t")]
  True,
  #[token("#f")]
  False,
  #[regex(r":[a-zA-Z_][a-zA-Z0-9_]*")]
  Keyword,

  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("=")]
  Equal,
  #[token("not")]
  Not,
  #[token("if")]
  If,

  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  #[token("'")]
  Quote,
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Token::Whitespace => "whitespace",
      Token::Comment => "comment",
      Token::Int => "integer",
      Token::True => "#t",
      Token::False => "#f",
      Token::Keyword => "keyword",
      Token::Plus => "+",
      Token::Minus => "-",
      Token::Star => "*",
      Token::Slash => "/",
      Token::Equal => "=",
      Token::Not => "not",
      Token::If => "if",
      Token::LParen => "(",
      Token::RParen => ")",
      Token::Quote => "'",
    })
  }
}

impl TokenT<'_> for Token {
  type Kind = Token;
  type Error = LexError;

  // The sink refuses a trivia-skipping lexer at compile time; this is the opt-in.
  const SURFACES_TRIVIA: bool = true;

  fn kind(&self) -> Token {
    *self
  }
  fn is_trivia(&self) -> bool {
    matches!(self, Token::Whitespace | Token::Comment)
  }
}

type SExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Error ───────────────────────────────────────────────────────────────────────
//
// The two `From` impls are what let `Fatal` wrap this error: `FromEmitterError` is blanket-
// implemented for any error convertible from both a lexer error and an unexpected token.

#[derive(Debug)]
enum SExprError {
  Lex,
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for SExprError {
  fn from(_: LexError) -> Self {
    SExprError::Lex
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, SExprLexer<'inp>>> for SExprError {
  fn from(_: UnexpectedTokenOf<'inp, SExprLexer<'inp>>) -> Self {
    SExprError::UnexpectedToken
  }
}

// ── Unified syntax-kind space ─────────────────────────────────────────────────────
//
// One `#[repr(u16)]` enum holds token images, node kinds, and the three bookkeeping kinds.
// Committed tokens enter the tree only through `map_token` below; node kinds are declared by
// the `node(...)` calls in the grammar.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum SyntaxKind {
  // Token images.
  Whitespace,
  Comment,
  Int,
  True,
  False,
  Keyword,
  Plus,
  Minus,
  Star,
  Slash,
  Equal,
  Not,
  If,
  LParen,
  RParen,
  Quote,
  // Node kinds.
  Atom,
  List,
  Quoted,
  // Bookkeeping: recovery holes, materialization gap tiles, the synthetic root.
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

/// One compiler-exhaustive match from lexer token to unified kind.
fn map_token(tok: &Token) -> u16 {
  (match tok {
    Token::Whitespace => K::Whitespace,
    Token::Comment => K::Comment,
    Token::Int => K::Int,
    Token::True => K::True,
    Token::False => K::False,
    Token::Keyword => K::Keyword,
    Token::Plus => K::Plus,
    Token::Minus => K::Minus,
    Token::Star => K::Star,
    Token::Slash => K::Slash,
    Token::Equal => K::Equal,
    Token::Not => K::Not,
    Token::If => K::If,
    Token::LParen => K::LParen,
    Token::RParen => K::RParen,
    Token::Quote => K::Quote,
  }) as u16
}

/// Rowan's side of the bargain: raw ↔ typed kind conversion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum SExprLang {}

impl Language for SExprLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    // `#[repr(u16)]` with default discriminants: raw value == declaration index.
    const KINDS: [SyntaxKind; 22] = [
      K::Whitespace,
      K::Comment,
      K::Int,
      K::True,
      K::False,
      K::Keyword,
      K::Plus,
      K::Minus,
      K::Star,
      K::Slash,
      K::Equal,
      K::Not,
      K::If,
      K::LParen,
      K::RParen,
      K::Quote,
      K::Atom,
      K::List,
      K::Quoted,
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

// ── Grammar: structure declared by `node(...)` ───────────────────────────────────
//
// Every function that declares tree structure bounds its emitter as `CstEmitter`; helpers
// that merely consume keep the plain `Emitter` bound. The same functions would run over a
// tree-less `Fatal` emitter at zero cost — the tree is a side effect of the emitter choice.

type SExprIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, SExprLexer<'inp>, Ctx>;

/// Commits any leading trivia, then reports the next token's kind without consuming it.
/// Committing trivia during a peek is safe over a lossless stream: it belongs to the tree
/// no matter which branch wins.
fn peek<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<Option<Token>, SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  let mut ahead = None;
  inp.try_expect(|t| {
    ahead = Some(t.data().kind());
    false
  })?;
  Ok(ahead)
}

/// Skips leading trivia, then consumes exactly `want` (error if it is not next).
fn expect<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>, want: Token) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.try_expect(|t| t.data().kind() == want)? {
    Some(_) => Ok(()),
    None => Err(SExprError::UnexpectedToken),
  }
}

/// `expr := atom | list | quoted` — dispatch by peek.
fn expr<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, SExprLexer<'inp>> + Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  match peek(inp)? {
    Some(Token::LParen) => list(inp),
    Some(Token::Quote) => quoted(inp),
    Some(Token::RParen) | None => Err(SExprError::UnexpectedEot),
    Some(_) => atom(inp),
  }
}

/// `atom := INT | #t | #f | KEYWORD | + | - | * | / | = | not | if` — one significant token
/// wrapped in an `Atom` node. Leading trivia was already committed by the caller's `peek`, so
/// it lands in the enclosing node, not inside the atom.
fn atom<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, SExprLexer<'inp>> + Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  node(
    K::Atom.raw(),
    |inp: &mut SExprIn<'inp, '_, Ctx>| match inp.try_expect(|t| {
      matches!(
        t.data().kind(),
        Token::Int
          | Token::True
          | Token::False
          | Token::Keyword
          | Token::Plus
          | Token::Minus
          | Token::Star
          | Token::Slash
          | Token::Equal
          | Token::Not
          | Token::If
      )
    })? {
      Some(_) => Ok(()),
      None => Err(SExprError::UnexpectedToken),
    },
  )
  .parse_input(inp)
}

/// `list := "(" expr* ")"` — one `node()` bracket over the whole shape: the parens, the
/// trivia between elements, and every child expression land inside the `List` node.
fn list<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, SExprLexer<'inp>> + Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  node(K::List.raw(), |inp: &mut SExprIn<'inp, '_, Ctx>| {
    expect(inp, Token::LParen)?;
    loop {
      match peek(inp)? {
        Some(Token::RParen) => return expect(inp, Token::RParen),
        None => return Err(SExprError::UnexpectedEot),
        Some(_) => expr(inp)?,
      }
    }
  })
  .parse_input(inp)
}

/// `quoted := "'" expr` — the quote mark and the quoted expression under a `Quoted` node.
fn quoted<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, SExprLexer<'inp>> + Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  node(K::Quoted.raw(), |inp: &mut SExprIn<'inp, '_, Ctx>| {
    expect(inp, Token::Quote)?;
    expr(inp)
  })
  .parse_input(inp)
}

/// The top-level driver: one expression, then any trailing trivia (so every byte is
/// committed and the round trip holds). Both commit at depth 0, landing under the synthetic
/// `Root` that `finish` wraps around the whole parse.
fn program<'inp, Ctx>(inp: &mut SExprIn<'inp, '_, Ctx>) -> Result<(), SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, SExprLexer<'inp>> + Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  expr(inp)?;
  inp.skip_while(|t| t.is_trivia())?;
  Ok(())
}

// ── Tree dump ─────────────────────────────────────────────────────────────────────

/// Renders the tree as an indented outline: node kinds on their own line, tokens with their
/// exact source text quoted beside them.
fn dump(node: &SyntaxNode<SExprLang>, depth: usize, out: &mut String) {
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
  // A comment and nested lists — none of it mentioned by a grammar rule, all of it kept.
  let src = "; double a number\n(* 2 (+ 3 4))";

  // The sink wraps a fail-fast `Fatal` emitter and takes the dialect corner: the token
  // mapper plus the error/gap bookkeeping kinds. It stays outside the parse in the context
  // seat, because materialization needs it back afterwards.
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
  // buffer the tree's text is sliced out of are the same argument of the same call.
  let (cst, parsed) = parse_lossless(
    src,
    Default::default(),
    Fatal::<SExprError>::new(),
    profile,
    DefaultCache::<SExprLexer<'_>>::default(),
    program,
  );
  parsed.expect("parse succeeds");

  // Materialize once. The sink is consumed; the inner emitter comes back with the tree.
  let (green, _emitter) = cst.finish(K::Root.raw());
  let tree = SyntaxNode::<SExprLang>::new_root(green.expect("well-formed tree"));

  // The round-trip law — the whole reason to build a CST.
  assert_eq!(tree.text().to_string(), src, "lossless round trip");
  println!("round-trip: tree.text() == source  ✓\n");

  let mut out = String::new();
  dump(&tree, 0, &mut out);
  print!("{out}");

  // One typed traversal: find the outer list and read its head atom (the operator).
  let outer_list = tree
    .children()
    .find(|n| n.kind() == SyntaxKind::List)
    .expect("a top-level list");
  let head = outer_list
    .children()
    .find(|n| n.kind() == SyntaxKind::Atom)
    .expect("the list's head atom");
  let atoms = tree
    .descendants()
    .filter(|n| n.kind() == SyntaxKind::Atom)
    .count();
  println!(
    "\nouter list head atom: {:?};  {atoms} atom node(s) in the whole tree",
    head.text().to_string()
  );

  assert_eq!(head.text().to_string(), "*");
  assert_eq!(atoms, 5); // *, 2, +, 3, 4
  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
