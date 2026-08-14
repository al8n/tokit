#![cfg(all(feature = "std", feature = "combinators", feature = "logos_0_16"))]

//! The additive parser surfaces, driven through the public API.
//!
//! Each cell here is a *call-site* claim — the shape a consumer writes — as opposed to
//! the protocol cells that live beside the code they pin. The shared `common` fixture is
//! the same one the rest of the integration suite uses.

mod common;

use common::{E, TestLexer, Token, TokenKind};
use tokora::{
  Accumulator, InputRef, Parse, ParseInput, Parser, ParserContext, emitter::Silent, parser::expect,
  utils::Expected, while_head, while_kind,
};

type Ctx<'a> = ParserContext<'a, TestLexer<'a>, Silent<E>>;

fn ctx() -> Ctx<'static> {
  ParserContext::new(Silent::<E>::new())
}

fn parse_num<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E> {
  expect(|t: &Token| {
    if matches!(t, Token::Num(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Num))
    }
  })
  .parse_input(inp)
  .map(|t| match t {
    Token::Num(n) => n,
    _ => unreachable!(),
  })
}

// ── The width-1 decision adapters ────────────────────────────────────────────
//
// The shape being replaced is `.repeated_while::<_, U1>(hook)` where `hook` is
// `fn(Peeked<'_, 'inp, L, U1>, &mut Ctx::Emitter) -> Result<Action, _>` — a turbofish, a
// typenum, a `Peeked`, and `Ctx` dragged into a free function's signature. None of that
// appears below: the adapters are `Decision` impls pinned at `W = U1`, so the driver's
// window parameter infers from the adapter.

#[test]
fn while_head_drives_a_repetition_with_no_turbofish() {
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<Vec<i64>, E> {
    parse_num
      .repeated_while(while_head(|t: &Token| matches!(t, Token::Num(_))))
      .collect()
      .parse_input(inp)
  }

  // Stops at a head that fails the predicate...
  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("1 2 3 ;")
    .unwrap();
  assert_eq!(got, vec![1, 2, 3]);

  // ...and at end of input, which is the arm a `_ => Continue` body would get wrong.
  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(got, vec![1, 2]);

  // An immediately-failing head yields nothing and consumes nothing.
  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str(";")
    .unwrap();
  assert_eq!(got, Vec::<i64>::new());
}

#[test]
fn while_kind_drives_a_repetition_with_zero_annotation() {
  // No `|t: &Token|` ascription and no turbofish: `while_kind` takes the kind by value,
  // so nothing has to be inferred through an impl-side `Fn` bound.
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<Vec<i64>, E> {
    parse_num
      .repeated_while(while_kind(TokenKind::Num))
      .collect()
      .parse_input(inp)
  }

  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("4 5 +")
    .unwrap();
  assert_eq!(got, vec![4, 5]);

  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("")
    .unwrap();
  assert_eq!(got, Vec::<i64>::new());
}

#[test]
fn while_head_reads_until_with_one_bang() {
  // The cut `until_head` adapter, written as the control that replaces it: the "until"
  // reading survives as a negated predicate, which is why no second adapter ships.
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<Vec<i64>, E> {
    parse_num
      .repeated_while(while_head(|t: &Token| !matches!(t, Token::Semi)))
      .collect()
      .parse_input(inp)
  }

  let got: Vec<i64> = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("7 8 ;")
    .unwrap();
  assert_eq!(got, vec![7, 8]);
}

