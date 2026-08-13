# 10. Testing

Calc works. But *how do you know*, and — the harder question — how would you find out if it
stopped?

Testing a parser's grammar is ordinary work: feed it strings, check the trees. What is *not*
ordinary is the layer underneath. tokora's input machinery does things to your lexer that no
hand-written driver would: it truncates the token cache after a rollback, it re-lexes a rewound
region on demand, it resumes a lexer from a saved [`State`](crate::State) at an arbitrary
offset. Every one of those moves is sound **only because the [`Lexer`](crate::Lexer) contract
holds**. If your lexer quietly violates it — a token whose identity depends on lookahead past
its own span, a `with_state` + [`bump`](crate::Lexer::bump) resume that does not reproduce the
suffix — nothing fails loudly. Instead your parser is subtly wrong *only after a backtrack*,
which is the worst bug in this entire crate to find by hand.

So do not find it by hand.

## The conformance kit

The [`conformance`](crate::conformance) module (feature `conformance`) ships a
[`Harness`](crate::conformance::Harness) that drives your lexer against the contract and panics,
with the input index, position, operation and expected-vs-got, at the first violation. Build it
over a corpus with [`new`](crate::conformance::Harness::new) or
[`over`](crate::conformance::Harness::over), then call
[`run`](crate::conformance::Harness::run). It checks:

1. **replay identity** — two fresh runs produce the identical token/span/slice sequence;
2. **state-resume faithfulness** — at *every* position, saving the state and resuming there
   reproduces the rest of the run. This is the prefix-replay assumption, verbatim;
3. **monotone progress** — spans advance, none is empty, and the run terminates;
4. **sticky exhaustion** — once [`lex`](crate::Lexer::lex) returns `None`, it keeps doing so;
5. **span/slice coherence** — every slice equals the source over its span;
6. **gap-free tiling** — opt-in via [`lossless`](crate::conformance::Harness::lossless), for a
   lexer that emits trivia as tokens rather than skipping it. Calc's skips whitespace, so Calc
   does *not* ask for this one.

On top of the trait tier it drives a real `Input` session through fixed, named
save/peek/drain/restore schedules and requires the committed token stream to equal the
straight-lex stream — no randomness, the schedules are enumerated — and
[`run_partial`](crate::conformance::Harness::run_partial) adds chapter 9's tier: for **every**
split point of every input, a non-final drain of the prefix must yield a *prefix of* the tokens
before the cut and end incomplete, while a final drain of the whole source must reproduce the
complete parse exactly. That is the check that catches a lexer which is unfaithful under
truncation — and truncation is exactly what a stream does to you.

It asks for a prefix rather than equality because withholding *more* is always sound, and a lexer
that reports a read frontier past its own spans does exactly that. Calc's lexer is logos-backed
and does not declare [`READ_FRONTIER_CLASS`](crate::Token::READ_FRONTIER_CLASS), so it
takes the conservative default and this run withholds every token until the input is final. The
run still passes, and it still catches a token that *changes*. See
[`Lexer::read_frontier`](crate::Lexer::read_frontier) for what declaring the class buys back.

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
# impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for CalcError {
#   fn from(_: UnexpectedEot<O, Lang, Set>) -> Self { CalcError::UnexpectedEnd }
# }
# impl<'inp, L: tokora::Lexer<'inp>, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for CalcError {
#   fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self { CalcError::UnexpectedEnd }
# }
use tokora::{
  Emitter, InputRef, Parse, ParseContext, ParseInput, Parser,
  conformance::Harness,
  traced,
};

// A corpus: the shapes Calc actually sees, plus the empty source, which is where
// off-by-one bugs live.
const CORPUS: [&str; 5] = [
  "",
  "let x = 1 ;",
  "print 1 , 2 ;",
  "( 1 + 2 ) * 3 ^ 4 ;",
  "let ab = 12 ; print ab ;",
];

