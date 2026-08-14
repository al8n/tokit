#![cfg(all(
  feature = "std",
  feature = "combinators",
  feature = "tinyvec_1",
  feature = "logos_0_16"
))]

//! What the repetition drivers may assume about a `Container`, and what they may not.
//!
//! `Container` is implementable outside this crate, so it is untrusted input to the drivers'
//! accounting. Two halves:
//!
//! * **The built-in adapters keep the one obligation the trait states.** A fixed-capacity
//!   adapter refuses through `Err(item)` — it never panics, never grows, and never reports
//!   success for an item it did not take. `tinyvec`'s `SliceVec` used to call the panicking
//!   upstream `push` and then return `Ok(())` unconditionally, so an element count the *input*
//!   chooses unwound the parser instead of reaching the `FullContainer` path.
//! * **The drivers' numbers are their own.** `FullContainer`'s count used to be
//!   `container.len() + 1`: a number a caller-implemented method invented, believed on the
//!   strength of laws the trait never stated, and added to unchecked. The rows below drive
//!   containers that answer `len()` in ways no documentation forbade.
//! * **And so is what the drivers may say about them.** Dropping `len()` also dropped any
//!   knowledge of the destination's *occupancy*, which is what an "exceeds the capacity" claim
//!   rests on. A destination that is already occupied when the construct starts refuses the
//!   construct's **first** element at any capacity, so the last rows pin the diagnostic's
//!   rendered text and not only its numbers.

mod common;

use common::{TestLexer, Token};
use tinyvec_1::SliceVec;
use tokora::{
  Accumulator, InputRef, Parse, ParseInput, Parser, ParserContext, TryParseInput,
  container::Container,
  emitter::Verbose,
  error::syntax::{FullContainer, MissingSyntax, TooFew, TooMany},
  error::token::{MissingToken, SeparatedError, UnexpectedToken},
  error::{Unclosed, UnexpectedEot},
  parser::Collect,
  try_parse_input::ParseAttempt,
};

// ── A payload-preserving diagnostic ───────────────────────────────────────────

/// `Full` carries the payload's two numbers **and what they render as**: the defect this file
/// pins last was a payload whose numbers were individually defensible and whose sentence was
/// false, so a row that reads only the numbers cannot see it.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Diag {
  Full(usize, usize, String),
  Other,
}

macro_rules! other {
  ($($ty:ty),* $(,)?) => {$(
    impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<$ty> for Diag {
      fn from(_: $ty) -> Self {
        Diag::Other
      }
    }
  )*};
}

other!(
  UnexpectedToken<'a, T, Kind, S, Lang>,
  SeparatedError<'a, T, Kind, S, Lang>,
);

impl From<()> for Diag {
  fn from(_: ()) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<FullContainer<S, Lang>> for Diag {
  fn from(e: FullContainer<S, Lang>) -> Self {
    Diag::Full(e.nums(), e.capacity(), e.to_string())
  }
}

impl<S, Lang: ?Sized> From<TooFew<S, Lang>> for Diag {
  fn from(_: TooFew<S, Lang>) -> Self {
    Diag::Other
  }
}

impl<S, Lang: ?Sized> From<TooMany<S, Lang>> for Diag {
  fn from(_: TooMany<S, Lang>) -> Self {
    Diag::Other
  }
}

impl<O, Lang: ?Sized> From<MissingSyntax<O, Lang>> for Diag {
  fn from(_: MissingSyntax<O, Lang>) -> Self {
    Diag::Other
  }
}

impl<'a, Kind: Clone, O, Lang: ?Sized> From<MissingToken<'a, Kind, O, Lang>> for Diag {
  fn from(_: MissingToken<'a, Kind, O, Lang>) -> Self {
    Diag::Other
  }
}

impl<Set: Clone + 'static> From<UnexpectedEot<usize, (), Set>> for Diag {
  fn from(_: UnexpectedEot<usize, (), Set>) -> Self {
    Diag::Other
  }
}