#[test]
fn peek_then_head_sees_the_head_by_reference_or_none_at_end_of_input() {
  use std::cell::Cell;

  thread_local! {
    static SEEN: Cell<Option<Option<TokenKind>>> = const { Cell::new(None) };
  }

  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E> {
    // The hook's whole signature: one `Option<Spanned<&Token, &Span>>`. No `Peeked`, no
    // typenum, no emitter — so it does not have to name `Ctx` to be written.
    parse_num
      .peek_then_head(|head| {
        SEEN.with(|c| {
          c.set(Some(
            head
              .as_ref()
              .map(|sp| <Token as tokora::Token>::kind(sp.data)),
          ))
        });
        match head {
          Some(sp) if matches!(sp.data, Token::Num(_)) => Ok(()),
          _ => Err(E),
        }
      })
      .parse_input(inp)
  }

  SEEN.with(|c| c.set(None));
  let got = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("9")
    .unwrap();
  assert_eq!(got, 9);
  assert_eq!(
    SEEN.with(Cell::take),
    Some(Some(TokenKind::Num)),
    "the hook saw the head itself, not a window"
  );

  SEEN.with(|c| c.set(None));
  assert!(
    Parser::with_context(ctx())
      .apply(parse)
      .parse_str("")
      .is_err(),
    "an empty input reaches the hook's `None` arm"
  );
  assert_eq!(
    SEEN.with(Cell::take),
    Some(None),
    "end of input is `None`, not an empty window the hook has to interrogate"
  );
}

// ── Three-way speculation and the span bracket ───────────────────────────────

#[test]
fn attempt_parse_rolls_a_decline_back_so_the_token_is_seen_again() {
  use tokora::try_parse_input::ParseAttempt;

  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<(ParseAttempt<i64>, i64), E> {
    // The speculation consumes a token and THEN declines. If the decline committed
    // instead of rolling back, the follow-up parse would see the second number.
    let attempt = inp.attempt_parse(|inp| {
      let _eaten = parse_num(inp)?;
      Ok::<ParseAttempt<i64>, E>(ParseAttempt::Decline)
    })?;
    let after = parse_num(inp)?;
    Ok((attempt, after))
  }

  let (attempt, after) = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(attempt, ParseAttempt::Decline);
  assert_eq!(
    after, 1,
    "a declined attempt rolled back, so the follow-up parse re-reads the FIRST number"
  );
}

#[test]
fn attempt_parse_keeps_an_accept_and_propagates_an_error_after_rolling_back() {
  use tokora::try_parse_input::ParseAttempt;

  // Accept commits: the follow-up parse reads the second number.
  fn accept<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<(i64, i64), E> {
    let taken = inp.attempt_parse(|inp| parse_num(inp).map(ParseAttempt::Accept))?;
    let after = parse_num(inp)?;
    Ok((taken.unwrap_accept(), after))
  }
  let (taken, after) = Parser::with_context(ctx())
    .apply(accept)
    .parse_str("1 2")
    .unwrap();
  assert_eq!((taken, after), (1, 2));

  // `Err` rolls back too, and the error arrives untouched.
  fn err<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E> {
    let outcome = inp.attempt_parse(|inp| {
      let _eaten = parse_num(inp)?;
      Err::<ParseAttempt<i64>, E>(E)
    });
    assert!(outcome.is_err(), "the closure's error propagates");
    parse_num(inp)
  }
  let after = Parser::with_context(ctx())
    .apply(err)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(
    after, 1,
    "an errored attempt rolled back, so the follow-up parse re-reads the FIRST number"
  );
}

#[test]
fn spanning_reports_exactly_what_the_closure_consumed() {
  fn parse<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<(usize, usize, i64), E> {
    // One leading token is consumed OUTSIDE the bracket, so a body that spanned from the
    // input's start instead of from the cursor would report 0 rather than 1. The start is
    // the previous token's committed END (leading trivia included) — `spanned`'s own
    // bracket, which the next cell pins by comparison rather than by restating.
    let _first = parse_num(inp)?;
    let (span, sum) = inp.spanning(|inp| {
      let a = parse_num(inp)?;
      let b = parse_num(inp)?;
      Ok::<_, E>(a + b)
    })?;
    Ok((
      tokora::span::Span::start(&span),
      tokora::span::Span::end(&span),
      sum,
    ))
  }

  let (start, end, sum) = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("1 22 333")
    .unwrap();
  assert_eq!(
    (start, end, sum),
    (1, 8, 355),
    "the span brackets exactly what the closure consumed, from the cursor it started at"
  );
}

