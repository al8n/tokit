# 9. Partial input

Every parse so far started with the whole source in hand. A network socket does not work that
way: bytes arrive in chunks, and a parser that must wait for the last one is a parser that
cannot stream. But the naive fix is a bug factory. Given the bytes `1 + 2`, is the final token
the integer `2`? Or is it the first digit of `23`, whose second digit is still in flight? A
parser that guesses will, sooner or later, guess wrong.

tokora's answer is to make the *question* representable, in the type system.

## The `Completeness` typestate

An input carries a [`Completeness`](crate::Completeness) parameter:

- [`Complete`](crate::Complete) — the default, and what chapters 1-8 have been using without
  ever saying so. The source is all of it. Every frontier rule below is written
  `if Cmpl::PARTIAL && …`, so under `Complete` they are compiled *out of existence*: the
  typestate is not a runtime mode, and the fast path pays exactly nothing for the existence of
  the slow one.
- [`Partial`](crate::Partial) — the source is a **prefix of a stream that may still grow**. It
  carries one runtime bit, `is_final`, that the **driver** states when the last chunk lands —
  [`parse_partial`](crate::parse_partial)'s `is_final` argument.

## The three frontier rules

While a partial input is **non-final**, three conservative rules fire at the scan chokepoint,
each surfacing an [`Incomplete`](crate::error::Incomplete) rather than committing to an answer
that later bytes could contradict:

1. **The holdback.** A token the lexer decided by *reading as far as the end of the buffer* is
   *withheld* — it might be the prefix of a longer one.
2. **A lexer error at the frontier** is withheld the same way: garbage that abuts the end of
   the buffer might be the beginning of something valid.
3. **End of input, when the input is not final,** is not end of input. It is a request for more
   bytes.

"Read as far as the end" is a fact the lexer reports, through
[`Lexer::read_frontier`](crate::Lexer::read_frontier) — **not** the item's span reaching the end.
The two coincide only for a lexer that never reads past what it emits
([`SpanEnd`](crate::ReadFrontier::SpanEnd)). A lexer that probes ahead and backtracks — which the
bundled logos backend's DFA does whenever one pattern is a proper prefix of another, an integer
beside a float — decides an item at `0..1` by looking at byte 2, and rule 1 holds *that* item back
even though its span sits behind the end. Keying on the span instead was the pre-0.10.0 proxy, and
it committed exactly the items one more byte would change.

Rule 1 is the one you feel, and it has a name: **frontier latency**. A token whose decision
consulted the end of a non-final buffer becomes visible only when more input arrives *or*
`is_final` is set. That is not a limitation to be engineered around — it is the only sound answer.
The sole proof that `2` is not the start of `23` is another byte, or a promise that there will not
be one. How many tokens it costs is the lexer's answer, not the buffer's: one for a `SpanEnd`
lexer, more for a lookahead one, and *every* token until the seal for one that reports
[`Unbounded`](crate::ReadFrontier::Unbounded).

## `is_final` belongs to the driver, and it only goes one way

Notice who makes that promise. `is_final` is not a fact about the *parse* — it is a fact about the
**world**: *the caller has told us no more bytes are coming.* A parser combinator cannot possibly
know it. Only the code holding the socket can.

So there is no `set_final` on an [`InputRef`](crate::InputRef), and there never will be. You state
finality where you build the input — `parse_partial`'s `is_final` argument — and the parser you hand
the input to simply cannot reach it. That is enforced by the borrow checker, not by convention: the
flag lives on the input, the handle borrows the input, and the borrow lasts as long as the handle
does.

Two bugs fall out of that one line, and it is worth seeing both, because they are mirrors:

- **A parser that could end a stream** would break the holdback. Speculate, call `set_final(true)`,
  fail, roll back — and the rollback would not undo it, because rolling back the *world* is not a
  thing rollback does. The next read would then hand you a token the frontier owed an `Incomplete`
  for: the very `2`-that-might-be-`23` this chapter is about.
- **A rollback that could un-end a stream** — the "obvious" fix of checkpointing the flag and
  restoring it — is worse. Your last chunk lands, you mark the stream final, the parser rolls back
  across that moment, and `is_final` quietly reverts to `false`. Now the parser asks for a refill
  that can never come, and your program waits forever. That trades a wrong token for a hang.

