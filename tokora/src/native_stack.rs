//! The **native call stack** a Pratt frame sits on, and the two things that bound how many of them
//! fit: the recursion budget, which refuses, and the optional `stacker` feature, which moves where
//! the refusing has to happen.
//!
//! # NO INTRA-DOC LINKS IN THIS FILE, DELIBERATELY
//!
//! `native_stack` is a **private** module, so `cargo doc` never documents its items and never
//! resolves a `[link]` written here — a dangling one passes `-D warnings` on all three rustdoc
//! legs. Prose in a private module is unchecked prose, so this file spells names in plain code
//! spans and the load-bearing, consumer-facing half of what follows lives on
//! `RecursionLimiter::PARSE_DEFAULT_DEPTH`, which is public and whose links *are* checked.
//!
//! # The ceiling does not go away, it changes owner
//!
//! Without the `stacker` feature the ceiling is a **hardware constant**: a thread gets 2 MiB (or 8
//! MiB on the main thread), a pratt level costs what it costs, and the quotient is a number nobody
//! chose. Exceeding it is not an error — it is `fatal runtime error: stack overflow`, which
//! carries no diagnostic, cannot be caught, and takes the process with it. The recursion budget
//! exists to put a refusal on the `Result` channel *before* that quotient is reached, which is
//! only possible while the budget sits below it.
//!
//! With the feature on, a frame that is inside `RED_ZONE` bytes of the end of its stack runs on a
//! fresh `SEGMENT`-byte heap allocation instead. **The ceiling stops being a hardware constant and
//! becomes a number someone chose** — whatever `InputContext::with_recursion_limiter` says.
//!
//! **FOR THESE FRAMES ONLY, and the feature therefore moves no default.** `maybe_grow` is called
//! from the two Pratt frame prologues and from nowhere else, while
//! `RecursionLimiter::PARSE_DEFAULT_DEPTH` seeds every `InputContext` — including the budget a
//! consumer's own `descending` production draws on, which is an ordinary unsegmented frame on an
//! ordinary thread stack. For one revision this feature raised that shared constant from 16 to
//! 1024, which authorised ~41 MiB of unsegmented native stack against a 2 MiB thread: the abort
//! this whole module is written around, reintroduced by the thing meant to remove it. The figure
//! that IS defensible over segmented frames is published separately, as
//! `RecursionLimiter::SEGMENTED_PRATT_DEPTH`, and a caller opts into it because only a caller
//! knows whether the precondition holds of their grammar.
//!
//! **That is the whole of what it buys, and it is not a safety mechanism.** `InputRef::descend`
//! stays, every call site of it stays, and the reason is measurable rather than cautious:
//!
//! * a segment is a heap allocation, so N levels cost N × (one level's frame) of memory, and at a
//!   large enough N that is more than the machine has;
//! * stacker's allocator is `mmap` followed by `assert_ne!(.., MAP_FAILED)` — **not** a fallible
//!   reserve. There is no `Err` for the parse to return, no `MaybeTerminal` for a recoverer to
//!   consult, and under `panic = "abort"` there is not even an unwind. It does not go through the
//!   Rust global allocator either, so a consumer who installs a limiting or fallible allocator
//!   does not bound it;
//! * and `RED_ZONE` is a *prediction* about how much stack one level needs. A grammar whose level
//!   costs more overruns the segment into a guard page, which is a `SIGSEGV` and not a diagnostic.
//!
//! So the feature raises the number, and the budget is still the only thing that turns a too-deep
//! input into a value a caller can handle. THE MEASUREMENTS below record the run that proves it.
//!
//! # Where the check goes
//!
//! The **frame prologue of both Pratt engines** — the same line `InputRef::descend` already
//! occupies, composed with it rather than substituted for it:
//!
//! ```rust,ignore
//! let mut frame = inp.descend()?;      // the budget: one level, refusable
//! let inp = &mut *frame;
//! native_stack::maybe_grow(move || {   // the stack: room for this frame, or a new segment
//!   // …the frame body, unchanged…
//! })
//! ```
//!
//! The prologue and not the recursion sites, for the reason the prologue owns `descend`: a frame
//! does substantial work before its first recursive call — the CST mark, the LHS parse, and
//! whatever grammar code either of those reaches — and a check placed at the recursion sites
//! leaves all of it running on the stack that is about to run out. A recursion site added later
//! inherits the check for free, which a per-site placement cannot promise.
//!
//! # THE MEASUREMENTS
//!
//! One pratt frame per level of nesting, on an explicitly sized **2 MiB** thread, bisected to the
//! last depth that completes before the native stack ends the process. tokora's own grammar, the
//! table `RecursionLimiter` carries:
//!
//! | build | typed driver | token driver |
//! |---|---|---|
//! | release | 3871 (~0.53 KiB/level) | 4247 (~0.48 KiB/level) |
//! | debug | 384 (~5.3 KiB/level) | **125** (~16.4 KiB/level) |
//!
//! And a **real consumer grammar**, debug, same 2 MiB thread, five axes:
//!
//! | axis | depth | KiB/level |
//! |---|---|---|
//! | aarch64, primary dialect | 57 | ~36 |
//! | x86_64 | 60 | ~34 |
//! | a second dialect | 53 | ~39 |
//! | that dialect's generic brackets | 52 | ~39 |
//! | an alternative source type | **51** | **~41** |
//!
//! **51 is the binding cell and ~41 KiB the binding cost, and both constants below are derived
//! from that row rather than from the 125 above it.** A consumer's grammar sits *inside* tokora's
//! recursion — its productions are what the pratt driver calls — so a tokora level and a consumer
//! level are one level, and it is the consumer that pays for both. Sizing a stack-safety number
//! against this crate's own frames measures the smaller half of what actually runs.
//!
//! ## The negative result, which is the point
//!
//! Feature **on**, limiter `RecursionLimiter::unlimited()`, prefix chain deepened until something
//! stops it. **Nothing on the `Result` channel ever does.** Measured on aarch64-apple-darwin,
//! debug, on the same 2 MiB thread, through `tests/pratt_limit.rs`'s
//! `the_unlimited_budget_has_no_ceiling_to_refuse_at` — `#[ignore]`d precisely because its
//! terminating case costs the machine:
//!
//! | depth | peak RSS | outcome |
//! |---|---|---|
//! | 1 000 | 4 MB | `Ok`, 1 000 operators folded |
//! | 10 000 | 17 MB | `Ok` |
//! | 50 000 | 263 MB | `Ok` |
//! | 100 000 | 549 MB | `Ok` |
//! | 400 000 | 2 245 MB | `Ok` |
//! | 1 000 000 | 5 588 MB | `Ok` |
//!
//! Linear at **~5.7 KiB per level** — the debug typed driver's own frame, which is what the table
//! above predicts — from a source of two bytes per level. No knee, and no refusal at any depth:
//! 1 000 000 levels is 258× the 3 871 the *release* build fits on that thread, and it still returns
//! `Ok`. The sweep stopped at a 6 GB ceiling **imposed from outside the process**, because this
//! machine is shared; extrapolating the fit, its 48 GiB of RAM plus 7 GiB of swap is gone at
//! roughly 10 million levels, from a ~20 MB document. **Having to invent an external ceiling in
//! order to end the run is the finding.**
//!
//! ### What ends it is not on any channel
//!
//! Two deaths, both measured, neither a value:
//!
//! * **the realistic one** — a 2 MiB segment is `mmap`'d lazily, so the *allocation* succeeds and
//!   the process dies later, when the frames touch the pages and physical memory runs out. That is
//!   a kernel kill: no unwind, no destructor, no diagnostic, nothing to catch;
//! * **stacker's own refusal**, when a request is large enough for `mmap` to reject outright. It is
//!   an `assert_ne!` and not a fallible reserve. Measured directly against `stacker 0.1.25`:
//!   `panicked at mmap_stack_restore_guard.rs:35: assertion left != right failed: mmap failed to
//!   allocate stack: Cannot allocate memory (os error 12)`, exit **101** — and exit **134**,
//!   `SIGABRT`, from the same binary built with `-C panic=abort`.
//!
//! So there is no `Err` to return, no `MaybeTerminal` for a recoverer to consult, and no `?` that
//! could carry it. That is the property this module exists to state: **the budget is what makes
//! deep input refusable; `stacker` is not a substitute for it.**

