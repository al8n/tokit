# 7. Diagnostics

Everything Calc has done so far stops at the first thing it does not understand. That is
correct for a config loader and useless for a compiler: a compiler that reports one error per
run is a compiler nobody wants to use. But "report everything" is not a property of the
grammar — it is a property of *what you do with a diagnostic once you have one*. So tokora puts
that decision in one replaceable object, the **emitter**, and leaves the parser alone.

The parser calls [`emit_error`](crate::Emitter::emit_error) and carries on with `?`. What
happens next is the emitter's business:

- [`Fatal`](crate::emitter::Fatal) — [`emit_error`](crate::Emitter::emit_error) *returns* the
  error, so the `?` at the call site ends the parse. Nothing is stored, nothing is allocated;
  the diagnostic is the `Err` value the caller already gets. This is what
  [`Parser::new`](crate::parser::Parser::new) gives you, and every chapter so far has used it
  without saying so.
- [`Verbose`](crate::emitter::Verbose) — the same call *records* the error and returns `Ok`,
  so the `?` does nothing and the parser keeps going. At the end you read the whole harvest
  off the emitter.

Same parser code, same `?`s, opposite behaviour. That is the whole point of the design: you
do not write a "collecting parser" and a "fail-fast parser" — you write *a* parser and hand it
an emitter.

## Two tiers, and only two

[`Severity`](crate::emitter::Severity) has exactly two rungs — `Error` and `Warning` — and the
tier is a *classification*, not a control-flow decision. A warning is never fatal:
[`Fatal`](crate::emitter::Fatal) has no warning sink and drops it on the floor;
[`Verbose`](crate::emitter::Verbose) files it in a channel parallel to the errors. Note that
both tiers carry the *same* payload type — your error enum — so a warning is a value of it
too.

## Labels: "while parsing X"

[`labelled(name, parser)`](crate::labelled) pushes a `&'static str` onto the emitter's open
-label stack for the duration of a sub-parse. Every diagnostic recorded inside is stamped with
the labels open **at emit time** — a snapshot, not a pointer — which is what makes labels
survive chapter 6: a rollback that drops a diagnostic drops its labels with it, and a
re-emission on the committed path re-derives them from the then-current stack. Around a
non-collecting emitter both the push and the pop inline away to nothing, so `labelled` is
free when nobody is listening.

## Reading the harvest

[`Verbose`](crate::emitter::Verbose) exposes span-keyed channels —
[`errors()`](crate::emitter::Verbose::errors), [`warnings()`](crate::emitter::Verbose::warnings),
[`labels()`](crate::emitter::Verbose::labels) (parallel to `errors()`, span-for-span and
index-for-index), and [`skipped_regions()`](crate::emitter::Verbose::skipped_regions) (chapter
8's recovery holes) — plus one view that the maps cannot express on their own:
[`diagnostics()`](crate::emitter::Verbose::diagnostics) walks *every* channel interleaved in
true **emission order**, each entry a [`Diagnostic`](crate::emitter::Diagnostic) carrying its
span, its label snapshot, and its [`DiagnosticKind`](crate::emitter::DiagnosticKind). That is
the view a renderer wants; tokora ships the data and takes on no dependency on ariadne, miette,
or anything else.

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
# use tokora::error::{UnexpectedEot, token::UnexpectedToken};
# #[derive(Debug, Clone, PartialEq)]
# enum CalcError { Lex, Unexpected, UnexpectedEnd }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  cache::DefaultCache,
  emitter::{Severity, Verbose},
  labelled,
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
/// Skip to the end of the broken statement so the parse has somewhere to resume.
/// (Chapter 8 replaces this with real, nesting-aware recovery.)
fn skip_to_semi<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  while let Some(tok) = inp.next()? {
    if matches!(tok.data(), Tok::Semi) {
      break;
    }
  }
  Ok(())
}

/// `let <ident> = <int> ;`, with the `let` already consumed by the caller.
///
/// `Ok(None)` means "reported and resynchronised" — the statement is gone, the parse is not.
/// (Hidden alongside: `expect_int`, chapter 2's helper.)
fn parse_let<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Option<Stmt<'inp>>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  if inp.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_none() {
    let at = *inp.span();
    // THE line this chapter is about. Under `Fatal` this `?` propagates and the parse is
    // over; under `Verbose` the error is filed and execution simply continues to the next
    // statement. The parser does not know, and does not need to.
    inp.emit_error(Spanned::new(at, CalcError::Unexpected))?;
    skip_to_semi(inp)?;
    return Ok(None);
  }
  let name = inp.slice();
  if inp.try_expect(|t| matches!(t.data(), Tok::Assign))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  let value = expect_int(inp)?;
  if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  Ok(Some(Stmt::Let(name, value)))
}

