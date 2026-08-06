//! RECORD_CENSUS — every [`Verbose`](super::Verbose) emit channel routes through the record
//! chokepoint, and the raw store is spelled in exactly one file.
//!
//! # Why a census, and why here
//!
//! `checkpoint`, `rewind` and `diagnostics()` are all defined in terms of the shared emission
//! log. A channel that appends to a payload map without logging leaves the mark unmoved, the
//! unwind blind, and the replay short by one — for that channel only, silently, with the rest
//! of the emitter still perfectly exact. Eleven channels once did precisely that.
//!
//! The private `store` submodule makes the raw write a compile error rather than a defect
//! (see [`store`](super::store)), so this census is not the primary rail — it is the one that
//! catches the *shapes privacy cannot see*: an emit method that records twice, an emit method
//! that records nothing at all, a route spelled through a second alias, and above all a
//! **new** channel that lands without joining either the routing law or the conformance
//! matrix. Rust has no call-site reflection, so — exactly like `SETTLE_CENSUS` in
//! `input_ref/census_tests.rs` — the residual "notice a new site exists" step is a greppable
//! marker and a consciously-maintained count. `grep RECORD_CENSUS` finds every anchor.
//!
//! # The census (what must stay true)
//!
//! - **Seventeen emit channels, seventeen routes.** Per *method*, not merely per file: each
//!   `fn emit_*` body contains exactly one `self.record(` / `self.record_warning(` /
//!   `self.record_hole(` call, and the one its family names. File totals alone would let a
//!   double-recording method cancel out a non-recording neighbour in the same file.
//! - **The ten specialized channel files never name the storage.** No `.errs`, `.warns`,
//!   `.holes`, `.log`, `label_snapshots`, `.store`, `.entry(` or `.push(` — the text-level
//!   twin of the privacy wall, which also closes the `self.store.record(…)` alternate
//!   spelling that would route correctly but drift the counts.
//! - **The chokepoint is singular.** `record`/`record_warning`/`record_hole` and `rewind_to`
//!   are defined once, in `store.rs`, which is also the only file that pushes to the log or
//!   opens a payload group; `mod.rs` holds exactly the three one-line delegates.
//! - **The module list is itself pinned.** A twelfth channel in a *new* file would leave every
//!   per-file count untouched and both totals intact, so the count of module declarations in
//!   `mod.rs` is asserted too: a new neighbour cannot ship without being registered here.
//!
//! # Scope
//!
//! This census covers `impl_/verbose/**`. One `impl … for Verbose` lives outside it —
//! `emitter/cst.rs`'s `CstEmitter` impl, whose whole body is the required `cst_demote`
//! discard — which carries no diagnostic payload and could not record if it wanted to
//! (`Verbose::record` is private to this subtree). It is asserted to contain no emit channel
//! at all, so the scope statement is a test rather than a claim.
//!
//! Counting is line-based and skips `//`-prefixed lines, so doc references to these names do
//! not count; only code does. Keep code mentions of the counted names off comment-trailing
//! positions and out of string literals in the censused files — the route comment every
//! channel carries sits directly above its counted call for exactly that reason.

/// The `impl_/verbose` sources under census. Every non-test source file of the module tree is
/// listed, so a channel added in a *new* neighbour file lands in the sweep once that file is
/// registered here — and [`record_census_module_list_is_registered`] makes forgetting to
/// register it a failing test rather than a missed review comment.
///
/// `tests.rs` and this file are deliberately absent: test code may name anything in prose or
/// in a fixture.
const SOURCES: &[(&str, &str)] = &[
  ("mod.rs", include_str!("mod.rs")),
  ("store.rs", include_str!("store.rs")),
  ("diagnostic.rs", include_str!("diagnostic.rs")),
  ("full_container.rs", include_str!("full_container.rs")),
  (
    "missing_leading_separator.rs",
    include_str!("missing_leading_separator.rs"),
  ),
  (
    "missing_trailing_separator.rs",
    include_str!("missing_trailing_separator.rs"),
  ),
  ("pratt.rs", include_str!("pratt.rs")),
  ("separator.rs", include_str!("separator.rs")),
  ("too_few.rs", include_str!("too_few.rs")),
  ("too_many.rs", include_str!("too_many.rs")),
  ("unclosed.rs", include_str!("unclosed.rs")),
  (
    "unexpected_leading_separator.rs",
    include_str!("unexpected_leading_separator.rs"),
  ),
  (
    "unexpected_trailing_separator.rs",
    include_str!("unexpected_trailing_separator.rs"),
  ),
];

