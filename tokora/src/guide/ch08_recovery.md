# 8. Recovery

Chapter 7's parser collects many errors, but it resynchronises by scanning to the next `;` and
that is not good enough. Consider a broken `let` whose garbage *contains* a semicolon:

```text
let = ( 2 ; 3 ) ;
```

A bracket-blind skip stops at the `;` inside the parentheses, resumes at `3 ) ;` — which is
not a statement either — and reports a *second* error that exists only because the first
recovery landed badly. That is a cascade, and it is why a compiler that reports twenty errors
for one typo is worse than useless.

## `sync_balanced`: the skip that can count

[`sync_balanced`](crate::InputRef::sync_balanced) skips forward to a sync point **at nesting
depth zero**. You give it two things:

- a **classifier** — which token kinds open and close a pair. Any
  `FnMut(&Kind) -> Balance<P>` is one, via the blanket [`DelimClass`](crate::DelimClass) impl;
  [`Balance`](crate::Balance) is `Open(pair)`, `Close(pair)`, or `Neutral`.
- a **sync predicate** — what you want to land on. It is consulted **only at depth zero**, so
  garbage containing balanced pairs skips straight over any sync tokens buried inside them.

It returns `Option<`[`Hole`](crate::Hole)`>` — the region it skipped
([`span()`](crate::Hole::span) and [`skipped()`](crate::Hole::skipped), the token count) — and
stops *before* the sync token, leaving it for the parse that resumes.

Two properties are worth naming because they are what make it safe. Depth counting is
**token-level**: a composite token (a block string, a raw literal) is one token whose lexer
already swallowed any brackets inside it, so nothing *within* a token can move the depth. And
it is **pair-blind**: a closer closes the innermost open pair whatever its identity, because
inside garbage the mismatched pairs are part of what is being thrown away.

## One hole, one diagnostic

A skip does not report the tokens it dropped one by one — that would be the cascade again, in
a different costume. A successful sync that skipped at least one token reports the whole
region **exactly once** through [`emit_skipped_region`](crate::Emitter::emit_skipped_region)
(a defaulted no-op, so a fail-fast emitter pays nothing;
[`Verbose`](crate::emitter::Verbose) records it, and you read it back with
[`skipped_regions()`](crate::emitter::Verbose::skipped_regions) or interleaved in
[`diagnostics()`](crate::emitter::Verbose::diagnostics)). A sync that finds nothing emits
nothing at all and rewinds without a trace: **no diagnostic for a failed hole**.

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
# use tokora::error::{UnexpectedEot, token::UnexpectedToken};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl From<UnexpectedEot> for CalcError {
#   fn from(_: UnexpectedEot) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{
  Balance, DelimClass, Emitter, InputRef, Parse, ParseContext, Parser,
  cache::DefaultCache,
  emitter::Verbose,
  span::Spanned,
};

