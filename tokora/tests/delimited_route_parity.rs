#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]
#![allow(clippy::type_complexity)]
//! The two delimited routes, held to the same answers.
//!
//! A `()`-delimited construct can be parsed two ways in this crate, and both ship:
//!
//! - the **named** route — [`parens`], [`braces`], [`brackets`], [`angles`] — which asks the
//!   *token* what it is, through `PunctuatorToken`'s `close_paren() -> Option<Kind>` family;
//! - the **generic** route — `delimited::<D>` — which asks the *pair marker*, through
//!   `Punctuator::kind()`, and therefore needs `<L::Token as Token>::Kind: From<CloseParen>`.
//!
//! Both funnel into the same `commit_delim_close` body; what differs is the five slots each
//! fills. The obvious tidy-up — fold the four named bodies onto the generic one — was attempted
//! and **stopped**: the two routes are keyed on two different traits, and `PunctuatorToken` does
//! not imply `Kind: From<OpenParen>`/`From<CloseParen>`, so folding adds those bounds to the four
//! public named parsers. They are not the same function with different spelling; they are two
//! vocabularies that happen to agree.
//!
//! *Happen to agree* is the part worth pinning, because nothing makes them. A token type declares
//! its punctuator kinds twice — once as `PunctuatorToken::close_paren()`, once as
//! `From<CloseParen<(), (), ()>> for Kind` — and no coherence rule ties the two declarations
//! together. A token type whose two declarations disagree would send the two routes to different
//! answers, silently. This suite is where that would show.
//!
//! Three tables, all exhaustive rather than sampled:
//!
//! 1. **the close predicate**, over every `TokenKind` × all four pairs;
//! 2. **the close-miss error**, compared as a *value* (`UnexpectedToken` derives `PartialEq`, so
//!    equal values render equally under every emitter) over the same grid;
//! 3. **the corpus** — 4 pairs × {unclosed-at-EOF, wrong-closer, wrong-opener, nested-unclosed},
//!    parsed both ways under a recording emitter, compared as whole recorded vectors plus the
//!    final cursor.
//!
//! The corpus carries its own positive control: a deliberately mismatched pair of routes must
//! report a difference, or the comparison is not looking at anything.

mod common;

use common::{TestLexer, Token, TokenKind};
use tokora::{
  InputRef, Parse, Parser, ParserContext, SimpleSpan,
  delimiter::Delimiter,
  emitter::Verbose,
  error::{
    Unclosed,
    syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
    token::{MissingToken, SeparatedError, UnexpectedToken},
  },
  parser::{angles, braces, brackets, delimited, parens},
  punct::{
    Angle, Brace, Bracket, CloseAngle, CloseBrace, CloseBracket, CloseParen, Paren, Punctuator,
  },
  span::Spanned,
  token::{PunctuatorTokenExt, SpannedPunctuatorToken},
  utils::Expected,
};

/// Every kind `TestLexer` can produce. Spelled out rather than iterated so a new variant is a
/// compile-visible decision here too.
const KINDS: &[TokenKind] = &[
  TokenKind::Num,
  TokenKind::Comma,
  TokenKind::Semi,
  TokenKind::Plus,
  TokenKind::Minus,
  TokenKind::Star,
  TokenKind::Slash,
  TokenKind::LParen,
  TokenKind::RParen,
  TokenKind::LBracket,
  TokenKind::RBracket,
  TokenKind::LBrace,
  TokenKind::RBrace,
  TokenKind::LAngle,
  TokenKind::RAngle,
  TokenKind::Eq,
  TokenKind::Ident,
];

fn tok_of(k: TokenKind) -> Token {
  match k {
    TokenKind::Num => Token::Num(7),
    TokenKind::Comma => Token::Comma,
    TokenKind::Semi => Token::Semi,
    TokenKind::Plus => Token::Plus,
    TokenKind::Minus => Token::Minus,
    TokenKind::Star => Token::Star,
    TokenKind::Slash => Token::Slash,
    TokenKind::LParen => Token::LParen,
    TokenKind::RParen => Token::RParen,
    TokenKind::LBracket => Token::LBracket,
    TokenKind::RBracket => Token::RBracket,
    TokenKind::LBrace => Token::LBrace,
    TokenKind::RBrace => Token::RBrace,
    TokenKind::LAngle => Token::LAngle,
    TokenKind::RAngle => Token::RAngle,
    TokenKind::Eq => Token::Eq,
    TokenKind::Ident => Token::Ident,
  }
}