#[test]
fn spanning_agrees_with_the_spanned_combinator() {
  use tokora::span::Spanned;

  // The two spellings must not be able to disagree — `spanning` is `spanned`'s bracket,
  // written imperatively.
  fn imperative<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<(usize, usize), E> {
    let (span, _) = inp.spanning(parse_num)?;
    Ok((
      tokora::span::Span::start(&span),
      tokora::span::Span::end(&span),
    ))
  }
  fn combinator<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<(usize, usize), E> {
    let out: Spanned<i64, _> = parse_num.spanned().parse_input(inp)?;
    Ok((
      tokora::span::Span::start(out.span_ref()),
      tokora::span::Span::end(out.span_ref()),
    ))
  }

  let a = Parser::with_context(ctx())
    .apply(imperative)
    .parse_str("  42")
    .unwrap();
  let b = Parser::with_context(ctx())
    .apply(combinator)
    .parse_str("  42")
    .unwrap();
  assert_eq!(
    a, b,
    "the imperative and combinator spellings cannot disagree — same bracket"
  );
  assert_eq!(
    a,
    (0, 4),
    "and the shared answer is the cursor-to-cursor extent"
  );
}

// ── `into_option` and `attempt!` ─────────────────────────────────────────────

#[test]
fn into_option_projects_both_arms() {
  use tokora::try_parse_input::ParseAttempt;

  assert_eq!(ParseAttempt::Accept(7i64).into_option(), Some(7));
  assert_eq!(ParseAttempt::<i64>::Decline.into_option(), None);
  // The composition it exists for: the whole of `Option`'s vocabulary, without an
  // alias per method on `ParseAttempt`.
  assert_eq!(
    ParseAttempt::<i64>::Decline.into_option().ok_or("declined"),
    Err("declined")
  );
}

#[test]
fn attempt_macro_propagates_a_decline_and_carries_an_accept() {
  use tokora::try_parse_input::ParseAttempt;

  fn try_num<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<ParseAttempt<i64>, E> {
    inp
      .try_expect(|t| matches!(t.data(), Token::Num(_)))
      .map(|o| {
        o.map(|tok| match tok.into_data() {
          Token::Num(n) => ParseAttempt::Accept(n),
          _ => unreachable!(),
        })
        .unwrap_or(ParseAttempt::Decline)
      })
  }

  // Two tentative steps in sequence: the second's decline must exit the production as
  // a decline, not as an error and not as a half-built value.
  fn try_pair<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<ParseAttempt<(i64, i64)>, E> {
    let a = tokora::attempt!(try_num(inp));
    let b = tokora::attempt!(try_num(inp));
    Ok(ParseAttempt::Accept((a, b)))
  }

  let got = Parser::with_context(ctx())
    .apply(try_pair)
    .parse_str("1 2")
    .unwrap();
  assert_eq!(got, ParseAttempt::Accept((1, 2)));

  let got = Parser::with_context(ctx())
    .apply(try_pair)
    .parse_str("1 ;")
    .unwrap();
  assert_eq!(
    got,
    ParseAttempt::Decline,
    "the second step's decline propagates out of the production"
  );

  let got = Parser::with_context(ctx())
    .apply(try_pair)
    .parse_str(";")
    .unwrap();
  assert_eq!(got, ParseAttempt::Decline, "so does the first step's");
}

// ── Output pinning and the fluent parity set ─────────────────────────────────

#[test]
fn pinned_fixes_an_ambiguous_receiver_so_the_next_call_needs_no_annotation() {
  use tokora::pinned;

  // `.collect()` has several output impls, so a downstream site that does not name the
  // container is ambiguous. `pinned::<Vec<i64>, _>` fixes it at exactly one, and the
  // `.len()` below — which names nothing — then resolves.
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<usize, E> {
    let v = pinned::<Vec<i64>, _>(
      parse_num
        .repeated_while(while_kind(TokenKind::Num))
        .collect(),
    )
    .parse_input(inp)?;
    Ok(v.len())
  }

  let n = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("1 2 3")
    .unwrap();
  assert_eq!(n, 3);
}