The way out is to notice that the two bugs share a premise — that a parser can touch the bit at all.
Take that away and both are gone: finality is set by the driver, before any parser exists, and it is
**monotone** (a stream cannot un-end). Nothing to roll back, and nothing that would want to.

## No growable source: the caller owns the buffer

tokora has **no internal growable source**, and that is a deliberate architectural line, not an
omission. An [`InputRef`](crate::InputRef) borrows one immutable slice for its whole life —
which is precisely what makes zero-copy slices free, and makes a checkpoint a snapshot copy
rather than a journalled edit. Backtracking is cheap *because* the source cannot move under it.

So resumption lives with the caller. It owns the byte buffer; on an incomplete result it
appends the next chunk to *its own* buffer and rebuilds the input over the larger slice.
Re-lexing the prefix each round is cheap, and it keeps the frontier rules a pure function of
the current slice — which is what "Sans-I/O" means: tokora never reads, never waits, never owns
a socket. It parses what you hand it and tells you when it needs more.

[`parse_partial`](crate::parse_partial) wires up one round of that loop: it builds a
[`Partial`](crate::Partial) input over your slice, seals it if this is the last chunk, and drives
any [`ParseInput`](crate::ParseInput)`<…, `[`Partial`](crate::Partial)`>` — a typed fn like the
one below, a named combinator chain, or a parser written generic over its completeness
parameter. Since 0.3.0 that bound *is* the partial driver's whole signature: the closure era's
bare-`FnOnce` bypass is gone, and partial mode is simply the other instantiation of the same
trait vocabulary the complete drivers use.

Partial mode adds exactly **two** requirements to your error type, both one-liners: it must
implement `From<Incomplete<L::Offset>>`, so the frontier has a way to speak — and
[`MaybeIncomplete`](crate::error::MaybeIncomplete), so the frontier can be *recognized*: your
refill loop keys off `is_incomplete()`, and inside the parse the resilient collection loops
(`repeated`/`separated`) consult the same trait to re-raise a frontier `Incomplete` untouched
instead of spending it as a diagnostic — the never-recoverable law, enforced at the atom layer.

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
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
use tokora::{
  InputRef, Partial, parse_partial,
  cache::DefaultCache,
  emitter::Fatal,
  error::{Incomplete, MaybeIncomplete},
};

// Chapter 3's `CalcError`, plus the one variant partial mode asks for.
#[derive(Debug, Clone, PartialEq)]
enum CalcError {
  Lex,
  Unexpected,
  UnexpectedEnd,
  /// The frontier speaking: "ask me again when you have more bytes."
  Incomplete,
}
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl From<UnexpectedEot> for CalcError {
#   fn from(_: UnexpectedEot) -> Self { CalcError::UnexpectedEnd }
# }
// THE requirement. `L::Offset` is `usize` for a `str` source: the offset the input ran out at.
impl From<Incomplete<usize>> for CalcError {
  fn from(_: Incomplete<usize>) -> Self {
    CalcError::Incomplete
  }
}

// And how a caller *recognises* it — the same trait chapter 8's recovery consults before it
// dares to skip anything.
impl MaybeIncomplete for CalcError {
  fn is_incomplete(&self) -> bool {
    matches!(self, CalcError::Incomplete)
  }
}

type CalcCtx<'a> = (Fatal<CalcError>, DefaultCache<'a, CalcLexer<'a>>);

/// Sum every integer in the chunk. The `Partial` in the signature is the whole difference:
/// the frontier rules exist for this parser and are compiled away for a `Complete` one.
fn sum<'inp>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, CalcCtx<'inp>, (), Partial>,
) -> Result<i64, CalcError> {
  // Rollback-on-drop (chapter 6). An incomplete attempt must leave *no trace*, because the
  // caller is about to re-drive this same parser over a longer buffer.
  let mut txn = inp.begin();
  let mut total = 0i64;
  while let Some(tok) = txn.next()? {
    // ↑ the frontier rules live in `next()`; a withheld token surfaces here as `Incomplete`,
    //   the `?` propagates it, and the guard's drop rewinds everything on the way out.
    match tok.into_data() {
      Tok::Int(n) => total += n,
      Tok::Plus => {}
      _ => return Err(CalcError::Unexpected),
    }
  }
  txn.commit();
  Ok(total)
}

