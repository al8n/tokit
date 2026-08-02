#![cfg(feature = "std")]

//! The render freeze: one pinned `Debug` and `Display` string per public error type.
//!
//! # Why this suite exists
//!
//! A `Debug` or `Display` render is public API that no signature describes. `cargo
//! semver-checks` cannot see it — it compares *shapes*, and a derived `Debug` that gains a
//! field has the same shape it always had. Nothing else in the toolchain reads rendered text
//! at all.
//!
//! That gap has cost this crate twice, both times the same way:
//!
//! - `UnexpectedEnd` gained a private `terminal: bool`. The derive picked it up, four
//!   downstream parity tests and five golden fixtures went red, and the change was disclosed
//!   nowhere.
//! - `MissingToken` and `UnexpectedToken` composed `Expected`'s `Display` — which opens with
//!   the word "expected" — under a literal `", expected {}"`, rendering `expected expected`.
//!   `MissingToken`'s doubling was pinned, so it was *known*. `UnexpectedToken`'s was pinned
//!   nowhere, so it survived the change that repaired its sibling.
//!
//! The second is the argument for freezing **every** type rather than the ones someone
//! happened to write a test for. A carrier with no render pin is a carrier whose render can
//! move without anyone noticing, and the way you find out is a consumer's frozen fixtures.
//!
//! # What a failure here means
//!
//! Not "you broke something" — it means **a rendered surface moved and has to be disclosed**.
//! Blessing is a reviewed diff of the string, which is the disclosure discipline made
//! mechanical: the reviewer sees the exact bytes a consumer's frozen fixtures will see.
//!
//! # This suite is born green, so it carries its own mutation check
//!
//! A freeze suite pins what is already true and has never been shown able to fail — the
//! vacuous-cell shape this crate refuses everywhere else. `the_freeze_can_fail` is the answer:
//! it renders a value that differs from a pinned one in exactly one field and asserts the two
//! strings differ. If the freeze were insensitive to a field addition, that cell would fail.

use std::format;

use tokora::SimpleSpan;
use tokora::error::token::{MissingToken, SeparatedError, SeparatorPosition, UnexpectedToken};
use tokora::error::{
  NonAssociativeChain, RecursionLimitReached, Unclosed, UnexpectedEot,
  syntax::{FullContainer, TooFew, TooMany},
};
use tokora::punct::Paren;
use tokora::state::recursion_tracker::{RecursionLimiter, RecursionTracker};
use tokora::utils::CowStr;

/// The tracker's own report, built the one way a caller can build one: exceed a limiter.
///
/// `RecursionLimitReached` stores it whole and renders two of its numbers, so freezing the outer
/// type's `Display` means going through this.
fn exceeded(
  limitation: usize,
  depth: usize,
) -> tokora::state::recursion_tracker::RecursionLimitExceeded {
  let mut limiter = RecursionLimiter::with_limitation(limitation);
  for _ in 0..depth {
    limiter.increase();
  }
  RecursionTracker::check(&limiter).expect_err("the depth was walked past the limitation")
}

/// A `Display` shim for the carriers that render through an inherent `display_fmt` rather than
/// a `Display` impl. Freezing them needs the same wrapper a consumer would write.
macro_rules! show {
  ($name:ident, $ty:ty) => {
    struct $name<'a>($ty);
    impl core::fmt::Display for $name<'_> {
      fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.display_fmt(f)
      }
    }
  };
}

show!(ShowMissing, MissingToken<'a, &'a str, usize>);
show!(
  ShowUnexpected,
  UnexpectedToken<'a, &'a str, &'a str, SimpleSpan>
);

/// The `Debug` twin of [`show!`]. Measured, not assumed: `MissingToken` and `UnexpectedToken`
/// implement **neither** `Debug` nor `Display` — both expose only inherent `debug_fmt` /
/// `display_fmt`, so a consumer wanting either writes exactly this wrapper. `SeparatedError`
/// has a hand-written `Debug` impl; `UnexpectedEnd` derives one. Four carriers, three different
/// mechanisms.
macro_rules! dbg_show {
  ($name:ident, $ty:ty) => {
    struct $name<'a>($ty);
    impl core::fmt::Debug for $name<'_> {
      fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.0.debug_fmt(f)
      }
    }
  };
}

dbg_show!(DebugMissing, MissingToken<'a, &'a str, usize>);

// ── Display ─────────────────────────────────────────────────────────────────

