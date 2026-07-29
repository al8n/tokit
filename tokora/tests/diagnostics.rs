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
//! `TRYBUILD=overwrite cargo +1.87 test -p tokora --all-features --test diagnostics` and
//! review the diff.
//!
//! # The toolchain pin, and its honest cost
//!
//! rustc's diagnostic rendering churns between releases — note ordering, `help:` wording,
//! and the underline art all move — so a `.stderr` committed against stable would go red
//! on a schedule nobody controls. The files here are therefore generated on the crate's
//! **MSRV toolchain, 1.87**, and the test **skips itself on any other rustc**.
//!
//! The cost of that is exact and worth stating rather than glossing: **this rail proves
//! the message only on 1.87.** A change that keeps the 1.87 rendering intact while
//! breaking a later one passes here. What it does catch is the thing it was built for —
//! somebody deleting or rewriting an `on_unimplemented` attribute and the curated message
//! silently reverting to rustc's default, which is a source change, not a rustc change.
//!
//! Because the default gates run on a newer toolchain, this rail is only live where 1.87
//! is: CI's `msrv` job runs it explicitly.

/// The toolchain the committed `.stderr` files were generated on. A rustc that does not
/// report this version skips the cases rather than diffing against another release's
/// rendering.
const PINNED_RUSTC: &str = "1.87.0";

/// The rustc trybuild's inner `cargo` will use: the `RUSTC` override if the environment
/// sets one, otherwise whatever `rustc` resolves to on `PATH` — which is the rustup shim,
/// and so honours `+1.87` / `RUSTUP_TOOLCHAIN` exactly as the inner build will.
fn rustc_version() -> Option<String> {
  let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
  let out = std::process::Command::new(rustc)
    .arg("--version")
    .output()
    .ok()?;
  if !out.status.success() {
    return None;
  }
  // `rustc 1.87.0 (17067e9ac 2025-05-09)` -> `1.87.0`
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
         Run `cargo +1.87 test -p tokora --all-features --test diagnostics`.",
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

/// Cases that pin a **bound** rather than a curated message.
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
const BOUND_CASES: &[&str] = &[
  "tests/ui/session_sink.rs",
  "tests/ui/bundle1_policy.rs",
  "tests/ui/node_brand_mismatch.rs",
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