/// The one `impl … for Verbose` outside the censused directory — see the scope note above.
const CST_MOD: &str = include_str!("../../cst.rs");

/// The ten specialized channel files: everything in [`SOURCES`] that is neither the emitter's
/// own module, the storage, nor the read-side iterator.
const CHANNEL_FILES: &[&str] = &[
  "full_container.rs",
  "missing_leading_separator.rs",
  "missing_trailing_separator.rs",
  "pratt.rs",
  "separator.rs",
  "too_few.rs",
  "too_many.rs",
  "unclosed.rs",
  "unexpected_leading_separator.rs",
  "unexpected_trailing_separator.rs",
];

/// Fetches a censused source by name.
fn source(name: &str) -> &'static str {
  SOURCES
    .iter()
    .find(|(n, _)| *n == name)
    .map(|(_, s)| *s)
    .unwrap_or_else(|| panic!("RECORD_CENSUS: `{name}` is not a censused source"))
}

/// Counts occurrences of `needle` on the non-comment lines of `hay`.
///
/// Line-based on purpose: `//`-prefixed lines (`//`, `///`, `//!`) are documentation and may
/// name the censused methods freely. The censused files keep code mentions of these names off
/// comment-trailing positions, so this is exact for them.
fn count(hay: &str, needle: &str) -> usize {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .map(|line| line.matches(needle).count())
    .sum()
}

/// The source lines of a two-space-indented `fn <name>(` in `hay`, from its signature through
/// the closing brace at that same indentation.
///
/// Body-scoped rather than file-scoped because the law below is per-method: it is not enough
/// that a file mentions the chokepoint N times, each *channel* has to route through it exactly
/// once. The censused files are rustfmt-formatted with two-space indentation, so a method's
/// own closing brace is the first `  }` at or after its signature and every brace inside it is
/// deeper.
fn method_body(hay: &str, name: &str) -> std::string::String {
  // Most censused methods take no generics of their own (`fn name(`); `emit_unclosed` carries
  // one (`fn emit_unclosed<Delimiter>(`), so its parameter list opens with `<` rather than
  // `(`. Either opener starts the same body search.
  let paren = std::format!("fn {name}(");
  let generic = std::format!("fn {name}<");
  let lines: std::vec::Vec<&str> = hay.lines().collect();
  let start = lines
    .iter()
    .position(|line| {
      line.starts_with("  ")
        && !line.starts_with("   ")
        && (line.contains(&paren) || line.contains(&generic))
    })
    .unwrap_or_else(|| panic!("RECORD_CENSUS: no two-space-indented `fn {name}(` in the source"));
  let len = lines[start..]
    .iter()
    .position(|line| *line == "  }")
    .unwrap_or_else(|| panic!("RECORD_CENSUS: `fn {name}` has no closing brace at its indent"));
  lines[start..=start + len].join("\n")
}

/// The three routes, by the channel family each one records into.
const ROUTES: &[&str] = &["self.record(", "self.record_warning(", "self.record_hole("];

