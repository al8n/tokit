#![cfg(feature = "std")]

//! R8 — the bundle's failure messages stay useful.
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

#[test]
fn on_unimplemented_messages_match_their_committed_stderr() {
  match rustc_version() {
    Some(version) if version == PINNED_RUSTC => {}
    other => {
      eprintln!(
        "R8 skipped: the committed .stderr is pinned to rustc {PINNED_RUSTC}, this is {}. \
         Run `cargo +1.87 test -p tokora --all-features --test diagnostics`.",
        other.as_deref().unwrap_or("unknown")
      );
      return;
    }
  }

  let t = trybuild::TestCases::new();
  for (case, _, _) in CASES {
    t.compile_fail(case);
  }
}

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
];

/// The half of R8 that does not need the pinned toolchain — and the half that closes the
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