/// The stack a frame is guaranteed to have beneath it, and the threshold at which the next frame
/// moves to a new segment.
///
/// # Derivation
///
/// This has to exceed **everything one level of nesting spends between two consecutive checks** —
/// one pratt frame plus every grammar frame it calls before its child's prologue checks again. The
/// binding figure is therefore a per-*level* cost and not a per-*frame* one, and the largest
/// measured is the consumer row of THE MEASUREMENTS: **≈41 KiB**, from the debug build that aborts
/// at 51 levels on a 2 MiB thread.
///
/// 256 KiB is the next power of two above 4× that: **6.2×** the heaviest measured level, and 15.6×
/// tokora's own ≈16.4 KiB. The margin is not decoration — 41 KiB is one grammar on one platform
/// under one toolchain, and guessing low fails as a guard-page `SIGSEGV` with no diagnostic, which
/// is the worse half of the asymmetry this module is written around.
///
/// **It is a prediction, and predictions can be wrong.** A grammar whose non-pratt recursion
/// between two levels exceeds 256 KiB overruns the segment, and nothing here can detect that:
/// stacker measures the stack pointer, not the frame about to be pushed. The repair, if a consumer
/// ever meets it, is to make this pair configurable through `InputContext` — not built today,
/// because no consumer needs it and this crate does not publish API on speculation.
#[cfg(feature = "stacker")]
pub(crate) const RED_ZONE: usize = 256 * 1024;

