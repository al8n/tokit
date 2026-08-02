//! The repetition drivers: `repeated`, `separated`, their `*_while` twins, and the delimited
//! forms of all four.
//!
//! # Resilience, and the granularity floor under it
//!
//! The four **try-driven** families are resilient by design: an element's `Err` is filed as a
//! diagnostic and the loop goes on to the next element. Three failures must never be spent that
//! way, because no further input clears any of them — a frontier `Incomplete`, a terminal
//! *scanner* stop, and a *descent* budget trip. [`file_element_failure`] is the **one** place an
//! element failure is either filed or re-raised, and it gates on all three before it files; the
//! drivers below carry no swallow of their own.
//!
//! **A stop does not need an `Err` to be spent.** An element that meets a terminal stop and answers
//! it *itself* — catching the trip, or declining on a lookahead window the scanner truncated — hands
//! the driver an `Ok`, and the driver reads it as "no more elements" and **succeeds**. That is the
//! same defect through the exit the failure chokepoint does not cover: a resource-budget stop
//! becomes an accepted absence. [`absence_after_element`] is the second chokepoint, and the **one**
//! place a driver's non-`Err` exits consult either witness. Why the two are separate functions, and
//! why the two witnesses take different baselines, is on it.
//!
//! The descent witness is not carried by the error value. It is a **monotone session counter** on
//! the input (`Input::resource_trips`), bumped by [`InputRef::descend`](crate::InputRef::descend)
//! before the grammar's `From` runs, and the gate compares it against a baseline taken **once per
//! element** (`InputRef::trip_snapshot` / `InputRef::tripped_during_attempt`). That is what makes
//! the re-raise independent of the error type: a
//! [`RecursionLimitReached`](crate::error::RecursionLimitReached) that a discarding sink erases on
//! conversion — `()` does — still cannot be filed here.
//!
//! ## What a counter witnesses, and what it cannot
//!
//! `tripped_during_attempt` proves that **a** trip happened while the element ran. It does not
//! prove that the `Err` in hand **is** that trip. Element code that catches a trip itself, carries
//! on, and then fails *ordinarily inside the same element* hands the driver an ordinary error with
//! the counter already moved — and the driver re-raises it instead of filing it and continuing.
//! That is a real residual, and
//! `tokora/tests/collection_resource_trip.rs::a_caught_trip_inside_one_element_re_raises_its_ordinary_failure`
//! pins the behaviour rather than wishing it away.
//!
//! **The strong form — "re-raise only when this very error is the trip" — is not implementable**:
//! deciding it means interrogating the error value, and the grammar's error type may be `()`,
//! which discards the trip on conversion. A sink that discards is the whole premise of the change
//! that put the witness on the input, so a design that could tell the two errors apart would be
//! reading a payload that is not there.
//!
//! So the floor is **one element** here, and **one attempt** for
//! [`Recover`](crate::parser::Recover), [`InplaceRecover`](crate::parser::InplaceRecover) and
//! [`skip_then_retry`](crate::ParseInput::skip_then_retry). Within that unit the verdict is
//! "something no further input clears happened here", and it **fails closed**: an ordinary failure
//! sharing its unit with a caught trip is re-raised, never the reverse — a real trip is never
//! filed as a diagnostic and never recovered from.
//!
//! Finer granularity is reachable, but only with cooperation from whoever catches the trip: an
//! explicit **rebaseline** — code that deliberately catches a trip declaring it settled, so the
//! enclosing baselines move past it. That is the design if a consumer ever needs it. It is not
//! built, because the escape hatch has no consumer yet and this crate does not publish API on
//! speculation.

use crate::{
  Decision, Emitter, ParseContext, ParseInput, Window,
  container::Container as ContainerT,
  emitter::FullContainerEmitter,
  error::syntax::FullContainer,
  input::{CloseStatus, Cursor, InputRef},
  lexer::Lexer,
  span::{Span as _, Spanned},
};

use super::*;
use handler::*;

pub use delim::*;
pub use handler::{DelimiterHandler, SeparatorHandler};
pub use options::*;
pub use repeated::*;
pub use repeated_while::*;
pub use sep::*;
pub use sep_while::*;

#[macro_use]
mod macros;

mod delim;
mod handler;
mod repeated;
mod repeated_while;

mod options;
mod sep;
mod sep_while;

/// Pushes one parsed element and reports a refused push exactly once per construct.
///
/// The single chokepoint for both container-accounting laws; it replaced twelve separate
/// emission sites that had grown three different conventions between them.
///
/// * **`nums` counts elements the driver PARSED**, not elements the container stored. Count
///   bounds are a property of the input: a container that ran out of room must not turn a
///   satisfied `at_least` into a `TooFew`, nor silently swallow a violated `at_most` by
///   clamping the count it is judged on. Container inadequacy has its own diagnostic, below.
/// * **`FullContainer` is emitted once**, at the first refusal, latched by `full`. A container
///   that refuses one push refuses every later one, so the old per-dropped-element re-emission
///   produced a count that climbed past the capacity it named. `nums` in the payload is the
///   count at the refusal *including* the refused element, so the type's "found N … exceeds …
///   C" reading is true.
///
/// The latch is read only on the refusal arm, so the success path is exactly the pre-existing
/// `push` plus the increment.
#[inline(always)]
pub(super) fn push_element<'inp, 'closure, C, O, L, Ctx, Lang: ?Sized, Cmpl>(
  nums: &mut usize,
  full: &mut bool,
  container: &mut C,
  item: O,
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  anchor: &Cursor<'inp, 'closure, L>,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::Completeness,
  Ctx::Emitter: FullContainerEmitter<'inp, L, Lang>,
  C: ContainerT<O>,
{
  if container.push(item).is_err() && !*full {
    *full = true;
    let span = inp.span_since(anchor);
    let stored = container.len();
    inp.emitter().emit_full_container(FullContainer::of(
      span,
      stored + 1,
      container.max_capacity(),
    ))?;
  }
  *nums += 1;
  Ok(())
}

/// Files one element failure as a diagnostic, or re-raises it — **the single place either can
/// happen** in the try-driven collection families.
///
/// The section-4 never-recoverable law, moved off the four loop bodies and into one chokepoint.
/// It used to be four hand-written copies of a three-way match guard, pinned by a census that
/// counted one exact spelling of the swallow beneath it; a fifth loop written with a different
/// spelling, or written in a file the census did not name, swallowed ungated and the census stayed
/// green. Here there is no second swallow to spell: a driver hands its element failure to this
/// function or propagates it with `?`, and the gate is upstream of the only `emit_error` in the
/// tree.
///
/// # The three witnesses, in order
///
/// * `Cmpl::is_incomplete_error(&err)` — the frontier `Incomplete`. A constant `false` under
///   [`Complete`](crate::input::Complete), so the complete path keeps its codegen.
/// * `inp.at_committed_boundary()` — the **scanner** stop. Reads the *committed* cursor, so it is
///   attempt-relative: a boundary a prior lookahead already latched does not mis-charge an
///   ordinary element failure short of it. Never the lex-offset `at_latched_boundary`, which a
///   prefilled cache would make false-positive on that same ordinary failure.
/// * `inp.tripped_during_attempt(trips)` — the **descent** budget trip, which latches no boundary
///   (it has a control stack, not a position) and so is invisible to the witness above.
///
/// All three are positional/session facts read on the failure path, so a successful element does
/// zero terminal work and none of them costs a trait bound: no `MaybeTerminal` appears in these
/// families.
///
/// # What holds each witness in the guard
///
/// One behavioural suite per witness, each confirmed by neutering the term and watching the suite
/// go red. `GATE_CENSUS` also scans this body for all three, but a needle scan proves presence and
/// ordering, never that the term gates — the census says so itself, and says what it measured.
/// Editing the guard means keeping these green:
///
/// * `Cmpl::is_incomplete_error(&err)` — `input::input_ref::partial_tests`, 2 cells.
/// * `inp.at_committed_boundary()` — `tokora/tests/collection_terminal_stop.rs`, the `r1b_*` cells,
///   5 of them. Its `r1_*` sibling pins only the negative direction (the witness must not fire on
///   a boundary the cursor has not reached), which is why the term was for a while deletable with
///   the whole suite still green.
/// * `inp.tripped_during_attempt(trips)` — `tokora/tests/collection_resource_trip.rs`, 12 cells.
///
/// # `trips` is the caller's, and it must be per element
///
/// The baseline belongs to the *attempt this call judges*, so the caller takes
/// `inp.trip_snapshot()` **inside its element loop**, once per element — not hoisted out beside
/// `latch_snapshot()`, whose absence witness genuinely is per driver attempt. Hoisted, the
/// comparison degrades into a read of the monotone session counter, and every element failure
/// after the parse's first trip re-raises — ordinary syntax errors included. `GATE_CENSUS` pins
/// the placement; the module docs state the granularity floor this leaves, and why the floor
/// cannot be lowered without cooperation from whoever catches a trip.
#[inline(always)]
pub(super) fn file_element_failure<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  since: &Cursor<'inp, 'closure, L>,
  trips: usize,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  if Cmpl::is_incomplete_error(&err)
    || inp.at_committed_boundary()
    || inp.tripped_during_attempt(trips)
  {
    return Err(err);
  }
  let span = inp.span_since(since);
  inp.emitter().emit_error(Spanned::new(span, err))
}

