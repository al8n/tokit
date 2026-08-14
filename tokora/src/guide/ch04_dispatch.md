# 4. Deterministic choice

A Calc statement starts with `let`, `print`, or an integer. Choosing between alternatives
is where many combinator libraries reach for speculative "try each in order" choice; tokora
deliberately does not. Its choice shapes are **deterministic**: look at the next token's
[`Kind`](crate::Token::Kind) once, decide, and run exactly one branch. No branch is ever
half-run and unwound, so a dispatch failure is *committed* — the error cannot be lost to
backtracking, and its expected set is exact.

Three surfaces, one decision rule:

- [`peek_then_choice`](crate::ParseChoice::peek_then_choice) — you write the decision
  handler yourself over a peek window (any fan-in, your own failure diagnostic);
- [`dispatch_on_kind`](crate::ParseChoice::dispatch_on_kind) — the decision **is a static
  table**: `table[i]` is the viable first-token kind for branch `i`. On a miss, the
  emitted [`UnexpectedToken`](crate::error::token::UnexpectedToken) carries the whole
  table as an `expected one of …` set ([`Expected::OneOf`](crate::utils::Expected)); at
  end of input it is [`UnexpectedEnd`](crate::error::UnexpectedEnd) instead. Use
  `peek_then_choice` when several kinds route to one branch; the table form is one kind
  per branch;
- [`select!`](crate::select) — the table is written **beside the patterns**, one arm per
  kind, and the classified head moves into its arm *by value*. Its runtime is
  [`dispatch_take`](crate::parser::dispatch_take), and the declining twin
  [`try_select!`](crate::try_select) /
  [`try_dispatch_take`](crate::parser::try_dispatch_take) is the same shape that returns
  [`Decline`](crate::try_parse_input::ParseAttempt::Decline) on a head outside the table
  instead of committing.

## Peeked versus fused