#[test]
fn display_renders_are_frozen() {
  // `MissingToken` — every combination of the name and message channels, because the name
  // channel is the one that moved most recently and the composition rule is the one that moved
  // it back.
  let bare: MissingToken<'_, &str, usize> = MissingToken::new(5);
  assert_eq!(format!("{}", ShowMissing(bare)), "missing token at 5");

  let named: MissingToken<'_, &str, usize> =
    MissingToken::new(5).with_name(CowStr::from_static("comma"));
  assert_eq!(
    format!("{}", ShowMissing(named)),
    "missing token 'comma' at 5"
  );

  let expected: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  assert_eq!(
    format!("{}", ShowMissing(expected)),
    "missing token at 5, expected '}'"
  );

  let full: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}")
    .with_name(CowStr::from_static("comma"))
    .with_message(CowStr::from_static("needed"));
  assert_eq!(
    format!("{}", ShowMissing(full)),
    "missing token 'comma' at 5, expected '}', message: needed"
  );

  // `UnexpectedToken` — all four arms. These had NO render pin before this release, which is
  // exactly why its doubled "expected" outlived the repair of its sibling's.
  let u_bare: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(5usize, 10usize));
  assert_eq!(format!("{}", ShowUnexpected(u_bare)), "unexpected token");

  let u_one = UnexpectedToken::expected_one(SimpleSpan::new(5usize, 10usize), "}");
  assert_eq!(
    format!("{}", ShowUnexpected(u_one)),
    "unexpected token, expected '}'"
  );

  let u_found =
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";");
  assert_eq!(
    format!("{}", ShowUnexpected(u_found)),
    "unexpected token ':', expected ';'"
  );

  let u_oneof = UnexpectedToken::expected_one_of(SimpleSpan::new(5usize, 10usize), &["+", "-"]);
  assert_eq!(
    format!("{}", ShowUnexpected(u_oneof)),
    "unexpected token, expected one of: '+', '-'"
  );

  // `SeparatedError` — a rendered surface that did not exist before this release.
  let sep: SeparatedError<'_, &str, &str, SimpleSpan> = SeparatedError::leading(
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";"),
  )
  .with_name(CowStr::from_static("comma"));
  assert_eq!(
    format!("{sep}"),
    "separator 'comma' at the leading position: unexpected token ':', expected ';'"
  );

  // `UnexpectedEnd` — the type whose undisclosed field addition is half this suite's reason
  // for existing.
  let eot: UnexpectedEot<usize> = UnexpectedEot::eot(7);
  assert_eq!(
    format!("{eot}"),
    "unexpected end of token stream, expected token"
  );

  // The count/container family.
  let too_few: TooFew<SimpleSpan> = TooFew::new(SimpleSpan::new(0usize, 3usize), 1, 3);
  assert_eq!(
    format!("{too_few}"),
    "too few elements: found 1, but minimum is 3 at 0..3"
  );

  let too_many: TooMany<SimpleSpan> = TooMany::new(SimpleSpan::new(0usize, 3usize), 4, 3);
  assert_eq!(
    format!("{too_many}"),
    "too many elements: found 4, but maximum is 3 at 0..3"
  );

  let full_c: FullContainer<SimpleSpan> = FullContainer::new(SimpleSpan::new(0usize, 3usize), 4, 3);
  assert_eq!(
    format!("{full_c}"),
    "found 4 elements, which exceeds the maximum capacity of 3"
  );

  // The delimiter family, whose `kind` field moved a derived `Debug` this release.
  let unclosed: Unclosed<Paren, SimpleSpan> = Unclosed::paren(SimpleSpan::new(0usize, 1usize));
  assert_eq!(format!("{unclosed}"), "unclosed delimiter '()'");

  // The two pratt carriers. Both are new in this release, and both were unpinned here until a
  // review found `NonAssociativeChain` rendering `non-associative operator at 5 …` for an offset
  // that is the **handback** position — on `1 ; 2 ; 3` the operator starts at 6, not 5. `offset()`
  // was pinned in four suites and every one of them stayed green, which is the argument for a
  // freeze per *type* rather than per accessor: a carrier nobody pinned is a carrier whose text
  // can say anything.
  let chain: NonAssociativeChain = NonAssociativeChain::of(5);
  assert_eq!(
    format!("{chain}"),
    "non-associative operator cannot be chained at its own power; input handed back at 5, at or \
     before the operator"
  );

  // The trip's number is the other kind: committed consumption at the frame that could not be
  // entered, which is where the limit was reached and is not a claim about any construct.
  let trip: RecursionLimitReached = RecursionLimitReached::of(15, exceeded(8, 9));
  assert_eq!(
    format!("{trip}"),
    "recursion limit reached at 15: depth 9, maximum 8"
  );
}

// ── Debug ───────────────────────────────────────────────────────────────────