#[test]
fn pinning_preserves_send_regardless_of_the_pinned_type() {
  use tokora::pinned;

  // The phantom is `fn() -> O`, not `O`. `Pinned` stores no `O`, so it must not inherit
  // `O`'s auto-traits: pinning to a `*const u8` output must not un-`Send` a `Send`
  // parser. A `PhantomData<O>` phantom makes this line fail to compile — variance and
  // auto-traits are public API the moment they ship.
  fn assert_send<T: Send>(_: &T) {}
  fn assert_sync<T: Sync>(_: &T) {}

  let parser = parse_num;
  assert_send(&parser);
  let p = pinned::<*const u8, _>(parser);
  assert_send(&p);
  assert_sync(&p);

  // And `Debug` is hand-written: it renders the inner parser and nothing about `O`,
  // which is a phantom with no value to show.
  #[derive(Debug)]
  struct Inner;
  assert_eq!(
    format!("{:?}", pinned::<*const u8, _>(Inner)),
    "Pinned { inner: Inner }"
  );
}

// The name a fluent form routes is only observable where the name goes. `labelled` hands
// it to `Emitter::enter_label`, so under `Silent` — which this file's `Ctx` uses — it is
// discarded and a parity cell that only compares parsed values cannot fail: routing a
// constant instead of the caller's name leaves it green. That was measured, not supposed.
// The cell below runs on `Verbose`, which records the label against the diagnostic raised
// inside the scope, and asserts the recorded name is the one passed in.
#[test]
fn fluent_labelled_matches_the_free_function() {
  use tokora::{
    emitter::Verbose,
    parser::labelled,
    span::{SimpleSpan, Spanned},
  };

  type VCtx<'a> = ParserContext<'a, TestLexer<'a>, Verbose<E>>;

  fn num_v<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<i64, E> {
    expect(|t: &Token| {
      if matches!(t, Token::Num(_)) {
        Ok(())
      } else {
        Err(Expected::one(TokenKind::Num))
      }
    })
    .parse_input(inp)
    .map(|t| match t {
      Token::Num(n) => n,
      _ => unreachable!(),
    })
  }

  fn method<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<i64, E> {
    num_v.labelled("number").parse_input(inp)
  }
  fn free<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<i64, E> {
    labelled("number", num_v).parse_input(inp)
  }

  // The success path first: the wrapper must not disturb the parse it wraps.
  let a = Parser::with_context(ParserContext::new(Verbose::<E>::new()))
    .apply(method)
    .parse_str("5")
    .unwrap();
  let b = Parser::with_context(ParserContext::new(Verbose::<E>::new()))
    .apply(free)
    .parse_str("5")
    .unwrap();
  assert_eq!((a, b), (5, 5));

  // Then the path that carries the name. A label is stamped onto diagnostics *emitted*
  // inside the scope — a returned `Err` is not one — so the wrapped sub-parser emits one
  // explicitly, which is what the emitter suite's own label cell does. The recording is
  // read through `inp.emitter()` during the parse, the only point it is reachable.
  fn marker<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>) -> Result<(), E> {
    inp.emit_error(Spanned::new(SimpleSpan::new(0usize, 1usize), E))
  }
  fn recorded<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Vec<&'static str> {
    inp
      .emitter_ref()
      .labels()
      .values()
      .flatten()
      .flatten()
      .copied()
      .collect()
  }
  // Two *different* names, and neither is a fixed point of the transformations a delegate
  // could apply. The padding is load-bearing: with `"alpha"` these rows passed a delegate
  // that routed `name.trim()`, because trimming `"alpha"` yields `"alpha"` and the mutation
  // is invisible. Whitespace at both ends, plus exact equality rather than a substring
  // check, makes any re-slicing of the caller's string observable.
  const ALPHA: &str = " alpha 1 ";
  const BETA: &str = " beta 2 ";

  // Two *different* names, and that is the whole point. Asserting one name is observable
  // does not show the name was routed: a witness that matches text is satisfiable by every
  // producer of that text, and an implementation that ignored its argument and passed the
  // literal this cell asserts on would be such a producer. That is not hypothetical — it
  // was the defect here, found by mutation after the first repair. One name cannot
  // distinguish "routed" from "hardcoded to the value I happened to check"; two can, since
  // no constant satisfies both rows.
  fn probe_method_alpha<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<&'static str>, E> {
    let _ = marker.labelled(ALPHA).parse_input(inp);
    Ok(recorded(inp))
  }
  fn probe_method_beta<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<&'static str>, E> {
    let _ = marker.labelled(BETA).parse_input(inp);
    Ok(recorded(inp))
  }
  fn probe_free_alpha<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<&'static str>, E> {
    let _ = labelled(ALPHA, marker).parse_input(inp);
    Ok(recorded(inp))
  }

  let method_alpha = Parser::with_context(ParserContext::new(Verbose::<E>::new()))
    .apply(probe_method_alpha)
    .parse_str(";")
    .unwrap();
  let method_beta = Parser::with_context(ParserContext::new(Verbose::<E>::new()))
    .apply(probe_method_beta)
    .parse_str(";")
    .unwrap();
  let free_alpha = Parser::with_context(ParserContext::new(Verbose::<E>::new()))
    .apply(probe_free_alpha)
    .parse_str(";")
    .unwrap();

  assert_eq!(
    method_alpha,
    vec![ALPHA],
    "the fluent form must record the caller's name exactly, not a re-slice of it"
  );
  assert_eq!(
    method_beta,
    vec![BETA],
    "a second, different name must be recorded too — one row cannot tell routing from a constant"
  );
  assert_eq!(
    method_alpha, free_alpha,
    "the fluent form must record exactly what the free function records"
  );
}

