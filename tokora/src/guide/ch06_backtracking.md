# 6. Backtracking

Every chapter so far has been deterministic: one look at the next token decided everything,
and no parser ever un-did work. That is tokora's default posture, and it is the right one — a
decision that is never re-taken cannot lose a diagnostic. But some grammars genuinely need a
*second* token before they can choose, and a few need an unbounded one. Calc is about to grow
exactly such a shape.

Give Calc plain assignment (`x = 1 ;`) alongside expression statements (`x + 1 ;`). Both start
with an identifier. Chapter 4's dispatch cannot help: it decides on *one* kind, and here the
kind is the same. The decision lives on the *second* token.

## The tools, in the order you should reach for them

| Shape | Reach for it when |
|-------|-------------------|
| [`attempt`](crate::InputRef::attempt) | speculation in a closure; a decline is `None` and carries nothing out |
| [`try_attempt`](crate::InputRef::try_attempt) | the same, but the failure is a value you need |
| [`begin`](crate::InputRef::begin) → [`Transaction`](crate::Transaction) | imperative flow with several exits (loops, `match` arms) |
| [`begin_with::<Commit>`](crate::InputRef::begin_with) | the same, but *keeping* progress is the common case |
| [`begin_stacked`](crate::InputRef::begin_stacked) → [`StackedTransaction`](crate::StackedTransaction) | several live fallback points at once (best/longest match) |
| [`begin_point`](crate::InputRef::begin_point) → session points | a driver that marks, parses, and decides across *separate calls* |

All of them are the *same* mechanism — save a checkpoint, maybe restore it — wearing a
different shape. A rollback is total: position, span, lexer state, the token cache, the
diagnostics emitted since the save, the lexer-error dedup watermark, and the poison boundary
all return to what they were. Restoring is a snapshot copy, not a journal replay: the source
is immutable, so there is nothing to undo.

Beneath all of them sits the raw `save`/`restore` pair. It is gated behind the `unstable-raw`
feature and it is **not** the API you are meant to use: the guards exist because the raw pair
has a last-in-first-out contract that a human must uphold by hand, and every guard upholds it
by construction — a nested [`Transaction`](crate::Transaction) mutably borrows its parent, so
deciding the parent while a child is undecided is a *borrow error*, not a runtime bug. Guards
first. Always.

## Closure-shaped speculation

[`attempt`](crate::InputRef::attempt) runs a closure and rolls back if it returns `None`.
[`try_attempt`](crate::InputRef::try_attempt) is its `Result` sibling: roll back on `Err`, and
hand the error to the caller. The difference matters, because a speculative parse has *two*
ways to not work out — "this isn't the shape I was looking for" (a decline; try something
else) and "this is the shape, and it is broken" (a real error; report it). Keep them apart or
you will report the wrong one.

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
use tokora::{Emitter, InputRef, Parse, ParseContext, Parser};

/// Calc's two identifier-initial statements.
#[derive(Debug, Clone, PartialEq)]
enum Stmt<'a> {
  Assign(&'a str, Vec<&'a str>), // x = 1 + 2 ;
  Expr(Vec<&'a str>),            // x + 1 ;
}

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
/// This chapter's stand-in for chapter 5's Pratt engine: `atom (+ atom)*`, where an atom
/// is an integer or a variable. It yields the atoms' source text. (Hidden alongside it:
/// `expect_tok`, chapter 2's one-token helper.)
fn parse_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Vec<&'inp str>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut atoms = Vec::new();
  loop {
    expect_tok(inp, |t| matches!(t, Tok::Int(_) | Tok::Ident))?;
    atoms.push(inp.slice());
    if inp.try_expect(|t| matches!(t.data(), Tok::Plus))?.is_none() {
      return Ok(atoms);
    }
  }
}

// ── `attempt`: speculate, and decline unconditionally — an unbounded lookahead. ──

/// Answers a question by *parsing* it and then throwing the parse away. The closure always
/// returns `None`, so the input always rewinds: the answer travels out through a captured
/// variable, not through the return value.
///
/// For a fixed, shallow window `peek` is cheaper and does not re-lex. What `attempt` buys
/// is *unbounded* lookahead — the whole speculative parse — paid for by doing the work
/// twice.
fn looks_like_assignment<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> bool
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut answer = false;
  let _: Option<()> = inp.attempt(|inp| {
    // A lexer error here folds into a "no". That is safe, not sloppy: the lookahead
    // consumes nothing, so the committed parse walks into the same bad token and emits
    // the diagnostic there — the rolled-back one is re-emitted, exactly once in total.
    answer = inp
      .try_expect(|t| matches!(t.data(), Tok::Ident))
      .ok()
      .flatten()
      .is_some()
      && inp
        .try_expect(|t| matches!(t.data(), Tok::Assign))
        .ok()
        .flatten()
        .is_some();
    None // always decline → the input rewinds whatever we found
  });
  answer
}