#[test]
fn debug_renders_are_frozen() {
  // Derived. A field addition moves this by itself — the `UnexpectedEnd`/`terminal` class.
  let eot: UnexpectedEot<usize> = UnexpectedEot::eot(7);
  let rendered = format!("{eot:?}");
  assert!(
    rendered.contains("terminal: false"),
    "the derived Debug must carry the terminal flag it gained; got {rendered:?}"
  );

  // Hand-written. A field addition leaves these STALE unless someone edits them, which is the
  // opposite failure mode and the reason mechanism is recorded per type in the changelog.
  let missing: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}")
    .with_name(CowStr::from_static("comma"))
    .with_message(CowStr::from_static("needed"));
  let rendered = format!("{:?}", DebugMissing(missing));
  for field in ["offset", "expected", "message", "name"] {
    assert!(
      rendered.contains(field),
      "MissingToken's hand-written Debug dropped `{field}`; got {rendered:?}"
    );
  }

  let sep: SeparatedError<'_, &str, &str, SimpleSpan> = SeparatedError::leading(
    UnexpectedToken::expected_one_with_found(SimpleSpan::new(5usize, 10usize), ":", ";"),
  )
  .with_name(CowStr::from_static("comma"));
  let rendered = format!("{sep:?}");
  for field in ["position", "name", "span", "found", "expected"] {
    assert!(
      rendered.contains(field),
      "SeparatedError's hand-written Debug dropped `{field}`; got {rendered:?}"
    );
  }

  assert_eq!(
    format!("{:?}", SeparatorPosition::Leading),
    "Leading",
    "the position enum's derived Debug"
  );

  // The two pratt carriers, both derived. These are the one rendered surface on either type that
  // could not have mislabelled the offset: a derived `Debug` prints the field's declared name, so
  // the label is `at`, and `at`'s own doc is what defines it.
  let chain: NonAssociativeChain = NonAssociativeChain::of(5);
  assert_eq!(
    format!("{chain:?}"),
    "NonAssociativeChain { at: 5, _lang: PhantomData<()> }"
  );

  let trip: RecursionLimitReached = RecursionLimitReached::of(15, exceeded(8, 9));
  assert_eq!(
    format!("{trip:?}"),
    "RecursionLimitReached { at: 15, exceeded: RecursionLimitExceeded(RecursionLimiter { max: 8, \
     current: 9 }), _lang: PhantomData<()> }"
  );
}

// ── The suite's own teeth ───────────────────────────────────────────────────

/// **A freeze suite that has never failed is a freeze suite nobody has tested.**
///
/// This is not a test of the crate; it is a test of the suite above. Each pair below differs
/// in exactly one field, and the two renders must differ — which is what says the freeze is
/// sensitive to the kind of change it exists to catch. If a field stopped reaching a render,
/// the corresponding assertion here fails while every pinned string above keeps passing.
#[test]
fn the_freeze_can_fail() {
  // The name channel: present vs absent.
  let without: MissingToken<'_, &str, usize> = MissingToken::expected_one(5, "}");
  let with: MissingToken<'_, &str, usize> =
    MissingToken::expected_one(5, "}").with_name(CowStr::from_static("comma"));
  assert_ne!(
    format!("{}", ShowMissing(without)),
    format!("{}", ShowMissing(with)),
    "the name channel must reach Display, or the pins above cannot see it move"
  );

  // The terminal flag: the exact field whose silent addition cost a bisect.
  let ordinary: UnexpectedEot<usize> = UnexpectedEot::eot(7);
  let terminal: UnexpectedEot<usize> = UnexpectedEot::eot(7).into_terminal();
  assert_ne!(
    format!("{ordinary:?}"),
    format!("{terminal:?}"),
    "the terminal flag must reach Debug, or this suite could not have caught the R1 break"
  );

  // The `Expected` composition: with and without an expected set.
  let plain: UnexpectedToken<'_, &str, &str, SimpleSpan> =
    UnexpectedToken::new(SimpleSpan::new(5usize, 10usize));
  let expecting = UnexpectedToken::expected_one(SimpleSpan::new(5usize, 10usize), "}");
  assert_ne!(
    format!("{}", ShowUnexpected(plain)),
    format!("{}", ShowUnexpected(expecting)),
    "the expected set must reach Display"
  );

  // The separator name channel on the newest rendered surface.
  let unnamed: SeparatedError<'_, &str, &str, SimpleSpan> =
    SeparatedError::leading(UnexpectedToken::new(SimpleSpan::new(0usize, 1usize)));
  let named: SeparatedError<'_, &str, &str, SimpleSpan> =
    SeparatedError::leading(UnexpectedToken::new(SimpleSpan::new(0usize, 1usize)))
      .with_name(CowStr::from_static("comma"));
  assert_ne!(
    format!("{unnamed}"),
    format!("{named}"),
    "the separator name must reach Display — that is the whole point of the channel"
  );

  // The offset on the two pratt carriers. Both renders are built from a single field, so a pin
  // that could not see that field move would be pinning a constant.
  assert_ne!(
    format!("{}", NonAssociativeChain::<usize>::of(5)),
    format!("{}", NonAssociativeChain::<usize>::of(6)),
    "the handback offset must reach Display — it is the only thing this render carries, and \
     naming it wrongly is exactly the defect the pin above exists for"
  );
  assert_ne!(
    format!("{}", RecursionLimitReached::<usize>::of(15, exceeded(8, 9))),
    format!("{}", RecursionLimitReached::<usize>::of(15, exceeded(4, 5))),
    "the tracker's report must reach Display, and not only the offset in front of it"
  );
}