/// Every emit channel, the file it lives in, and the route its family requires.
const CHANNELS: &[(&str, &str, &str)] = &[
  ("mod.rs", "emit_lexer_error", "self.record("),
  ("mod.rs", "emit_error", "self.record("),
  ("mod.rs", "emit_unexpected_token", "self.record("),
  ("mod.rs", "emit_warning", "self.record_warning("),
  ("mod.rs", "emit_skipped_region", "self.record_hole("),
  ("full_container.rs", "emit_full_container", "self.record("),
  ("separator.rs", "emit_missing_separator", "self.record("),
  ("separator.rs", "emit_missing_element", "self.record("),
  (
    "missing_leading_separator.rs",
    "emit_missing_leading_separator",
    "self.record(",
  ),
  (
    "missing_trailing_separator.rs",
    "emit_missing_trailing_separator",
    "self.record(",
  ),
  (
    "unexpected_leading_separator.rs",
    "emit_unexpected_leading_separator",
    "self.record(",
  ),
  (
    "unexpected_trailing_separator.rs",
    "emit_unexpected_trailing_separator",
    "self.record(",
  ),
  ("too_few.rs", "emit_too_few", "self.record("),
  ("too_many.rs", "emit_too_many", "self.record("),
  ("pratt.rs", "emit_unexpected_end_of_lhs", "self.record("),
  ("pratt.rs", "emit_unexpected_end_of_rhs", "self.record("),
  ("unclosed.rs", "emit_unclosed", "self.record("),
];

/// RECORD_CENSUS — every emit channel routes through the chokepoint, once, on the right
/// channel. Per method body, so two defects in one file cannot cancel.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_every_emit_channel_routes_through_the_chokepoint() {
  for (file, method, route) in CHANNELS {
    let body = method_body(source(file), method);
    for candidate in ROUTES {
      let got = count(&body, candidate);
      let want = usize::from(candidate == route);
      assert!(
        got == want,
        "RECORD_CENSUS drift: `{file}`'s `{method}` has {got} `{candidate}` call(s), expected \
         {want}. A Verbose emit channel that does not route through \
         record/record_warning/record_hole exactly once voids checkpoint/rewind/diagnostics \
         for exactly that channel — add the route AND its conformance row, then update this \
         census in the same commit (grep RECORD_CENSUS)."
      );
    }
  }
}

/// RECORD_CENSUS — the cheap outer net: the number of declared channels and the number of
/// routes must both be 17 and must move together. A new `fn emit_` anywhere in the censused
/// files fails this until it is routed and registered in [`CHANNELS`] above.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_channel_count_matches_route_count() {
  let declared: usize = SOURCES.iter().map(|(_, src)| count(src, "fn emit_")).sum();
  let routed: usize = SOURCES
    .iter()
    .map(|(_, src)| ROUTES.iter().map(|r| count(src, r)).sum::<usize>())
    .sum();
  assert!(
    declared == 17 && routed == 17 && declared == routed,
    "RECORD_CENSUS drift: {declared} declared emit channel(s) against {routed} route call(s), \
     expected 17 of each. A Verbose emit channel that does not route through \
     record/record_warning/record_hole voids checkpoint/rewind/diagnostics for exactly that \
     channel — add the route AND its conformance row, then update this census in the same \
     commit (grep RECORD_CENSUS)."
  );
  assert!(
    CHANNELS.len() == 17,
    "RECORD_CENSUS drift: the per-method table lists {} channel(s), expected 17 \
     (grep RECORD_CENSUS).",
    CHANNELS.len()
  );
}

/// RECORD_CENSUS — the ten specialized channel files never name the storage. The text-level
/// twin of the privacy wall, and the one that also rejects the correctly-routing but
/// count-drifting `self.store.record(…)` spelling.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_channel_files_never_name_the_store() {
  const BLACKOUT: &[&str] = &[
    ".errs",
    ".warns",
    ".holes",
    ".log",
    "label_snapshots",
    ".store",
    ".entry(",
    ".push(",
  ];
  for file in CHANNEL_FILES {
    let src = source(file);
    for needle in BLACKOUT {
      let got = count(src, needle);
      assert!(
        got == 0,
        "RECORD_CENSUS drift: `{file}` names the raw emission storage ({got}× `{needle}`). A \
         channel file's only write is `self.record(…)` and its two mirrors — reaching past \
         them, or routing through a second spelling, is what left eleven channels off the \
         emission log (grep RECORD_CENSUS)."
      );
    }
  }
}

