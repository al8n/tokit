//! The lossless-CST twin of `json.rs`.
//!
//! The twin parses JSON to a [`JsonValue`] AST with high-level combinators (`separated_by`,
//! `delimited`, `peek_then_choice`). This twin parses the same grammar into a **lossless**
//! rowan concrete syntax tree through a [`cst::Sink`](tokora::cst::Sink), using the
//! [`node`](tokora::parser::node) combinator to declare structure — a clean recursive-descent
//! shape, chosen over reproducing the twin's container combinators because an example teaches
//! best when its structure is on the surface. Every byte, whitespace included, survives.
//!
//! Instead of the twin's 107 KB `sample.json` (whose tree dump would be unreadable) this uses
//! a small inline document, so the printed tree fits on a screen.
//!
//! The payoff, shown in `main`: `tree.text() == source`, an indented tree dump, and one typed
//! traversal that reads the top object's members.
//!
//! Run: `cargo run --example json_cst --features "std,logos,rowan"`

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
// No `skip` rule: whitespace is a real trivia token. Tokens are fieldless — the tree keeps
// each token's source text, so there is no number or string body to parse out here.

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
  #[regex(r"[ \t\r\n\f]+")]
  Whitespace,

  #[token("true")]
  True,
  #[token("false")]
  False,
  #[token("null")]
  Null,
  #[regex(r"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?")]
  Number,
  #[regex(r#""([^"\\\x00-\x1F]|\\(["\\bnfrt/]|u[a-fA-F0-9]{4}))*""#)]
  String,

  #[token("{")]
  LBrace,
  #[token("}")]
  RBrace,
  #[token("[")]
  LBracket,
  #[token("]")]
  RBracket,
  #[token(":")]
  Colon,
  #[token(",")]
  Comma,
}

impl core::fmt::Display for Token {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    f.write_str(match self {
      Token::Whitespace => "whitespace",
      Token::True => "true",
      Token::False => "false",
      Token::Null => "null",
      Token::Number => "number",
      Token::String => "string",
      Token::LBrace => "{",
      Token::RBrace => "}",
      Token::LBracket => "[",
      Token::RBracket => "]",
      Token::Colon => ":",
      Token::Comma => ",",
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
    matches!(self, Token::Whitespace)
  }
}

type JsonLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Error ───────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum JsonError {
  Lex,
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for JsonError {
  fn from(_: LexError) -> Self {
    JsonError::Lex
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, JsonLexer<'inp>>> for JsonError {
  fn from(_: UnexpectedTokenOf<'inp, JsonLexer<'inp>>) -> Self {
    JsonError::UnexpectedToken
  }
}

// ── Unified syntax-kind space ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u16)]
enum SyntaxKind {
  // Token images.
  Whitespace,
  True,
  False,
  Null,
  Number,
  String,
  LBrace,
  RBrace,
  LBracket,
  RBracket,
  Colon,
  Comma,
  // Node kinds.
  Object,
  Member,
  Array,
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
    Token::True => K::True,
    Token::False => K::False,
    Token::Null => K::Null,
    Token::Number => K::Number,
    Token::String => K::String,
    Token::LBrace => K::LBrace,
    Token::RBrace => K::RBrace,
    Token::LBracket => K::LBracket,
    Token::RBracket => K::RBracket,
    Token::Colon => K::Colon,
    Token::Comma => K::Comma,
  }) as u16
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum JsonLang {}

impl Language for JsonLang {
  type Kind = SyntaxKind;

  fn kind_from_raw(raw: rowan::SyntaxKind) -> SyntaxKind {
    const KINDS: [SyntaxKind; 18] = [
      K::Whitespace,
      K::True,
      K::False,
      K::Null,
      K::Number,
      K::String,
      K::LBrace,
      K::RBrace,
      K::LBracket,
      K::RBracket,
      K::Colon,
      K::Comma,
      K::Object,
      K::Member,
      K::Array,
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

type JsonIn<'inp, 'x, Ctx> = InputRef<'inp, 'x, JsonLexer<'inp>, Ctx>;

/// Commits leading trivia, then reports the next token's kind without consuming it.
fn peek<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<Option<Token>, JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  let mut ahead = None;
  inp.try_expect(|t| {
    ahead = Some(t.data().kind());
    false
  })?;
  Ok(ahead)
}

/// Skips leading trivia, then consumes exactly `want`.
fn expect<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>, want: Token) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  inp.skip_while(|t| t.is_trivia())?;
  match inp.try_expect(|t| t.data().kind() == want)? {
    Some(_) => Ok(()),
    None => Err(JsonError::UnexpectedToken),
  }
}

/// `value := object | array | string | number | true | false | null` — scalars stay bare
/// tokens (no node); only the two container shapes and members open nodes.
fn value<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, JsonLexer<'inp>> + Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  match peek(inp)? {
    Some(Token::LBrace) => object(inp),
    Some(Token::LBracket) => array(inp),
    Some(Token::String | Token::Number | Token::True | Token::False | Token::Null) => {
      // A scalar: consume the one token the peek already settled trivia in front of.
      match inp.try_expect(|_| true)? {
        Some(_) => Ok(()),
        None => Err(JsonError::UnexpectedEot),
      }
    }
    _ => Err(JsonError::UnexpectedToken),
  }
}

