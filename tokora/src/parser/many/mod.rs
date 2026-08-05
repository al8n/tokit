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
//! drivers below carry no swallow of their own. The scanner stop is read there through **two**
//! witnesses rather than one, because the obvious reading of it lives inside the rollback set —
//! see "the two scanner readings" below.
//!
//! **A stop does not need an `Err` to be spent.** An element that meets a terminal stop and answers
//! it *itself* — catching the trip, or declining on a lookahead window the scanner truncated — hands
//! the driver an `Ok`, and the driver reads it as "no more elements" and **succeeds**. That is the
//! same defect through the exit the failure chokepoint does not cover: a resource-budget stop
//! becomes an accepted absence. [`absence_after_element`] is the second chokepoint, and the **one**
//! place a driver's *absence* exits consult either witness. Why the two are separate functions, and
//! why the two witnesses take different baselines, is on it.
//!
//! **The absence and real-closer chokepoints are not confined to the try-driven four.** The other
//! eight guard-bearing sources — the four `*_while` collection drivers and the four fold sources —
//! never file an element's `Err` at all, so a trip an element *hands back* propagates untouched and
//! is terminal there without any gate. What they could not see is the same `Ok`: an element that
//! answers the trip itself and then declines, or accepts consuming nothing, ended those collections
//! cleanly and successfully. All twelve now take the per-element trip baseline and route every
//! absence exit through [`absence_after_element`] and every probed closer through
//! [`close_after_element`]; `GATE_CENSUS`'s
//! `every_driver_baselines_its_trip_witness_inside_its_element_loop` is what keeps the baseline in
//! the loop that hosts each call, in all twelve.
//!
//! **The two FACTS are not the same kind, and a committed closer settles only one of them.** A
//! scanner stop is, in its positional aspect, a fact about a *token position*: a construct that
//! closed on a real pre-trip token closed **before** the boundary, so a boundary latched past that
//! closer says nothing about it, and gating a real close on it would fail a parse a wider window
//! completes identically. A descent trip is a *counter event that happened during the element
//! attempt*: a valid closer arriving afterwards does not unmake it. An element that caught one,
//! reported *no more elements*, and was then followed by a real closer therefore yields a
//! **successfully closed collection that silently spent a resource-limit stop** — the same defect,
//! through the one exit the absence chokepoint deliberately does not cover. So the real-closer
//! exits take the descent witness and **neither** scanner one, through [`close_after_element`].
//! Three chokepoints rather than one guard with a mode flag, because the three exits differ in what
//! they hold: an error to re-raise, every witness, or the descent counter alone.
//!
//! The descent witness is not carried by the error value. It is a **monotone session counter** on
//! the input (`Input::resource_trips`), bumped by [`InputRef::descend`](crate::InputRef::descend)
//! before the grammar's `From` runs, and the gate compares it against a baseline taken **once per
//! element** (`InputRef::trip_snapshot` / `InputRef::tripped_during_attempt`). That is what makes
//! the re-raise independent of the error type: a
//! [`RecursionLimitReached`](crate::error::RecursionLimitReached) that a discarding sink erases on
//! conversion — `()` does — still cannot be filed here.
//!
//! ## The two scanner readings, and why a latch alone cannot answer
//!
//! The scanner half has a counter of its own, for a reason the descent half never had to face. The
//! two readings the drivers *used* to have — the poison boundary's value at an absence exit, and
//! the committed cursor's position at a failure exit — are both inside the **rollback set**: a
//! [`Checkpoint`](crate::input::Checkpoint) carries the boundary and a restore copies it back
//! verbatim, along with the cursor, the cache and the emissions.
//!
//! Grammar code is entitled to catch a scanner stop inside an [`attempt`](crate::InputRef::attempt)
//! of its **own** and then decline it. That inner rollback puts the boundary back to the value the
//! *driver* snapshotted and the cursor back behind it, so at the exit that follows every reading of
//! either says clean — over a stop that is live, already diagnosed, and will re-trip on the next
//! scan of the same prefix. Relocating a read closes one nesting level and opens the next: **a cell
//! inside the rollback set cannot witness an event across a rollback at any depth.**
//!
//! So the load-bearing scanner witness is `Input::scanner_trips`, a monotone session counter bumped
//! by `InputRef::latch_if_limit_tripped` — the crate's sole terminal predicate, reached by both
//! lexing drivers — read through `InputRef::scanner_trip_snapshot` /
//! `InputRef::scanner_tripped_during_attempt`. It is outside the rollback set by construction, and
//! it is the same answer `parser::recovery_gate` reached for the
//! recovery combinators. Its baseline here is **per collection**, beside `latch_snapshot()` and
//! deliberately unlike the descent counter's per-element one; [`absence_after_element`] says why
//! the two granularities differ.
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
//! "something no further input clears happened here", and it **fails closed** at the failure,
//! absence and real-closer chokepoints, and at those three recovery combinators: an ordinary
//! failure sharing its unit with a caught trip is re-raised, never the reverse, and a real trip
//! that reaches one of them is never filed as a diagnostic and never recovered from. `Accept`
//! reaches none of these gates — see "the channel neither chokepoint closes" below for the one
//! exit this floor does not cover.
//!
//! Finer granularity is reachable, but only with cooperation from whoever catches the trip: an
//! explicit **rebaseline** — code that deliberately catches a trip declaring it settled, so the
//! enclosing baselines move past it. That is the design if a consumer ever needs it. It is not
//! built, because the escape hatch has no consumer yet and this crate does not publish API on
//! speculation.
//!
//! ## The channel neither chokepoint closes, by design
//!
//! [`absence_after_element`] and [`close_after_element`] gate every exit that concludes the
//! collection from an element's **absence** — a decline, a no-progress stall, or a real closer
//! committed after either. An `Accept` concludes nothing of the driver's: the element produced a
//! value, and the driver is faithfully collecting what it was handed rather than manufacturing "no
//! more elements" out of a stop the caller never learns about. Neither chokepoint is called on
//! that path, and neither should be — refusing it would mean refusing a value the grammar actually
//! produced, on the strength of a stop the grammar already answered.
//!
//! So an element that catches a trip, still consumes, and still answers `Accept` spends it —
//! permanently, not only for that one element. The next cycle takes its own trip baseline
//! ([`InputRef::trip_snapshot`](crate::InputRef::trip_snapshot)) fresh, *after* the accepting
//! cycle's trip already happened, so by the time that next cycle's own absence or close exit runs,
//! the counter has not moved *during it* and there is nothing left for either chokepoint to see.
//! This is the same per-element granularity floor described above, not a special case carved out
//! of it: an accepting cycle was never a unit either chokepoint judges in the first place, so there
//! was never a baseline for a later cycle to inherit.
//!
//! **Gating `Accept` would not be a narrower version of this design; it would be a different one.**
//! Decline and the no-progress stall are gated because the *driver* is the one concluding the
//! construct ended, on evidence a stop may have truncated — that conclusion is the driver's to
//! guard. `Accept` is the element's own conclusion, carrying a value the driver did not manufacture
//! and has no independent reason to distrust. Refusing it would mean a value-producing element
//! could never recover from a budget it deliberately caught — true of every error a grammar is
//! free to catch and answer, not only this one, since tokora has no way to stop a grammar from
//! catching an error and returning a value without diagnosing it. That is a broader contract than
//! this crate makes for any other error, and nothing here proposes making it.
//!
//! `tokora/tests/collection_resource_trip.rs`'s section 6 pins this directly: an element that
//! catches a trip, consumes, and answers `Accept`, followed by a real closer, and the collection
//! succeeds identically whether or not the budget actually tripped. It exists so this boundary
//! cannot drift silently in either direction — narrower, if `Accept` starts being gated; or wider,
//! if the decline/stall/closer exits above stop being gated and the section's non-vacuity controls
//! stop noticing.