fn fresh_ctx<'a>() -> CalcCtx<'a> {
  (Fatal::of(), DefaultCache::<'a, CalcLexer<'a>>::default())
}

// ── The flip. Same bytes, one bit of difference. ──

// Not final: the `2` touches the end of the buffer, so it is withheld. It might yet be a
// `23`, and nothing in these bytes can prove otherwise.
assert_eq!(
  parse_partial(fresh_ctx(), "1 + 2", (), false, sum),
  Err(CalcError::Incomplete)
);

// Final: the promise that no more bytes are coming. The frontier rules go inert, the token
// yields, and the parse finishes — this is now *exactly* a `Complete` parse.
assert_eq!(parse_partial(fresh_ctx(), "1 + 2", (), true, sum), Ok(3));

// ── And the loop that falls out of it: the caller owns the buffer. ──

let chunks = ["1 +", " 2", " + 30"];
let mut buffer = String::new();
let mut refills = 0;
let mut answer = None;

for (i, chunk) in chunks.iter().enumerate() {
  buffer.push_str(chunk); // the growable thing is *yours*, not tokora's
  let is_final = i + 1 == chunks.len();
  match parse_partial(fresh_ctx(), buffer.as_str(), (), is_final, sum) {
    Ok(total) => {
      answer = Some(total);
      break;
    }
    // Not a failure — a request. Append the next chunk and re-drive over the longer slice.
    Err(e) if e.is_incomplete() => refills += 1,
    Err(other) => panic!("a real parse error: {other:?}"),
  }
}

