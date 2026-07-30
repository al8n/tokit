#![cfg(all(
  feature = "std",
  any(feature = "logos_0_16", feature = "logos_0_15", feature = "logos_0_14")
))]

//! Session points, exercised through the **public API only**.
//!
//! This is an integration test on purpose. It links against tokora the way a downstream crate does
//! — no `pub(crate)`, no `#[cfg(test)]` back door — so whatever compiles here is reachable
//! downstream by construction. The raw checkpoint triple (`save`/`restore`/`commit`) is invisible
//! from out here without `unstable-raw`, and `Checkpoint` can be neither made nor spent: the
//! session verbs below are the whole surface.
//!
//! What is being proved is a *shape*, not just a set of return values. A transaction guard
//! ([`Transaction`](tokora::Transaction)) is a borrow of the input, so a driver cannot hold one
//! beside the input it borrows — the value would be self-referential — and a speculative scope
//! can therefore never outlive the call that opened it. [`Driver`] below does exactly that
//! forbidden thing with session points: it holds the input handle as a field, `mark()`s a position
//! in one method call, consumes **real tokens** in later ones, and decides the point in a call
//! after that. Rewrite `Driver` with a guard and it does not compile.

mod common;

use common::{TestLexer, Token};
use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, ParserContext, SessionPointId,
  emitter::Verbose,
  error::{UnexpectedEot, token::UnexpectedToken},
  span::{SimpleSpan, Spanned},
};

// ── Error type + context ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum SessionError {
  Lex,
  Unexpected,
  UnexpectedEnd,
  /// The application diagnostic the tests emit and then watch a rollback take back.
  Speculative,
}

impl From<()> for SessionError {
  fn from(_: ()) -> Self {
    SessionError::Lex
  }
}

impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>>
  for SessionError
{
  fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
    SessionError::Unexpected
  }
}

impl From<UnexpectedEot> for SessionError {
  fn from(_: UnexpectedEot) -> Self {
    SessionError::UnexpectedEnd
  }
}

/// A collecting (non-fatal) emitter, so a rolled-back diagnostic is observably *gone* rather than
/// having aborted the parse.
fn verbose_ctx() -> ParserContext<'static, TestLexer<'static>, Verbose<SessionError>> {
  ParserContext::new(Verbose::new())
}

/// The input handle a downstream parser function is handed. Everything below runs on this.
type Ir<'a, 'inp, 'closure> = &'a mut InputRef<
  'inp,
  'closure,
  TestLexer<'inp>,
  ParserContext<'inp, TestLexer<'inp>, Verbose<SessionError>>,
>;

// ── The driver: the shape a borrowing guard cannot express ────────────────────

/// A parse driver that **owns its handle on the input** and is stepped through separate method
/// calls. Speculation opened in one call is decided in a later one — with real token consumption
/// in between.
struct Driver<'a, 'inp, 'closure> {
  inp: Ir<'a, 'inp, 'closure>,
  /// The ids of the open marks, oldest first. A driver whose speculation is non-lexical has to
  /// keep this itself — which is the point: the settle verbs name the mark rather than trusting a
  /// position, so a mark cannot be settled by accident once the stack has moved under it.
  marks: Vec<SessionPointId<'closure>>,
}

impl<'inp> Driver<'_, 'inp, '_> {
  /// Mark the current position. Note what is *not* in the signature: no guard, and no borrow held
  /// afterwards — only a plain id stored in a field — so `step()` below is callable with the mark
  /// still open.
  fn mark(&mut self) {
    let point = self.inp.begin_point();
    self.marks.push(point);
  }

  /// Consume one real token and hand back its source text — a separate call, made *through* an
  /// open mark.
  fn step(&mut self) -> Result<Option<&'inp str>, SessionError> {
    Ok(match self.inp.next()? {
      Some(_) => Some(self.inp.slice()),
      None => None,
    })
  }

  /// Record an application diagnostic at `at` (checkpoint-tracked work, like the tokens).
  fn complain(&mut self, at: usize) {
    let span = SimpleSpan::new(at, at + 1);
    <Verbose<SessionError> as Emitter<'inp, TestLexer<'inp>>>::emit_error(
      self.inp.emitter(),
      Spanned::new(span, SessionError::Speculative),
    )
    .expect("Verbose is non-fatal");
  }

  /// Decide the newest open mark: keep its work, or take it all back.
  fn decide(&mut self, keep: bool) {
    let point = self.marks.pop().expect("a mark is open");
    if keep {
      self.inp.commit_point(point);
    } else {
      self.inp.rollback_point(point);
    }
  }

  /// How deep the speculation currently is.
  fn depth(&self) -> usize {
    self.inp.points()
  }

  /// Where the input is now — the fact only real consumption moves.
  fn at(&self) -> usize {
    *self.inp.cursor().as_inner()
  }

  /// How many diagnostics the emitter is currently holding.
  fn diagnostics(&mut self) -> usize {
    self.inp.emitter().errors().values().map(|g| g.len()).sum()
  }
}