// `UnexpectedEot` reaches this family's descendants through their `use super::*`; the drivers
// under `repeated*/`, `sep*/` and `delim/` all name it in a `From` bound and none of them import
// it directly.
use crate::{
  Decision, Emitter, ParseContext, ParseInput, Window,
  container::Container as ContainerT,
  emitter::FullContainerEmitter,
  error::{UnexpectedEot, syntax::FullContainer},
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
/// # The four witnesses, in order
///
/// Three facts, and two readings of the scanner one — neither of which subsumes the other, because
/// one is positional and inside the rollback set while the other is a counter outside it:
///
/// * `Cmpl::is_incomplete_error(&err)` — the frontier `Incomplete`. A constant `false` under
///   [`Complete`](crate::input::Complete), so the complete path keeps its codegen.
/// * `inp.at_committed_boundary()` — the **scanner** stop, *positionally*. Reads the *committed*
///   cursor, so it is attempt-relative: a boundary a prior lookahead already latched does not
///   mis-charge an ordinary element failure short of it. Never the lex-offset
///   `at_latched_boundary`, which a prefilled cache would make false-positive on that same ordinary
///   failure. **Not redundant** with the counter below and the reverse of the absence gate's
///   situation: this one sees a boundary latched *before the driver started* that the element has
///   now run onto, which no per-collection counter baseline can see.
/// * `inp.scanner_tripped_during_attempt(scans)` — the **scanner** stop, as a *session counter
///   event*. The reading no rollback reaches. An element that catches a trip inside an
///   [`attempt`](crate::InputRef::attempt) of its own and then fails ordinarily has that inner
///   restore rewind the cursor, the cache, the emissions **and** the poison boundary together — so
///   the positional witness above reads clean, the trip's own diagnostic is gone from the log, and
///   without this term the driver files a spent scanner budget as an ordinary syntax error and
///   keeps looping. See the module docs' "the two scanner readings" for why relocating a
///   rollback-set read cannot fix this at any depth.
/// * `inp.tripped_during_attempt(trips)` — the **descent** budget trip, which latches no boundary
///   (it has a control stack, not a position) and so is invisible to both witnesses above.
///
/// All four are positional/session facts read on the failure path, so a successful element does
/// zero terminal work and none of them costs a trait bound: no `MaybeTerminal` appears in these
/// families.
///
/// # What holds each witness in the guard
///
/// One behavioural suite per witness, each confirmed by neutering the term and watching the suite
/// go red. `GATE_CENSUS` also scans this body for all four, but a needle scan proves presence and
/// ordering, never that the term gates — the census says so itself, and says what it measured.
/// Editing the guard means keeping these green:
///
/// * `Cmpl::is_incomplete_error(&err)` — `input::input_ref::partial_tests`, 2 cells.
/// * `inp.at_committed_boundary()` — `tokora/tests/collection_terminal_stop.rs`, the `r1b_*` cells,
///   5 of them. Its `r1_*` sibling pins only the negative direction (the witness must not fire on
///   a boundary the cursor has not reached), which is why the term was for a while deletable with
///   the whole suite still green.
/// * `inp.scanner_tripped_during_attempt(scans)` — `tokora/tests/collection_terminal_stop.rs`'s
///   `r1c_*` cell, which no other witness in this guard can answer.
/// * `inp.tripped_during_attempt(trips)` — `tokora/tests/collection_resource_trip.rs`, 12 cells.
///
/// # `trips` is the caller's per ELEMENT, and `scans` is the caller's per COLLECTION
///
/// The descent baseline belongs to the *attempt this call judges*, so the caller takes
/// `inp.trip_snapshot()` **inside its element loop**, once per element — not hoisted out beside
/// `latch_snapshot()`. Hoisted, the comparison degrades into a read of the monotone session
/// counter, and every element failure after the parse's first trip re-raises — ordinary syntax
/// errors included. `GATE_CENSUS` pins the placement; the module docs state the granularity floor
/// this leaves, and why the floor cannot be lowered without cooperation from whoever catches a
/// trip.
///
/// The scanner baseline is the opposite, and takes the *latch's* granularity rather than the
/// descent counter's, because it answers the latch's question: `inp.scanner_trip_snapshot()` beside
/// `latch_snapshot()`, once per collection. A caught descent trip stops being true of the input; a
/// spent scanner budget does not — the token stream ends where it ended, and an element failure
/// after that point is a failure on a view the stop truncated. Per element it would also miss the
/// case an inner rollback creates: element *n* trips and Accepts, element *n+1* then fails against
/// the truncated remainder having tripped nothing itself. [`absence_after_element`] states the same
/// asymmetry for the exits with no error in hand.
#[inline(always)]
pub(super) fn file_element_failure<'inp, 'closure, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
  err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  since: &Cursor<'inp, 'closure, L>,
  scans: usize,
  trips: usize,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  if Cmpl::is_incomplete_error(&err)
    || inp.at_committed_boundary()
    || inp.scanner_tripped_during_attempt(scans)
    || inp.tripped_during_attempt(trips)
  {
    return Err(err);
  }
  let span = inp.span_since(since);
  inp.emitter().emit_error(Spanned::new(span, err))
}