// ── `try_attempt`: speculate for real, and keep the distinction. ──

/// The speculation's own error channel. `try_attempt` rolls back on *any* `Err`, so the
/// two failure kinds must stay distinguishable on the far side of the rollback.
enum Speculation {
  NotAnAssignment,  // wrong shape — rewind and try the other branch
  Failed(CalcError) // right shape, broken — rewind, then report
}

fn parse_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let attempted = inp.try_attempt(|inp| {
    let ident = inp
      .try_expect(|t| matches!(t.data(), Tok::Ident))
      .map_err(Speculation::Failed)?;
    if ident.is_none() {
      return Err(Speculation::NotAnAssignment);
    }
    let name = inp.slice();
    let eq = inp
      .try_expect(|t| matches!(t.data(), Tok::Assign))
      .map_err(Speculation::Failed)?;
    if eq.is_none() {
      // The decision point. The identifier we already consumed is put back too —
      // that is the whole reason this is an attempt and not a peek.
      return Err(Speculation::NotAnAssignment);
    }
    // Committed to `x = …` from here: a failure now is a real error, not a decline.
    let value = parse_expr(inp).map_err(Speculation::Failed)?;
    expect_tok(inp, |t| matches!(t, Tok::Semi)).map_err(Speculation::Failed)?;
    Ok(Stmt::Assign(name, value))
  });

  match attempted {
    Ok(stmt) => Ok(stmt),
    Err(Speculation::Failed(e)) => Err(e),
    Err(Speculation::NotAnAssignment) => {
      // Rolled back: the identifier is on the input again, so the expression parser
      // sees it as its own first atom.
      let value = parse_expr(inp)?;
      expect_tok(inp, |t| matches!(t, Tok::Semi))?;
      Ok(Stmt::Expr(value))
    }
  }
}

/// Runs the lookahead *and then* the real parse, so a passing assertion also proves the
/// lookahead left the input exactly where it found it.
fn stmt_with_lookahead<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(bool, Stmt<'inp>), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let guessed = looks_like_assignment(inp);
  Ok((guessed, parse_stmt(inp)?))
}