/// Refuses an **absence** conclusion that a terminal stop produced — **the single place either
/// witness is consulted** on the try-driven families' non-`Err` exits.
///
/// [`file_element_failure`] is this function's twin on the `Err` exits, and the two together are the
/// whole of the never-recoverable law in these four drivers. They are deliberately **two**
/// functions, not one:
///
/// * the failure path has an error in hand and **re-raises that value**; there is none here, so this
///   synthesizes a terminal end-of-input instead — the same value the scanner-stop exits have always
///   produced;
/// * the failure path's scanner witness is positional ([`at_committed_boundary`]), because it is
///   discriminating a *failure* that reached the poison from one that did not. An absence exit needs
///   [`latched_during_attempt`] instead: a restore over the latch rewinds the cursor behind a
///   boundary that survives, so every positional reading is clean there while the stop is live;
/// * and the two scanner witnesses take different baselines — one none at all, the other a
///   per-collection snapshot. A single helper would have to carry both and pick, which is a
///   `match` on which caller it has, not a chokepoint.
///
/// [`at_committed_boundary`]: crate::InputRef::at_committed_boundary
/// [`latched_during_attempt`]: crate::InputRef::latched_during_attempt
///
/// # The two witnesses, and the two granularities
///
/// An absence exit concludes "no more elements" from what the last element attempt did. Two facts
/// make that attempt's evidence unrepresentative of the input, and an element that hits either can
/// still return `Ok`, which is exactly why neither reaches the failure chokepoint:
///
/// * `inp.latched_during_attempt(latch)` — a terminal **scanner** stop. An element's own lookahead
///   ([`peek`](crate::InputRef::peek) and friends) latches one and still hands back `Ok` with a
///   short window, so the element may decline, or accept consuming nothing, on a truncated view.
///   Its baseline is **per collection**: the latch is a position, an element that reaches it leaves
///   it standing for every element after, and the stop genuinely ends the token stream there.
/// * `inp.tripped_during_attempt(trips)` — a **descent** budget trip the element caught itself and
///   answered with `Decline`, or with an `Accept` that consumed nothing. It latches no boundary — it
///   has a control stack rather than a position — so the witness above reads `false` for one, and
///   without this term a resource-budget stop is spent as an *accepted absence*: the collection ends
///   clean and succeeds. Its baseline is **per element**, for the reason
///   [`file_element_failure`] gives at length: the counter is a monotone session fact, so a
///   collection-wide baseline would charge every later absence exit in the parse with a trip some
///   earlier element caught and legitimately parsed past.
///
/// The asymmetry in the two baselines is therefore a property of the two facts, not an oversight:
/// a latched boundary stays true of the input, a caught trip does not.
///
/// # Where it does **not** belong
///
/// Only on the exits that conclude absence *from an element attempt*. Not on an exit resting on a
/// **real token**: the delimited drivers' `CloseStatus::Close` arms and the mid-scan closer in
/// `sep/delim` read a committed pre-trip token, so the construct genuinely closed and stays a
/// success — gating those turns a parse that a wider budget completes identically into a failure.
/// Not on a separator-slot probe that runs *before* this cycle's element either: nothing between the
/// cycle's baseline and that probe can trip, so the term would be a constant `false` there.
///
/// # What holds each witness in the guard
///
/// `GATE_CENSUS` pins that the four try-driven drivers spell neither witness themselves and that
/// both appear in this body ahead of the stop — but a needle scan proves presence and ordering,
/// never that a term gates. One behavioural suite per witness does, each confirmed by neutering the
/// term in place and watching the suite go red:
///
/// * `inp.latched_during_attempt(latch)` — `tokora/tests/absence_terminal_stop.rs`, 10 of its 46
///   cells.
/// * `inp.tripped_during_attempt(trips)` — `tokora/tests/collection_resource_trip.rs`'s section 4,
///   all 16 cells.
///
/// To re-check in five minutes rather than by re-deriving it: replace one term with
/// `let _ = <that term>;` above the `if`, leaving the other in the condition, and run
/// `cargo test -p tokora --all-features --no-fail-fast`. The suite named for that witness must go
/// red and the census must stay green. Repeat per witness.
#[inline(always)]
pub(super) fn absence_after_element<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  latch: &Option<L::Offset>,
  trips: usize,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::Completeness,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  if inp.latched_during_attempt(latch) || inp.tripped_during_attempt(trips) {
    return Err(
      UnexpectedEot::eot_of(inp.span().end())
        .into_terminal()
        .into(),
    );
  }
  Ok(())
}

