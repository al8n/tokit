#![cfg(feature = "std")]
// Excluded from miri entirely, and not by three `ignore`s: every test in this target reads
// `tests/ui/` off disk or spawns a compiler, and miri can do neither — `read_dir`/`open` are
// refused by its isolation and a process spawn aborts on `posix_spawnattr_init`. That abort is
// not a test failure; it takes the whole `cargo miri test --tests` run down before any other
// target is reached, which is how six miri jobs were failing while producing no result about
// the crate at all. Nothing here has a UB surface for miri to have an opinion about.
#![cfg(not(miri))]

//! The bundle's failure messages stay useful.
//!
//! Three `#[diagnostic::on_unimplemented]` attributes carry the curated text a consumer
//! reads when a type is not yet usable with tokora: [`FromTokenErrors`] (the five
//! token-level conversions), [`FromUnclosed`] (the delimiter half, which is also a root
//! obligation wherever an emitter names it directly), and [`Dialect`] (user-implemented,
//! never blanket). Each `tests/ui/*.rs` case fails to compile on purpose; the committed
//! `.stderr` beside it is the message, byte for byte.
//!
//! **The `.stderr` files are generated, never hand-edited.** Regenerate with
//! `TRYBUILD=overwrite cargo +<MSRV> test -p tokora --all-features --test diagnostics` (the
//! MSRV declared in the workspace `Cargo.toml`) and review the diff.
//!
//! # The toolchain pin, and its honest cost
//!
//! rustc's diagnostic rendering churns between releases — note ordering, `help:` wording,
//! and the underline art all move — so a `.stderr` committed against stable would go red
//! on a schedule nobody controls. The files here are therefore generated on the crate's
//! **MSRV toolchain** (currently 1.95; see [`PINNED_RUSTC`]), and the test **skips itself on
//! any other rustc**.
//!
//! The cost of that is exact and worth stating rather than glossing: **this rail proves
//! the message only on the pinned MSRV.** A change that keeps that rendering intact while
//! breaking a later one passes here. What it does catch is the thing it was built for —
//! somebody deleting or rewriting an `on_unimplemented` attribute and the curated message
//! silently reverting to rustc's default, which is a source change, not a rustc change.
//!
//! Because the default gates run on a newer toolchain, this rail is only live where the MSRV
//! is: CI's `msrv` job runs it explicitly, on whatever toolchain `Cargo.toml`'s `rust-version`
//! currently declares. A bump to that declaration must regenerate the `.stderr` files and move
//! [`PINNED_RUSTC`] in the same change, or this rail goes back to skipping itself everywhere.

/// The toolchain the committed `.stderr` files were generated on. A rustc that does not
/// report this version skips the cases rather than diffing against another release's
/// rendering. Kept in step with the workspace `Cargo.toml`'s `rust-version` — see the module
/// docs above.
const PINNED_RUSTC: &str = "1.95.0";

/// The rustc trybuild's inner `cargo` will use: the `RUSTC` override if the environment
/// sets one, otherwise whatever `rustc` resolves to on `PATH` — which is the rustup shim,
/// and so honours `+<MSRV>` / `RUSTUP_TOOLCHAIN` exactly as the inner build will.
fn rustc_version() -> Option<String> {
  let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
  let out = std::process::Command::new(rustc)
    .arg("--version")
    .output()
    .ok()?;
  if !out.status.success() {
    return None;
  }
  // `rustc 1.95.0 (59807616e 2026-04-14)` -> `1.95.0`
  String::from_utf8(out.stdout)
    .ok()?
    .split_whitespace()
    .nth(1)
    .map(str::to_owned)
}

/// The line CI greps for. A skip must be **loud where it runs**, not merely printed: without
/// this the msrv job is green whether the harness compiled four cases or none, and a toolchain
/// bump would silently retire every compile-time rail routed through here.
///
/// Format is deliberately machine-shaped and deliberately not a doc-test: the CI step asserts
/// on the count, not on exit 0.
const LIVENESS_MARKER: &str = "TOKORA_TRYBUILD_EXECUTED_CASES";

#[test]
fn on_unimplemented_messages_match_their_committed_stderr() {
  match rustc_version() {
    Some(version) if version == PINNED_RUSTC => {}
    other => {
      // Report ZERO explicitly. The marker is always emitted, so "the job never printed it"
      // and "the job printed 0" are different observations, and CI can tell them apart.
      println!("{LIVENESS_MARKER}=0");
      eprintln!(
        "R8 skipped: the committed .stderr is pinned to rustc {PINNED_RUSTC}, this is {}. \
         Run under the toolchain the workspace Cargo.toml declares as `rust-version` (e.g. \
         `cargo +1.95 test -p tokora --all-features --test diagnostics`).",
        other.as_deref().unwrap_or("unknown")
      );
      return;
    }
  }

  let t = trybuild::TestCases::new();
  let mut executed = 0usize;
  for (case, _, _) in CASES {
    t.compile_fail(case);
    executed += 1;
  }
  for case in BOUND_CASES {
    t.compile_fail(case);
    executed += 1;
  }

  println!("{LIVENESS_MARKER}={executed}");
  assert!(
    executed > 0,
    "the trybuild harness handed zero cases to trybuild on the pinned toolchain — every \
     compile-time rail routed through this file is inert, and the test would otherwise have \
     passed by doing nothing"
  );
}