assert_eq!(
  Parser::new().apply(stmt_with_lookahead).parse_str("x = 1 + 2 ;"),
  Ok((true, Stmt::Assign("x", vec!["1", "2"])))
);
// The `x` was consumed by the speculation and put back by the rollback, so the
// expression branch still finds it.
assert_eq!(
  Parser::new().apply(stmt_with_lookahead).parse_str("x + 1 ;"),
  Ok((false, Stmt::Expr(vec!["x", "1"])))
);
// Committed and broken: the error survives the rollback instead of becoming a decline.
assert_eq!(
  Parser::new().apply(stmt_with_lookahead).parse_str("x = ;"),
  Err(CalcError::Unexpected)
);
```

## Guard-shaped speculation

A closure is a poor fit for control flow with several exits — a loop with two `break`s, a
`match` with an early return. [`begin`](crate::InputRef::begin) hands you a
[`Transaction`](crate::Transaction) guard instead: parse *through* it (it dereferences to the
`InputRef`), then [`commit`](crate::Transaction::commit) to keep the work or
[`rollback`](crate::Transaction::rollback) to discard it. Say nothing and the drop decides —
and **the default is rollback**, so an early `return`, a `break`, or a `?` that propagates an
error all rewind on the way out. You cannot forget to undo a speculative branch, because
undoing it is what happens if you write no code at all.

The dual exists too. [`begin_with::<Commit>`](crate::InputRef::begin_with) flips the drop
policy — the guard *keeps* progress unless you roll it back explicitly. That is what an
operator loop wants: every successful iteration keeps its tokens with no `commit()` call on
the hot path, and only the branch that backs out of a half-consumed operator says so. The
policy is a zero-sized [typestate](crate::DropPolicy) parameter: the choice is compiled in,
not branched on.

One thing overrides the policy: **a panic is not a decision**. An undecided guard dropped while
the thread is unwinding rolls back whatever policy it carries (std builds), because an unwind
aborts the region rather than completing it — keeping half an iteration would leave the input in
a state no normal execution can reach, which a host that catches would then see. `?` is a return,
not an unwind, so the keep-on-`?` behaviour above is untouched. Under `no_std` there is no
`panicking()` to read and the divergence is documented rather than fixed.

And when you need *several* live fallback points at once — the longest-match shape, where you
keep parsing and want to return to the best position you have seen — reach for
[`begin_stacked`](crate::InputRef::begin_stacked). Its
[`savepoint`](crate::StackedTransaction::savepoint)s follow SQL semantics:
[`rollback_to`](crate::StackedTransaction::rollback_to) an older savepoint destroys every
younger one (out-of-order revival is impossible by construction) while the target stays valid
for a later rollback, and [`release`](crate::StackedTransaction::release) forgets savepoints
while *keeping* the parsed progress. A [`SavepointId`](crate::SavepointId) is lifetime-branded
to its transaction, so it cannot outlive it.

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
use tokora::{Commit, Emitter, InputRef, Parse, ParseContext, Parser};

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
# fn parse_expr<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<Vec<&'inp str>, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   let mut atoms = Vec::new();
#   loop {
#     expect_tok(inp, |t| matches!(t, Tok::Int(_) | Tok::Ident))?;
#     atoms.push(inp.slice());
#     if inp.try_expect(|t| matches!(t.data(), Tok::Plus))?.is_none() {
#       return Ok(atoms);
#     }
#   }
# }
# #[derive(Debug, Clone, PartialEq)]
# enum Stmt<'a> {
#   Assign(&'a str, Vec<&'a str>),
#   Expr(Vec<&'a str>),
# }
// (Hidden: `expect_tok`, `parse_expr`, and `Stmt` from the previous example.)

// ── `begin`: rollback-on-drop, so every exit path rewinds unless you say otherwise. ──

fn parse_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Stmt<'inp>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  {
    let mut txn = inp.begin(); // ── speculative scope ──
    if txn.try_expect(|t| matches!(t.data(), Tok::Ident))?.is_some() {
      let name = txn.slice();
      if txn.try_expect(|t| matches!(t.data(), Tok::Assign))?.is_some() {
        // Committed shape. A `?` failure below still rewinds — the guard's drop runs on
        // the error path too — and the error itself propagates untouched.
        let value = parse_expr(&mut txn)?;
        expect_tok(&mut txn, |t| matches!(t, Tok::Semi))?;
        let stmt = Stmt::Assign(name, value);
        txn.commit(); // keep the work
        return Ok(stmt);
      }
    }
    // Falling out of the block drops an undecided guard: the input rewinds to the begin
    // point. No explicit rollback, and no exit path that can forget one.
  }
  let value = parse_expr(inp)?;
  expect_tok(inp, |t| matches!(t, Tok::Semi))?;
  Ok(Stmt::Expr(value))
}

assert_eq!(
  Parser::new().apply(parse_stmt).parse_str("x = 1 + 2 ;"),
  Ok(Stmt::Assign("x", vec!["1", "2"]))
);
assert_eq!(
  Parser::new().apply(parse_stmt).parse_str("x + 1 ;"),
  Ok(Stmt::Expr(vec!["x", "1"]))
);

// ── `begin_with::<Commit>`: keep-on-drop, for a loop whose common path is success. ──

/// `atom (+ atom)*`, where a dangling `+` is *not* an error: it is simply not part of the
/// expression, and must be handed back to whatever comes next.
fn parse_expr_greedy<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<Vec<&'inp str>, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  expect_tok(inp, |t| matches!(t, Tok::Int(_) | Tok::Ident))?;
  let mut atoms = vec![inp.slice()];
  loop {
    let mut txn = inp.begin_with::<Commit>();
    if txn.try_expect(|t| matches!(t.data(), Tok::Plus))?.is_none() {
      break; // no operator: nothing was consumed, so keeping "progress" is a no-op
    }
    if txn.try_expect(|t| matches!(t.data(), Tok::Int(_) | Tok::Ident))?.is_none() {
      txn.rollback(); // a dangling `+`: put it back and stop. The one explicit branch.
      break;
    }
    atoms.push(txn.slice());
    // Success. The guard drops here and *keeps* the `+ atom` — no `commit()` on the
    // hot path, which is the entire point of the `Commit` policy.
  }
  Ok(atoms)
}

/// Parses an expression and then reports the kind of the very next token — so an
/// assertion can see whether the dangling `+` really came back.
fn expr_then_peek<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(Vec<&'inp str>, Option<TokKind>), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let atoms = parse_expr_greedy(inp)?;
  let next = inp.next()?.map(|t| t.data().kind());
  Ok((atoms, next))
}

assert_eq!(
  Parser::new().apply(expr_then_peek).parse_str("1 + 2 ;"),
  Ok((vec!["1", "2"], Some(TokKind::Semi)))
);
// The half-consumed operator was handed back, not swallowed.
assert_eq!(
  Parser::new().apply(expr_then_peek).parse_str("1 + 2 + ;"),
  Ok((vec!["1", "2"], Some(TokKind::Plus)))
);

// ── `begin_stacked`: several live fallback points, and return to the best one. ──

/// Calc's `print` takes coordinate *pairs*, so a trailing odd atom is not part of the
/// list. Take a savepoint after every complete pair and, at the end, roll back to the
/// last one — the classic longest-valid-prefix shape.
fn parse_pairs<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<usize, CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut txn = inp.begin_stacked();
  let mut best = txn.savepoint(); // the empty list is always a valid answer
  let mut seen = 0usize;
  loop {
    if txn
      .try_expect(|t| matches!(t.data(), Tok::Int(_) | Tok::Ident))?
      .is_none()
    {
      break;
    }
    seen += 1;
    if seen % 2 == 0 {
      best = txn.savepoint(); // a complete pair: a better place to fall back to
    }
  }
  txn.rollback_to(best); // discard the trailing half-pair, if any
  txn.commit(); // and keep everything up to it
  Ok(seen - seen % 2)
}

fn pairs_then_peek<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(usize, Option<TokKind>), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let n = parse_pairs(inp)?;
  let next = inp.next()?.map(|t| t.data().kind());
  Ok((n, next))
}

assert_eq!(
  Parser::new().apply(pairs_then_peek).parse_str("1 2 3 4 ;"),
  Ok((4, Some(TokKind::Semi)))
);
// The odd `3` is rewound: the savepoint after the second atom wins.
assert_eq!(
  Parser::new().apply(pairs_then_peek).parse_str("1 2 3 ;"),
  Ok((2, Some(TokKind::Int)))
);
```