/// `print <int> ;`, with the `print` already consumed.
fn parse_print<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Option<Stmt<'inp>>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let value = expect_int(inp)?;
  if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
    return Err(CalcError::Unexpected);
  }
  Ok(Some(Stmt::Print(value)))
}

fn parse_program<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Vec<Stmt<'inp>>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut stmts = Vec::new();
  while let Some(head) = inp.next()? {
    let at = *inp.span();
    match head.into_data() {
      // The label is pushed for the sub-parse and popped afterwards; anything the
      // sub-parse emits is stamped with it.
      Tok::Let => {
        if let Some(stmt) = labelled("a `let` binding", parse_let).parse_input(inp)? {
          stmts.push(stmt);
        }
      }
      Tok::Print => {
        if let Some(stmt) = labelled("a `print` statement", parse_print).parse_input(inp)? {
          stmts.push(stmt);
        }
      }
      // An empty statement. Worth saying, not worth stopping for — so it is a *warning*,
      // and a warning is never fatal: `Fatal` drops this on the floor and carries on.
      Tok::Semi => {
        inp.emit_warning(Spanned::new(at, CalcError::Unexpected))?;
      }
      _ => {
        inp.emit_error(Spanned::new(at, CalcError::Unexpected))?;
        skip_to_semi(inp)?;
      }
    }
  }
  Ok(stmts)
}

// A program with one stray `;` (a warning) and one broken `let` (an error).
const SRC: &str = "let x = 1 ; ; let = 2 ; print 3 ;";

// ── Fatal (what `Parser::new()` hands you): the first error is the last event. ──
assert_eq!(
  Parser::new().apply(parse_program).parse_str(SRC),
  Err(CalcError::Unexpected)
);

// ── Verbose: the very same `parse_program`, run to the end of the file. ──
let mut emitter = Verbose::<CalcError>::new();
let cache = DefaultCache::<'_, CalcLexer<'_>>::default();
let stmts = Parser::with_context((&mut emitter, cache))
  .apply(parse_program)
  .parse_str(SRC)
  .expect("Verbose never fails the parse: it files the diagnostics instead");

// The good statements all came through; the broken one is simply absent.
assert_eq!(stmts, [Stmt::Let("x", 1), Stmt::Print(3)]);

// Errors and warnings are independent channels, each keyed by span.
assert_eq!(emitter.errors().values().flatten().count(), 1);
assert_eq!(emitter.warnings().values().flatten().count(), 1);

// `labels()` is parallel to `errors()`: same span, same index. The broken `let` was
// emitted inside the labelled sub-parse, so it knows what it was doing at the time.
let (span, group) = emitter.errors().iter().next().expect("one error");
assert_eq!(group.as_slice(), &[CalcError::Unexpected]);
assert_eq!(emitter.labels()[span], vec![vec!["a `let` binding"]]);

// And `diagnostics()` interleaves every channel in *emission* order — the stray `;`
// warning was emitted before the broken `let`, and here that is visible. The span-keyed
// maps above cannot tell you this; this is the view a renderer consumes.
let timeline: Vec<Severity> = emitter.diagnostics().map(|d| d.severity()).collect();
assert_eq!(timeline, [Severity::Warning, Severity::Error]);
```

## Expected sets come for free

Not every diagnostic is one you write. The combinators build structured errors themselves —
[`UnexpectedToken`](crate::error::token::UnexpectedToken) with an
[`Expected`](crate::utils::Expected) set, [`UnexpectedEnd`](crate::error::UnexpectedEnd) at
end of input, [`TooFew`](crate::error::syntax::TooFew) from a bounded repetition — and hand
them to the emitter through the same two verbs. Chapter 4's
[`dispatch_on_kind`](crate::ParseChoice::dispatch_on_kind) is the clearest case: on a miss it
reports the *whole table* as `expected one of …`, which it can only do because the decision is
committed and never rolled back. Your error enum absorbs each of these through a `From` impl —
that is what the `From` impls on `CalcError` have been for since chapter 2.

## Choosing

Reach for [`Fatal`](crate::emitter::Fatal) when the first error ends the job anyway (a config
file, a query, a protocol frame): it stores nothing, allocates nothing, and the diagnostic is
the `Err` you already handle. Reach for [`Verbose`](crate::emitter::Verbose) when a human is
going to read the output. [`Silent`](crate::emitter::Silent) and
[`Ignored`](crate::emitter::Ignored) round out the set for the cases where you want the parse
and not the diagnostics.

But notice what `skip_to_semi` above quietly is: a hand-rolled, bracket-blind resynchroniser
that would happily stop at the `;` *inside* a parenthesised expression. Collecting many errors
is only half of it — the other half is landing somewhere sane afterwards.
Next: [chapter 8](super::ch08_recovery).