#[cfg(test)]
mod gate_census {
  //! GATE_CENSUS — the section-4 never-recoverable gate, locked to one chokepoint.
  //!
  //! Every resilient emit-and-continue loop body in the try-driven collection families must refuse
  //! to spend a frontier `Incomplete`, a tripped *scanner* limit, or a tripped *descent* budget as
  //! a diagnostic. The three witnesses live in [`file_element_failure`](super::file_element_failure)
  //! and are documented there; what this module pins is that **nothing can reach the swallow
  //! without passing them**.
  //!
  //! # What the previous shape of this census could not see
  //!
  //! It counted the literal text `emit_error(Spanned::new(span,` in four hard-coded sources and
  //! required one gate per match. Both halves could be defeated without noticing:
  //!
  //! * a swallow spelled `emit_error(Spanned::new(other_span, err))` matched nothing, so the tally
  //!   stayed balanced and the census stayed green over an ungated site;
  //! * a swallow in a *fifth* source was not read at all.
  //!
  //! A census that answers by not looking is worse than no census, because it is quoted as
  //! evidence. So the swallow moved: there is exactly **one** `emit_error` call in the whole
  //! `many`/`fold` tree, it is the chokepoint's, and the gate sits above it. The three tests below
  //! pin, in order: that no driver files a diagnostic itself *whatever the spelling*; that each
  //! driver's per-element baseline is taken inside the loop that hosts its chokepoint call; and
  //! that no module of the tree has been added without being classified.
  //!
  //! # The other half of the law: the exits that carry no error
  //!
  //! A stop does not need an `Err` to be spent. An element that catches a *descent* trip itself and
  //! then declines — or that declines on a window a *scanner* stop truncated — hands the driver an
  //! `Ok`, and the driver's absence exit reads it as "no more elements" and succeeds. The failure
  //! chokepoint above never sees it. [`absence_after_element`](super::absence_after_element) is the
  //! second chokepoint and holds both witnesses for those exits, and
  //! [`every_absence_exit_carries_the_terminal_witnesses`] applies the same shape of proof to it:
  //! the try-driven drivers spell **neither** witness themselves, so there is nothing left for them
  //! to spell differently, and the count of chokepoint calls is pinned per source so a new absence
  //! exit cannot land ungated.
  //!
  //! # The one thing the chokepoint cannot centralize
  //!
  //! `trips` is the caller's. It must be `inp.trip_snapshot()` taken **inside the element loop**,
  //! once per element — not hoisted out beside `latch_snapshot()`, whose absence witness genuinely
  //! is per driver attempt. The counter is a monotone session fact and is never cleared, so a
  //! baseline hoisted above the loop is arithmetically a session-absolute read for every element
  //! after the first: an ordinary syntax error in an unrelated construct then ends the collection
  //! instead of being filed, and one deep expression early in a document suppresses every
  //! diagnostic after it. Hoisting it also restores the defect one nesting level up — a trip inside
  //! an *inner* collection that an element swallows charged to the enclosing driver's next ordinary
  //! failure. That placement is a per-caller property, so it is scanned per caller.

  use super::end_state_census::{code_find, code_matches};

  /// The swallow itself — matched on the **call**, not on the arguments, so no spelling of the
  /// span or the error binding can slip past. This is the needle whose count must be zero
  /// everywhere but the chokepoint.
  const EMIT: &str = "emit_error(";

  /// The chokepoint: its call in a driver, and its definition in this module's own source.
  const CHOKEPOINT: &str = "file_element_failure(";
  const CHOKEPOINT_DEF: &str = "fn file_element_failure";

  /// The three witnesses the chokepoint must consult before it reaches [`EMIT`].
  const WITNESSES: [&str; 3] = [
    "Cmpl::is_incomplete_error(&",
    "inp.at_committed_boundary()",
    "inp.tripped_during_attempt(",
  ];

  /// The absence chokepoint: its call in a driver, and its definition in this module's own source.
  const ABSENCE: &str = "absence_after_element(";
  const ABSENCE_DEF: &str = "fn absence_after_element";

  /// The terminal end-of-input the absence chokepoint produces — the needle that ends its guard
  /// region, the way [`EMIT`] ends the failure chokepoint's.
  const ABSENCE_STOP: &str = "into_terminal()";

  /// The two witnesses the absence chokepoint must consult before it reaches [`ABSENCE_STOP`]. The
  /// scanner half is **not** the failure path's `at_committed_boundary`: a restore over the latch
  /// rewinds the cursor behind a boundary that survives, so every positional reading is clean at an
  /// absence exit while the stop is live.
  const ABSENCE_WITNESSES: [&str; 2] =
    ["inp.latched_during_attempt(", "inp.tripped_during_attempt("];

  /// The element attempt whose failure a driver must route through the chokepoint, the descent
  /// witness's baseline, and the loop opener that baseline must be taken after.
  const ATTEMPT: &str = "try_parse_input(inp)";
  const BASELINE: &str = "inp.trip_snapshot()";
  const HOSTING_LOOP: &str = "loop {";

  /// SWALLOW SCAN — every source in the `many` and `fold` trees that can reach an [`InputRef`]
  /// inside a repetition: the eight collection drivers, the four folds, and the three tree `mod.rs`
  /// files that could host a driver of their own. Only these are read for [`EMIT`], and
  /// [`the_gate_census_covers_every_driver_module`] is what keeps the list from silently falling
  /// behind the tree.
  ///
  /// [`InputRef`]: crate::InputRef
  fn swallow_scan_sites() -> [(&'static str, &'static str); 15] {
    let mut sites = [("", ""); 15];
    let (guarded, rest) = sites.split_at_mut(12);
    guarded.copy_from_slice(&progress_guard_sites());
    rest.copy_from_slice(&[
      ("many/delim/mod.rs", include_str!("delim/mod.rs")),
      ("many/sep/mod.rs", include_str!("sep/mod.rs")),
      ("many/sep_while/mod.rs", include_str!("sep_while/mod.rs")),
    ]);
    sites
  }

  /// The four try-driven families: the only drivers that swallow, and therefore the only ones that
  /// call the chokepoint.
  fn try_driven_sites() -> [(&'static str, &'static str); 4] {
    [
      ("many/repeated/mod.rs", include_str!("repeated/mod.rs")),
      ("many/delim/repeated.rs", include_str!("delim/repeated.rs")),
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      ("many/sep/delim/mod.rs", include_str!("sep/delim/mod.rs")),
    ]
  }

  /// A diagnostic filed for an element failure passes the never-recoverable gate first, because
  /// there is exactly one place a diagnostic can be filed and the gate is above it.
  ///
  /// This is the spelling-independent half. The needle is `emit_error(` — the call, with no
  /// argument text — so a swallow renamed, re-spanned or re-wrapped still counts, and the
  /// assertion is that the count is **zero** in every driver rather than that it balances against
  /// something. There is nothing left to balance: the drivers have no swallow of their own.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_element_failure_routes_through_the_gated_chokepoint() {
    for (name, src) in swallow_scan_sites() {
      assert_eq!(
        code_matches(src, EMIT),
        0,
        "{name}: a driver hands its element failure to `file_element_failure` or propagates it \
         with `?` — it never files a diagnostic itself. A swallow written here bypasses the \
         never-recoverable gate whatever it is spelled, which is the defect this census exists to \
         make unspellable"
      );
    }

    // …and the one place it can be filed is the chokepoint, whose own source is read here rather
    // than assumed. `code_matches` ignores whole-line comments, so the prose above and the
    // chokepoint's own doc comment cannot stand in for the call.
    let prod = super::end_state_census::many_mod_production();
    assert_eq!(
      code_matches(prod, EMIT),
      1,
      "`many/mod.rs`: the tree files element diagnostics through exactly one `emit_error` call, \
       inside `file_element_failure`"
    );