/// Slot 3 — `t.is_close_x()` (token-keyed) against `<CloseX as Punctuator>::eval(kind)`
/// (marker-keyed), over the whole kind set and all four pairs.
#[test]
fn close_predicate_agrees_on_every_kind() {
  for k in KINDS {
    let t = tok_of(*k);
    let by_token = (
      t.is_close_paren(),
      t.is_close_brace(),
      t.is_close_bracket(),
      t.is_close_angle(),
    );
    let by_marker = (
      <CloseParen<(), (), ()> as Punctuator<'_, TestLexer<'_>, ()>>::eval(k),
      <CloseBrace<(), (), ()> as Punctuator<'_, TestLexer<'_>, ()>>::eval(k),
      <CloseBracket<(), (), ()> as Punctuator<'_, TestLexer<'_>, ()>>::eval(k),
      <CloseAngle<(), (), ()> as Punctuator<'_, TestLexer<'_>, ()>>::eval(k),
    );
    assert_eq!(
      by_token, by_marker,
      "the two close predicates disagree at {k:?}: the token's own `is_close_*` says \
       {by_token:?} and the pair marker's `Punctuator::eval` says {by_marker:?}. They are two \
       independent declarations on the same token type, and the named and generic delimited \
       routes read one each"
    );
  }
  // The grid must contain a `true` in each column, or agreement is agreement about nothing.
  let closers = [
    TokenKind::RParen,
    TokenKind::RBrace,
    TokenKind::RBracket,
    TokenKind::RAngle,
  ];
  for c in closers {
    let t = tok_of(c);
    assert!(
      t.is_close_paren() || t.is_close_brace() || t.is_close_bracket() || t.is_close_angle(),
      "{c:?} must match some close predicate, or the table is all-false and proves nothing"
    );
  }
}

macro_rules! close_miss_table {
  ($label:literal, $pair:ty, $expect:ident, $misses:ident) => {{
    let mut misses = 0usize;
    for k in KINDS {
      let sp = Spanned::new(SimpleSpan::new(3, 4), tok_of(*k));
      let by_token = SpannedPunctuatorToken::<'_, TestLexer<'_>, ()>::$expect(sp.clone());
      let by_marker = <$pair as Delimiter<'_, TestLexer<'_>, ()>>::unexpected_close_token(sp);
      if let Err(e) = by_token {
        assert!(
          e == by_marker,
          "the close-miss error differs for {} at {k:?}: token route expected {:?} / found \
           {:?} / span {:?}, marker route expected {:?} / found {:?} / span {:?}",
          $label,
          e.expected(),
          e.found(),
          e.span(),
          by_marker.expected(),
          by_marker.found(),
          by_marker.span(),
        );
        misses += 1;
      }
    }
    $misses += misses;
  }};
}

/// Slot 5 — the close-miss diagnostic, compared as a value. `UnexpectedToken` derives
/// `PartialEq`, so equal values are equal renders under every emitter; this is stronger than
/// comparing two rendered strings and does not depend on a `Display` shim.
///
/// The matching kind is excluded from the comparison by construction, and deliberately: there the
/// token route returns `Ok(the closer)` while the marker route returns a self-contradicting
/// `expected ')' found ')'`. `commit_delim_close` documents that arm as unreachable — the close
/// probe has already reported a wrong token — so the two routes disagree only where neither is
/// ever consulted. It is recorded here rather than hidden because it is exactly the kind of
/// difference a fold would adopt silently.
#[test]
fn close_miss_error_agrees_on_every_kind() {
  let mut misses = 0usize;
  close_miss_table!("()", Paren<(), (), ()>, expect_close_paren, misses);
  close_miss_table!("{}", Brace<(), (), ()>, expect_close_brace, misses);
  close_miss_table!("[]", Bracket<(), (), ()>, expect_close_bracket, misses);
  close_miss_table!("<>", Angle<(), (), ()>, expect_close_angle, misses);
  assert_eq!(
    misses,
    KINDS.len() * 4 - 4,
    "every kind but each pair's own closer must take the close-miss branch, or the table \
     compared fewer errors than it looks like it did"
  );
}