assert_eq!(answer, Some(33));
// Each non-final chunk ended mid-token, so each one cost exactly one refill: that is the
// one-token frontier latency, and it is the whole price of correctness here.
assert_eq!(refills, 2);
```

## Write once, run in both modes

Everything above used a parser whose signature *names* `Partial`. That is one honest way to
write a streaming parser — but 0.3.0's point is that you do not have to choose. Write the
parser **generic over its completeness** and it is one item with two instantiations: the
complete combinator driver pins `Cmpl = `[`Complete`](crate::Complete), `parse_partial` pins
`Cmpl = `[`Partial`](crate::Partial), and the frontier rules exist in exactly one of the two
monomorphizations. The bound to reach for is
[`SurfaceIncomplete`](crate::SurfaceIncomplete) — the [`Completeness`](crate::Completeness)
refinement the scan chokepoint itself uses, satisfied by `Complete` unconditionally and by
`Partial` wherever the error type meets the two requirements above.

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
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
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
# use tokora::{
#   InputRef, Partial, parse_partial,
#   cache::DefaultCache,
#   emitter::Fatal,
#   error::{Incomplete, MaybeIncomplete},
# };
#
# // Chapter 3's `CalcError`, plus the one variant partial mode asks for.
#[derive(Debug, Clone, PartialEq)]
# enum CalcError {
#   Lex,
#   Unexpected,
#   UnexpectedEnd,
#   /// The frontier speaking: "ask me again when you have more bytes."
#   Incomplete,
# }
# impl From<LexError> for CalcError { fn from(_: LexError) -> Self { CalcError::Lex } }
# impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for CalcError {
#   fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self { CalcError::Unexpected }
# }
# impl From<UnexpectedEot> for CalcError {
#   fn from(_: UnexpectedEot) -> Self { CalcError::UnexpectedEnd }
# }
# // THE requirement. `L::Offset` is `usize` for a `str` source: the offset the input ran out at.
# impl From<Incomplete<usize>> for CalcError {
#   fn from(_: Incomplete<usize>) -> Self {
#     CalcError::Incomplete
#   }
# }
#
# // And how a caller *recognises* it — the same trait chapter 8's recovery consults before it
# // dares to skip anything.
# impl MaybeIncomplete for CalcError {
#   fn is_incomplete(&self) -> bool {
#     matches!(self, CalcError::Incomplete)
#   }
# }
#
# type CalcCtx<'a> = (Fatal<CalcError>, DefaultCache<'a, CalcLexer<'a>>);
#
# /// Sum every integer in the chunk. The `Partial` in the signature is the whole difference:
# /// the frontier rules exist for this parser and are compiled away for a `Complete` one.
# fn sum<'inp>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, CalcCtx<'inp>, (), Partial>,
# ) -> Result<i64, CalcError> {
#   // Rollback-on-drop (chapter 6). An incomplete attempt must leave *no trace*, because the
#   // caller is about to re-drive this same parser over a longer buffer.
#   let mut txn = inp.begin();
#   let mut total = 0i64;
#   while let Some(tok) = txn.next()? {
#     // ↑ the frontier rules live in `next()`; a withheld token surfaces here as `Incomplete`,
#     //   the `?` propagates it, and the guard's drop rewinds everything on the way out.
#     match tok.into_data() {
#       Tok::Int(n) => total += n,
#       Tok::Plus => {}
#       _ => return Err(CalcError::Unexpected),
#     }
#   }
#   txn.commit();
#   Ok(total)
# }
#
# fn fresh_ctx<'a>() -> CalcCtx<'a> {
#   (Fatal::of(), DefaultCache::<'a, CalcLexer<'a>>::default())
# }
use tokora::{Parse, Parser, input::SurfaceIncomplete};

/// ONE parser. The only new thing is the `Cmpl` parameter where chapter 9's `sum` wrote
/// `Partial`: same transaction, same loop, same commit.
fn sum_generic<'inp, Cmpl>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, CalcCtx<'inp>, (), Cmpl>,
) -> Result<i64, CalcError>
where
  Cmpl: SurfaceIncomplete<'inp, CalcLexer<'inp>, CalcCtx<'inp>, ()>,
{
  let mut txn = inp.begin();
  let mut total = 0i64;
  while let Some(tok) = txn.next()? {
    match tok.into_data() {
      Tok::Int(n) => total += n,
      Tok::Plus => {}
      _ => return Err(CalcError::Unexpected),
    }
  }
  txn.commit();
  Ok(total)
}

// (a) The COMPLETE drive: the ordinary whole-input combinator API, `Cmpl = Complete`.
let complete = Parser::with_context(fresh_ctx())
  .apply(sum_generic)
  .parse_str("1 + 2 + 30");
assert_eq!(complete, Ok(33));

// (b) The PARTIAL drive: the SAME item over chunks, `Cmpl = Partial`.
let chunks = ["1 +", " 2", " + 30"];
let mut buffer = String::new();
let mut refills = 0;
let mut answer = None;
for (i, chunk) in chunks.iter().enumerate() {
  buffer.push_str(chunk);
  let is_final = i + 1 == chunks.len();
  match parse_partial(fresh_ctx(), buffer.as_str(), (), is_final, sum_generic) {
    Ok(total) => {
      answer = Some(total);
      break;
    }
    Err(e) if e.is_incomplete() => refills += 1,
    Err(other) => panic!("a real parse error: {other:?}"),
  }
}

// One parser, two modes, one answer — and the exact frontier latency of the typed version.
assert_eq!(answer, Some(33));
assert_eq!(refills, 2);
```

This scales past single functions: the builder methods thread the same parameter, so a whole
chain — `expect(…).map(…).repeated().collect()` — assembled inside a `Cmpl`-generic function
runs under both drivers too. The combinators that *cannot* run partial yet (the
decision-window `*_while`/peek/dispatch families and the CST `node` family — see the
[combinator reference](super::ref_combinators)) stay pinned at `Complete`, so reaching for one
from partial code is a compile error at the drive site, never a silent wrong parse.

## The bigger example

The [`input`](crate::input) module documents [the Sans-I/O resumption
loop](crate::input#the-sans-io-resumption-loop) end to end, with a hand-written lexer instead
of a logos one — worth reading once, because it shows the frontier rules interacting with a
`lex`/`bump` implementation you can see all of.

## Why chapter 8 came first

Recall the never-recoverable law: an [`Incomplete`](crate::error::Incomplete) is re-raised
untouched by every recovery combinator, checked *before* any skip. Now you can see what it
buys. Recovery skips input on the theory that the input is **wrong**. At a stream frontier the
input is not wrong — it is merely **unfinished**, and skipping it would silently discard bytes
that had not arrived yet, turning a refill request into data loss. The two features compose
only because that law holds: a streaming parser can use recovery, and recovery will never eat
the frontier.

Calc's implementation is complete: it lexes, parses, dispatches, computes expressions,
speculates, reports, recovers, and streams. Chapter 10, Testing, finishes the fundamentals by
showing how to verify a lexer and parser.