/// The harness's own census: the number of `.rs` cases in `tests/ui/` must equal the number
/// this file hands to trybuild.
///
/// A floor cannot detect a missing member — only an exact total can. A case file added to
/// `tests/ui/` and never registered in [`CASES`] or [`BOUND_CASES`] compiles nothing, proves
/// nothing, and is invisible to a `>= 1` check. This runs on **every** toolchain, because it
/// reads a directory rather than compiling anything.
#[test]
fn every_ui_case_file_is_registered() {
  let mut on_disk: std::vec::Vec<String> = std::fs::read_dir("tests/ui")
    .expect("tests/ui is readable")
    .filter_map(|e| {
      let name = e.ok()?.file_name().into_string().ok()?;
      name.ends_with(".rs").then_some(name)
    })
    .collect();
  on_disk.sort();

  let mut registered: std::vec::Vec<String> = CASES
    .iter()
    .map(|(c, _, _)| *c)
    .chain(BOUND_CASES.iter().copied())
    .map(|c| c.trim_start_matches("tests/ui/").to_owned())
    .collect();
  registered.sort();

  assert_eq!(
    on_disk, registered,
    "tests/ui/ and the harness's case lists disagree. A file on disk but not registered is a \
     rail that compiles nothing; a registered file not on disk is a rail that cannot run."
  );
}

/// Cases that pin a **bound**, or a lint, rather than a curated message.
///
/// No `on_unimplemented` attribute is involved, so these are compiled by the trybuild half
/// above and are deliberately **not** in [`CASES`] — the curated-headline check below asks a
/// question about attributes that does not apply to them.
///
/// `session_sink` pins the fact `PartialSession::parse`'s emitter bound leans on: a recording
/// `Sink` is not a `ValueKeyedEmitter`. Scope worth stating, because it is narrower than it
/// looks: this fails if somebody makes `Sink` value-keyed, which would silently re-open the
/// session-plus-sink pairing. It does **not** fail if somebody deletes the bound from `parse`
/// itself — pinning that needs a case that drives the method through its whole where-clause,
/// whose failure would name a pile of unsatisfied obligations rather than this one.
/// `bundle1_policy` pins the lattice between the two emitter bundles: `ComposableEmitter` is
/// the default-policy surface and does NOT reach `TooManyEmitter`. Its teeth are inverted from
/// the usual shape — if someone widens bundle-1, the snippet starts compiling and the case
/// FAILS, which is precisely when the two-tier documentation must be revisited.
/// `descent_dropped_early` pins an **attribute**, not a bound: `Descent` is `#[must_use]`, and
/// that lint is the only thing standing between a downstream recursive combinator and
/// `inp.descend()?;` — a line that releases the recursion level before the frame it bounds and
/// silently restores the unbounded descent. Its subject can be deleted by a one-line edit; the
/// only other thing that would notice is `src/input/input_ref/descent_tests.rs`'s
/// `#[expect(unused_must_use, …)]`, which is the same rail on every toolchain rather than only on
/// the pinned one. That file also carries the runtime half: this attribute catches **one** of the
/// five ways a caller can release the level early, and the recommended shape,
/// `InputRef::descending`, cannot express any of them.
///
/// The two `session_point_cannot_cross_into_a_bracket*` cases pin a **reachability wall**, which
/// is a different kind of subject again: nothing in the crate reads them, and their whole value
/// is that the code inside them does not compile. `node()`'s up-front bracket spends its start
/// mark on the failing exit through an assert that fires in **every** build, and the only thing
/// keeping that assert unreachable from safe code is that a `SessionPointId` cannot be carried
/// across the bracket boundary — an invariant brand meeting `ParseInput`'s universally
/// quantified handle lifetime. The runtime settle scan is *not* a second wall: `take_point`'s
/// newest-first check would accept a pre-bracket point, because no younger point is open above
/// it. So the type system is the whole defence, and an untested type-system guarantee is one
/// signature change away from silently ceasing to hold. The closure case is the shape a grammar
/// would actually write; the `_impl` case closes the "that is just closure inference" reading by
/// attacking the trait boundary directly.
///
/// `cst_demote_cannot_be_inherited` pins a **missing default**, which is a subject with no runtime
/// shadow at all: `CstEmitter::cst_demote` has no default body, so a wrapper that forwards the four
/// 0.8 CST methods and stops fails with `E0046` instead of inheriting an empty failing exit. The
/// case's teeth point forward rather than back — the day someone re-adds that default "for
/// convenience", this file starts compiling and a `compile_fail` case that compiles is a failing
/// test. That is the *only* signal there would be: an inherited demote is silent on every success
/// path, byte-perfect in the buffer, and its error-path residue is byte-identical to an escaped
/// panic's, so no sink-side check and no other gate can see it. Same reason the two session-point
/// cases exist — the guarantee lives entirely in the type system, so it needs a test that compiles
/// nothing.
///
/// `tracker_combined_check_cannot_be_narrowed` pins a **removed customization point**, the mirror
/// image of that missing default: `TrackerExt`'s five update-and-check operations come from one
/// blanket impl over every `Tracker`, so coherence — not review — is what stops a tracker
/// supplying a combined method that checks less than `Tracker::check` does. That is the repair
/// for #265, where `Limiter` overrode two of them (then `Tracker`'s own provided methods) and
/// disambiguated to `TokenTracker::check`, so a recursion depth already past its maximum answered
/// `Ok(())` while the token count held. A narrowed combined check has no runtime shadow either —
/// it is indistinguishable from a tracker that was within its limits — so the only thing that can
/// hold the guarantee is a case that compiles nothing, and its teeth point forward the same way:
/// swap the blanket impl for per-type ones and this file starts compiling.
///
/// The last two pin **removed capability**, where a compile failure is not a proxy for the
/// guarantee but *is* the guarantee. `emitter_ref_cannot_checkpoint` holds the receiver on
/// `Emitter::checkpoint`: capturing a mark is a capability, so it must not travel on the shared
/// reference `InputRef::emitter_ref` hands a parser (#257). It is written against a generic
/// `Ctx::Emitter` on purpose — the wall is not the recording sink's, even though the sink is where
/// the corruption was found. `errors_has_no_structural_mutation_door` holds **both** halves of
/// #247's removal, `DerefMut` and the derived `AsMut<C>`, in one file: they were the same door
/// under two names, and a case pinning one would go green over a re-derived other. Neither has a
/// runtime shadow after the fix — that is what "unreachable" means — so the sibling tests in
/// `src/cst/sink/tests.rs` and `src/error/errors/tests.rs` cover what still compiles, and these
/// two cover what must not.
const BOUND_CASES: &[&str] = &[
  "tests/ui/session_sink.rs",
  "tests/ui/bundle1_policy.rs",
  "tests/ui/node_brand_mismatch.rs",
  "tests/ui/descent_dropped_early.rs",
  "tests/ui/session_point_cannot_cross_into_a_bracket.rs",
  "tests/ui/session_point_cannot_cross_into_a_bracket_impl.rs",
  "tests/ui/cst_demote_cannot_be_inherited.rs",
  "tests/ui/emitter_ref_cannot_checkpoint.rs",
  "tests/ui/errors_has_no_structural_mutation_door.rs",
  "tests/ui/diagnose_adapters_are_not_exact_size.rs",
  "tests/ui/tracker_combined_check_cannot_be_narrowed.rs",
];