/// Refuses an **absence** conclusion that a terminal stop produced — **the single place any of its
/// witnesses is consulted** on every guard-bearing driver's non-`Err` exits.
///
/// All twelve reach it: the four try-driven collection families, the four `*_while` ones, and the
/// four fold sources. [`file_element_failure`] is this function's twin on the `Err` exits, and it
/// is the narrower of the two — only the try-driven four swallow an element failure, so only they
/// need a gate above the swallow. The other eight propagate an element's `Err` with `?`, which
/// makes a trip they are *handed* terminal without any gate; what they needed was this one, for the
/// trip an element answers and hands back as `Ok`. They are deliberately **two** functions, not one:
///
/// * the failure path has an error in hand and **re-raises that value**; there is none here, so this
///   synthesizes a terminal end-of-input instead — the same value the scanner-stop exits have always
///   produced;
/// * the failure path's positional scanner witness ([`at_committed_boundary`]) is
///   load-bearing there, because it is discriminating a *failure* that reached the poison from one
///   that did not — including a poison latched before the driver even started. An absence exit
///   cannot use a positional reading at all: a restore over the latch rewinds the cursor behind a
///   boundary that survives, so every positional reading is clean there while the stop is live;
/// * and the scanner witnesses take different baselines between the two — the positional one none
///   at all, the counter a per-collection snapshot. A single helper would have to carry each and
///   pick, which is a `match` on which caller it has, not a chokepoint.
///
/// [`at_committed_boundary`]: crate::InputRef::at_committed_boundary
///
/// # The three witnesses, the two facts, and the two granularities
///
/// An absence exit concludes "no more elements" from what the last element attempt did. Two facts
/// make that attempt's evidence unrepresentative of the input, and an element that hits either can
/// still return `Ok`, which is exactly why neither reaches the failure chokepoint:
///
/// * `inp.scanner_tripped_during_attempt(scans)` — a terminal **scanner** stop, as a *counter
///   event*. This is the load-bearing scanner reading, and the only one of the three that no
///   rollback can erase. An element's own lookahead ([`peek`](crate::InputRef::peek) and friends)
///   latches a stop and still hands back `Ok` with a short window, so the element may decline, or
///   accept consuming nothing, on a truncated view — and it may equally catch that stop inside an
///   [`attempt`](crate::InputRef::attempt) of its own, decline the attempt, and have the restore put
///   the boundary back to the value this driver snapshotted. Its baseline is **per collection**, for
///   the reason the latch's is.
/// * `inp.latched_during_attempt(latch)` — the same stop as a *boundary value*, compared against a
///   per-collection snapshot. The narrower reading: it answers *a different stop is standing at this
///   exit*, where the counter answers *a stop happened inside this collection*. **It is subsumed**
///   — every transition this crate can produce that moves the latch goes through
///   `latch_if_limit_tripped`, which bumps the counter first, so this term adds no verdict the
///   counter does not already give. It is kept for the reason
///   `parser::recovery_gate`'s equivalent term is: it is the reading the
///   boundary itself is the subject of, it costs one `Option<L::Offset>` clone per collection on a
///   path that already takes one, and a witness removed on a subsumption argument is a witness
///   removed on an argument rather than on a measurement.
/// * `inp.tripped_during_attempt(trips)` — a **descent** budget trip the element caught itself and
///   answered with `Decline`, or with an `Accept` that consumed nothing. It latches no boundary and
///   bumps no scanner counter — it has a control stack rather than a position — so both witnesses
///   above read `false` for one, and without this term a resource-budget stop is spent as an
///   *accepted absence*: the collection ends clean and succeeds. Its baseline is **per element**,
///   for the reason [`file_element_failure`] gives at length: the counter is a monotone session
///   fact, so a collection-wide baseline would charge every later absence exit in the parse with a
///   trip some earlier element caught and legitimately parsed past.
///
/// The asymmetry in the baselines is therefore a property of the two facts, not an oversight: a
/// spent scanner budget stays true of the input, a caught descent trip does not. Concretely, the
/// per-collection scanner baseline is what keeps *element 1 tripped and accepted, element 2
/// declines* refused; per element, element 2's baseline would be taken after element 1's trip and
/// the exit would read clean.
///
/// # Where it does **not** belong
///
/// Only on the exits that conclude absence *from an element attempt*. Not, **as a group**, on an
/// exit resting on a **real token**: the delimited drivers' `CloseStatus::Close` arms and the
/// mid-scan closer in `sep/delim` read a committed pre-trip token. That token settles the *position*
/// question and only the position question — the construct closed before any boundary a later
/// lookahead latched, so *neither* scanner term belongs there: the counter would refuse the same
/// parse the latch would, and a wider scan window completes it to the identical value. It settles
/// nothing about the *descent counter*: a trip the element already caught is not unmade by a closer
/// arriving after it. Those exits therefore take the descent witness alone, through
/// [`close_after_element`], and this function stays off them.
///
/// Not on a separator-slot probe that runs *before* this cycle's element either: nothing between the
/// cycle's baseline and that probe can trip, so the descent term would be a constant `false` there.
///
/// And not, ever, on an `Accept` — the third exemption, and the one worth naming rather than
/// leaving to be inferred from "this function is about absence exits". An `Accept` is not an
/// absence conclusion, so there is nothing here for any witness to refuse: the element produced
/// a value, and the driver collects it. That holds even when the element caught this very trip to
/// produce it. This is not a hole left open by accident — gating a value a grammar legitimately
/// produced would forbid recovering from a caught budget, for this error alone among every error a
/// grammar may catch and answer. See the module docs' "the channel neither chokepoint closes"
/// section for the reasoning in full, and `tokora/tests/collection_resource_trip.rs`'s section 6 for
/// the pin.
///
/// The exemption's *reach* differs per witness, and this is the one place to say so plainly. For the
/// **descent** counter it is permanent: the next element's own baseline is taken *after* the
/// accepting one's trip, so no later absence exit in this collection ever sees it either. For the
/// two **scanner** readings it is local to the accepting call, because both are baselined per
/// collection — an element that trips, accepts, and is followed by a declining element is refused at
/// *that* element's absence exit. That has always been the latch's behaviour; the counter now
/// answers the same way when a rollback erased the latch.
///
/// # What holds each witness in the guard
///
/// `GATE_CENSUS` pins that all twelve guard-bearing drivers spell none of these witnesses themselves
/// and that all three appear in this body ahead of the stop — but a needle scan proves presence and
/// ordering, never that a term gates. One behavioural suite per witness does, each confirmed by
/// neutering the term in place and watching the suite go red:
///
/// * `inp.scanner_tripped_during_attempt(scans)` —
///   `tokora/tests/absence_terminal_stop.rs::a_trip_caught_inside_an_elements_own_attempt_still_surfaces_terminal`.
///   The one cell no other witness here can answer: with the term neutered it returns `Ok([])` over
///   a spent budget, and the other 46 cells stay green.
/// * `inp.latched_during_attempt(latch)` — **nothing**. Measured, not assumed: with this term
///   replaced by `let _ = inp.latched_during_attempt(latch);` above the `if` and the counter left in
///   the condition, the whole `--all-features` suite passes — all 105 test binaries, including the
///   10 `absence_terminal_stop.rs` cells this term used to be the sole holder of. That is the
///   subsumption argument above turned into an observation, and it is the honest reading of this
///   row: the term is kept for the reasons given, not because a test would notice its absence.
/// * `inp.tripped_during_attempt(trips)` — `tokora/tests/collection_resource_trip.rs`'s section 4,
///   all 16 cells, and its section 7, all 32.
///
/// To re-check in five minutes rather than by re-deriving it: replace one term with
/// `let _ = <that term>;` above the `if`, leaving the others in the condition, and run
/// `cargo test -p tokora --all-features --no-fail-fast`. The suite named for that witness must go
/// red and the census must stay green. Repeat per witness.
#[inline(always)]
pub(super) fn absence_after_element<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  latch: &Option<L::Offset>,
  scans: usize,
  trips: usize,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::Completeness,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  if inp.scanner_tripped_during_attempt(scans)
    || inp.latched_during_attempt(latch)
    || inp.tripped_during_attempt(trips)
  {
    return Err(
      UnexpectedEot::eot_of(inp.span().end())
        .into_terminal()
        .into(),
    );
  }
  Ok(())
}