#[derive(Debug, Clone, PartialEq)]
enum Stmt<'a> {
  Let(&'a str, i64),
  Print(i64),
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
# fn parse_let<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<Stmt<'inp>, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   if inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   let name = inp.slice();
#   if inp.try_expect(|t| matches!(t.data(), Tok::Assign))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   let value = expect_int(inp)?;
#   if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   Ok(Stmt::Let(name, value))
# }
# fn parse_print<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<Stmt<'inp>, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   let value = expect_int(inp)?;
#   if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   Ok(Stmt::Print(value))
# }
// (Hidden: `parse_let` and `parse_print` — chapter 4's branch parsers, with the head
//  keyword already consumed. Both simply return `Err` when they do not fit.)

/// The classifier: Calc's only pair is the parenthesis. A named `fn` (not a closure), so the
/// higher-ranked `FnMut(&Kind)` bound the blanket `DelimClass` impl wants is satisfied for
/// free.
fn parens(kind: &TokKind) -> Balance<()> {
  match kind {
    TokKind::LParen => Balance::Open(()),
    TokKind::RParen => Balance::Close(()),
    _ => Balance::Neutral,
  }
}

/// A deliberately bracket-blind classifier, so the two can be compared side by side.
fn flat(_kind: &TokKind) -> Balance<()> {
  Balance::Neutral
}

/// Parse statements; on a bad one, report it and skip to the next depth-0 `;`. Returns the
/// statements it salvaged and the size of each hole it punched.
fn recover_program<'inp, Ctx, D>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
  classifier: D,
) -> Result<(Vec<Stmt<'inp>>, Vec<usize>), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
  D: DelimClass<TokKind> + Copy,
{
  let mut stmts = Vec::new();
  let mut holes = Vec::new();
  while let Some(head) = inp.next()? {
    let at = *inp.span();
    let parsed = match head.into_data() {
      Tok::Let => parse_let(inp),
      Tok::Print => parse_print(inp),
      _ => Err(CalcError::Unexpected),
    };
    match parsed {
      Ok(stmt) => stmts.push(stmt),
      Err(_) => {
        inp.emitter()
          .emit_error(Spanned::new(at, CalcError::Unexpected))?;
        // The skip. `pred` is only consulted at depth zero, so a `;` inside `( … )` is
        // skipped over rather than mistaken for the end of the statement.
        if let Some(hole) = inp.sync_balanced(classifier, |t| matches!(t.data(), Tok::Semi))? {
          holes.push(hole.skipped());
        }
        // The sync stops *before* the `;`. Eat it, so the next statement starts clean.
        let _ = inp.try_expect(|t| matches!(t.data(), Tok::Semi))?;
      }
    }
  }
  Ok((stmts, holes))
}

# fn program_nested<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<(Vec<Stmt<'inp>>, Vec<usize>), CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   recover_program(inp, parens)
# }
# fn program_flat<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<(Vec<Stmt<'inp>>, Vec<usize>), CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   recover_program(inp, flat)
# }
// (Hidden: `program_nested` and `program_flat`, one-line wrappers that pin the classifier.)

// The middle statement is broken, and its garbage contains a semicolon.
const SRC: &str = "print 1 ; let = ( 2 ; 3 ) ; print 4 ;";

// ── Nesting-aware: one mistake, one hole, one error. ──
let mut emitter = Verbose::<CalcError>::new();
let cache = DefaultCache::<'_, CalcLexer<'_>>::default();
let (stmts, holes) = Parser::with_context((&mut emitter, cache))
  .apply(program_nested)
  .parse_str(SRC)
  .unwrap();

assert_eq!(stmts, [Stmt::Print(1), Stmt::Print(4)]);
// `= ( 2 ; 3 )` — seven tokens minus the `;` we stop before: the inner `;` was *inside*
// the parentheses, at depth 1, so the predicate never even saw it.
assert_eq!(holes, [6]);
assert_eq!(emitter.errors().values().flatten().count(), 1);
// The skip reported itself, once, without being asked.
assert_eq!(emitter.skipped_regions().values().flatten().count(), 1);

// ── Bracket-blind: the same code with a classifier that counts nothing. ──
let mut emitter = Verbose::<CalcError>::new();
let cache = DefaultCache::<'_, CalcLexer<'_>>::default();
let (stmts, holes) = Parser::with_context((&mut emitter, cache))
  .apply(program_flat)
  .parse_str(SRC)
  .unwrap();

assert_eq!(stmts, [Stmt::Print(1), Stmt::Print(4)]);
// The cascade, measured: the first skip stopped at the `;` *inside* the parentheses and
// resumed at `3 ) ;`, which is not a statement either — so a second error and a second
// hole were invented by the recovery itself.
assert_eq!(holes, [3, 1]);
assert_eq!(emitter.errors().values().flatten().count(), 2);
assert_eq!(emitter.skipped_regions().values().flatten().count(), 2);
```

## `skip_then_retry`: recovery as a combinator

Writing the loop by hand is fine, but the common shape — *try the parser; if it fails, skip to
a sync point and try again* — is [`skip_then_retry`](crate::ParseInput::skip_then_retry). It
takes the same classifier and predicate, and it carries the thing a hand-rolled retry loop
usually forgets: a **mandatory progress guard**. A retry cycle that consumes nothing bails out
with the error that triggered it rather than spinning; a cycle that fails *after* real
progress consumes the sync token before re-syncing, so every continuing cycle advances by at
least one token and the loop provably terminates.

Note the sync predicate here is *where a statement may begin*, not `;`. Sync to what you are
about to retry.

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
# use tokora::error::{UnexpectedEot, token::UnexpectedToken};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl From<UnexpectedEot> for CalcError {
#   fn from(_: UnexpectedEot) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{
  Balance, Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  cache::DefaultCache,
  emitter::Verbose,
  error::{MaybeIncomplete, MaybeTerminal},
};

// THE NEVER-RECOVERABLE LAW AND ITS TERMINAL DUAL. `skip_then_retry` requires the emitter's error
// type to answer two questions: *are you an `Incomplete`?* and *are you a terminal scanner stop?*
// Either one is re-raised untouched, before any skip and from any retry — recovery synthesises
// progress over a *malformed* construct, while an incomplete one is merely *unfinished* (skipping
// it throws away input that has not arrived) and a terminal stop is a limit no skip can clear.
// `CalcError` is never either (it parses whole strings with no resource limit), so the traits'
// default answers — `false` — are the right ones.
impl MaybeIncomplete for CalcError {}
impl MaybeTerminal for CalcError {}

# #[derive(Debug, Clone, PartialEq)]
# enum Stmt<'a> {
#   Let(&'a str, i64),
#   Print(i64),
# }
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
# fn parse_stmt<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<Stmt<'inp>, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   match inp.next()? {
#     Some(tok) => match tok.into_data() {
#       Tok::Let => {
#         if inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_none() {
#           return Err(CalcError::Unexpected);
#         }
#         let name = inp.slice();
#         if inp.try_expect(|t| matches!(t.data(), Tok::Assign))?.is_none() {
#           return Err(CalcError::Unexpected);
#         }
#         let value = expect_int(inp)?;
#         if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
#           return Err(CalcError::Unexpected);
#         }
#         Ok(Stmt::Let(name, value))
#       }
#       Tok::Print => {
#         let value = expect_int(inp)?;
#         if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
#           return Err(CalcError::Unexpected);
#         }
#         Ok(Stmt::Print(value))
#       }
#       _ => Err(CalcError::Unexpected),
#     },
#     None => Err(CalcError::UnexpectedEnd),
#   }
# }
# fn parens(kind: &TokKind) -> Balance<()> {
#   match kind {
#     TokKind::LParen => Balance::Open(()),
#     TokKind::RParen => Balance::Close(()),
#     _ => Balance::Neutral,
#   }
# }
// (Hidden: `parse_stmt` — chapter 4's dispatcher, failing with `Err` on anything that is
//  not a statement — and the `parens` classifier from above.)

/// The whole recovery policy, as one wrapper.
fn recovered_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  parse_stmt
    .skip_then_retry(parens, |t| matches!(t.data(), Tok::Let | Tok::Print))
    .parse_input(inp)
}

// A parenthesised lump of garbage — containing a `let` and a `;` that a blind skip would
// have fallen for — followed by the real statement.
let mut emitter = Verbose::<CalcError>::new();
let cache = DefaultCache::<'_, CalcLexer<'_>>::default();
let stmt = Parser::with_context((&mut emitter, cache))
  .apply(recovered_stmt)
  .parse_str("( let y = 9 ; ) let x = 1 ;")
  .unwrap();

assert_eq!(stmt, Stmt::Let("x", 1));
// One hole, seven tokens: the entire `( … )` lump. The `let` *inside* it sat at depth 1,
// so it was never a candidate sync point.
let holes: Vec<usize> = emitter.skipped_regions().values().flatten().copied().collect();
assert_eq!(holes, [7]);
```

## The law, once more

Recovery skips input. An [`Incomplete`](crate::error::Incomplete) error says *there is more
input coming*. Skipping past it would throw away bytes that have not arrived yet — so both
[`skip_then_retry`](crate::ParseInput::skip_then_retry) and
[`Recover`](crate::parser::Recover) check
[`is_incomplete()`](crate::error::MaybeIncomplete::is_incomplete) **before** they skip, and
re-raise such an error untouched. Recovery is for the malformed, never for the unfinished.

That guarantee is what makes the next chapter safe to build.
Next: [chapter 9](super::ch09_streaming).