## Speculation that outlives the call — session points

Every tool so far is **lexical**. A guard *is* a borrow of the input, so the speculative scope
it opens can only end where that borrow does: inside one expression, one block, one call. Most
of the time that is exactly what you want, and it is why the guards cannot be misused.

But it rules out one shape. A **driver** — a REPL, an IDE, an incremental reparser — is stepped
through separate method calls: it marks a position on one call, parses on the next few, and only
*later* decides whether to keep that work. Write that with a guard and you get a value that
borrows the very input it is stored beside — self-referential, and rejected:

```rust,ignore
struct Driver<'a, 'inp, 'closure, Ctx> {
  inp: &'a mut InputRef<'inp, 'closure, CalcLexer<'inp>, Ctx>,
  txn: Transaction<'a, 'inp, 'closure, CalcLexer<'inp>, Ctx>, // ✗ borrows `inp`, beside `inp`
}
```

A **session point** is the non-lexical form. It is a *value on the input*, not a borrow of it:
[`begin_point`](crate::InputRef::begin_point) pushes a checkpoint onto the input's own stack and
hands back a plain [`SessionPointId`](crate::SessionPointId) — a `Copy` token, not a borrow, so
nothing stays borrowed and the whole consume surface — [`next`](crate::InputRef::next),
[`try_expect`](crate::InputRef::try_expect), any parser you hand the input to — is still callable
with the point open, in this call and in later ones.
[`commit_point`](crate::InputRef::commit_point) keeps the work;
[`rollback_point`](crate::InputRef::rollback_point) takes it all back — cursor, lexer state, the
token cache, and the diagnostics emitted since the mark. Both take the id. Points still settle
newest-first, so the stack *is* the last-in, first-out order; what the id buys is that a settle
names *its own* point — an id whose point is gone is refused instead of quietly settling whatever
is newest, and it cannot come to mean a different point as the stack moves under it.
[`points()`](crate::InputRef::points) is the live depth.