/// Refuses a **close** that a descent trip the element already answered stands behind — the
/// descent-only third chokepoint, for the delimited drivers' exits that commit a REAL closer after
/// an element attempt.
///
/// [`absence_after_element`] holds every witness because all of them bear on an absence conclusion.
/// Here exactly one does, and the difference is not a matter of degree — the **scanner** stop and
/// the **descent** trip are facts of different kinds, and a committed pre-trip closer settles one of
/// them and not the other:
///
/// * the **scanner** stop is, for this exit, a fact about a *token position*. A `CloseStatus::Close`
///   verdict is cache-first, so it rests on a real token that was read before any stop; the
///   construct closed **ahead of** whatever boundary the element's lookahead went on to latch, and
///   that boundary is simply not about this construct. Reading it here would fail a parse that a
///   wider scan window completes to the identical value. That is why **both** of the absence gate's
///   scanner readings are deliberately absent from this body — the latch
///   (`latched_during_attempt`) and the session counter (`scanner_tripped_during_attempt`) alike,
///   since a counter bumped by the very lookahead that latched past the closer answers exactly as
///   wrongly. `GATE_CENSUS` pins both absences rather than leaving them to be noticed;
/// * the **descent** trip is a *counter event that happened inside the element attempt*. Nothing
///   arriving afterwards unmakes it. An element that catches a
///   [`RecursionLimitReached`](crate::error::RecursionLimitReached), reports *no more elements*, and
///   is then followed by a real closer produces a **successfully closed collection that silently
///   spent a resource-limit stop** — the position was fine and the budget was not.
///
/// So the verdict "a real token closed this construct" is a true statement about *where* the parse
/// is and says nothing about *what the attempt before it cost*. One gate per fact.
///
/// # Baseline, and the exits this is for
///
/// `trips` is the caller's, per **element**, exactly as [`file_element_failure`] and
/// [`absence_after_element`] take it, and for the same reason: the counter is a monotone session
/// fact, so a collection-wide baseline would charge every later close in the parse with a trip some
/// earlier element caught and legitimately parsed past.
///
/// It belongs on **every probed closer** — every `CloseStatus::Close` verdict in the tree — and the
/// rule is deliberately that flat, with no per-site exemption. Where the probe follows an element
/// attempt whose baseline is still in hand the term is the whole point of this function:
/// `delim/repeated`'s decline arm and its stall epilogue, `delim/repeated_while`'s epilogue,
/// `sep/delim`'s epilogue, and `sep_while/delim`'s stall arm. Where a probe instead sits at the TOP
/// of a cycle — `delim/repeated_while`'s mid-scan arm, and `sep_while/delim`'s front-empty and
/// `Action::Stop` arms — only an *accepting* element can precede it, this cycle's baseline is taken
/// a few lines above, and the term is a constant `false`; the call stays anyway, because a `usize`
/// comparison costs less than an exemption table, and because it lets `GATE_CENSUS` scan the
/// verdict→commit region **per verdict** rather than compare tallies that any arrangement of
/// needles would satisfy. It also fails closed if a later refactor moves an element attempt above
/// one of those probes.
///
/// The two exempt exits are **direct** closers, committed straight from a driver's own scan with no
/// probe verdict at all: `sep/delim`'s and `sep_while/delim`'s mid-scan arms. They are exempt for
/// the same structural reason, and are counted by the census as `direct` so a third cannot land
/// unnoticed.
///
/// Not on an `Accept` either — deliberately, and permanently, not merely "not yet reached". An
/// element that catches a trip and still produces a value has answered it, not concluded absence:
/// the driver is faithfully collecting what it was handed, not manufacturing a stop of its own, so
/// there is nothing here for this gate to refuse. The very next cycle's baseline is taken *after*
/// the accepting cycle's trip, which is why this stays true even when that next cycle is the one
/// that reaches a real closer: by then the counter has not moved *during* the cycle this call is
/// judging, and gating it anyway would mean refusing a value the grammar legitimately produced.
/// This is the same granularity floor the module docs state, applied to the one exit that never
/// takes a baseline for it to apply to — see the module docs' "the channel neither chokepoint
/// closes" section for the reasoning at length.
///
/// # What holds the witness in the guard
///
/// `GATE_CENSUS`'s `every_real_closer_exit_after_an_element_is_trip_gated` pins that every
/// `CloseStatus::Close` verdict in the tree reaches this call before it commits, and
/// `the_close_chokepoint_reads_the_counter_and_not_the_position` scans this body in **both**
/// directions — the descent witness present, and **both** scanner witnesses absent. A needle scan
/// proves presence and ordering, never that a term gates; the behaviour is
/// `tokora/tests/collection_resource_trip.rs`'s section 5, where an element catches a trip and then
/// declines — or accepts consuming nothing — with the closer genuinely present, over both sinks.
/// Neuter the term (`let _ = inp.tripped_during_attempt(trips);` above the `if`) and that section
/// reds while the census stays green.
#[inline(always)]
pub(super) fn close_after_element<'inp, L, Ctx, Lang: ?Sized, Cmpl>(
  inp: &InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  trips: usize,
) -> Result<(), <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: crate::input::Completeness,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
{
  if inp.tripped_during_attempt(trips) {
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
  //! second chokepoint and holds all three witnesses for those exits, and
  //! [`every_absence_exit_carries_the_terminal_witnesses`] applies the same shape of proof to it:
  //! **all twelve** guard-bearing drivers spell **none** of them themselves, so there is nothing
  //! left for them to spell differently, and the count of chokepoint calls is pinned per source so a
  //! new absence exit cannot land ungated.
  //!
  //! One of those three is a *counter*, not a boundary, and that is the whole of what made this half
  //! reach one nesting level deeper than it used to. A driver's two rollback-set readings — the
  //! poison boundary at an absence exit, the committed cursor at a failure exit — are both erased by
  //! an element that catches a stop inside an [`attempt`](crate::InputRef::attempt) of its own and
  //! declines it, so both answered `false` over a live stop. `Input::scanner_trips` is outside the
  //! rollback set by construction; [`every_driver_baselines_its_scanner_witness_once_per_collection`]
  //! pins its baseline at the granularity the fact has, which is the *opposite* of the descent one's.
  //!
  //! This half reaches further than the swallow scan does, and deliberately so. Only the try-driven
  //! four can spend an element's `Err`, so only they need a gate above a swallow; the four `*_while`
  //! drivers and the four fold sources propagate an element failure with `?` and are terminal on
  //! that path for free. What they shared with the four was the exit with no `Err` in it — the
  //! element answering the trip itself and then reporting absence — and until the per-element
  //! baseline [`every_driver_baselines_its_trip_witness_inside_its_element_loop`] pins, those eight
  //! could not even ask the question. [`absence_exit_shapes`] records what that cost and how it was
  //! measured.
  //!
  //! # The third half: the exits that close on a real token
  //!
  //! An exit that commits a REAL closer is not an absence exit, and for a long time that was read as
  //! "no witness belongs there". It is true of the scanner ones. A committed pre-trip closer is a
  //! fact about *position*: the construct ended ahead of whatever boundary the element's lookahead
  //! went on to latch, so neither scanner witness is about it — the boundary reading and the counter
  //! reading come from the same lookahead — and gating on either would fail a parse that a wider
  //! scan window completes identically. It is not a fact about the *descent counter*: a trip the
  //! element caught happened inside the attempt that concluded "no more elements", and a valid
  //! closer arriving afterwards does not unmake it — the collection closes successfully having
  //! silently spent a resource-limit stop. [`close_after_element`](super::close_after_element) is the
  //! third chokepoint and holds the descent witness **alone**;
  //! [`every_real_closer_exit_after_an_element_is_trip_gated`] pins that every close verdict in the
  //! tree reaches it before committing — **every** one, with no per-site exemption, which is what
  //! lets that test scan the verdict→commit region per verdict instead of comparing tallies — and
  //! [`the_close_chokepoint_reads_the_counter_and_not_the_position`] pins the asymmetry in both
  //! directions, since either scanner term smuggled into that body is as much a defect as a missing
  //! descent one. The only exits it exempts are the two **direct** closers, committed from a
  //! driver's own scan with no probe verdict at all, and it counts those rather than ignoring them.
  //!
  //! # The two things the chokepoints cannot centralize, and they pull opposite ways
  //!
  //! Both baselines are the caller's, and getting either one's granularity from the other's is a
  //! defect. They are scanned per caller because placement is a per-caller property.
  //!
  //! `trips` must be `inp.trip_snapshot()` taken **inside the element loop**, once per element — not
  //! hoisted out beside `latch_snapshot()`. The counter is a monotone session fact and is never
  //! cleared, so a baseline hoisted above the loop is arithmetically a session-absolute read for
  //! every element after the first: an ordinary syntax error in an unrelated construct then ends the
  //! collection instead of being filed, and one deep expression early in a document suppresses every
  //! diagnostic after it. Hoisting it also restores the defect one nesting level up — a trip inside
  //! an *inner* collection that an element swallows charged to the enclosing driver's next ordinary
  //! failure.
  //!
  //! `scans` must be `inp.scanner_trip_snapshot()` taken **outside** it, once per collection, beside
  //! `latch_snapshot()` — the exact opposite, because the two facts decay differently. A descent trip
  //! an element caught and parsed past stops being true of the input; a spent scanner budget does
  //! not, and every element after it is reading a stream the stop truncated. Taken per element the
  //! baseline is re-read after each accepted trip, and *element 1 tripped and accepted, element 2
  //! declines* concludes cleanly over a spent budget — the case the latch beside it has always
  //! refused. [`every_driver_baselines_its_scanner_witness_once_per_collection`] pins that placement
  //! in both directions, as its sibling does for `trips`.

  use super::end_state_census::{code_find, code_matches};

  /// The swallow itself — matched on the **call**, not on the arguments, so no spelling of the
  /// span or the error binding can slip past. This is the needle whose count must be zero
  /// everywhere but the chokepoint.
  const EMIT: &str = "emit_error(";

  /// The chokepoint: its call in a driver, and its definition in this module's own source.
  const CHOKEPOINT: &str = "file_element_failure(";
  const CHOKEPOINT_DEF: &str = "fn file_element_failure";

  /// The four witnesses the chokepoint must consult before it reaches [`EMIT`] — three facts, with
  /// the scanner one read both positionally and as a session counter. Neither scanner reading
  /// subsumes the other: the positional one sees a boundary latched *before* the driver started that
  /// the element has now run onto, and the counter sees a stop an inner rollback erased.
  const WITNESSES: [&str; 4] = [
    "Cmpl::is_incomplete_error(&",
    "inp.at_committed_boundary()",
    "inp.scanner_tripped_during_attempt(",
    "inp.tripped_during_attempt(",
  ];

  /// The absence chokepoint: its call in a driver, and its definition in this module's own source.
  const ABSENCE: &str = "absence_after_element(";
  const ABSENCE_DEF: &str = "fn absence_after_element";

  /// The terminal end-of-input the absence chokepoint produces — the needle that ends its guard
  /// region, the way [`EMIT`] ends the failure chokepoint's.
  const ABSENCE_STOP: &str = "into_terminal()";

  /// The three witnesses the absence chokepoint must consult before it reaches [`ABSENCE_STOP`].
  ///
  /// Neither scanner half is the failure path's `at_committed_boundary`: a restore over the latch
  /// rewinds the cursor behind a boundary that survives, so every positional reading is clean at an
  /// absence exit while the stop is live. The **counter** is the load-bearing one — a restore taken
  /// *before* the trip puts the boundary back too, so the latch is clean there as well — and it is
  /// listed first for that reason.
  const ABSENCE_WITNESSES: [&str; 3] = [
    "inp.scanner_tripped_during_attempt(",
    "inp.latched_during_attempt(",
    "inp.tripped_during_attempt(",
  ];

  /// The descent-only close chokepoint: its call in a driver, and its definition in this module's
  /// own source. It shares [`ABSENCE_STOP`] as the needle that ends its guard region.
  const CLOSE_GATE: &str = "close_after_element(";
  const CLOSE_GATE_DEF: &str = "fn close_after_element";

  /// The one witness [`CLOSE_GATE`] must read, and the two it must **not**.
  ///
  /// The asymmetry is the whole point of there being a third chokepoint, so it is pinned in both
  /// directions rather than only in the direction that is easy to check. A real pre-trip closer
  /// settles the *scanner* fact — the construct ended ahead of any boundary a later lookahead
  /// latched — and settles nothing about the *descent counter* fact, which is a trip that already
  /// happened inside the element attempt this exit is concluding from.
  ///
  /// **Both** scanner readings are non-witnesses here, not just the latch. The lookahead that
  /// latched past the closer also bumped the session counter, so a counter reading in this body
  /// refuses exactly the parses the latch reading would — every delimited parse a wider scan window
  /// completes to the identical value. Adding a witness to the absence gate without adding it here
  /// as a *non*-witness is the shape that quietly breaks that, so the list is a list.
  const CLOSE_WITNESS: &str = "inp.tripped_during_attempt(";
  const CLOSE_NON_WITNESSES: [&str; 2] = [
    "inp.latched_during_attempt(",
    "inp.scanner_tripped_during_attempt(",
  ];

  /// A real-closer exit, in the two spellings a driver can reach one by: the probe verdict that
  /// hands the closer over, and the by-value commit that settles it. `CLOSE_HANDOFF` is the
  /// container call every closer of either origin passes through, so a *new* closer commit of any
  /// shape moves its tally.
  const CLOSE_VERDICT: &str = "CloseStatus::Close(";
  const CLOSE_COMMIT: &str = "inp.commit_probed(";
  const CLOSE_HANDOFF: &str = "on_close_delimiter(";

  /// The element attempt whose failure a driver must route through the chokepoint, the descent
  /// witness's baseline, and the loop opener that baseline must be taken after.
  const ATTEMPT: &str = "try_parse_input(inp)";
  const BASELINE: &str = "inp.trip_snapshot()";
  const HOSTING_LOOP: &str = "loop {";

  /// The **scanner** witness's baseline, and the latch baseline it must sit beside.
  ///
  /// The mirror image of [`BASELINE`], and the reason it is scanned separately: this one is per
  /// COLLECTION and must be taken *above* the element loop, exactly where hoisting the descent one
  /// would be the defect. A spent scanner budget stays true of the input, so a per-element baseline
  /// would be taken after the trip an earlier element caught and every later exit would read clean —
  /// which is the case "element 1 tripped and accepted, element 2 declines".
  const SCANNER_BASELINE: &str = "inp.scanner_trip_snapshot()";
  const LATCH_BASELINE: &str = "inp.latch_snapshot()";

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
        "`many/mod.rs`: the chokepoint must consult all four never-recoverable witnesses before \
         it files — `{witness}` is missing from, or duplicated in, the code ahead of its \
         `emit_error`. The two scanner readings are both required: the positional one for a \
         boundary latched before the driver started, the counter for a stop an inner rollback erased"
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

  /// How many element loops each guard-bearing source runs — and therefore how many per-element
  /// trip baselines it takes. Index-aligned with [`progress_guard_sites`].
  ///
  /// One row per source rather than a bare total, because the two fold sources that host three
  /// driver impls each are exactly where a fourth impl could land without one. The count is
  /// asserted against **two** independent needles — the `loop {` openers and the
  /// `trip_snapshot()` calls — so a loop added without a baseline, or a baseline added outside a
  /// loop, breaks the equality rather than being absorbed by it.
  fn element_loop_counts() -> [(&'static str, usize); 12] {
    [
      ("many/repeated/mod.rs", 1),
      ("many/repeated_while/mod.rs", 1),
      ("many/delim/repeated.rs", 1),
      ("many/delim/repeated_while.rs", 1),
      ("many/sep/parse/mod.rs", 1),
      ("many/sep/delim/mod.rs", 1),
      ("many/sep_while/parse/mod.rs", 1),
      ("many/sep_while/delim/mod.rs", 1),
      ("fold/mod.rs", 3),
      ("fold/rfold.rs", 1),
      ("fold/fold_while.rs", 3),
      ("fold/rfold_while.rs", 1),
    ]
  }

  /// **All twelve** guard-bearing drivers take their trip baseline inside the element loop, once per
  /// element, and every call to any of the three chokepoints reads one taken there.
  ///
  /// [`every_element_loop_baselines_its_trip_witness_per_element`] is this test's narrower sibling:
  /// it pins the *failure* chokepoint's placement in the four try-driven sources, where the extra
  /// `try_parse_input`-per-chokepoint pairing is meaningful. This one covers the other two
  /// chokepoints and the other eight sources, which is where the baseline was missing entirely
  /// until the `*_while` drivers and the folds adopted it.
  ///
  /// The half the chokepoints cannot own is the same one stated on
  /// [`file_element_failure`](super::file_element_failure): `trips` is the caller's, so the caller
  /// decides whether the comparison means "this element tripped" or "this parse has tripped". The
  /// counter is a monotone session fact, never cleared, so a baseline hoisted above the loop is
  /// arithmetically a session-absolute read for every element after the first — and an absence exit
  /// reading that would end every later collection in the document over a trip some earlier element
  /// legitimately caught and parsed past.
  ///
  /// The `rfind` panics rather than passing when a chokepoint call is not inside a loop at all, so
  /// this cannot be satisfied by a source that has stopped looking like a driver.
  ///
  /// [`every_driver_baselines_its_scanner_witness_once_per_collection`] is the exact mirror of this
  /// test for the *scanner* counter, whose baseline must be **outside** the loop this one requires
  /// its baseline inside. The two are separate tests because they are opposite properties, and
  /// writing them as one would need a flag naming which is which at every assertion.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_driver_baselines_its_trip_witness_inside_its_element_loop() {
    let sites = progress_guard_sites();
    let counts = element_loop_counts();
    let mut baselines = 0;
    let mut gates = 0;
    for i in 0..sites.len() {
      let (name, src) = sites[i];
      let (classified, loops) = counts[i];
      assert_eq!(
        name, classified,
        "the guard-bearing source list and the element-loop classification have drifted apart at \
         row {i}: `{name}` against `{classified}`. They are index-aligned on purpose — a row that \
         names a source the scan does not read is a row that checks nothing"
      );
      assert_eq!(
        code_matches(src, HOSTING_LOOP),
        loops,
        "{name}: expected {loops} element loop(s). A driver impl added to this source needs its own \
         baseline and its own classification row before it can be trusted"
      );
      assert_eq!(
        code_matches(src, BASELINE),
        loops,
        "{name}: exactly one `trip_snapshot()` baseline per element loop — a loop without one leaves \
         its exits ungated, and a second, stray read of the counter is how a session-absolute test \
         gets back in"
      );

      // Every chokepoint call, of all three kinds, reads a baseline taken inside the loop that
      // hosts it. The region is `loop {` → call, so a baseline hoisted above the loop leaves the
      // region empty and fails here.
      for needle in [CHOKEPOINT, ABSENCE, CLOSE_GATE] {
        let mut from = 0;
        while let Some(at) = code_find(&src[from..], needle) {
          let call_at = from + at;
          let loop_at = src[..call_at].rfind(HOSTING_LOOP).unwrap_or_else(|| {
            panic!("{name}: a `{needle}` call that is not inside a repetition loop")
          });
          assert_eq!(
            code_matches(&src[loop_at..call_at], BASELINE),
            1,
            "{name}: the descent witness is attempt-relative — take the `trip_snapshot()` baseline \
             INSIDE the element loop, once per element. Hoisted out of it the comparison degrades \
             into a read of the monotone session counter, and every absence exit after the parse's \
             first trip refuses, ordinary ends of construct included"
          );
          gates += 1;
          from = call_at + needle.len();
        }
      }
      baselines += loops;
    }
    assert_eq!(
      (baselines, gates),
      (16, 42),
      "sixteen element loops across the twelve sources, each with its own baseline, and \
       forty-two chokepoint calls reading one: four failure gates, thirty absence gates and eight \
       real-closer gates. The totals are pinned so a gate cannot move between sources unnoticed"
    );
  }

  /// **All twelve** guard-bearing drivers take their scanner-trip baseline once per COLLECTION,
  /// beside the latch and **outside** the element loop — and every gate that reads one is a gate
  /// that has one hoisted above it.
  ///
  /// The exact mirror of
  /// [`every_driver_baselines_its_trip_witness_inside_its_element_loop`], and the reason the two
  /// cannot be one test: the descent counter's baseline must be *inside* the loop and the scanner
  /// counter's must be *outside* it, because the two facts decay differently. A descent trip an
  /// element caught and legitimately parsed past stops being true of the input; a spent scanner
  /// budget does not — the token stream ends where it ended.
  ///
  /// What a per-element scanner baseline would cost, stated as the case it drops: element 1 trips,
  /// catches the stop and *accepts*; element 2 declines. Element 2's baseline would be taken after
  /// element 1's trip, the counter would not have moved during element 2, and the collection would
  /// conclude cleanly over a spent budget. Hoisted, it refuses — which is what the latch beside it
  /// has always done, and what the counter now also does when a rollback erased the latch.
  ///
  /// Both directions are pinned. The count per source stops a second, stray baseline from being
  /// added (which would silently re-narrow one gate), and the region scan — `loop {` → call, which
  /// must contain **zero** of them — stops the baseline being moved into the loop. The `rfind`
  /// panics rather than passing when a call is not inside a loop at all.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_driver_baselines_its_scanner_witness_once_per_collection() {
    let sites = progress_guard_sites();
    let counts = element_loop_counts();
    let mut baselines = 0;
    let mut gates = 0;
    for i in 0..sites.len() {
      let (name, src) = sites[i];
      let (classified, loops) = counts[i];
      assert_eq!(
        name, classified,
        "the guard-bearing source list and the element-loop classification have drifted apart at \
         row {i}: `{name}` against `{classified}`. They are index-aligned on purpose — a row that \
         names a source the scan does not read is a row that checks nothing"
      );
      assert_eq!(
        code_matches(src, SCANNER_BASELINE),
        loops,
        "{name}: exactly one `{SCANNER_BASELINE}` baseline per collection — the same count as the \
         `{LATCH_BASELINE}` it sits beside, because the two answer the same question at the same \
         granularity. A collection without one leaves its absence and failure exits blind to a stop \
         an element caught inside an `attempt` of its own"
      );
      assert_eq!(
        code_matches(src, SCANNER_BASELINE),
        code_matches(src, LATCH_BASELINE),
        "{name}: the scanner counter's baseline and the latch's are taken together, once per \
         collection. A count that has drifted apart means one gate is reading a per-collection fact \
         and its neighbour is not"
      );

      // The two gates that read it, and the negative half: NOTHING between the hosting loop and the
      // call may take this baseline, because taking it there makes it per element.
      for needle in [CHOKEPOINT, ABSENCE] {
        let mut from = 0;
        while let Some(at) = code_find(&src[from..], needle) {
          let call_at = from + at;
          let loop_at = src[..call_at].rfind(HOSTING_LOOP).unwrap_or_else(|| {
            panic!("{name}: a `{needle}` call that is not inside a repetition loop")
          });
          assert_eq!(
            code_matches(&src[loop_at..call_at], SCANNER_BASELINE),
            0,
            "{name}: the scanner witness is COLLECTION-relative — take `{SCANNER_BASELINE}` above \
             the element loop, beside the latch. Taken inside it, the baseline is re-read after \
             every trip an earlier element caught and accepted, and the very next exit reads clean \
             over a spent budget"
          );
          gates += 1;
          from = call_at + needle.len();
        }
      }
      baselines += loops;
    }
    assert_eq!(
      (baselines, gates),
      (16, 34),
      "sixteen collections across the twelve sources, each with its own scanner baseline, and \
       thirty-four gates reading one: four failure gates and thirty absence gates. The eight \
       real-closer gates are deliberately NOT among them — see `close_after_element` — so this total \
       is the previous test's forty-two minus exactly those eight"
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

  /// The absence-exit shape of every guard-bearing driver, as data: how many [`ABSENCE`] calls the
  /// source must carry.
  ///
  /// Index-aligned with [`progress_guard_sites`], and
  /// [`every_absence_exit_carries_the_terminal_witnesses`] checks the two lists name the same
  /// sources in the same order, so neither can drift.
  ///
  /// **Every row is nonzero, and that is the property.** This table used to carry eight zeroes,
  /// marking an older *inline* shape that spelled `latched_during_attempt` at each exit and read no
  /// descent witness at all — the four `*_while` drivers and the four fold sources. They had the
  /// same hole the chokepoint closes for the try-driven four, reached the same way: an element that
  /// catches a trip itself and then declines, or accepts consuming nothing, ends the collection
  /// cleanly and successfully. Measured rather than inferred, before and after: `fold` over such an
  /// element returned the same `Ok(6)` on `"1 2 3"` under a budget the element exceeded as under one
  /// it did not, and `repeated_while` returned the same `Ok([1, 2, 3, -1])` over the stalling shape.
  /// Closing it needed the per-element trip baseline
  /// [`every_driver_baselines_its_trip_witness_inside_its_element_loop`] now pins in all twelve
  /// sources — which is why three of the folds stopped being `while let` loops, a `while let` having
  /// nowhere to snapshot the counter before the element attempt its own condition runs.
  ///
  /// So there is no longer a two-shape classification here, and re-introducing one means adding a
  /// zero, which fails the per-row assertion rather than passing quietly.
  fn absence_exit_shapes() -> [(&'static str, usize); 12] {
    [
      ("many/repeated/mod.rs", 1),
      ("many/repeated_while/mod.rs", 2),
      ("many/delim/repeated.rs", 4),
      ("many/delim/repeated_while.rs", 3),
      ("many/sep/parse/mod.rs", 2),
      ("many/sep/delim/mod.rs", 2),
      ("many/sep_while/parse/mod.rs", 2),
      ("many/sep_while/delim/mod.rs", 6),
      ("fold/mod.rs", 3),
      ("fold/rfold.rs", 1),
      ("fold/fold_while.rs", 3),
      ("fold/rfold_while.rs", 1),
    ]
  }

  /// Every guard-bearing driver's absence exits reach **every** never-recoverable witness, through
  /// one chokepoint no driver can spell around.
  ///
  /// A driver's absence exits — the no-progress stall, the element-decline break, a condition's
  /// `Action::Stop`, and in the delimited drivers the close probe's `WrongToken`/`Eof` arms —
  /// conclude "no more elements" from what the last element attempt did. Two facts make that
  /// attempt's evidence unrepresentative while it still returns `Ok`, so neither reaches
  /// [`file_element_failure`](super::file_element_failure):
  ///
  /// * a terminal **scanner** stop the element's own lookahead latched, which leaves the pre-trip
  ///   tokens cached and the window short (the eager decision gate does not see it: the next window
  ///   is served whole from that cache and carries no terminal flag). Two witnesses, both baselined
  ///   per collection: `scanner_tripped_during_attempt` against a `scanner_trip_snapshot`, and
  ///   `latched_during_attempt` — presence-plus-change against a `latch_snapshot`, never a positional
  ///   reading, because a rollback over the latch moves the offset back behind a boundary that
  ///   survives. The counter is the load-bearing one, and the latch is the narrower reading it
  ///   subsumes: a rollback taken *before* the trip puts the boundary back as well as the offset, so
  ///   the latch is clean there too, and only a cell outside the rollback set answers;
  /// * a **descent** budget trip the element caught itself. It latches no boundary and bumps no
  ///   scanner counter, so both witnesses above read `false` for one, and the absence exit spends the
  ///   stop as a *success*. The witness is `tripped_during_attempt` against the per-**element**
  ///   baseline, for the reason [`every_element_loop_baselines_its_trip_witness_per_element`] gives.
  ///
  /// In the delimited drivers **this** gate belongs *inside* the close probe's `WrongToken`/`Eof`
  /// arms, never ahead of the probe: the probe is cache-first, so a `Close` verdict rests on a real
  /// pre-trip token, and *neither* scanner witness may reach it — a boundary latched past the closer
  /// is not about a construct that ended before it, the counter was bumped by the very lookahead
  /// that latched it, and reading either there would fail a parse a wider window completes
  /// identically. The *descent* witness is a different kind of fact and does reach it, through the
  /// separate [`close_after_element`](super::close_after_element) gate that
  /// [`every_real_closer_exit_after_an_element_is_trip_gated`] counts.
  ///
  /// What this pins, per source: the baseline is taken; the source carries the count
  /// [`absence_exit_shapes`] classifies it with; and the source spells **none** of the witnesses
  /// itself, which is the "nothing left to balance" form — the strongest of these the codebase has,
  /// and the one the swallow scan above uses. What it does not pin is that the chokepoint's own guard
  /// *gates*, which is
  /// [`the_absence_chokepoint_consults_every_witness_before_it_stops`]'s region scan for presence
  /// and `tokora/tests/collection_resource_trip.rs`'s sections 4 and 7 for behaviour.
  #[test]
  fn every_absence_exit_carries_the_terminal_witnesses() {
    let sites = progress_guard_sites();
    let shapes = absence_exit_shapes();
    let mut chokepointed = 0;
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
      assert!(
        calls > 0,
        "{name}: a repetition driver concludes the construct ended somewhere, and every such exit \
         goes through `absence_after_element`. A zero here is the old inline shape coming back — see \
         `absence_exit_shapes` for what that shape spent, and how it was measured"
      );
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
          "{name}: a driver hands its absence exits to `absence_after_element` — it never reads \
           `{witness}` itself. A hand-spelled gate can consult two witnesses of three and look \
           complete, which is exactly the defect the chokepoint exists to make unspellable"
        );
      }
      chokepointed += calls;
    }
    assert_eq!(
      chokepointed, 30,
      "the twelve guard-bearing sources carry exactly thirty chokepointed absence exits between \
       them. The total is pinned so that adding an exit, or losing one, has to be deliberate — and \
       so that a source re-cut to a smaller count cannot be balanced by a larger one next door"
    );
  }

  /// The absence chokepoint consults **every** never-recoverable witness before it produces its
  /// terminal end-of-input, and it is the only place in the tree that reads the scanner latch.
  ///
  /// The absence twin of the region scan in
  /// [`every_element_failure_routes_through_the_gated_chokepoint`], and it proves exactly as much
  /// and as little: each witness is present, named, and textually ahead of the stop. That is a
  /// tripwire on the source — a witness dropped, renamed or duplicated reds one cheap unit test —
  /// and **not** a proof of control-flow domination, which a body reading them all into `let _ =`
  /// bindings would satisfy with the gate gone. Verified, not reasoned about: with the descent term
  /// so neutered this test and the whole census stayed green while
  /// `tokora/tests/collection_resource_trip.rs` red 16 cells. What proves each gate gates is that
  /// suite — section 4's 16 cells for the descent term,
  /// `tokora/tests/absence_terminal_stop.rs`'s 10 (of 47) for the latch, and that file's
  /// `a_trip_caught_inside_an_elements_own_attempt_still_surfaces_terminal` for the scanner counter.
  /// Both `find`s panic rather than pass when what they scan for is absent.
  ///
  /// The two tree-wide tallies below are the "counted globally" form of the asymmetry
  /// [`the_close_chokepoint_reads_the_counter_and_not_the_position`] pins locally: the latch is read
  /// **once** in the whole tree and the scanner counter **twice** — once at each of the two gates
  /// that judge from an element attempt, and never at the third.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_absence_chokepoint_consults_every_witness_before_it_stops() {
    let prod = super::end_state_census::many_mod_production();
    assert_eq!(
      code_matches(prod, "inp.latched_during_attempt("),
      1,
      "`many/mod.rs`: the scanner latch is read once in this tree, inside `absence_after_element`"
    );
    assert_eq!(
      code_matches(prod, "inp.scanner_tripped_during_attempt("),
      2,
      "`many/mod.rs`: the scanner counter is read exactly twice in this tree — in \
       `absence_after_element` and in `file_element_failure`, the two gates that judge from an \
       element attempt. A third reading is `close_after_element` having grown one, which fails a \
       delimited parse a wider scan window completes identically; a count of one is the witness \
       dropped from a gate whose remaining witnesses answer the same way on every case anyone \
       thought to write"
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
        "`many/mod.rs`: the absence chokepoint must consult all three never-recoverable witnesses \
         before it stops — `{witness}` is missing from, or duplicated in, the code ahead of its \
         terminal end-of-input. One witness alone leaves the others' stops spendable as an accepted \
         absence"
      );
    }
  }

  /// The real-closer shape of every guard-bearing driver, as data — index-aligned with
  /// [`progress_guard_sites`].
  ///
  /// `(source, probed closers, direct closers)`.
  ///
  /// * a **probed** closer is a `CloseStatus::Close` verdict, committed by value with
  ///   [`CLOSE_COMMIT`]. **Every one of them carries [`CLOSE_GATE`]**, with no per-site exemption —
  ///   which is what lets [`every_real_closer_exit_after_an_element_is_trip_gated`] scan the
  ///   verdict→commit region *per verdict* instead of comparing two tallies that any arrangement of
  ///   needles would satisfy. Where a probe sits at the TOP of a cycle the gate is a constant
  ///   `false` (this cycle's baseline is taken a few lines above it and nothing since can trip), and
  ///   it is kept anyway: a `usize` comparison is cheaper than an exemption table, and it fails
  ///   closed if a later refactor moves an element attempt above it;
  /// * a **direct** closer is one the driver committed straight from its own scan, with no probe
  ///   verdict at all. There are exactly two in the tree — `sep/delim`'s and `sep_while/delim`'s
  ///   mid-scan arms — and they are exempt for a structural reason, not because they "rest on a real
  ///   token": each is reached from the top of a cycle, so only an *accepting* element can precede
  ///   it (a decline and a stall each leave for an exit that gates) and the descent term would be
  ///   the same constant `false`. They are counted rather than ignored, so a third one cannot land
  ///   unnoticed.
  ///
  /// The eight non-delimited sources carry no closer at all, and that `(0, 0)` is checked rather
  /// than skipped: it is what stops a closer landing in a source this census reads but does not
  /// expect one in.
  fn close_exit_shapes() -> [(&'static str, usize, usize); 12] {
    [
      ("many/repeated/mod.rs", 0, 0),
      ("many/repeated_while/mod.rs", 0, 0),
      ("many/delim/repeated.rs", 2, 0),
      ("many/delim/repeated_while.rs", 2, 0),
      ("many/sep/parse/mod.rs", 0, 0),
      ("many/sep/delim/mod.rs", 1, 1),
      ("many/sep_while/parse/mod.rs", 0, 0),
      ("many/sep_while/delim/mod.rs", 3, 1),
      ("fold/mod.rs", 0, 0),
      ("fold/rfold.rs", 0, 0),
      ("fold/fold_while.rs", 0, 0),
      ("fold/rfold_while.rs", 0, 0),
    ]
  }

  /// Every exit that commits a real closer **after an element attempt** consults the descent
  /// witness first.
  ///
  /// The absence census above pins the exits that conclude the construct ended from *nothing*. This
  /// pins the ones that conclude it from *a real token* — the shape the previous revision left
  /// ungated on the reasoning that a committed pre-trip closer settles the question. It settles the
  /// **position** question. The element attempt that declined, or stalled, may have caught a descent
  /// trip on its way there, and a valid closer arriving afterwards does not unmake a counter event
  /// that already happened: without a gate that is a successfully closed collection that spent a
  /// resource-limit stop in silence.
  ///
  /// What this pins, per source: the counted shape [`close_exit_shapes`] classifies it as, in three
  /// independent tallies — the verdict, the by-value commit, and the container handoff that closers
  /// of *both* origins pass through — plus a **region** scan requiring each verdict to reach
  /// [`CLOSE_GATE`] before its commit. The region scan is what makes this more than three tallies
  /// that three needles scattered anywhere would satisfy, and its `code_find` panics rather than
  /// passing when a verdict has no commit after it at all.
  ///
  /// What it does not pin is that the gate *gates* — see
  /// [`the_close_chokepoint_reads_the_counter_and_not_the_position`] for the same limit stated at
  /// length, and `tokora/tests/collection_resource_trip.rs`'s section 5 for the behaviour.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn every_real_closer_exit_after_an_element_is_trip_gated() {
    let sites = progress_guard_sites();
    let shapes = close_exit_shapes();
    let mut gated = 0;
    let mut exempt = 0;
    for i in 0..sites.len() {
      let (name, src) = sites[i];
      let (classified, probed, direct) = shapes[i];
      assert_eq!(
        name, classified,
        "the guard-bearing source list and the real-closer classification have drifted apart at row \
         {i}: `{name}` against `{classified}`. They are index-aligned on purpose — a row that names \
         a source the scan does not read is a row that checks nothing"
      );
      assert_eq!(
        code_matches(src, CLOSE_VERDICT),
        probed,
        "{name}: expected {probed} close verdict(s) (`{CLOSE_VERDICT}`). A new one is a new exit \
         that concludes the construct ended, and it has to be classified here before it can be \
         trusted"
      );
      assert_eq!(
        code_matches(src, CLOSE_COMMIT),
        probed,
        "{name}: every close verdict is committed by value exactly once (`{CLOSE_COMMIT}`) — a \
         verdict committed by re-scanning instead would leave this census measuring the wrong \
         program point"
      );
      assert_eq!(
        code_matches(src, CLOSE_HANDOFF),
        probed + direct,
        "{name}: expected {} closer handoff(s) (`{CLOSE_HANDOFF}`), {probed} probed and {direct} \
         direct. A closer committed by any other route is an exit this census cannot see",
        probed + direct
      );
      assert_eq!(
        code_matches(src, CLOSE_GATE),
        probed,
        "{name}: every probed closer reached after an element attempt gates on the descent witness \
         (`{CLOSE_GATE}`). An exit added without one closes the construct successfully over a \
         resource-limit stop the element already answered — and an exit whose gate was removed is \
         the same defect, which is why this is a count and not a presence test"
      );

      // Each verdict reaches the gate before it commits. The region is verdict → commit, so a gate
      // sitting anywhere else in the file — including inside a close-MISS arm, where the absence
      // chokepoint belongs instead — does not satisfy it.
      let mut checked = 0;
      let mut from = 0;
      while let Some(at) = code_find(&src[from..], CLOSE_VERDICT) {
        let verdict_at = from + at;
        let commit_at = code_find(&src[verdict_at..], CLOSE_COMMIT).unwrap_or_else(|| {
          panic!(
            "{name}: a `{CLOSE_VERDICT}` verdict with no `{CLOSE_COMMIT}` after it. The driver no \
             longer commits the probed closer by value; re-cut this census against whatever \
             replaced it before trusting a green run"
          )
        });
        assert_eq!(
          code_matches(&src[verdict_at..verdict_at + commit_at], CLOSE_GATE),
          1,
          "{name}: the descent gate belongs INSIDE the close verdict's arm, ahead of the commit. \
           Found none there, or more than one — a gate in a neighbouring arm leaves this exit \
           closing over a trip the element answered"
        );
        checked += 1;
        from = verdict_at + CLOSE_VERDICT.len();
      }
      assert_eq!(
        checked, probed,
        "{name}: every close verdict must have been inspected"
      );
      gated += probed;
      exempt += direct;
    }
    assert_eq!(
      (gated, exempt),
      (8, 2),
      "the four delimited families carry exactly eight gated real-closer exits — every \
       `CloseStatus::Close` verdict in the tree, whether or not its own descent term can be nonzero \
       — and exactly two exempt DIRECT closers, `sep/delim`'s and `sep_while/delim`'s mid-scan arms, \
       which commit from their own scan with no probe verdict at all. Both halves are pinned so that \
       exempting a third direct closer, or losing a gate from one of the eight, has to be deliberate"
    );
  }

  /// The close chokepoint reads the **counter** and not the **position**, and the census says so in
  /// both directions.
  ///
  /// The third of the region scans, and it proves exactly as much and as little as its two
  /// siblings: the witness is present, named, and textually ahead of the stop. That is a tripwire on
  /// the source and **not** a proof of control-flow domination — a body reading the counter into a
  /// `let _ =` binding satisfies it with the gate gone, exactly as
  /// [`the_absence_chokepoint_consults_every_witness_before_it_stops`] records for its own. What
  /// proves this one gates is `tokora/tests/collection_resource_trip.rs`'s section 5.
  ///
  /// The negative half has no sibling: **neither** scanner reading may appear here — not the latch
  /// and not the session counter. Either one smuggled into this body would fail every delimited
  /// parse whose element lookahead latched a boundary past a closer that legitimately closed (the
  /// lookahead that latched also bumped the counter, so the two refuse the same parses) — a
  /// regression a wider scan window makes invisible, and the exact reason the previous revision left
  /// the whole arm ungated. Pinning the absence is cheap; noticing it later is not.
  /// `many/mod.rs`'s tree-wide tallies for both, asserted next door, are the same claim counted
  /// globally.
  #[test]
  #[cfg_attr(
    miri,
    ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
  )]
  fn the_close_chokepoint_reads_the_counter_and_not_the_position() {
    let prod = super::end_state_census::many_mod_production();
    let def_at = code_find(prod, CLOSE_GATE_DEF).unwrap_or_else(|| {
      panic!(
        "`many/mod.rs`: `{CLOSE_GATE_DEF}` is gone. The real-closer chokepoint has been renamed or \
         removed; re-cut this census against whatever replaced it before trusting a green run"
      )
    });
    let body = &prod[def_at..];
    let stop_at = code_find(body, ABSENCE_STOP).unwrap_or_else(|| {
      panic!(
        "`many/mod.rs`: `close_after_element` no longer surfaces the stop as a terminal \
         end-of-input (`{ABSENCE_STOP}`)"
      )
    });
    let guard = &body[..stop_at];
    assert_eq!(
      code_matches(guard, CLOSE_WITNESS),
      1,
      "`many/mod.rs`: the close chokepoint must consult the descent witness before it stops — \
       `{CLOSE_WITNESS}` is missing from, or duplicated in, the code ahead of its terminal \
       end-of-input. Without it a closer that arrives after a trip the element answered closes the \
       collection successfully"
    );
    for non_witness in CLOSE_NON_WITNESSES {
      assert_eq!(
        code_matches(guard, non_witness),
        0,
        "`many/mod.rs`: the close chokepoint must read NEITHER scanner witness (`{non_witness}` is \
         present). A committed pre-trip closer settles the scanner fact — the construct ended ahead \
         of any boundary a later lookahead latched, and that same lookahead is what bumped the \
         session counter — so reading either here fails a parse a wider scan window completes to \
         the identical value. The asymmetry between the scanner facts and the descent one is why \
         this chokepoint exists apart from `absence_after_element`"
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
pub(super) mod end_state_census {
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
  /// Shared with [`gate_census`](super::gate_census) and with
  /// [`parser::recovery_gate`](crate::parser::recovery_gate)'s `RECOVERY_GATE_CENSUS`, whose
  /// baseline tallies need the same comment-blindness for the same reason.
  pub(crate) fn code_matches(src: &str, needle: &str) -> usize {
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
  pub(crate) fn code_find(src: &str, needle: &str) -> Option<usize> {
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