// The contract, checked. Note the absence of `.lossless()`: Calc's lexer *skips* whitespace,
// so its spans legitimately leave gaps. Ask for gap-free tiling only from a lossless lexer.
Harness::<CalcLexer<'_>>::over(CORPUS).run();

// And the streaming tier: every split point of every input, chunked-equivalence checked.
Harness::<CalcLexer<'_>>::over(CORPUS).run_partial();

// ── Debugging a parse you do not understand ──
# fn parse_stmt<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<i64, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   if inp.try_expect(|t| matches!(t.data(), Tok::Print))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   let value = match inp.next()? {
#     Some(tok) => match tok.into_data() {
#       Tok::Int(n) => n,
#       _ => return Err(CalcError::Unexpected),
#     },
#     None => return Err(CalcError::UnexpectedEnd),
#   };
#   if inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_none() {
#     return Err(CalcError::Unexpected);
#   }
#   Ok(value)
# }
// (Hidden: `parse_stmt`, a `print <int> ;` parser in chapter 2's style.)

/// Wrapping a parser in [`traced`] prints an indented `enter` / `exit` transcript to stderr
/// as it runs — including the crate's own instrumented combinators, and including the
/// backtracks, which is usually the part you could not see.
fn traced_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<i64, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  traced("statement", parse_stmt).parse_input(inp)
}

// With the `trace` feature *off*, `traced(name, p)` is literally `p` — no wrapper type, no
// branch, nothing to strip before shipping. The line above can stay where it is.
assert_eq!(
  Parser::new().apply(traced_stmt).parse_str("print 7 ;"),
  Ok(7)
);
```

## Fuzzing the machinery, not just the grammar

The conformance kit checks your *lexer* against the contract. The
`fuzz` module (feature `fuzz`) checks the layer above it: a deterministic
**operation-script** fuzzer that drives the input and backtracking machinery — consume, peek,
the `sync` family, `attempt`, the transaction guards, stacked savepoints, session points,
partial-mode chunking — against a scriptable synthetic lexer, and verifies the documented laws
after *every* operation. `run_case` runs one
`Case`; `run_seeds` sweeps a range of them. They
are ordinary `#[test]`s on stable Rust — no nightly, no external fuzzer — so the arbitrary
operation orders your grammar will eventually produce get exercised long before your users
produce them.

## A testing ladder for your language

1. **Conformance** your lexer, once, over a corpus of real inputs — including the empty one,
   the one-token one, and the one that ends mid-token. If you use a
   [`LogosLexer`](crate::lexer::LogosLexer) this is already true, and the check is cheap
   insurance against the day you replace `LogosLexer` with a custom lexer.
2. **Golden-test the grammar**: source in, AST out. Ordinary table tests.
3. **Golden-test the *diagnostics*** under [`Verbose`](crate::emitter::Verbose) — chapter 7's
   [`diagnostics()`](crate::emitter::Verbose::diagnostics) view is a stable, orderable thing to
   snapshot. A recovery regression shows up here as a changed hole, and nowhere else.
4. **Reach for [`traced`](crate::traced)** the moment a parse surprises you, and delete
   nothing afterwards — it costs nothing with the feature off.

## Where to go next

Calc is finished: it lexes, parses, dispatches, folds expressions by precedence, speculates and
rolls back, reports many diagnostics at once, recovers without cascading, and streams. The Calc
fundamentals are complete.

The four programs in `examples/` — `json`, `calculator`, `s_expression`, and `c_expression` —
are canonical complete programs in the same style, each leaning on a different corner of the
crate. Next: [chapter 11](super::ch11_real_parser) uses them to show how a real parser comes
together. Read [`InputRef`](crate::InputRef) for the primitives, the [`parser`](crate::parser)
module for the combinator catalogue, and the [`emitter`](crate::emitter) module when you decide
what your diagnostics should *do*. When something behaves in a way the documentation did not
predict, that is a bug in one of them — the guide's doctests exist so that it is never quietly
the guide.