    // The three witnesses, in the chokepoint's own body and BEFORE the emission.
    //
    // WHAT THIS PROVES, EXACTLY: each witness is present, named, and textually ahead of the one
    // `emit_error`. Scanning the region from the definition to the call — rather than counting
    // needles over the whole file — is what makes that much true: three independent tallies are
    // equally satisfied by three witnesses scattered anywhere, and a region scan is not. Both
    // `find`s panic rather than pass when the thing being scanned for is absent, so a chokepoint
    // that has been renamed or gutted reports that instead of quietly finding nothing to check.
    //
    // WHAT IT DOES NOT PROVE — and an earlier revision of this comment wrongly claimed it did — is
    // that the witnesses GATE the emission. Textual presence ahead of a call is not control-flow
    // domination. This exact body keeps the census green with the gate entirely gone:
    //
    //     let _ = Cmpl::is_incomplete_error(&err);
    //     let _ = inp.at_committed_boundary();
    //     let _ = inp.tripped_during_attempt(trips);
    //     let span = inp.span_since(since);
    //     inp.emitter().emit_error(Spanned::new(span, err))
    //
    // Verified, not reasoned about: that body was compiled and the whole `--all-features` suite run
    // over it. This test passed. Proving domination from source text means writing a Rust parser
    // inside a test module that must also compile under `--no-default-features` with no `String`
    // and no `Vec`; the cost is out of proportion to a needle scan's value, and it is not what
    // makes the runtime behaviour safe.
    //
    // WHAT PROVES THE GATE GATES is behaviour, one suite per witness, each one confirmed by
    // neutering its term and watching the suite go red:
    //
    // * `Cmpl::is_incomplete_error(&err)` — `input::input_ref::partial_tests`
    //   (`gate_propagates_frontier_incomplete_out_of_repeated`,
    //   `lego_chain_runs_both_modes_to_equivalence`); 2 tests.
    // * `inp.at_committed_boundary()` — `tokora/tests/collection_terminal_stop.rs`, the five `r1b_*`
    //   cells; 5 tests. These were written FOR this finding: the mutation run that produced this
    //   comment found the witness could be deleted with the entire suite still green, because R1
    //   only pinned where it must stay quiet. R1b pins the other direction.
    // * `inp.tripped_during_attempt(trips)` — `tokora/tests/collection_resource_trip.rs`; 12 tests.
    //
    // So this scan is a fast tripwire on the source — it catches a witness dropped, renamed or
    // duplicated in one cheap unit test — and the three suites above are the proof. Delete a
    // witness and both fire; neuter one in place and only the suite does.
    //
    // TO RE-CHECK, in five minutes rather than by re-deriving it: replace one term of the guard
    // with `let _ = <that term>;` above the `if`, leaving the other two in the condition, and run
    // `cargo test -p tokora --all-features --no-fail-fast`. The suite named for that witness must
    // go red, and this test must stay green. Repeat per witness. Automating the loop needs
    // `cargo-mutants` — a full rebuild-and-test per mutant across a 100-binary suite, plus a
    // survivor baseline to triage — which is out of proportion to three terms in one function.
    let def_at = code_find(prod, CHOKEPOINT_DEF).unwrap_or_else(|| {
      panic!(
        "`many/mod.rs`: `{CHOKEPOINT_DEF}` is gone. The swallow chokepoint has been renamed or \
         removed; re-cut this census against whatever replaced it before trusting a green run"
      )
    });
    let body = &prod[def_at..];
    let emit_at = code_find(body, EMIT).unwrap_or_else(|| {
      panic!("`many/mod.rs`: the one `emit_error` call is not inside `file_element_failure`")
    });
    let guard = &body[..emit_at];
    for witness in WITNESSES {
      assert_eq!(
        code_matches(guard, witness),
        1,
        "`many/mod.rs`: the chokepoint must consult all three never-recoverable witnesses before \
         it files — `{witness}` is missing from, or duplicated in, the code ahead of its \
         `emit_error`"
      );
    }
  }

  /// Every driver that calls the chokepoint takes its trip baseline **inside the loop that hosts
  /// the call**, once per element.
  ///
  /// The half the chokepoint cannot own: `trips` is passed in, so the caller decides whether the
  /// comparison means "this element tripped" or "this parse has tripped". The `rfind` panics
  /// rather than passing when a call is not inside a loop at all, so this cannot be satisfied by a
  /// source that has stopped looking like a driver.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_element_loop_baselines_its_trip_witness_per_element() {
    let mut routed = 0;
    for (name, src) in try_driven_sites() {
      let calls = code_matches(src, CHOKEPOINT);
      assert_eq!(
        calls, 1,
        "{name}: one element loop, one chokepoint call — found {calls}"
      );
      assert_eq!(
        code_matches(src, ATTEMPT),
        calls,
        "{name}: every element attempt's failure must reach the chokepoint. An attempt without \
         one either swallows behind the gate's back or propagates with `?`; if the second, move \
         it out of this list and say why"
      );
      assert_eq!(
        code_matches(src, BASELINE),
        calls,
        "{name}: exactly one `trip_snapshot()` baseline per chokepoint call — a second, stray read \
         of the counter is how a session-absolute test gets back in"
      );

      let mut checked = 0;
      let mut from = 0;
      while let Some(at) = code_find(&src[from..], CHOKEPOINT) {
        let call_at = from + at;
        let loop_at = src[..call_at].rfind(HOSTING_LOOP).unwrap_or_else(|| {
          panic!("{name}: a chokepoint call that is not inside a repetition loop")
        });
        assert_eq!(
          code_matches(&src[loop_at..call_at], BASELINE),
          1,
          "{name}: the descent witness is attempt-relative — take the `trip_snapshot()` baseline \
           INSIDE the element loop, once per element. Hoisted out of the loop it reads the \
           monotone session counter, and every element failure after the parse's first trip \
           re-raises, ordinary syntax errors included"
        );
        checked += 1;
        from = call_at + CHOKEPOINT.len();
      }
      assert_eq!(
        checked, calls,
        "{name}: every chokepoint call must have been inspected"
      );
      routed += calls;
    }
    assert_eq!(
      routed, 4,
      "the try-driven families carry exactly four gated loop bodies"
    );
  }

  /// The bound arrangements every driver family re-exposes, and the separator arrangements the
  /// four separated families add. Named once: the leaves are formulaic, and the point of listing
  /// them is that a name **not** in the pattern shows up.
  const BOUNDS: &[&str] = &["at_least", "at_most", "bounded", "unbounded"];
  const ARRANGEMENTS: &[&str] = &[
    "allow_leading",
    "allow_leading_require_trailing",
    "allow_surrounded",
    "allow_trailing",
    "require_leading",
    "require_leading_allow_trailing",
    "require_surrounded",
    "require_trailing",
  ];

  /// The census's own frontier, stated as data: every `mod` declaration in the `many` and `fold`
  /// driver trees, grouped.
  ///
  /// One row for every source [`swallow_scan_sites`] reads, plus the three tree `mod.rs` files that
  /// declare them. A row with no groups declares no modules today, and a `mod` added to it reds
  /// this test rather than quietly widening the tree past the scan.
  ///
  /// **What this closes and what it does not.** It closes the "fifth file" hole for the trees the
  /// drivers live in: a new module beside a driver, or a new driver family, changes a declaration
  /// list here. It does **not** descend into `many/handler` or `many/options`, which hold no
  /// [`InputRef`](crate::InputRef) and run no loop; if one ever does, it belongs in the swallow
  /// scan and its parent belongs here.
  fn driver_tree() -> [(
    &'static str,
    &'static str,
    &'static [&'static [&'static str]],
  ); 16] {
    [
      (
        "many/mod.rs",
        super::end_state_census::many_mod_production(),
        &[&[
          "delim",
          "handler",
          "macros",
          "options",
          "repeated",
          "repeated_while",
          "sep",
          "sep_while",
        ]],
      ),
      (
        "many/delim/mod.rs",
        include_str!("delim/mod.rs"),
        &[&["repeated", "repeated_while"]],
      ),
      (
        "many/sep/mod.rs",
        include_str!("sep/mod.rs"),
        &[&["delim", "parse"]],
      ),
      (
        "many/sep_while/mod.rs",
        include_str!("sep_while/mod.rs"),
        &[&["delim", "parse"]],
      ),
      (
        "fold/mod.rs",
        include_str!("../fold/mod.rs"),
        &[&["fold_while", "rfold", "rfold_while"]],
      ),
      ("fold/rfold.rs", include_str!("../fold/rfold.rs"), &[]),
      (
        "fold/fold_while.rs",
        include_str!("../fold/fold_while.rs"),
        &[],
      ),
      (
        "fold/rfold_while.rs",
        include_str!("../fold/rfold_while.rs"),
        &[],
      ),
      (
        "many/repeated/mod.rs",
        include_str!("repeated/mod.rs"),
        &[BOUNDS],
      ),
      (
        "many/repeated_while/mod.rs",
        include_str!("repeated_while/mod.rs"),
        &[BOUNDS],
      ),
      (
        "many/delim/repeated.rs",
        include_str!("delim/repeated.rs"),
        &[BOUNDS],
      ),
      (
        "many/delim/repeated_while.rs",
        include_str!("delim/repeated_while.rs"),
        &[BOUNDS],
      ),
      (
        "many/sep/parse/mod.rs",
        include_str!("sep/parse/mod.rs"),
        &[BOUNDS, ARRANGEMENTS],
      ),
      (
        "many/sep/delim/mod.rs",
        include_str!("sep/delim/mod.rs"),
        &[BOUNDS, ARRANGEMENTS],
      ),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
        &[BOUNDS, ARRANGEMENTS],
      ),
      (
        "many/sep_while/delim/mod.rs",
        include_str!("sep_while/delim/mod.rs"),
        &[BOUNDS, ARRANGEMENTS],
      ),
    ]
  }

  /// No module of the driver trees has landed without being classified.
  ///
  /// The other half of "the census cannot pass by not looking": the swallow scan reads a fixed
  /// list of sources, and a list is only as good as the guarantee that it is complete. This reads
  /// the `mod` declarations that *define* the tree and requires each one to be accounted for, so a
  /// new file next to a driver cannot be invisible to the scan — it fails here first, naming
  /// itself.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_gate_census_covers_every_driver_module() {
    for (name, src, groups) in driver_tree() {
      let mut declared = 0;
      let mut known = 0;
      for group in groups {
        known += group.len();
      }
      for line in src.lines() {
        let line = line.trim();
        if line.starts_with("//") {
          continue;
        }
        // Visibility first, so a `pub mod` cannot slip through a prefix test written for `mod`.
        let line = line
          .strip_prefix("pub(crate) ")
          .or_else(|| line.strip_prefix("pub(super) "))
          .or_else(|| line.strip_prefix("pub "))
          .unwrap_or(line);
        let rest = match line.strip_prefix("mod ") {
          Some(rest) => rest,
          None => continue,
        };
        // `mod x;` and `mod x {` alike.
        let module = rest
          .split(|c: char| c == ';' || c == '{' || c.is_whitespace())
          .next()
          .unwrap_or("");
        assert!(
          groups.iter().any(|group| group.contains(&module)),
          "{name}: `mod {module};` is new to the driver tree and this census does not know it. If \
           it can reach an `InputRef` inside a repetition, add its source to `swallow_scan_sites` \
           so its element failures are read; if it is an `Apply`/options leaf, add its name here. \
           Do not do neither — the swallow scan reads a fixed list, and an unlisted source is a \
           source nothing checks"
        );
        declared += 1;
      }
      assert_eq!(
        declared, known,
        "{name}: this census names {known} module(s) and the source declares {declared}. A name \
         that has been removed or renamed leaves the census scanning for something that is not \
         there, which is how a list stops meaning anything"
      );
    }
  }

  /// Every `*_while` driver's decision-window peek is terminal-aware: it uses the
  /// terminal-reporting `peek_with_emitter_terminal` (never the bare `peek_with_emitter`, whose
  /// short window a mid-window trip would hide) and surfaces the stop with `into_terminal`, so a
  /// resource-limit trip during the decision peek is never read as a clean end of list.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_while_decision_gate_is_terminal_aware() {
    let sites = [
      (
        "many/repeated_while/mod.rs",
        include_str!("repeated_while/mod.rs"),
      ),
      (
        "many/delim/repeated_while.rs",
        include_str!("delim/repeated_while.rs"),
      ),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
      (
        "many/sep_while/delim/mod.rs",
        include_str!("sep_while/delim/mod.rs"),
      ),
      ("fold/fold_while.rs", include_str!("../fold/fold_while.rs")),
      (
        "fold/rfold_while.rs",
        include_str!("../fold/rfold_while.rs"),
      ),
    ];
    for (name, src) in sites {
      assert!(
        src.contains("peek_with_emitter_terminal::<"),
        "{name}: the decision-window peek must use the terminal-reporting variant \
         (`peek_with_emitter_terminal`)"
      );
      assert!(
        !src.contains("peek_with_emitter::<"),
        "{name}: no bare decision-window peek may remain — a mid-window trip would hide in its \
         short window; use `peek_with_emitter_terminal`"
      );
      assert!(
        src.contains("into_terminal()"),
        "{name}: a decision-window terminal stop must be surfaced (`into_terminal`)"
      );
      // Exactly one eager terminal gate per decision peek: the flag is consulted immediately after
      // the peek and nowhere else. Per-arm re-scatter — a fallible or emitting call between the peek
      // and the check — reopens the terminal-precedence hole, so the gate count must track the peek
      // count (the turbofish form keeps prose mentions from skewing either tally).
      assert_eq!(
        src.matches("peek_with_emitter_terminal::<").count(),
        src.matches("if terminal").count(),
        "{name}: exactly one terminal gate per decision peek — the eager gate immediately after the \
         peek; per-arm re-scatter reopens the terminal-precedence hole"
      );
    }
  }

  /// Every non-delimited separated driver's separator-slot decision gate is terminal-aware: it
  /// probes with `try_expect_or_stop` (never the bare `try_expect`, whose `Ok(None)` folds a trip
  /// together with genuine absence and ends the list cleanly). The delimited separated drivers
  /// route their separator-slot `None` through `probe_close`, whose `Tripped` arm surfaces the stop
  /// instead — so they are exempt here and covered by that path.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_nondelim_separator_slot_surfaces_terminal() {
    let sites = [
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
    ];
    for (name, src) in sites {
      assert!(
        src.contains("try_expect_or_stop(|t"),
        "{name}: the separator-slot decision gate must probe with `try_expect_or_stop`, so a \
         terminal stop surfaces instead of folding into a clean end"
      );
    }
  }

  /// The absence-exit shape of every guard-bearing driver, as data.
  ///
  /// The count is how many [`ABSENCE`] calls the source must carry; **0** marks the older *inline*
  /// shape, which spells `latched_during_attempt` at each exit and consults no descent witness at
  /// all. Index-aligned with [`progress_guard_sites`], and
  /// [`every_absence_exit_carries_the_terminal_witnesses`] checks the two lists name the same
  /// sources in the same order, so neither can drift.
  ///
  /// **The zeroes are a residual, recorded rather than blessed.** The four `*_while` drivers and the
  /// four folds have the *same* hole the chokepoint closes for the try-driven four: their element
  /// runs through `parse_input`/`try_parse_input` and propagates a trip with `?`, so a trip is
  /// terminal there **unless the element catches it itself** — and an element that catches one and
  /// then declines, or accepts consuming nothing, ends those collections cleanly and successfully,
  /// exactly as it did in the try-driven four before this. Measured rather than inferred: `fold`
  /// over such an element returns the same `Ok` under a budget the element exceeds as under one it
  /// does not, and so does `repeated_while` over the stalling shape. Closing it there needs a
  /// per-element trip baseline each of those loops does not take (two of the folds would have to
  /// stop being `while let` loops to take one), and it is a separate change with its own
  /// regressions. What this row does hold is that a `0` source cannot *half*-adopt: it must take no
  /// trip baseline and read no trip witness, so the day one does, this classification goes red and
  /// has to be re-cut.
  fn absence_exit_shapes() -> [(&'static str, usize); 12] {
    [
      ("many/repeated/mod.rs", 1),
      ("many/repeated_while/mod.rs", 0),
      ("many/delim/repeated.rs", 4),
      ("many/delim/repeated_while.rs", 0),
      ("many/sep/parse/mod.rs", 2),
      ("many/sep/delim/mod.rs", 2),
      ("many/sep_while/parse/mod.rs", 0),
      ("many/sep_while/delim/mod.rs", 0),
      ("fold/mod.rs", 0),
      ("fold/rfold.rs", 0),
      ("fold/fold_while.rs", 0),
      ("fold/rfold_while.rs", 0),
    ]
  }

  /// Every guard-bearing driver's absence exits reach the never-recoverable witnesses, and the
  /// try-driven four reach **both** of them through one chokepoint they cannot spell around.
  ///
  /// A driver's absence exits — the no-progress stall, the element-decline break, a condition's
  /// `Action::Stop`, and in the delimited drivers the close probe's `WrongToken`/`Eof` arms —
  /// conclude "no more elements" from what the last element attempt did. Two facts make that
  /// attempt's evidence unrepresentative while it still returns `Ok`, so neither reaches
  /// [`file_element_failure`](super::file_element_failure):
  ///
  /// * a terminal **scanner** stop the element's own lookahead latched, which leaves the pre-trip
  ///   tokens cached and the window short (the eager decision gate does not see it: the next window
  ///   is served whole from that cache and carries no terminal flag). The witness is
  ///   `latched_during_attempt` — presence-plus-change against a per-collection `latch_snapshot`,
  ///   never a positional reading, because a rollback over the latch moves the offset back behind a
  ///   boundary that survives;
  /// * a **descent** budget trip the element caught itself. It latches no boundary, so the witness
  ///   above reads `false` for one, and the absence exit spends the stop as a *success*. The witness
  ///   is `tripped_during_attempt` against the per-**element** baseline, for the reason
  ///   [`every_element_loop_baselines_its_trip_witness_per_element`] gives.
  ///
  /// In the delimited drivers the gate belongs *inside* the close probe's `WrongToken`/`Eof` arms,
  /// never ahead of the probe: the probe is cache-first, so a `Close` verdict on a real pre-trip
  /// token is a genuine close and must keep parsing — as is a reached bound, and as is the mid-scan
  /// closer in `sep/delim`. Gating those would fail a parse a wider budget completes identically.
  ///
  /// What this pins, per source: the baseline is taken; the source carries the shape
  /// [`absence_exit_shapes`] classifies it as, by count; and a chokepointed source spells **neither**
  /// witness itself, which is the "nothing left to balance" form — the strongest of these the
  /// codebase has, and the one the swallow scan above uses. What it does not pin is that the
  /// chokepoint's own guard *gates*, which is
  /// [`the_absence_chokepoint_consults_both_witnesses_before_it_stops`]'s region scan for presence
  /// and `tokora/tests/collection_resource_trip.rs`'s section 4 for behaviour.
  #[test]
  fn every_absence_exit_carries_the_terminal_witnesses() {
    let sites = progress_guard_sites();
    let shapes = absence_exit_shapes();
    let mut chokepointed = 0;
    let mut inline = 0;
    for i in 0..sites.len() {
      let (name, src) = sites[i];
      let (classified, calls) = shapes[i];
      assert_eq!(
        name, classified,
        "the guard-bearing source list and the absence-exit classification have drifted apart at \
         row {i}: `{name}` against `{classified}`. They are index-aligned on purpose — a row that \
         names a source the scan does not read is a row that checks nothing"
      );
      assert!(
        src.contains("latch_snapshot()"),
        "{name}: the absence witness is attempt-relative — take the `latch_snapshot()` baseline once \
         per attempt, or a boundary an enclosing lookahead latched is mis-charged to this driver"
      );

      if calls == 0 {
        // The inline shape: the scanner latch at each exit, and no descent witness anywhere. It
        // must not have half-adopted the other half — see `absence_exit_shapes`.
        assert!(
          src.contains("latched_during_attempt("),
          "{name}: every absence exit must witness a terminal stop this attempt latched \
           (`latched_during_attempt`) before concluding the construct ended"
        );
        assert_eq!(
          code_matches(src, ABSENCE),
          0,
          "{name}: this source is classified as the inline absence shape but calls the chokepoint. \
           Re-cut `absence_exit_shapes` with the call count, so the count is what is checked"
        );
        assert_eq!(
          code_matches(src, BASELINE) + code_matches(src, "inp.tripped_during_attempt("),
          0,
          "{name}: this source is classified as taking NO descent-trip baseline, and now reads one. \
           Half-adopting the trip witness is how an absence exit ends up gated on one witness and \
           not the other — route the exits through `absence_after_element` and re-cut \
           `absence_exit_shapes`"
        );
        inline += 1;
      } else {
        assert_eq!(
          code_matches(src, ABSENCE),
          calls,
          "{name}: expected {calls} absence-exit gate(s) (`{ABSENCE}`). An exit added without one \
           concludes the construct ended on evidence a terminal stop truncated — and an exit whose \
           gate was removed is the same defect, which is why this is a count and not a presence test"
        );
        for witness in ABSENCE_WITNESSES {
          assert_eq!(
            code_matches(src, witness),
            0,
            "{name}: a chokepointed driver hands its absence exits to `absence_after_element` — it \
             never reads `{witness}` itself. A hand-spelled gate can consult one witness and not the \
             other, which is exactly the defect the chokepoint exists to make unspellable"
          );
        }
        chokepointed += calls;
      }
    }
    assert_eq!(
      (chokepointed, inline),
      (9, 8),
      "the try-driven families carry exactly nine chokepointed absence exits across four sources, \
       and eight sources still carry the inline shape. Both halves are pinned so that adopting the \
       chokepoint in a ninth source, or losing an exit from one of the four, has to be deliberate"
    );
  }

  /// The absence chokepoint consults **both** never-recoverable witnesses before it produces its
  /// terminal end-of-input, and it is the only place in the tree that reads the scanner latch.
  ///
  /// The absence twin of the region scan in
  /// [`every_element_failure_routes_through_the_gated_chokepoint`], and it proves exactly as much
  /// and as little: each witness is present, named, and textually ahead of the stop. That is a
  /// tripwire on the source — a witness dropped, renamed or duplicated reds one cheap unit test —
  /// and **not** a proof of control-flow domination, which a body reading both into `let _ =`
  /// bindings would satisfy with the gate gone. Verified, not reasoned about: with the descent term
  /// so neutered this test and the whole census stayed green while
  /// `tokora/tests/collection_resource_trip.rs` red 16 cells. What proves each gate gates is that
  /// suite — section 4's 16 cells for the descent term, and
  /// `tokora/tests/absence_terminal_stop.rs`'s 10 (of 46) for the scanner term. Both `find`s panic
  /// rather than pass when what they scan for is absent.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_absence_chokepoint_consults_both_witnesses_before_it_stops() {
    let prod = super::end_state_census::many_mod_production();
    assert_eq!(
      code_matches(prod, "inp.latched_during_attempt("),
      1,
      "`many/mod.rs`: the scanner latch is read once in this tree, inside `absence_after_element`"
    );
    let def_at = code_find(prod, ABSENCE_DEF).unwrap_or_else(|| {
      panic!(
        "`many/mod.rs`: `{ABSENCE_DEF}` is gone. The absence chokepoint has been renamed or \
         removed; re-cut this census against whatever replaced it before trusting a green run"
      )
    });
    let body = &prod[def_at..];
    let stop_at = code_find(body, ABSENCE_STOP).unwrap_or_else(|| {
      panic!(
        "`many/mod.rs`: `absence_after_element` no longer surfaces the stop as a terminal \
         end-of-input (`{ABSENCE_STOP}`)"
      )
    });
    let guard = &body[..stop_at];
    for witness in ABSENCE_WITNESSES {
      assert_eq!(
        code_matches(guard, witness),
        1,
        "`many/mod.rs`: the absence chokepoint must consult both never-recoverable witnesses before \
         it stops — `{witness}` is missing from, or duplicated in, the code ahead of its terminal \
         end-of-input. One witness alone leaves the other's stop spendable as an accepted absence"
      );
    }
  }

  /// Every no-progress guard measures committed consumption (`span().end()`), never a cache-front
  /// cursor comparison. A lookahead fill moves the cursor across skipped trivia without consuming
  /// anything, which a cursor-keyed guard reads as false progress — a zero-width element behind a
  /// trivia gap then runs an extra cycle. This pins the metric across every guard-bearing driver so
  /// the class cannot regrow.
  #[test]
  fn every_progress_guard_reads_committed_progress() {
    for (name, src) in progress_guard_sites() {
      assert!(
        !src.contains(".as_inner() == ") && !src.contains(".as_inner() != "),
        "{name}: no-progress guards must compare committed consumption (`span().end()`), never a \
         cache-front cursor (`.as_inner()` equality) — a lookahead fill moves the cursor without \
         committing"
      );
    }
  }

  /// The guard-bearing driver sources: every collection and fold driver that runs a repetition loop
  /// and therefore needs both the committed-progress metric and the absence witness.
  pub(super) fn progress_guard_sites() -> [(&'static str, &'static str); 12] {
    [
      ("many/repeated/mod.rs", include_str!("repeated/mod.rs")),
      (
        "many/repeated_while/mod.rs",
        include_str!("repeated_while/mod.rs"),
      ),
      ("many/delim/repeated.rs", include_str!("delim/repeated.rs")),
      (
        "many/delim/repeated_while.rs",
        include_str!("delim/repeated_while.rs"),
      ),
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      ("many/sep/delim/mod.rs", include_str!("sep/delim/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
      (
        "many/sep_while/delim/mod.rs",
        include_str!("sep_while/delim/mod.rs"),
      ),
      ("fold/mod.rs", include_str!("../fold/mod.rs")),
      ("fold/rfold.rs", include_str!("../fold/rfold.rs")),
      ("fold/fold_while.rs", include_str!("../fold/fold_while.rs")),
      (
        "fold/rfold_while.rs",
        include_str!("../fold/rfold_while.rs"),
      ),
    ]
  }
}

#[cfg(test)]
mod end_state_census {
  //! END_STATE_CENSUS and its companions — the accounting laws of the repetition drivers,
  //! locked by count.
  //!
  //! Every exit that returns `Ok` from a repetition driver runs that driver's end-state pass
  //! exactly once before producing the success value; `Err` exits run it never. The defect this
  //! guards against is a second `Ok`-returning exit that jumps over the pass — and because the
  //! skipping exit is the one taken on WELL-FORMED input, the pass becomes dead code precisely
  //! where it matters and every test that feeds conforming input stays green and blind.

  /// Counts occurrences of `needle` on lines that are not whole-line comments, so prose
  /// mentions of a counted name do not skew a tally.
  ///
  /// Shared with [`gate_census`](super::gate_census), whose baseline tallies need the same
  /// comment-blindness for the same reason.
  pub(super) fn code_matches(src: &str, needle: &str) -> usize {
    src
      .lines()
      .filter(|line| !line.trim_start().starts_with("//"))
      .map(|line| line.matches(needle).count())
      .sum()
  }

  /// The byte offset of `needle`'s first occurrence on a line that is not a whole-line comment,
  /// or `None` when there is none.
  ///
  /// The positional twin of [`code_matches`], and comment-blind for the same reason: a region scan
  /// anchored on a needle a doc comment also spells would measure the wrong region, which is a
  /// quieter failure than measuring nothing.
  pub(super) fn code_find(src: &str, needle: &str) -> Option<usize> {
    let mut at = 0;
    for line in src.split_inclusive('\n') {
      let hit = if line.trim_start().starts_with("//") {
        None
      } else {
        line.find(needle)
      };
      if let Some(k) = hit {
        return Some(at + k);
      }
      at += line.len();
    }
    None
  }

  /// This module's own file with the census modules cut off the end — several needles below
  /// appear verbatim in census source and would otherwise be counted as production sites.
  pub(super) fn many_mod_production() -> &'static str {
    let src = include_str!("mod.rs");
    src
      .split_once("#[cfg(test)]")
      .expect("the census marker must be present in its own source")
      .0
  }

  /// The eight collection drivers: source, the literal that spells the end-state pass, the
  /// number of times it must appear, the literal that spells a success exit, and its count.
  const SITES: &[(&str, &str, &str, usize, &str, usize)] = &[
    (
      "many/repeated/mod.rs",
      include_str!("repeated/mod.rs"),
      "rh.on_stop(",
      1,
      "rh.on_stop(",
      1,
    ),
    (
      "many/repeated_while/mod.rs",
      include_str!("repeated_while/mod.rs"),
      "rh.on_stop(",
      2,
      "rh.on_stop(",
      2,
    ),
    (
      "many/delim/repeated.rs",
      include_str!("delim/repeated.rs"),
      "on_stop(nums, inp, &span)",
      2,
      ".map(|_| mem::take(container))",
      2,
    ),
    (
      "many/delim/repeated_while.rs",
      include_str!("delim/repeated_while.rs"),
      "on_stop(nums, inp, &span)",
      3,
      ".map(|_| mem::take(container))",
      3,
    ),
    (
      "many/sep/parse/mod.rs",
      include_str!("sep/parse/mod.rs"),
      "self.handle_end(",
      3,
      "return self.handle_end(",
      3,
    ),
    (
      "many/sep/delim/mod.rs",
      include_str!("sep/delim/mod.rs"),
      "parser.handle_end(",
      2,
      "Ok(inp.span_since(&anchor))",
      2,
    ),
    (
      "many/sep_while/parse/mod.rs",
      include_str!("sep_while/parse/mod.rs"),
      "self.handle_end(",
      3,
      "return self.handle_end(",
      3,
    ),
    (
      "many/sep_while/delim/mod.rs",
      include_str!("sep_while/delim/mod.rs"),
      "parser.handle_end(",
      4,
      "Ok(inp.span_since(&anchor))",
      4,
    ),
  ];

  /// END_STATE_CENSUS. What this actually pins, row by row — stated precisely, because the
  /// equality is not equally informative everywhere:
  ///
  /// * **Every row**: the *pinned totals*. Deleting an end-state pass, or adding a driver exit
  ///   without one, moves a count away from its literal and fails.
  /// * `sep/delim` and `sep_while/delim` (the two rows where the defect actually lived): the
  ///   pass literal and the exit literal are **independent**, so pass-count == exit-count is a
  ///   real assertion. Reverting the mid-scan-closer arm moves both, independently.
  /// * `sep{,_while}/parse`: the exit literal is a superstring of the pass literal, so equality
  ///   asserts that every `handle_end` call is the `return` of it — a non-returning call fails.
  /// * `repeated`, `repeated_while`, `delim/repeated{,_while}`: the exit and the pass are the
  ///   same physical expression by construction (`rh.on_stop(…)` is the tail;
  ///   `return on_stop(…).map(|_| mem::take(container))` fuses both), so equality holds
  ///   structurally there and only the totals bite.
  ///
  /// The bare-`return Ok(` check below is what closes the gap the equality leaves: it catches a
  /// *new* success exit of any shape, in any of the eight sources, including the six where the
  /// equality cannot.
  #[test]
  fn every_driver_ok_exit_runs_the_end_state_pass() {
    for (name, src, pass, pass_n, exit, exit_n) in SITES {
      let passes = code_matches(src, pass);
      let exits = code_matches(src, exit);
      assert_eq!(
        passes, *pass_n,
        "{name}: expected {pass_n} end-state pass call(s) (`{pass}`), found {passes}"
      );
      assert_eq!(
        exits, *exit_n,
        "{name}: expected {exit_n} success exit(s) (`{exit}`), found {exits}"
      );
      assert_eq!(
        passes, exits,
        "{name}: every `Ok`-returning exit runs the end-state pass exactly once"
      );

      // A driver's only bare `return Ok(...)` form is the whole-construct span the two
      // delimited separated drivers return; the other six have none at all. Any other
      // `return Ok(` is a success exit that was added without an end-state pass.
      let bare = code_matches(src, "return Ok(");
      let anchored = code_matches(src, "return Ok(inp.span_since(&anchor))");
      assert_eq!(
        bare, anchored,
        "{name}: a success exit that returns anything but the whole-construct span \
         (`return Ok(inp.span_since(&anchor))`) has been added without its end-state pass"
      );
    }
  }

  /// LIMIT_PAYLOAD_CENSUS 2a — `FullContainer` has exactly one emission site in the whole
  /// `many` tree: the `push_element` chokepoint. Twelve separate sites is how three different
  /// counting conventions grew between them, and how the once-per-construct latch was lost.
  #[test]
  fn full_container_has_one_emission_site() {
    let mut total = code_matches(many_mod_production(), "FullContainer::of(");
    for (name, src, ..) in SITES {
      let n = code_matches(src, "FullContainer::of(");
      assert_eq!(
        n, 0,
        "{name}: drivers must push through `push_element`, never emit `FullContainer` directly"
      );
      total += n;
    }
    assert_eq!(
      total, 1,
      "`FullContainer::of(` is constructed once, inside `push_element`"
    );
  }

  /// LIMIT_PAYLOAD_CENSUS 2b — every `TooMany` names a count that actually exceeds its limit.
  ///
  /// Both `TooMany` and `FullContainer` render as "found {nums} … exceeds … {limit}", so a
  /// `nums` equal to the limit renders a self-contradicting sentence. Each emission site
  /// therefore passes `limit + 1`, which is also the smallest count every one of the eight
  /// drivers can produce at its own detection point — the only value that makes one history
  /// yield one payload whichever builder produced it.
  ///
  /// The per-line conjunction assumes the site fits on one line, which all eight do at the
  /// current rustfmt width; a future wrap would need the needle re-cut rather than dropped.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_too_many_payload_exceeds_its_limit() {
    let sites: [(&str, &str); 7] = [
      ("parser/with.rs", include_str!("../with.rs")),
      (
        "many/handler/maximum.rs",
        include_str!("handler/maximum.rs"),
      ),
      (
        "many/handler/bounded.rs",
        include_str!("handler/bounded.rs"),
      ),
      (
        "many/delim/repeated/at_most.rs",
        include_str!("delim/repeated/at_most.rs"),
      ),
      (
        "many/delim/repeated/bounded.rs",
        include_str!("delim/repeated/bounded.rs"),
      ),
      (
        "many/delim/repeated_while/at_most.rs",
        include_str!("delim/repeated_while/at_most.rs"),
      ),
      (
        "many/delim/repeated_while/bounded.rs",
        include_str!("delim/repeated_while/bounded.rs"),
      ),
    ];
    let mut total = 0;
    for (name, src) in sites {
      for line in src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//") && l.contains("TooMany::of("))
      {
        assert!(
          line.contains("+ 1,"),
          "{name}: `TooMany`'s count must exceed the limit it names (`limit + 1`): {}",
          line.trim()
        );
        total += 1;
      }
    }
    assert_eq!(
      total, 8,
      "the eight `TooMany` emission sites: two end checks in `with.rs`, the two mid-loop \
       `on_element` hooks, and the four delimited-repeated `on_stop` closures"
    );
  }

  /// MID_LOOP_PAIRING_CENSUS — `RepeatedHandler::on_stop` deliberately no longer re-checks the
  /// maximum, which is sound *only* because every consumer calls `on_element` for every
  /// element: a construct exceeds `max` iff some element saw the pre-count equal to it, so the
  /// mid-loop hook has already reported the violation exactly once. This pins both halves of
  /// that pairing, and pins that no third consumer can land without them.
  #[test]
  fn every_repeated_handler_consumer_calls_the_mid_loop_hook() {
    // No `Vec` here: this module is compiled under `--no-default-features` too.
    let mut consumers = [""; 2];
    let mut found = 0;
    for (name, src) in super::gate_census::progress_guard_sites() {
      if code_matches(src, "rh.on_stop(") == 0 {
        continue;
      }
      assert!(
        code_matches(src, "rh.on_element(") > 0,
        "{name}: a `RepeatedHandler` consumer must call `on_element` for every element — \
         `on_stop` does not re-check the maximum"
      );
      assert!(
        found < consumers.len(),
        "{name}: a third `RepeatedHandler` consumer has landed; extend this census and check \
         that it pairs the mid-loop hook with the end pass"
      );
      consumers[found] = name;
      found += 1;
    }
    assert_eq!(
      consumers,
      ["many/repeated/mod.rs", "many/repeated_while/mod.rs"],
      "exactly two sources consume `RepeatedHandler`; a third must pair the mid-loop hook \
       with the end pass before it lands"
    );
  }

  /// SEPARATOR_DELIVERY_CENSUS — every separator a driver consumes goes through
  /// `observe_separator`, whose clone lives inside the `OBSERVES_SEPARATORS` guard. Four arms
  /// per file is the whole of `handle_separator`'s `State` match: leading, happy path,
  /// duplicate, and (via the state it leaves behind) trailing. A driver calling `on_separator`
  /// directly would bypass the guard and clone a token for a container that ignores it.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_separator_arm_delivers_through_the_guard() {
    let sites = [
      ("many/sep/parse/mod.rs", include_str!("sep/parse/mod.rs")),
      (
        "many/sep_while/parse/mod.rs",
        include_str!("sep_while/parse/mod.rs"),
      ),
    ];
    for (name, src) in sites {
      assert_eq!(
        code_matches(src, "observe_separator("),
        4,
        "{name}: all four `handle_separator` arms deliver their separator"
      );
      assert_eq!(
        code_matches(src, "on_separator("),
        0,
        "{name}: drivers deliver through `observe_separator`, never `on_separator` directly — \
         the direct call clones unconditionally"
      );
    }
  }
}
