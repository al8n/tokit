//! Match-first token dispatch — the [`select!`](crate::select) macro and its runtime,
//! [`dispatch_take`] / [`try_dispatch_take`].
//!
//! The macro is pure syntax; the semantics live in the two public functions. That keeps
//! the `pub(crate)` boundary machinery (`at_latched_boundary`) inside the crate — a
//! macro expanded in a consumer crate cannot reach it — and it makes the protocol
//! directly testable without going through the macro.

use crate::{
  ComposableParseContext, Emitter, Lexer, Token,
  error::{UnexpectedEnd, UnexpectedEot, token::UnexpectedToken},
  input::{InputRef, SurfaceIncomplete},
  span::{Span as _, Spanned},
  try_parse_input::ParseAttempt,
};

/// Committed match-first dispatch: classify the head against `table` once, commit it,
/// and hand it **by value** to `project` (the macro-generated match).
///
/// A head outside the table errors with the whole table as the expected set and the
/// found token; end of input errors with the table-shaped end-of-token-stream error,
/// marked terminal exactly when the boundary is latched. `project` returning `Err` is
/// the law-violation arm — the kind matched but the variant did not — and it hands the
/// token **back**, so the error is built here, where `Lang` is pinned, rather than in
/// consumer-expanded macro code where it cannot be inferred.
///
/// The single classification and the whole-table expected set are the fused dispatch
/// driver's protocol, re-based on
/// [`try_expect_take`](crate::input::InputRef::try_expect_take).
#[inline]
pub fn dispatch_take<'inp, L, Ctx, Lang, Cmpl, O, P>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  table: &'static [<L::Token as Token<'inp>>::Kind],
  project: P,
) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  L::Token: Clone,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: FnOnce(Spanned<L::Token, L::Span>) -> Result<O, Spanned<L::Token, L::Span>>,
{
  let mut miss = None;
  let taken = inp.try_expect_take(
    |t| {
      let kind = Token::kind(t.data);
      if table.contains(&kind) {
        true
      } else {
        // Miss capture for the committed error path only — the same clone the fused
        // driver pays (`fused/mod.rs`), on the error path, never on the hit path.
        miss = Some((t.span.clone(), t.data.clone()));
        false
      }
    },
    // The kind matched but the variant did not (the kind↔variant law is the dialect's
    // claim, not ours): a typed error built HERE, where `Lang` is pinned, never in
    // consumer-expanded macro code where it cannot infer.
    |sp| match project(sp) {
      Ok(o) => Ok(o),
      Err(back) => {
        let (span, found) = back.into_components();
        Err(
          UnexpectedToken::<_, _, _, Lang>::expected_one_of(span, table)
            .with_found(found)
            .into(),
        )
      }
    },
  )?;
  match taken {
    Some(v) => Ok(v),
    None => match miss {
      Some((span, found)) => Err(
        UnexpectedToken::<_, _, _, Lang>::expected_one_of(span, table)
          .with_found(found)
          .into(),
      ),
      None => {
        let eot = UnexpectedEnd::eot_expected_one_of(inp.span().end(), table);
        if inp.at_latched_boundary() {
          Err(eot.into_terminal().into())
        } else {
          Err(eot.into())
        }
      }
    },
  }
}

/// Tentative match-first dispatch: a head outside the table, or genuine end of input,
/// **declines with zero consumption**; a terminal stop is an error, never a decline.
///
/// No clone anywhere — the miss path builds no error, so it pays nothing.
#[inline]
pub fn try_dispatch_take<'inp, L, Ctx, Lang, Cmpl, O, P>(
  inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  table: &'static [<L::Token as Token<'inp>>::Kind],
  project: P,
) -> Result<ParseAttempt<O>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ComposableParseContext<'inp, L, Lang>,
  Lang: ?Sized,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  P: FnOnce(Spanned<L::Token, L::Span>) -> Result<O, Spanned<L::Token, L::Span>>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  let taken = inp.try_expect_take_or_stop(
    |t| {
      let kind = Token::kind(t.data);
      table.contains(&kind)
    },
    |sp| match project(sp) {
      Ok(o) => Ok(o),
      Err(back) => {
        let (span, found) = back.into_components();
        Err(
          UnexpectedToken::<_, _, _, Lang>::expected_one_of(span, table)
            .with_found(found)
            .into(),
        )
      }
    },
  )?;
  Ok(match taken {
    Some(v) => ParseAttempt::Accept(v),
    None => ParseAttempt::Decline,
  })
}