/// How much stack a new segment gets.
///
/// # Derivation
///
/// **2 MiB, because that is the unit every measurement above is already stated in.** It is what
/// `std::thread::spawn` and every libtest harness thread hands out, so one segment reproduces
/// exactly the ceiling the table describes: ~43 levels of the heaviest measured consumer grammar,
/// ~125 of tokora's own debug token driver — which is, and has to be, the same 125 the table's
/// binding cell records for a 2 MiB thread.
///
/// The three constraints it satisfies:
///
/// * **≫ `RED_ZONE`** — 8×. A segment only a little larger than the red zone is back inside it
///   almost immediately, so every frame would allocate.
/// * **bounded waste** — the tail is the only partly-used segment, so a parse over-allocates by at
///   most one 2 MiB segment however deep it goes. Per *level* the cost converges on that level's
///   own frame size, so a larger segment does not buy cheaper levels: it buys a larger worst-case
///   tail.
/// * **bounded frequency** — one `mmap` per ~43 levels of the heaviest measured grammar. At
///   `RecursionLimiter::SEGMENTED_PRATT_DEPTH`, the 1024 this feature publishes for a caller whose
///   whole descent is segmented Pratt frames, that is <= 24 allocations for a whole parse.
#[cfg(feature = "stacker")]
pub(crate) const SEGMENT: usize = 2 * 1024 * 1024;

/// Runs one Pratt frame with `RED_ZONE` bytes of native stack guaranteed beneath it, moving to a
/// fresh `SEGMENT`-byte heap segment when the current stack cannot promise that much.
///
/// A panic inside `frame` is caught on the segment and resumed on the calling stack by stacker
/// itself, so the frame's own guards release on an unwind exactly as they do without the feature —
/// including the `Descent` this call is nested inside, which lives in the *caller's* frame on the
/// *caller's* stack and is therefore not even on the segment that goes away.
#[cfg(feature = "stacker")]
#[inline(always)]
pub(crate) fn maybe_grow<R>(frame: impl FnOnce() -> R) -> R {
  stacker::maybe_grow(RED_ZONE, SEGMENT, frame)
}

/// The two derivations above, enforced by the compiler rather than by prose.
///
/// Every operand is a constant, so this is settled at build time: a tree whose red zone or segment
/// contradicts the measurement it claims to come from does not compile. The figures are the shared
/// ones in `state::recursion_tracker::measured`, so retuning a constant here and leaving the table
/// alone is what reds — which is the drift these exist to catch. `panic!` in a const context takes
/// no arguments, so the messages name what to read instead of printing what they compared.
#[cfg(feature = "stacker")]
const _: () = {
  use crate::state::recursion_tracker::measured::{CONSUMER_SYNTACTIC_BYTES_PER_LEVEL, STACK};

  // `RED_ZONE` is a prediction about how much stack one level spends between two consecutive
  // checks. A red zone below the heaviest level actually measured is not a prediction, it is a
  // known-wrong one: the frame overruns the segment into a guard page, which is a SIGSEGV and not
  // a diagnostic.
  assert!(
    RED_ZONE >= CONSUMER_SYNTACTIC_BYTES_PER_LEVEL * 4,
    "RED_ZONE is under 4x CONSUMER_SYNTACTIC_BYTES_PER_LEVEL, the heaviest measured per-level cost"
  );
  // A segment at or barely above the red zone re-enters it immediately, so every frame would
  // allocate.
  assert!(
    SEGMENT >= RED_ZONE * 8,
    "SEGMENT is under 8x RED_ZONE, so a fresh segment is back inside the red zone too soon"
  );
  // The claim in `SEGMENT`'s derivation is ~43 levels of the heaviest measured grammar; this is
  // the floor under it, and therefore under the allocation-frequency figure.
  assert!(
    (SEGMENT - RED_ZONE) / CONSUMER_SYNTACTIC_BYTES_PER_LEVEL >= 32,
    "a segment holds too few levels of the heaviest measured grammar for the allocation \
     frequency SEGMENT's derivation claims"
  );
  // The load-bearing coincidence, pinned: `SEGMENT` is the 2 MiB every measurement is stated in,
  // so 'levels per segment' is a figure the table already contains rather than a new one nobody
  // checked. Retuning `SEGMENT` reds this, and the per-segment claims then have to be restated.
  assert!(
    SEGMENT == STACK,
    "SEGMENT is no longer the thread size the measurements are stated in, so the per-segment \
     level counts in its derivation describe a unit that no longer exists"
  );
};

/// The identity, and the whole of what this costs when the feature is off.
///
/// Not a runtime branch on a disabled flag: the call disappears at monomorphization, so a build
/// without `stacker` emits the frame body exactly as it did before the feature existed.
#[cfg(not(feature = "stacker"))]
#[inline(always)]
pub(crate) fn maybe_grow<R>(frame: impl FnOnce() -> R) -> R {
  frame()
}