impl<Delimiter, S, Lang: ?Sized> From<Unclosed<Delimiter, S, Lang>> for Diag {
  fn from(_: Unclosed<Delimiter, S, Lang>) -> Self {
    Diag::Other
  }
}

// ── Fixture ───────────────────────────────────────────────────────────────────

type VCtx<'inp> = ParserContext<'inp, TestLexer<'inp>, Verbose<Diag>>;

fn verbose_ctx() -> VCtx<'static> {
  ParserContext::new(Verbose::new())
}

fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
) -> Result<ParseAttempt<i64>, Diag> {
  inp
    .try_expect(|t| matches!(t.data(), Token::Num(_)))
    .map(|opt| match opt {
      None => ParseAttempt::Decline,
      Some(tok) => ParseAttempt::Accept(match tok.into_data() {
        Token::Num(n) => n,
        _ => unreachable!(),
      }),
    })
}

/// What a container ended up holding, for containers whose contents are not otherwise
/// inspectable through the trait.
trait Snapshot {
  fn snapshot(&self) -> Vec<i64>;
}

/// Collects `src` into a `Default`-constructed `C` and returns what it held and the diagnostics
/// in emission order.
fn collect_into<C>(src: &'static str) -> (Vec<i64>, Vec<Diag>)
where
  C: Default + Container<i64> + Snapshot,
{
  fn go<'inp, C>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<(Vec<i64>, Vec<Diag>), Diag>
  where
    C: Default + Container<i64> + Snapshot,
  {
    let collected: C = try_num.repeated().collect().parse_input(inp)?;
    let diags = inp
      .emitter_ref()
      .diagnostics()
      .filter_map(|d| d.payload().cloned())
      .collect();
    Ok((collected.snapshot(), diags))
  }

  Parser::with_context(verbose_ctx())
    .apply(go::<C>)
    .parse_str(src)
    .expect("a refused push is a diagnostic, never a parse failure")
}

// ── #263 — the `SliceVec` adapter refuses instead of panicking ────────────────

/// The unit-level witness: `Container::push` at capacity hands the item back rather than
/// unwinding, and leaves what it already holds alone.
#[test]
fn slice_vec_refuses_at_capacity_without_panicking() {
  let mut storage = [0i64; 2];
  let mut target = SliceVec::from_slice_len(&mut storage, 0);

  assert_eq!(Container::max_capacity(&target), 2);
  assert_eq!(Container::push(&mut target, 10), Ok(()));
  assert_eq!(Container::push(&mut target, 20), Ok(()));

  // Exactly at capacity: the next item comes straight back, unchanged.
  assert_eq!(Container::push(&mut target, 30), Err(30));
  assert_eq!(Container::push(&mut target, 40), Err(40));
  assert_eq!(Container::len(&target), 2);
  assert_eq!(target.as_slice(), &[10, 20]);
}

/// The public-API witness: a repetition parse whose input carries more elements than the backing
/// slice can hold. This is the whole defect — untrusted input, a safe adapter, and an unwind
/// where the crate promises a diagnostic.
#[test]
fn a_repetition_parse_overflowing_a_slice_vec_reports_instead_of_unwinding() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<Diag>, Diag> {
    let mut storage = [0i64; 1];
    let target = SliceVec::from_slice_len(&mut storage, 0);
    let collected: SliceVec<'_, i64> = try_num.repeated().collect_with(target).parse_input(inp)?;
    assert_eq!(
      collected.as_slice(),
      &[1],
      "the elements past capacity are dropped, not stored and not fatal"
    );
    Ok(
      inp
        .emitter_ref()
        .diagnostics()
        .filter_map(|d| d.payload().cloned())
        .collect(),
    )
  }

  let diags = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("1 2 3")
    .expect("capacity exhaustion is a diagnostic, never a panic and never a parse failure");
  assert_eq!(
    diags,
    vec![Diag::Full(
      2,
      1,
      "element 2 of this construct was refused by a destination that holds at most 1".into()
    )],
    "capacity exhaustion is one `FullContainer` naming the adapter's real capacity"
  );
}