// ── The corpus ────────────────────────────────────────────────────────────────

/// A recorded diagnostic, reduced to the facts a consumer can see: which family, the expected and
/// found kinds, the pair identity, and the span. A fold that lost any of them would show here.
#[derive(Debug, Clone, PartialEq, Eq)]
enum R {
  Unexpected {
    expected: Option<TokenKind>,
    found: Option<TokenKind>,
    span: (usize, usize),
  },
  Unclosed {
    kind: String,
    name: String,
    span: (usize, usize),
  },
  Eot,
  Cursor(usize),
  Other(&'static str),
}

fn sp(s: SimpleSpan) -> (usize, usize) {
  (s.start(), s.end())
}

impl<Lang: ?Sized> From<UnexpectedToken<'_, Token, TokenKind, SimpleSpan, Lang>> for R {
  fn from(e: UnexpectedToken<'_, Token, TokenKind, SimpleSpan, Lang>) -> Self {
    R::Unexpected {
      expected: match e.expected() {
        Some(Expected::One(k)) => Some(*k),
        _ => None,
      },
      found: e.found().map(TokenKind::from),
      span: sp(e.span()),
    }
  }
}

impl<H, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEnd<H, usize, Lang, Set>>
  for R
{
  fn from(_: tokora::error::UnexpectedEnd<H, usize, Lang, Set>) -> Self {
    R::Eot
  }
}
impl From<()> for R {
  fn from(_: ()) -> Self {
    R::Other("unit")
  }
}
impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for R {
  fn from(_: FullContainer<S, Lang>) -> Self {
    R::Other("full-container")
  }
}
impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for R {
  fn from(_: TooFew<S, Lang>) -> Self {
    R::Other("too-few")
  }
}
impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for R {
  fn from(_: TooMany<S, Lang>) -> Self {
    R::Other("too-many")
  }
}
impl<'a, K: Clone, O, Lang: ?Sized> From<MissingToken<'a, K, O, Lang>> for R {
  fn from(_: MissingToken<'a, K, O, Lang>) -> Self {
    R::Other("missing-token")
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<SeparatedError<'a, T, K, S, Lang>> for R {
  fn from(_: SeparatedError<'a, T, K, S, Lang>) -> Self {
    R::Other("separated")
  }
}
impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for R {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    R::Other("missing-syntax")
  }
}
impl<D, Lang: ?Sized> From<Unclosed<D, SimpleSpan, Lang>> for R {
  fn from(e: Unclosed<D, SimpleSpan, Lang>) -> Self {
    R::Unclosed {
      kind: format!("{:?}", e.kind()),
      name: e.name_ref().to_string(),
      span: sp(e.span()),
    }
  }
}
impl<'inp, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, TestLexer<'inp>, Lang> for R {
  fn from_unclosed<D>(e: Unclosed<D, SimpleSpan, Lang>) -> Self {
    R::from(e)
  }
}

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<R>>;

fn vctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

/// Zero or more `Num`s, and it never fails — so a corpus row differs only where the delimiter
/// path differs.
fn nums<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<usize, R> {
  let mut n = 0;
  while inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))?
    .is_some()
  {
    n += 1;
  }
  Ok(n)
}