/// Per case: the ui file, the curated headline its trait's attribute must produce, and
/// rustc's default phrasing for the same obligation — the text that appears the moment
/// the attribute is gone.
const CASES: &[(&str, &str, &str)] = &[
  (
    "tests/ui/from_token_errors.rs",
    "cannot yet be a tokora parse error for lexer",
    "the trait bound `MyError: FromTokenErrors",
  ),
  (
    "tests/ui/from_unclosed.rs",
    "cannot absorb an unclosed-delimiter diagnostic for lexer",
    "the trait bound `MyError: FromUnclosed",
  ),
  (
    "tests/ui/dialect.rs",
    "is not a tokora dialect",
    "the trait bound `NotADialect: Dialect",
  ),
  (
    "tests/ui/composable_emitter.rs",
    "is not a composable emitter for lexer",
    "the trait bound `MyEmitter: ComposableEmitter",
  ),
  (
    "tests/ui/separated_emitter.rs",
    "cannot report separator diagnostics for lexer",
    "the trait bound `MyEmitter: SeparatedEmitter",
  ),
];

/// The half of this suite that does not need the pinned toolchain — and the half that closes the
/// golden-file rail's own hole.
///
/// A `.stderr` regenerated after an attribute was accidentally dropped is still a
/// perfectly valid rustc rendering, so the trybuild half above would bless it and go
/// green. This one would not: each golden file has to carry its trait's curated headline
/// and must not carry rustc's default phrasing for the same obligation. It reads files
/// only, so unlike the trybuild half it runs on **every** toolchain and in the default
/// gates.
#[test]
fn committed_stderr_carries_the_curated_message_not_rustcs_default() {
  for (case, curated, rustc_default) in CASES {
    let golden = case.replace(".rs", ".stderr");
    let text = std::fs::read_to_string(&golden)
      .unwrap_or_else(|err| panic!("{golden} is missing or unreadable: {err}"));

    assert!(
      text.contains(curated),
      "{golden} no longer carries the curated headline {curated:?} — either the \
       `on_unimplemented` attribute changed or the file was regenerated without it"
    );
    assert!(
      !text.contains(rustc_default),
      "{golden} carries rustc's default phrasing {rustc_default:?}, which means the \
       curated attribute is not reaching this obligation"
    );
  }
}