// ── #258 — the drivers' count is the drivers' own ─────────────────────────────

/// A container that holds one element and answers `len()` with a constant.
///
/// Nothing in the trait's contract says `len()` tracks pushes: it is a caller-implemented
/// method, and the drivers must not build a diagnostic out of a number it invented. The
/// refusal channel itself is kept: this container refuses exactly when it is full.
#[derive(Default)]
struct ConstantLen<const N: usize>(Option<i64>);

impl<const N: usize> Container<i64> for ConstantLen<N> {
  fn push(&mut self, item: i64) -> Result<(), i64> {
    match self.0 {
      None => {
        self.0 = Some(item);
        Ok(())
      }
      Some(_) => Err(item),
    }
  }

  fn first(&self) -> Option<&i64> {
    self.0.as_ref()
  }

  fn last(&self) -> Option<&i64> {
    self.0.as_ref()
  }

  fn len(&self) -> usize {
    N
  }

  fn max_capacity(&self) -> usize {
    1
  }
}

impl<const N: usize> Snapshot for ConstantLen<N> {
  fn snapshot(&self) -> Vec<i64> {
    self.0.into_iter().collect()
  }
}

/// `len()` of zero: the old `container.len() + 1` named the refusal at the construct's **first**
/// element when it was the second. The driver parsed two and says so.
#[test]
fn the_reported_count_is_the_drivers_not_the_containers() {
  let (collected, diags) = collect_into::<ConstantLen<0>>("1 2 3");
  assert_eq!(collected, vec![1]);
  assert_eq!(
    diags,
    vec![Diag::Full(
      2,
      1,
      "element 2 of this construct was refused by a destination that holds at most 1".into()
    )]
  );
}

/// `len()` of `usize::MAX`: the old `container.len() + 1` overflowed — a panic in debug and a
/// wrapped `0` in release, both from a number the container chose. The driver's own count cannot
/// reach `usize::MAX` without the element counter overflowing first, so there is nothing left to
/// check here.
#[test]
fn a_saturated_len_cannot_overflow_the_reported_count() {
  let (collected, diags) = collect_into::<ConstantLen<{ usize::MAX }>>("1 2 3");
  assert_eq!(collected, vec![1]);
  assert_eq!(
    diags,
    vec![Diag::Full(
      2,
      1,
      "element 2 of this construct was refused by a destination that holds at most 1".into()
    )]
  );
}

/// The adversarial shape #258 names: a container that refuses for a reason that is **not**
/// capacity, and then accepts again. It breaks the one obligation the trait states, so it gets
/// no promise about what the capacity payload means — but the drivers must still terminate,
/// still report once per construct, and still collect every element the container took.
///
/// One report per construct is a diagnostic policy, not an inference that a refusal repeats.
#[derive(Default)]
struct Selective(Vec<i64>);

impl Container<i64> for Selective {
  fn push(&mut self, item: i64) -> Result<(), i64> {
    if item % 2 == 0 {
      self.0.push(item);
      Ok(())
    } else {
      Err(item)
    }
  }

  fn first(&self) -> Option<&i64> {
    self.0.first()
  }

  fn last(&self) -> Option<&i64> {
    self.0.last()
  }

  fn len(&self) -> usize {
    self.0.len()
  }

  fn max_capacity(&self) -> usize {
    usize::MAX
  }
}

impl Snapshot for Selective {
  fn snapshot(&self) -> Vec<i64> {
    self.0.clone()
  }
}

#[test]
fn a_non_sticky_refusal_is_reported_once_and_stops_nothing() {
  let (collected, diags) = collect_into::<Selective>("1 2 3 4");
  assert_eq!(
    collected,
    vec![2, 4],
    "a refusal drops its element and the collection carries on"
  );
  assert_eq!(
    diags,
    vec![Diag::Full(
      1,
      usize::MAX,
      "element 1 of this construct was refused by a destination that holds at most \
       18446744073709551615"
        .into()
    )],
    "one report per construct, describing the refusal that was actually witnessed"
  );
}