#[test]
fn fluent_traced_is_the_identity_on_the_parse_it_wraps() {
  // With `trace` off this is literally the identity function; with it on it wraps.
  // Either way the parse it wraps must be unchanged, which is the only claim that
  // holds at both feature points.
  fn parse<'inp>(inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E> {
    parse_num.traced("number").parse_input(inp)
  }
  let got = Parser::with_context(ctx())
    .apply(parse)
    .parse_str("6")
    .unwrap();
  assert_eq!(got, 6);

  // The assertion above cannot observe the name: it compares a parsed value, and the
  // value is the same whatever name is routed. With `trace` off there is nothing to
  // observe — the name is discarded by design, the method being the identity — so the
  // routing claim is only decidable with the feature on, and that is where it is made.
  // `Traced` keeps the name in a private field that its derived `Debug` renders, and a
  // fn pointer is used as the wrapped parser because a fn *item* is not `Debug`.
  #[cfg(feature = "trace")]
  {
    type NumFn =
      for<'inp> fn(&mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>) -> Result<i64, E>;
    let f: NumFn = parse_num;

    // Two different names, for the reason given on the `labelled` cell above, and padded
    // for the same reason: an unpadded name is invariant under `trim`, so a delegate that
    // re-slices its argument stayed invisible. The comparison is against the `Debug`
    // rendering of the exact string, quotes and padding included, so any re-slice differs.
    const ALPHA: &str = " alpha 1 ";
    const BETA: &str = " beta 2 ";
    let want_alpha = format!("{ALPHA:?}");
    let want_beta = format!("{BETA:?}");

    let method_alpha = format!("{:?}", f.traced(ALPHA));
    let method_beta = format!("{:?}", f.traced(BETA));
    let free_alpha = format!("{:?}", tokora::traced(ALPHA, f));

    assert!(
      method_alpha.contains(&want_alpha) && !method_alpha.contains(&want_beta),
      "the fluent form must route the caller's name exactly, got {method_alpha}"
    );
    assert!(
      method_beta.contains(&want_beta) && !method_beta.contains(&want_alpha),
      "a second, different name must route too, got {method_beta}"
    );
    assert_ne!(
      method_alpha, method_beta,
      "two distinct names must produce distinct wrappers — a constant produces one"
    );
    assert_eq!(
      method_alpha, free_alpha,
      "the fluent form must build exactly what the free function builds"
    );
  }
}