/// Runs one parser over one source and returns everything observable: the recorded diagnostics in
/// span order, the returned error if the parse failed outright, and where the cursor came to rest.
fn observe<P, T>(mut p: P, src: &'static str) -> Vec<R>
where
  P: for<'c> FnMut(&mut InputRef<'static, 'c, TestLexer<'static>, VCtx<'static>>) -> Result<T, R>,
{
  let probe =
    move |inp: &mut InputRef<'static, '_, TestLexer<'static>, VCtx<'static>>| -> Result<Vec<R>, R> {
      let outcome = p(inp);
      let mut rows: Vec<R> = inp
        .emitter_ref()
        .errors()
        .values()
        .flatten()
        .cloned()
        .collect();
      if let Err(e) = outcome {
        rows.push(e);
      }
      rows.push(R::Cursor(*inp.cursor().as_inner()));
      Ok(rows)
    };
  Parser::with_context(vctx())
    .apply(probe)
    .parse_str(src)
    .expect("the probe never fails: it converts the parse outcome into rows")
}

macro_rules! corpus_row {
  ($pair:literal, $shape:literal, $src:literal, $named:expr, $generic:expr) => {{
    let by_name = observe($named, $src);
    let by_marker = observe($generic, $src);
    assert_eq!(
      by_name, by_marker,
      "{} / {} on {:?}: the named and generic routes diverged. Only renders may differ between \
       them, and only after review — a pair name, a kind or a span that moves is information a \
       consumer loses",
      $pair, $shape, $src
    );
    assert!(
      by_name.len() >= 2,
      "{} / {} on {:?} recorded no diagnostic at all — the row is not exercising a close miss",
      $pair,
      $shape,
      $src
    );
  }};
}

/// The corpus: 4 pairs × 4 shapes, both routes, whole recorded vectors.
#[test]
fn corpus_named_and_generic_routes_agree() {
  corpus_row!(
    "()",
    "unclosed-eof",
    "( 1",
    parens(nums),
    delimited::<Paren<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "()",
    "wrong-closer",
    "( 1 ]",
    parens(nums),
    delimited::<Paren<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "()",
    "wrong-opener",
    "x 1 )",
    parens(nums),
    delimited::<Paren<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "()",
    "nested-unclosed",
    "( ( 1 )",
    parens(parens(nums)),
    delimited::<Paren<(), (), ()>, _, _, _, _, _, _>(delimited::<
      Paren<(), (), ()>,
      _,
      _,
      _,
      _,
      _,
      _,
    >(nums))
  );

  corpus_row!(
    "{}",
    "unclosed-eof",
    "{ 1",
    braces(nums),
    delimited::<Brace<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "{}",
    "wrong-closer",
    "{ 1 ]",
    braces(nums),
    delimited::<Brace<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "{}",
    "wrong-opener",
    "x 1 }",
    braces(nums),
    delimited::<Brace<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "{}",
    "nested-unclosed",
    "{ { 1 }",
    braces(braces(nums)),
    delimited::<Brace<(), (), ()>, _, _, _, _, _, _>(delimited::<
      Brace<(), (), ()>,
      _,
      _,
      _,
      _,
      _,
      _,
    >(nums))
  );

  corpus_row!(
    "[]",
    "unclosed-eof",
    "[ 1",
    brackets(nums),
    delimited::<Bracket<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "[]",
    "wrong-closer",
    "[ 1 }",
    brackets(nums),
    delimited::<Bracket<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "[]",
    "wrong-opener",
    "x 1 ]",
    brackets(nums),
    delimited::<Bracket<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "[]",
    "nested-unclosed",
    "[ [ 1 ]",
    brackets(brackets(nums)),
    delimited::<Bracket<(), (), ()>, _, _, _, _, _, _>(delimited::<
      Bracket<(), (), ()>,
      _,
      _,
      _,
      _,
      _,
      _,
    >(nums))
  );

  corpus_row!(
    "<>",
    "unclosed-eof",
    "< 1",
    angles(nums),
    delimited::<Angle<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "<>",
    "wrong-closer",
    "< 1 ]",
    angles(nums),
    delimited::<Angle<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "<>",
    "wrong-opener",
    "x 1 >",
    angles(nums),
    delimited::<Angle<(), (), ()>, _, _, _, _, _, _>(nums)
  );
  corpus_row!(
    "<>",
    "nested-unclosed",
    "< < 1 >",
    angles(angles(nums)),
    delimited::<Angle<(), (), ()>, _, _, _, _, _, _>(delimited::<
      Angle<(), (), ()>,
      _,
      _,
      _,
      _,
      _,
      _,
    >(nums))
  );
}

/// The corpus comparison must be able to see a difference.
///
/// Every row above passes, which is exactly the shape of a comparison that is not comparing
/// anything. Two routes that genuinely disagree — a `(`-opened source parsed as a `{}` construct —
/// must come out unequal, or `corpus_named_and_generic_routes_agree` proves nothing.
#[test]
fn the_corpus_comparison_can_fail() {
  let by_name = observe(parens(nums), "( 1 ]");
  let mismatched = observe(
    delimited::<Brace<(), (), ()>, _, _, _, _, _, _>(nums),
    "( 1 ]",
  );
  assert_ne!(
    by_name, mismatched,
    "the corpus comparison cannot tell two different delimited routes apart, so its agreement \
     rows carry no information"
  );
}