/// `object := "{" (member ("," member)*)? "}"` — one `node()` bracket over the braces, the
/// commas, the trivia, and every member.
fn object<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, JsonLexer<'inp>> + Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  node(K::Object.raw(), |inp: &mut JsonIn<'inp, '_, Ctx>| {
    expect(inp, Token::LBrace)?;
    if let Some(Token::RBrace) = peek(inp)? {
      return expect(inp, Token::RBrace); // empty object
    }
    loop {
      member(inp)?;
      match peek(inp)? {
        Some(Token::Comma) => expect(inp, Token::Comma)?,
        Some(Token::RBrace) => return expect(inp, Token::RBrace),
        _ => return Err(JsonError::UnexpectedToken),
      }
    }
  })
  .parse_input(inp)
}

/// `member := string ":" value` — `Member[String, Colon, value]`, plus whatever trivia was
/// crossed along the way.
fn member<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, JsonLexer<'inp>> + Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  node(K::Member.raw(), |inp: &mut JsonIn<'inp, '_, Ctx>| {
    expect(inp, Token::String)?;
    expect(inp, Token::Colon)?;
    value(inp)
  })
  .parse_input(inp)
}

/// `array := "[" (value ("," value)*)? "]"`.
fn array<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, JsonLexer<'inp>> + Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  node(K::Array.raw(), |inp: &mut JsonIn<'inp, '_, Ctx>| {
    expect(inp, Token::LBracket)?;
    if let Some(Token::RBracket) = peek(inp)? {
      return expect(inp, Token::RBracket); // empty array
    }
    loop {
      value(inp)?;
      match peek(inp)? {
        Some(Token::Comma) => expect(inp, Token::Comma)?,
        Some(Token::RBracket) => return expect(inp, Token::RBracket),
        _ => return Err(JsonError::UnexpectedToken),
      }
    }
  })
  .parse_input(inp)
}

/// The top-level driver: one value, then any trailing trivia (kept, so the round trip holds).
fn document<'inp, Ctx>(inp: &mut JsonIn<'inp, '_, Ctx>) -> Result<(), JsonError>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter:
    CstEmitter<'inp, JsonLexer<'inp>> + Emitter<'inp, JsonLexer<'inp>, Error = JsonError>,
{
  value(inp)?;
  inp.skip_while(|t| t.is_trivia())?;
  Ok(())
}

// ── Tree dump ─────────────────────────────────────────────────────────────────────

fn dump(node: &SyntaxNode<JsonLang>, depth: usize, out: &mut String) {
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
  let src = r#"{"name": "tokit", "nums": [1, 2], "meta": {"ok": true, "note": null}}"#;

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
    Fatal::<JsonError>::new(),
    profile,
    DefaultCache::<JsonLexer<'_>>::default(),
    document,
  );
  parsed.expect("parse succeeds");

  let (green, _emitter) = cst.finish(K::Root.raw());
  let tree = SyntaxNode::<JsonLang>::new_root(green.expect("well-formed tree"));

  assert_eq!(tree.text().to_string(), src, "lossless round trip");
  println!("round-trip: tree.text() == source  ✓\n");

  let mut out = String::new();
  dump(&tree, 0, &mut out);
  print!("{out}");

  // One typed traversal: the top object's direct members, and each member's key.
  let obj = tree
    .children()
    .find(|n| n.kind() == SyntaxKind::Object)
    .expect("a top-level object");
  let keys: Vec<String> = obj
    .children()
    .filter(|n| n.kind() == SyntaxKind::Member)
    .filter_map(|m| {
      m.children_with_tokens()
        .filter_map(|el| el.into_token())
        .find(|t| t.kind() == SyntaxKind::String)
        .map(|t| t.text().to_string())
    })
    .collect();
  let total_members = tree
    .descendants()
    .filter(|n| n.kind() == SyntaxKind::Member)
    .count();
  println!("\ntop-level object keys: {keys:?}");
  println!("member nodes in the whole tree: {total_members}");

  assert_eq!(keys, ["\"name\"", "\"nums\"", "\"meta\""]);
  assert_eq!(total_members, 5); // 3 at top level + 2 in "meta"
  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