// ── The refusal is an event, not an arithmetic claim ──────────────────────────

/// A destination that is **already occupied** when the construct starts.
///
/// `Option` is conforming — it refuses exactly when full — so this is not an adversarial
/// container, and `collect_with` is the supported way to hand one to a driver. It refuses the
/// construct's *first* element, and every number the driver has says so: one element parsed,
/// destination capacity one. A diagnostic that asserted the first exceeded the second rendered
/// *"found 1 elements, which exceeds the maximum capacity of 1"*, which is false; the payload
/// before this branch said `2`, a total the driver can only reach by trusting an occupancy it
/// never reads.
///
/// The refusal is what the driver witnessed and the refusal is what it reports.
#[test]
fn a_seeded_owning_destination_reports_the_refusal_at_its_first_element() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<Diag>, Diag> {
    let collected: Option<i64> = try_num
      .repeated()
      .collect_with(Some(100i64))
      .parse_input(inp)?;
    assert_eq!(
      collected,
      Some(100),
      "a destination that refuses keeps what it already held"
    );
    Ok(
      inp
        .emitter_ref()
        .diagnostics()
        .filter_map(|d| d.payload().cloned())
        .collect(),
    )
  }

  let diags = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("1 2")
    .expect("a refused push is a diagnostic, never a parse failure");
  assert_eq!(
    diags,
    vec![Diag::Full(
      1,
      1,
      "element 1 of this construct was refused by a destination that holds at most 1".into()
    )]
  );
}

/// The same seeded destination through the **borrowed** collection path, where the caller owns
/// the container and a driver never sees a `Default` one at all. This is where a pre-occupied
/// destination is the ordinary case rather than the deliberate one, so a payload that assumed an
/// empty start is wrong here by default.
#[test]
fn a_seeded_borrowed_destination_reports_the_refusal_at_its_first_element() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<Diag>, Diag> {
    let mut inner = try_num.repeated();
    let mut container = Some(100i64);
    {
      let mut collect = Collect::new(&mut inner, &mut container);
      let _span = collect.parse_input(inp)?;
    }
    assert_eq!(
      container,
      Some(100),
      "the caller's container is left holding what it held"
    );
    Ok(
      inp
        .emitter_ref()
        .diagnostics()
        .filter_map(|d| d.payload().cloned())
        .collect(),
    )
  }

  let diags = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("1 2")
    .expect("a refused push is a diagnostic, never a parse failure");
  assert_eq!(
    diags,
    vec![Diag::Full(
      1,
      1,
      "element 1 of this construct was refused by a destination that holds at most 1".into()
    )]
  );
}

/// The empty-start control for the borrowed path: same driver, same destination type, same
/// input, and the refusal lands on element **two**. The pair is what shows the count tracks the
/// construct's own attempts and nothing about where the destination started.
#[test]
fn an_empty_borrowed_destination_reports_the_refusal_at_its_second_element() {
  fn go<'inp>(
    inp: &mut InputRef<'inp, '_, TestLexer<'inp>, VCtx<'inp>>,
  ) -> Result<Vec<Diag>, Diag> {
    let mut inner = try_num.repeated();
    let mut container: Option<i64> = None;
    {
      let mut collect = Collect::new(&mut inner, &mut container);
      let _span = collect.parse_input(inp)?;
    }
    assert_eq!(container, Some(1), "the first element fits");
    Ok(
      inp
        .emitter_ref()
        .diagnostics()
        .filter_map(|d| d.payload().cloned())
        .collect(),
    )
  }

  let diags = Parser::with_context(verbose_ctx())
    .apply(go)
    .parse_str("1 2")
    .expect("a refused push is a diagnostic, never a parse failure");
  assert_eq!(
    diags,
    vec![Diag::Full(
      2,
      1,
      "element 2 of this construct was refused by a destination that holds at most 1".into()
    )]
  );
}