/// RECORD_CENSUS — the chokepoint is singular: defined once in `store.rs`, delegated once in
/// `mod.rs`, and nowhere else. `store.rs` is likewise the only file that pushes to the log or
/// opens a payload group.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_chokepoint_is_singular() {
  // (needle, expected in store.rs)
  let in_store: &[(&str, usize)] = &[
    ("fn record(", 1),
    ("fn record_warning(", 1),
    ("fn record_hole(", 1),
    ("fn rewind_to(", 1),
    (".log.push(", 3),
    ("errs.entry(", 1),
    ("warns.entry(", 1),
    ("holes.entry(", 1),
  ];
  for (needle, want) in in_store {
    let got = count(source("store.rs"), needle);
    assert!(
      got == *want,
      "RECORD_CENSUS drift: `store.rs` has {got} `{needle}`, expected {want}. The write \
       surface is one body per channel family; a second one is a second place the log can \
       fall out of step with the payload maps (grep RECORD_CENSUS)."
    );
  }

  // The three delegates `mod.rs` holds, and nothing more.
  let in_mod: &[(&str, usize)] = &[
    ("fn record(", 1),
    ("fn record_warning(", 1),
    ("fn record_hole(", 1),
    ("self.store.record(", 1),
    ("self.store.record_warning(", 1),
    ("self.store.record_hole(", 1),
  ];
  for (needle, want) in in_mod {
    let got = count(source("mod.rs"), needle);
    assert!(
      got == *want,
      "RECORD_CENSUS drift: `mod.rs` has {got} `{needle}`, expected {want}. `mod.rs` holds \
       exactly the three one-line delegates the channel files call (grep RECORD_CENSUS)."
    );
  }

  // Everywhere else: none of it.
  for (name, src) in SOURCES {
    if *name == "store.rs" || *name == "mod.rs" {
      continue;
    }
    for needle in [
      "fn record(",
      "fn record_warning(",
      "fn record_hole(",
      "fn rewind_to(",
      ".log.push(",
      "errs.entry(",
      "warns.entry(",
      "holes.entry(",
    ] {
      assert!(
        count(src, needle) == 0,
        "RECORD_CENSUS drift: `{name}` defines or performs a raw emission write \
         (`{needle}`). The chokepoint lives in `store.rs` alone (grep RECORD_CENSUS)."
      );
    }
  }
}

/// RECORD_CENSUS — the module list is pinned, because [`SOURCES`] *is* the inventory: a
/// twelfth channel in a brand-new file would leave every per-file count untouched and both
/// totals intact, and ship unrouted and uncovered through both nets.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_module_list_is_registered() {
  let got = count(source("mod.rs"), "mod ");
  assert!(
    got == 14,
    "RECORD_CENSUS drift: `verbose/mod.rs` declares {got} module(s), expected 14. A new \
     `verbose/` module must be registered in RECORD_CENSUS `SOURCES` and in the conformance \
     suite's CONFORMANCE_CENSUS include list in the same commit, or a channel inside it is \
     invisible to both (grep RECORD_CENSUS)."
  );
}

/// RECORD_CENSUS — the scope statement, as a test: the one `impl … for Verbose` outside this
/// directory carries no emit channel, so nothing the census cannot see can record.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn record_census_scope_excludes_only_a_channelless_impl() {
  let got = count(CST_MOD, "fn emit_");
  assert!(
    got == 0,
    "RECORD_CENSUS drift: `emitter/cst.rs` gained {got} emit channel(s). This census covers \
     `impl_/verbose/**`; a channel implemented outside it needs its own routing law and its \
     own conformance row (grep RECORD_CENSUS)."
  );
}