/// What one driven session observed, carried out of the parse so the test can assert on it (the
/// emitter is consumed by the parse, so the observations must ride out on the output).
#[derive(Debug, PartialEq)]
struct Trace<'a> {
  at_mark: usize,
  depth_at_mark: usize,
  tokens_seen: Vec<&'a str>,
  at_after_work: usize,
  diags_after_work: usize,
  at_after_decide: usize,
  diags_after_decide: usize,
  depth_after_decide: usize,
  next_after_decide: Option<&'a str>,
}

// ── 1. Rollback: the tokens AND the diagnostics come back ─────────────────────

/// Marks, consumes two real tokens across separate calls, emits a diagnostic, then rolls back.
fn rollback_session<'inp>(inp: Ir<'_, 'inp, '_>) -> Result<Trace<'inp>, SessionError> {
  let mut d = Driver {
    inp,
    marks: Vec::new(),
  };

  // One token of committed work before any speculation.
  assert_eq!(d.step()?, Some("1"), "committed work precedes the session");

  let at_mark = d.at();
  d.mark(); //                                     ← the point opens here …
  let depth_at_mark = d.depth();

  // … and the parse keeps going, in calls the mark knows nothing about.
  let mut tokens_seen = Vec::new();
  tokens_seen.push(d.step()?.expect("2"));
  d.complain(2);
  tokens_seen.push(d.step()?.expect("3"));

  let at_after_work = d.at();
  let diags_after_work = d.diagnostics();

  d.decide(false); //                              ← … and is decided here, three calls later.

  let at_after_decide = d.at();
  let diags_after_decide = d.diagnostics();
  let depth_after_decide = d.depth();
  let next_after_decide = d.step()?;

  Ok(Trace {
    at_mark,
    depth_at_mark,
    tokens_seen,
    at_after_work,
    diags_after_work,
    at_after_decide,
    diags_after_decide,
    depth_after_decide,
    next_after_decide,
  })
}

#[test]
fn rollback_restores_the_cursor_and_the_emission_log() {
  let t = Parser::with_context(verbose_ctx())
    .apply(rollback_session)
    .parse_str("1 2 3 4")
    .expect("the session parses");

  assert_eq!(t.depth_at_mark, 1, "the mark is live");
  assert_eq!(
    t.tokens_seen,
    vec!["2", "3"],
    "real tokens were consumed through the open mark"
  );
  assert!(
    t.at_after_work > t.at_mark,
    "the speculative work genuinely moved the cursor ({} → {})",
    t.at_mark,
    t.at_after_work
  );
  assert_eq!(t.diags_after_work, 1, "and genuinely emitted a diagnostic");

  // The whole point of the wave:
  assert_eq!(
    t.at_after_decide, t.at_mark,
    "rollback restored the CURSOR to the mark"
  );
  assert_eq!(
    t.diags_after_decide, 0,
    "rollback restored the EMISSION LOG to the mark"
  );
  assert_eq!(t.depth_after_decide, 0, "the point settled");
  assert_eq!(
    t.next_after_decide,
    Some("2"),
    "the rewound tokens are back on the stream"
  );
}

// ── 2. Commit: the work through the point is kept ─────────────────────────────

fn commit_session<'inp>(inp: Ir<'_, 'inp, '_>) -> Result<Trace<'inp>, SessionError> {
  let mut d = Driver {
    inp,
    marks: Vec::new(),
  };

  assert_eq!(d.step()?, Some("1"));
  let at_mark = d.at();
  d.mark();
  let depth_at_mark = d.depth();

  let mut tokens_seen = Vec::new();
  tokens_seen.push(d.step()?.expect("2"));
  d.complain(2);
  tokens_seen.push(d.step()?.expect("3"));

  let at_after_work = d.at();
  let diags_after_work = d.diagnostics();

  d.decide(true); // keep it

  Ok(Trace {
    at_mark,
    depth_at_mark,
    tokens_seen,
    at_after_work,
    diags_after_work,
    at_after_decide: d.at(),
    diags_after_decide: d.diagnostics(),
    depth_after_decide: d.depth(),
    next_after_decide: d.step()?,
  })
}

#[test]
fn commit_keeps_the_work_done_through_the_point() {
  let t = Parser::with_context(verbose_ctx())
    .apply(commit_session)
    .parse_str("1 2 3 4")
    .expect("the session parses");

  assert_eq!(t.tokens_seen, vec!["2", "3"]);
  assert_eq!(
    t.at_after_decide, t.at_after_work,
    "commit kept the cursor where the speculative work left it"
  );
  assert_eq!(
    t.diags_after_decide, 1,
    "commit kept the diagnostic emitted through the point"
  );
  assert_eq!(t.depth_after_decide, 0, "the point settled");
  assert_eq!(
    t.next_after_decide,
    Some("4"),
    "the stream resumes after the committed work"
  );
}