Here is the shape the guards cannot express: `Speculator` holds the input, `mark`s in one call,
parses in the next, and decides in a third.

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
use tokora::{Emitter, InputRef, Parse, ParseContext, Parser, SessionPointId};

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
# /// Chapter 2's `atom (+ atom)*`, hidden: it yields the atoms' source text.
# fn parse_expr<'inp, Ctx>(
#   inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
# ) -> Result<Vec<&'inp str>, CalcError>
# where
#   Ctx: ParseContext<'inp, CalcLexer<'inp>>,
#   Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
# {
#   let mut atoms = Vec::new();
#   loop {
#     expect_tok(inp, |t| matches!(t, Tok::Int(_) | Tok::Ident))?;
#     atoms.push(inp.slice());
#     if inp.try_expect(|t| matches!(t.data(), Tok::Plus))?.is_none() {
#       return Ok(atoms);
#     }
#   }
# }
/// A driver that holds the input and is stepped through separate calls. Note what `mark` does
/// **not** return: no guard, only a plain id — so nothing stays borrowed, which is precisely
/// why `parse` below is callable with a mark still open.
struct Speculator<'a, 'inp, 'closure, Ctx>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
{
  inp: &'a mut InputRef<'inp, 'closure, CalcLexer<'inp>, Ctx>,
  open: Vec<SessionPointId<'closure>>,
}

impl<'inp, Ctx> Speculator<'_, 'inp, '_, Ctx>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  /// Call 1: mark where we are. The id goes in a field — no borrow is held.
  fn mark(&mut self) {
    let point = self.inp.begin_point();
    self.open.push(point);
  }

  /// Call 2: parse for real, *through* the open mark.
  fn parse(&mut self) -> Result<Vec<&'inp str>, CalcError> {
    parse_expr(self.inp)
  }

  /// Call 3: is the statement terminated? (More real parsing, still through the mark.)
  fn at_semi(&mut self) -> Result<bool, CalcError> {
    Ok(self.inp.try_expect(|t| matches!(t.data(), Tok::Semi))?.is_some())
  }

  /// Call 4: decide — long after the mark was made, by naming the mark.
  fn keep(&mut self) {
    let point = self.open.pop().expect("a mark is open");
    self.inp.commit_point(point);
  }

  fn undo(&mut self) {
    let point = self.open.pop().expect("a mark is open");
    self.inp.rollback_point(point);
  }

  fn depth(&self) -> usize { self.inp.points() }
}

/// Speculatively parse a statement. If it is not terminated, take the whole thing back —
/// a decision made three calls after the mark.
fn speculative_stmt<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, CalcLexer<'inp>, Ctx>,
) -> Result<(Option<Vec<&'inp str>>, usize), CalcError>
where
  Ctx: ParseContext<'inp, CalcLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, CalcLexer<'inp>, Error = CalcError>,
{
  let mut spec = Speculator { inp, open: Vec::new() };

  spec.mark();                       // ── the point opens …
  assert_eq!(spec.depth(), 1);
  let atoms = spec.parse()?;         //    … real tokens are consumed …
  if spec.at_semi()? {
    spec.keep();                     //    … and it is decided here.
    Ok((Some(atoms), spec.depth()))
  } else {
    spec.undo();                     //    Everything since `mark` is gone.
    Ok((None, spec.depth()))
  }
}

// Terminated: the point commits and the work stands.
assert_eq!(
  Parser::new().apply(speculative_stmt).parse_str("x + 1 ;"),
  Ok((Some(vec!["x", "1"]), 0)),
);
// Unterminated: the rollback puts every token back, and the stack is empty again.
assert_eq!(
  Parser::new().apply(speculative_stmt).parse_str("x + 1"),
  Ok((None, 0)),
);
```

Two rules keep sessions honest. A point **pins its base**, exactly as a guard does: a rewind
reaching *below* a live point would tear its foundation out, so it panics where it is requested
instead of corrupting the timeline — which means you must settle a point before the scope that
opened it ends. And dropping the input with live points does **nothing** for them: a session ends
*explicitly*. Implicitly rolling one back on drop would paper over a driver that lost track of
its own points — the deliberate opposite of a guard's drop policy, and for the same reason: the
failure you cannot see is the one that hurts.

Backtracking rewinds *diagnostics* too — which raises the question of what a diagnostic even
is here, and how a parser reports more than one. That is the next chapter.
Next: [chapter 7](super::ch07_diagnostics).