#[test]
fn fluent_opt_matches_the_free_function() {
  use tokora::{TryParseInput, parser::opt};

  fn try_num<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<tokora::try_parse_input::ParseAttempt<i64>, E> {
    inp
      .try_expect(|t| matches!(t.data(), Token::Num(_)))
      .map(|o| {
        o.map(|tok| match tok.into_data() {
          Token::Num(n) => tokora::try_parse_input::ParseAttempt::Accept(n),
          _ => unreachable!(),
        })
        .unwrap_or(tokora::try_parse_input::ParseAttempt::Decline)
      })
  }

  fn method<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<Option<i64>, E> {
    try_num.opt().parse_input(inp)
  }
  fn free<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, Ctx<'inp>>,
  ) -> Result<Option<i64>, E> {
    opt(try_num).parse_input(inp)
  }

  for src in ["7", ";"] {
    let a = Parser::with_context(ctx()).apply(method).parse_str(src);
    let b = Parser::with_context(ctx()).apply(free).parse_str(src);
    assert_eq!(a.unwrap(), b.unwrap(), "source {src:?}");
  }
  assert_eq!(
    Parser::with_context(ctx())
      .apply(method)
      .parse_str("7")
      .unwrap(),
    Some(7)
  );
  assert_eq!(
    Parser::with_context(ctx())
      .apply(method)
      .parse_str(";")
      .unwrap(),
    None
  );
}

// ── `syntax_kinds!`, exercised from OUTSIDE the crate ────────────────────────
//
// The macro's failure mode is a consumer-side one: a `macro_rules!` body does not
// name `KindValidator` until it is expanded, so tokora's own `cargo check` is
// green even when the expansion cannot compile. These cells expand it in an
// integration target, which is the position a real consumer occupies.

tokora::syntax_kinds! {
  /// A three-kind dialect, declared the way a consumer declares one.
  pub enum ProbeKinds {
    /// The first kind — raw 0.
    Name,
    Int,
    Document,
  }
}

#[test]
fn syntax_kinds_numbers_by_declaration_order_and_inverts_checked() {
  assert_eq!(ProbeKinds::COUNT, 3);
  assert_eq!(
    (
      ProbeKinds::Name.raw(),
      ProbeKinds::Int.raw(),
      ProbeKinds::Document.raw()
    ),
    (0, 1, 2)
  );
  assert_eq!(
    ProbeKinds::ALL,
    [ProbeKinds::Name, ProbeKinds::Int, ProbeKinds::Document],
    "ALL is declaration order — the property consumers index into"
  );

  // Round-trips inside the space...
  for k in ProbeKinds::ALL {
    assert_eq!(ProbeKinds::from_raw(k.raw()), Some(k));
  }
  // ...and refuses everything outside it, including the reserved band.
  assert_eq!(
    ProbeKinds::from_raw(3),
    None,
    "the boundary, one past the last"
  );
  assert_eq!(
    ProbeKinds::from_raw(u16::MAX),
    None,
    "the reserved tombstone"
  );
}

#[test]
fn syntax_kinds_validator_and_the_sink_profile_cannot_disagree() {
  use tokora::cst::CstProfile;

  fn map(k: &ProbeKinds) -> u16 {
    k.raw()
  }

  // Every declared kind is admitted by the generated validator, as read by the
  // profile's own construction-time enforcement.
  for k in ProbeKinds::ALL {
    let profile = CstProfile::new(
      map as fn(&ProbeKinds) -> u16,
      ProbeKinds::validator(),
      k.raw(),
      k.raw(),
    );
    let _ = profile;
  }

  // A kind one past the declaration is refused where the mistake is, not one
  // materialization later. This is the cell that fails if `validator()` is
  // generated from anything other than `COUNT` — `accept_all()` passes it green.
  let refused = std::panic::catch_unwind(|| {
    CstProfile::new(
      map as fn(&ProbeKinds) -> u16,
      ProbeKinds::validator(),
      ProbeKinds::COUNT,
      0,
    )
  });
  assert!(
    refused.is_err(),
    "an undeclared kind must not reach a sink through a profile built from this \
     declaration's own validator"
  );
}