// ── 3. Nesting is last-in, first-out, and `points()` tracks the depth ─────────

/// The depth after every step of a nested session, plus the cursor at each settle.
#[derive(Debug, PartialEq)]
struct Nested<'a> {
  depths: Vec<usize>,
  at_p1: usize,
  at_p3: usize,
  after_rollback_p3: usize,
  after_commit_p2: usize,
  after_rollback_p1: usize,
  replayed: Option<&'a str>,
}

fn nested_session<'inp>(inp: Ir<'_, 'inp, '_>) -> Result<Nested<'inp>, SessionError> {
  let mut d = Driver {
    inp,
    marks: Vec::new(),
  };
  let mut depths = Vec::new();

  depths.push(d.depth()); // 0
  d.step()?; // "1"
  let at_p1 = d.at();
  d.mark(); // P1
  depths.push(d.depth()); // 1

  d.step()?; // "2"
  d.mark(); // P2
  depths.push(d.depth()); // 2

  d.step()?; // "3"
  let at_p3 = d.at();
  d.mark(); // P3
  depths.push(d.depth()); // 3

  d.step()?; // "4" — speculative work under all three

  d.decide(false); // roll back the NEWEST (P3)
  depths.push(d.depth()); // 2
  let after_rollback_p3 = d.at();

  d.decide(true); // commit the middle (P2) — keeps the current position
  depths.push(d.depth()); // 1
  let after_commit_p2 = d.at();

  d.decide(false); // roll back the oldest (P1) — unaffected by the middle commit
  depths.push(d.depth()); // 0
  let after_rollback_p1 = d.at();

  let replayed = d.step()?;

  Ok(Nested {
    depths,
    at_p1,
    at_p3,
    after_rollback_p3,
    after_commit_p2,
    after_rollback_p1,
    replayed,
  })
}

#[test]
fn nested_points_settle_newest_first() {
  let n = Parser::with_context(verbose_ctx())
    .apply(nested_session)
    .parse_str("1 2 3 4 5")
    .expect("the session parses");

  assert_eq!(
    n.depths,
    vec![0, 1, 2, 3, 2, 1, 0],
    "points() tracks the live depth through the whole lifecycle"
  );
  assert_eq!(
    n.after_rollback_p3, n.at_p3,
    "rolling back the newest returns to ITS mark"
  );
  assert_eq!(
    n.after_commit_p2, n.at_p3,
    "committing the middle keeps the current position"
  );
  assert_eq!(
    n.after_rollback_p1, n.at_p1,
    "rolling back the oldest returns to ITS mark — undisturbed by the middle commit"
  );
  assert_eq!(
    n.replayed,
    Some("2"),
    "and the stream replays faithfully from there"
  );
}

// ── 4. A session point survives across an ordinary parser call ────────────────

/// An ordinary downstream parser function — it knows nothing about sessions.
fn sum_two_numbers<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx>,
) -> Result<i64, SessionError>
where
  Ctx: ParseContext<'inp, TestLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, TestLexer<'inp>, Error = SessionError>,
{
  let mut total = 0;
  for _ in 0..2 {
    match inp.next()? {
      Some(t) => match t.data() {
        Token::Num(n) => total += n,
        _ => return Err(SessionError::Unexpected),
      },
      None => return Err(SessionError::UnexpectedEnd),
    }
  }
  Ok(total)
}

fn session_around_a_parser(inp: Ir<'_, '_, '_>) -> Result<(i64, usize, usize, i64), SessionError> {
  let start = *inp.cursor().as_inner();

  // Mark, then hand the input — mark and all — to a plain parser that consumes through it.
  let point = inp.begin_point();
  let speculative = sum_two_numbers(inp)?;
  let depth_inside = inp.points();
  let moved = *inp.cursor().as_inner();
  assert!(moved > start, "the sub-parser consumed through the point");

  // Take it all back and re-run the same parser: it must see the same tokens again.
  inp.rollback_point(point);
  assert_eq!(
    *inp.cursor().as_inner(),
    start,
    "the sub-parser's work was rewound"
  );
  let replayed = sum_two_numbers(inp)?;

  Ok((speculative, depth_inside, inp.points(), replayed))
}

#[test]
fn a_point_spans_an_ordinary_parser_call() {
  let (speculative, depth_inside, depth_after, replayed) = Parser::with_context(verbose_ctx())
    .apply(session_around_a_parser)
    .parse_str("10 20 30")
    .expect("the session parses");

  assert_eq!(speculative, 30, "10 + 20, parsed speculatively");
  assert_eq!(depth_inside, 1, "the point stayed open across the call");
  assert_eq!(depth_after, 0, "and settled after it");
  assert_eq!(
    replayed, 30,
    "the rolled-back tokens re-lexed identically for the second run"
  );
}