/// Match-first token dispatch, one arm per kind.
///
/// ```ignore
/// let v = tokora::select!(inp, {
///   Kind::Int   => (span, Tok::Int(v))   => IntValue::new(span, v).into(),
///   Kind::Float => (span, Tok::Float(v)) => FloatValue::new(span, v).into(),
/// })?;
/// ```
///
/// Expands to one [`dispatch_take`](crate::parser::dispatch_take) call: the kind table
/// is written once next to the patterns (static-promoted), the token is classified
/// once, and each arm receives the **moved** payload — no `unreachable!()` arm is
/// written by hand, and the law-violation fallback (kind matched, variant did not) is a
/// typed error carrying the table.
///
/// # The kind expressions must be const-promotable
///
/// The expansion builds `&[$kind, ...]` and hands it to a runtime that takes
/// `&'static [Kind]`, so the slice must survive by **static promotion**. Every kind
/// expression therefore has to be promotable: a unit-variant path (`Kind::Int`), a
/// `const`, or any expression rustc can promote. A call, a method, or anything that
/// reads a local produces a temporary the borrow cannot outlive, and the call site
/// fails with:
///
/// ```text
/// error[E0716]: temporary value dropped while borrowed
/// ```
///
/// pointing at the `select!` invocation, not at the offending arm. If you hit it, bind
/// the kind to a `const` and use the const's path in the arm. Realistic grammars spell
/// kinds as unit-variant paths, so the constraint is invisible in practice — but the
/// diagnostic does not name the cause, which is why it is named here.
///
/// # Availability
///
/// `select!` is available wherever [`dispatch_take`](crate::parser::dispatch_take) is:
/// any [`Completeness`](crate::input::Completeness) — [`Complete`](crate::input::Complete)
/// and streaming [`Partial`](crate::input::Partial) alike — but the context must be a
/// [`ComposableParseContext`](crate::ComposableParseContext), i.e. its emitter error
/// must carry the standard token-error conversions
/// ([`FromTokenErrors`](crate::emitter::FromTokenErrors)). A bare
/// [`ParseContext`](crate::ParseContext) whose error type lacks one of those conversions
/// cannot use the macro; call `try_expect`/`try_expect_take` directly there.
#[macro_export]
macro_rules! select {
  ($inp:expr, { $($kind:expr => ($span:pat, $pat:pat) => $body:expr),+ $(,)? }) => {{
    let __table = &[$($kind),+];
    $crate::parser::dispatch_take($inp, __table, |__sp| {
      match (__sp.span, __sp.data) {
        $( ($span, $pat) => ::core::result::Result::Ok($body), )+
        // The give-back arm. `#[allow]` because a grammar whose arm patterns happen to
        // cover the token type exhaustively would otherwise get an `unreachable_patterns`
        // warning out of a macro expansion it cannot edit — and the arm still has to be
        // emitted, because exhaustiveness is a property of the *call site*, not of the
        // macro. An unreachable arm the user wrote is still reported: the allow sits on
        // this arm alone.
        #[allow(unreachable_patterns)]
        (__s, __found) => {
          ::core::result::Result::Err($crate::span::Spanned::new(__s, __found))
        }
      }
    })
  }};
}

/// The tentative twin of [`select!`](crate::select): a head not in the table (or genuine
/// end of input) declines with zero consumption; a terminal stop is an error.
///
/// The kind expressions carry the same const-promotability requirement as
/// [`select!`](crate::select) (`E0716` at the call site otherwise), and the same
/// [`ComposableParseContext`](crate::ComposableParseContext) requirement; both
/// completeness modes are supported.
#[macro_export]
macro_rules! try_select {
  ($inp:expr, { $($kind:expr => ($span:pat, $pat:pat) => $body:expr),+ $(,)? }) => {{
    let __table = &[$($kind),+];
    $crate::parser::try_dispatch_take($inp, __table, |__sp| {
      match (__sp.span, __sp.data) {
        $( ($span, $pat) => ::core::result::Result::Ok($body), )+
        // The give-back arm. `#[allow]` because a grammar whose arm patterns happen to
        // cover the token type exhaustively would otherwise get an `unreachable_patterns`
        // warning out of a macro expansion it cannot edit — and the arm still has to be
        // emitted, because exhaustiveness is a property of the *call site*, not of the
        // macro. An unreachable arm the user wrote is still reported: the allow sits on
        // this arm alone.
        #[allow(unreachable_patterns)]
        (__s, __found) => {
          ::core::result::Result::Err($crate::span::Spanned::new(__s, __found))
        }
      }
    })
  }};
}