[`DispatchOnKind`](crate::parser::DispatchOnKind) is the *peek* shape: the decision token
is peeked (staged in the token cache, including a lexer-state clone), the winning branch —
any [`ParseInput`](crate::ParseInput), with the token still on the input — consumes it back
out. [`FusedDispatchOnKind`](crate::parser::FusedDispatchOnKind), built by
[`fused_dispatch_on_kind`](crate::ParseTokenChoice::fused_dispatch_on_kind), is the
*lex-once* twin: the dispatcher consumes the head token as part of classifying it and hands
it to the winning arm (an `FnMut(head, inp)` — the [`ParseTokenChoice`](crate::ParseTokenChoice)
surface), skipping the cache round trip entirely. Failures are observationally identical;
only the hit path differs. **When each wins:** hot sum-type loops (a statement loop, a JSON
value loop) prefer the fused shape — the saved stage/unstage matters most when the lexer
state is expensive to clone — while branches that are self-contained `ParseInput` parsers,
reused elsewhere or wanting the head token left on the input, keep the peek shape. And per
the [dense-discriminant note](crate::parser::DispatchOnKind#performance-keep-token-kind-discriminants-dense),
keep your kind enum's discriminants dense (`0, 1, 2, …`) so kind matches beside the table
compile to jump tables.

The third shape, [`select!`](crate::select), is the fused one with the table moved next to
the patterns. Each arm is `kind => (span, pattern) => value`: the kinds *are* the table, so
it is written once instead of twice and cannot drift out of step with the arms; the head is
classified once against it, committed, and handed to the arm **moved**, so an arm binds the
payload (`Tok::Int(n)`) rather than re-matching a token it was already routed by. There is
no hand-written `unreachable!()` — an arm whose pattern is narrower than its kind hands the
token back and the runtime builds the same whole-table `UnexpectedToken` a miss would get.
One constraint the diagnostic does not name: the kind expressions must be
**const-promotable** (a unit-variant path or a `const`), because the expansion hands a
`&'static [Kind]` to `dispatch_take`; anything else fails at the invocation with `E0716`.
All three appear below, and the loop at the end asserts they agree.

```rust
# use tokora::{Token as TokenT, logos::{self, Logos}};
# #[derive(Clone, Debug, Default, PartialEq)]
# struct LexError;
# impl From<()> for LexError { fn from(_: ()) -> Self { LexError } }
# #[derive(Debug, Clone, PartialEq, Logos)]
# #[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
# enum Tok {
#   #[regex(r"[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
#   Int(i64),
#   #[token("let")] Let,
#   #[token("print")] Print,
#   #[regex(r"[A-Za-z_][A-Za-z0-9_]*")] Ident,
#   #[token("+")] Plus,
#   #[token("-")] Minus,
#   #[token("*")] Star,
#   #[token("/")] Slash,
#   #[token("^")] Caret,
#   #[token("=")] Assign,
#   #[token(";")] Semi,
#   #[token(",")] Comma,
#   #[token("(")] LParen,
#   #[token(")")] RParen,
# }
# #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
# enum TokKind { Int, Let, Print, Ident, Plus, Minus, Star, Slash, Caret, Assign, Semi, Comma, LParen, RParen }
# impl core::fmt::Display for TokKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     f.write_str(match self {
#       Self::Int => "integer", Self::Let => "`let`", Self::Print => "`print`",
#       Self::Ident => "identifier", Self::Plus => "`+`", Self::Minus => "`-`",
#       Self::Star => "`*`", Self::Slash => "`/`", Self::Caret => "`^`",
#       Self::Assign => "`=`", Self::Semi => "`;`", Self::Comma => "`,`",
#       Self::LParen => "`(`", Self::RParen => "`)`",
#     })
#   }
# }
# impl core::fmt::Display for Tok {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
#     match self {
#       Tok::Int(n) => write!(f, "{n}"),
#       other => core::fmt::Display::fmt(&other.kind(), f),
#     }
#   }
# }
# impl TokenT<'_> for Tok {
#   type Kind = TokKind;
#   type Error = LexError;
#   const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;
#   fn kind(&self) -> TokKind {
#     match self {
#       Tok::Int(_) => TokKind::Int, Tok::Let => TokKind::Let, Tok::Print => TokKind::Print,
#       Tok::Ident => TokKind::Ident, Tok::Plus => TokKind::Plus, Tok::Minus => TokKind::Minus,
#       Tok::Star => TokKind::Star, Tok::Slash => TokKind::Slash, Tok::Caret => TokKind::Caret,
#       Tok::Assign => TokKind::Assign, Tok::Semi => TokKind::Semi, Tok::Comma => TokKind::Comma,
#       Tok::LParen => TokKind::LParen, Tok::RParen => TokKind::RParen,
#     }
#   }
#   fn is_trivia(&self) -> bool { false }
# }
# type CalcLexer<'a> = tokora::lexer::LogosLexer<'a, Tok>;
# use tokora::error::{
#   UnexpectedEnd,
#   syntax::{FullContainer, MissingSyntax, TooFew},
#   token::{MissingToken, SeparatedError, UnexpectedToken},
# };
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<H, O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEnd<H, O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEnd<H, O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for CalcError {
#   fn from(_: MissingToken<'a, K, O, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for CalcError {
#   fn from(_: MissingSyntax<O, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for CalcError {
#   fn from(_: FullContainer<S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for CalcError {
#   fn from(_: TooFew<S, Lang>) -> Self { CalcError::Unexpected }
# }
use tokora::{
  ComposableParseContext, Emitter, InputRef, Parse, ParseChoice, ParseContext, ParseInput,
  ParseTokenChoice, Parser, SimpleSpan, span::Spanned,
};

/// Calc's statement AST (expressions stay integers until chapter 5).
#[derive(Debug, Clone, PartialEq)]
enum Stmt<'a> {
  Let(&'a str, i64),
  Print(Vec<i64>),
  Bare(i64),
}

# fn expect_int<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<i64, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   match inp.next()? {
#     Some(tok) => match tok.into_data() {
#       Tok::Int(n) => Ok(n),
#       _ => Err(CalcError::Unexpected),
#     },
#     None => Err(CalcError::UnexpectedEnd),
#   }
# }
# fn expect_tok<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
#   want: fn(&Tok) -> bool,
# ) -> Result<(), CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   if inp.try_expect(|t| want(t.data()))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   Ok(())
# }
// (Hidden here: `expect_int` and `expect_tok`, small helpers in chapter 2's style.)

// ── The three branch parsers, peek-shaped: the head token is still on the input. ──

fn stmt_let<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  expect_tok(inp, |t| matches!(t, Tok::Let))?;
  expect_tok(inp, |t| matches!(t, Tok::Ident))?;
  let name = inp.slice();
  expect_tok(inp, |t| matches!(t, Tok::Assign))?;
  let value = expect_int(inp)?;
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Let(name, value))
}

fn stmt_print<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  expect_tok(inp, |t| matches!(t, Tok::Print))?;
  let mut args = vec![expect_int(inp)?];
  while inp.try_expect(|t| matches!(t.data(), Tok::Comma))?.is_some() {
    args.push(expect_int(inp)?);
  }
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Print(args))
}

fn stmt_bare<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let value = expect_int(inp)?;
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Bare(value))
}

/// The peek-shaped dispatcher: `table[i]` names branch `i`'s first token.
fn parse_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  static TABLE: [TokKind; 3] = [TokKind::Let, TokKind::Print, TokKind::Int];
  (stmt_let, stmt_print, stmt_bare)
    .dispatch_on_kind(&TABLE)
    .parse_input(inp)
}

let stmt = Parser::new().apply(parse_stmt).parse_str("print 1 , 2 ;");
assert_eq!(stmt, Ok(Stmt::Print(vec![1, 2])));

// A committed dispatch failure: `;` is in no table slot, so the error carries
// the whole table as its expected set — `let`, `print`, or an integer.
assert_eq!(
  Parser::new().apply(parse_stmt).parse_str("; 1"),
  Err(CalcError::Unexpected)
);

// ── The fused twin: arms receive the already-lexed head token. ──

fn let_arm<'inp, Ctx>(
  _head: Spanned<Tok, SimpleSpan>, // the `let` keyword, already consumed
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  expect_tok(inp, |t| matches!(t, Tok::Ident))?;
  let name = inp.slice();
  expect_tok(inp, |t| matches!(t, Tok::Assign))?;
  let value = expect_int(inp)?;
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Let(name, value))
}

fn print_arm<'inp, Ctx>(
  _head: Spanned<Tok, SimpleSpan>, // the `print` keyword
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut args = vec![expect_int(inp)?];
  while inp.try_expect(|t| matches!(t.data(), Tok::Comma))?.is_some() {
    args.push(expect_int(inp)?);
  }
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Print(args))
}

fn bare_arm<'inp, Ctx>(
  head: Spanned<Tok, SimpleSpan>, // the integer itself — no re-consume
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let value = match head.into_data() {
    Tok::Int(n) => n,
    _ => unreachable!("the table routes only integers here"),
  };
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Bare(value))
}

fn parse_stmt_fused<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  static TABLE: [TokKind; 3] = [TokKind::Let, TokKind::Print, TokKind::Int];
  (let_arm, print_arm, bare_arm)
    .fused_dispatch_on_kind(&TABLE)
    .parse_input(inp)
}

// ── Match-first: the table lives beside the patterns. ──

/// What the head decided, with the span the arm was handed. `Tok::Int(n)` binds `n` **by
/// value** — the arm receives the moved payload, which an arm that only borrowed the head
/// could not do.
enum Head {
  Let(SimpleSpan),
  Print(SimpleSpan),
  Int(SimpleSpan, i64),
}

fn parse_stmt_select<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ComposableParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  // No `static TABLE` beside this one: the three kinds in the first column *are* the
  // table, so it cannot fall out of step with the arms.
  let head = tokora::select!(inp, {
    TokKind::Let => (span, Tok::Let) => Head::Let(span),
    TokKind::Print => (span, Tok::Print) => Head::Print(span),
    TokKind::Int => (span, Tok::Int(n)) => Head::Int(span, n),
  })?;
  // The input is borrowed for the classification, so the branch that keeps parsing runs
  // after it — and reuses the fused arms unchanged.
  match head {
    Head::Let(span) => let_arm(Spanned::new(span, Tok::Let), inp),
    Head::Print(span) => print_arm(Spanned::new(span, Tok::Print), inp),
    Head::Int(span, n) => bare_arm(Spanned::new(span, Tok::Int(n)), inp),
  }
}

// All three shapes agree — on hits and on misses.
for src in ["let x = 7 ;", "print 1 , 2 ;", "42 ;", "; nope"] {
  let peeked = Parser::new().apply(parse_stmt).parse_str(src);
  let fused = Parser::new().apply(parse_stmt_fused).parse_str(src);
  let selected = Parser::new().apply(parse_stmt_select).parse_str(src);
  assert_eq!(peeked, fused, "shapes diverged on {src:?}");
  assert_eq!(peeked, selected, "shapes diverged on {src:?}");
}
```

Calc still evaluates nothing but bare integers. Chapter 5 replaces them with real
expressions. Next: [chapter 5](super::ch05_pratt).
