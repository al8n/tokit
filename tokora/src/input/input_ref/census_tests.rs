//! SETTLE_CENSUS / RELEASE_CENSUS / CAPTURE_WINDOW — the source censuses of every place a
//! committed token settles, every place an emitter checkpoint is spent or forgotten, and every
//! place one is taken behind the preflight reservation that keeps an unwind from stranding it.
//!
//! # Why a census, and why here
//!
//! A token becomes *committed* at a handful of settle sites spread across the consume
//! surface: the `next` arms, the `consume_cached` family, the `try_expect` accept arms,
//! the shared scanner's skip/stop settles. Any side channel that must observe committed
//! tokens exactly once (a CST event stream riding the emitter, a lossless trivia view)
//! has to hook **all** of them — an invariant enforced at N call sites instead of one
//! chokepoint, with N ≈ a dozen on day one. The crate's answer is the same as everywhere
//! else: one primitive ([`InputRef::commit_token`](super::InputRef)), every consume
//! settle routed through it, and a census that fails the build the moment a settle
//! appears outside it.
//!
//! Rust has no call-site reflection, so — exactly like `OP_SURFACE_CENSUS` in
//! `src/fuzz/ops.rs` — the residual "notice a new site exists" step is anchored by a
//! greppable marker and a consciously-maintained count. These tests are that count: they
//! read the `input_ref` sources with `include_str!` and assert the exact number of
//! settle-surface calls per file. Adding a consume path without routing it through the
//! primitive moves a count and fails a named test whose message carries the checklist.
//! `grep SETTLE_CENSUS` finds every anchor.
//!
//! # The census (what must stay true)
//!
//! **Every 1:1 consume settle goes through `commit_token`** — the eighteen sites:
//! `next()`'s and `next_or_stop()`'s cached and fresh-lex arms (4), `consume_cached_one` and
//! `consume_cached_to`'s loop body (2 — `consume_all_cached` drains *through*
//! `consume_cached_to`, per token), the `try_expect`/`try_expect_map`/
//! `try_expect_and_then`/`try_expect_or_stop`/`try_expect_map_or_stop` cached and accept
//! arms (10), the by-value `commit_probed` settle of a probed closer (1), and
//! `SyncThrough::on_stop` (1).
//!
//! **The one skip settle is `AtFrontier::adopt`**, called only by `skip_and_report`: a
//! token a scan skips settles behind the frontier — not into the input's span — exactly
//! once, whichever origin fed it. It is the settle surface's second (and last) member.
//!
//! **The span funnel is not a settle.** `set_span_after_consume` is also written by
//! three non-token paths, and they must **never** grow a settle hook: `settle_fatal`
//! writes a *rejected lexer error's* span; `SyncTo::on_eof` writes the lexer's span at
//! exhaustion; `commit_at` batch-writes a whole skipped run's frontier (each skipped
//! token already settled via `adopt`). Peeks, declines, `unconsume`, and the
//! position-write surgeries (`set_state`, restores) touch no settle at all.
//!
//! Counting is line-based and skips `//`-prefixed lines, so doc references to these
//! names do not count; only code does. Keep code mentions of the counted names off
//! comment-trailing positions and out of string literals in these files.

/// The `input_ref` sources under census. Every non-test source file of the module tree
/// is listed, so a settle added in a *new* neighbour file still lands in the sweep once
/// that file is registered here — and the `mod.rs` module list makes an unregistered
/// neighbour impossible to miss in review.
const SOURCES: &[(&str, &str)] = &[
  ("mod.rs", include_str!("mod.rs")),
  ("scan.rs", include_str!("scan.rs")),
  ("try_expect.rs", include_str!("try_expect.rs")),
  (
    "consume_cached/mod.rs",
    include_str!("consume_cached/mod.rs"),
  ),
  ("peek/mod.rs", include_str!("peek/mod.rs")),
  ("sync_to.rs", include_str!("sync_to.rs")),
  ("sync_through.rs", include_str!("sync_through.rs")),
  ("sync_balanced.rs", include_str!("sync_balanced.rs")),
  ("skip_while.rs", include_str!("skip_while.rs")),
  ("fold.rs", include_str!("fold.rs")),
  ("pratt.rs", include_str!("pratt.rs")),
  ("session.rs", include_str!("session.rs")),
  ("drop_policy.rs", include_str!("drop_policy.rs")),
  ("trace.rs", include_str!("trace.rs")),
  ("transaction/mod.rs", include_str!("transaction/mod.rs")),
  ("stacked/mod.rs", include_str!("stacked/mod.rs")),
];

/// Fetches a censused source by name.
fn source(name: &str) -> &'static str {
  SOURCES
    .iter()
    .find(|(n, _)| *n == name)
    .map(|(_, s)| *s)
    .unwrap_or_else(|| panic!("SETTLE_CENSUS: `{name}` is not a censused source"))
}

/// Counts occurrences of `needle` on the non-comment lines of `hay`.
///
/// Line-based on purpose: `//`-prefixed lines (`//`, `///`, `//!`) are documentation and
/// may name the censused methods freely. The censused files keep code mentions of these
/// names off comment-trailing positions, so this is exact for them.
fn count(hay: &str, needle: &str) -> usize {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .map(|line| line.matches(needle).count())
    .sum()
}

/// The source lines of a two-space-indented `fn <name>(` in `hay`, from its signature
/// through the closing brace at that same indentation.
///
/// Body-scoped rather than file-scoped because the law below is per-path: it is not enough
/// that a file mentions the funnel N times, each *exit path* has to route through it. The
/// censused files are rustfmt-formatted with two-space indentation, so a method's own closing
/// brace is the first `  }` at or after its signature and every brace inside it is deeper.
fn method_body(hay: &str, name: &str) -> std::string::String {
  // Most censused methods take no generics of their own (`fn name(`); a few — `guard_with`,
  // `begin_stacked_with` — carry one (`fn name<D: DropPolicy>(`), so the signature's own
  // parameter list opens with `<` rather than `(`. Either opener starts the same body search.
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
    .unwrap_or_else(|| panic!("RELEASE_CENSUS: no two-space-indented `fn {name}(` in the source"));
  let len = lines[start..]
    .iter()
    .position(|line| *line == "  }")
    .unwrap_or_else(|| panic!("RELEASE_CENSUS: `fn {name}` has no closing brace at its indent"));
  lines[start..=start + len].join("\n")
}

/// Counts call sites of a method: `self.<name>(` + `ir.<name>(` receivers.
fn calls(hay: &str, name: &str) -> usize {
  let self_form = std::format!("self.{name}(");
  let ir_form = std::format!("ir.{name}(");
  count(hay, &self_form) + count(hay, &ir_form)
}

/// SETTLE_CENSUS — the sixteen 1:1 consume settles, each a `commit_token` call, and no
/// call anywhere else. A new consume path must route through the primitive **and** bump
/// its file's expected count here, in the same commit.
#[test]
fn settle_census_commit_token_routes_every_consume_settle() {
  // (file, expected `commit_token` call sites)
  let expected: &[(&str, usize)] = &[
    // `next()` and its committed-consume sibling `next_or_stop()`: each a cached arm and a
    // fresh-lex arm.
    ("mod.rs", 4),
    // `SyncThrough::on_stop`: the consumed stopper.
    ("scan.rs", 1),
    // The cached and on-input accept arms of `try_expect`/`_map`/`_and_then`, plus
    // `try_expect_or_stop`'s and `try_expect_map_or_stop`'s cached and accept arms, plus
    // `commit_probed`'s by-value settle of a closer carried out of `probe_close`.
    ("try_expect.rs", 11),
    // `consume_cached_one` + `consume_cached_to`'s loop body; `consume_all_cached`
    // drains through `consume_cached_to`, so every cached token settles per token.
    ("consume_cached/mod.rs", 2),
  ];

  for (name, want) in expected {
    let got = calls(source(name), "commit_token");
    assert!(
      got == *want,
      "SETTLE_CENSUS drift: `{name}` has {got} `commit_token` call sites, expected {want}. \
       A consume settle moved. Route the new/changed site through `InputRef::commit_token` \
       (never a raw span/state write), then update this census in the same commit \
       (grep SETTLE_CENSUS)."
    );
  }

  // Everywhere else: zero. A settle outside the censused files means a new consume
  // surface grew without joining the census.
  for (name, src) in SOURCES {
    if expected.iter().any(|(n, _)| n == name) {
      continue;
    }
    let got = calls(src, "commit_token");
    assert!(
      got == 0,
      "SETTLE_CENSUS drift: `{name}` gained {got} `commit_token` call site(s) outside the \
       census. Add the file's expected count to this test in the same commit \
       (grep SETTLE_CENSUS)."
    );
  }

  // The primitive is defined exactly once, in `mod.rs`.
  assert!(
    count(source("mod.rs"), "fn commit_token(") == 1,
    "SETTLE_CENSUS drift: `commit_token` must be defined exactly once, in `mod.rs`"
  );
}

/// SETTLE_CENSUS — the span funnel's callers are locked. `commit_token` is the only
/// *settle* that writes it; the three non-token writers (`settle_fatal`, `commit_at`,
/// `SyncTo::on_eof`) must never gain a settle hook, and no fourth caller may appear:
/// a new consume path writing the funnel directly would bypass the settle primitive.
#[test]
fn settle_census_span_funnel_callers_are_locked() {
  // (file, expected `set_span_after_consume` call sites, who they are)
  let expected: &[(&str, usize, &str)] = &[
    (
      "mod.rs",
      3,
      "`commit_token`'s body, `settle_fatal` (a rejected error's span — NOT a token), \
       and `commit_at` (a scan's batch frontier write — its tokens settled via `adopt`)",
    ),
    (
      "scan.rs",
      1,
      "`SyncTo::on_eof` (the lexer's span at exhaustion — NOT a token)",
    ),
  ];

  for (name, want, who) in expected {
    let got = calls(source(name), "set_span_after_consume");
    assert!(
      got == *want,
      "SETTLE_CENSUS drift: `{name}` has {got} `set_span_after_consume` call sites, \
       expected {want} ({who}). A consume settle must route through `commit_token`; \
       a position write that is not a token settle must be listed here, in the same \
       commit (grep SETTLE_CENSUS)."
    );
  }

  for (name, src) in SOURCES {
    if expected.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    let got = calls(src, "set_span_after_consume");
    assert!(
      got == 0,
      "SETTLE_CENSUS drift: `{name}` writes the span funnel directly ({got} site(s)). \
       Token settles go through `InputRef::commit_token`; non-token position writes \
       must be registered here (grep SETTLE_CENSUS)."
    );
  }
}

/// SETTLE_CENSUS — the committed-token side channel (`Emitter::commit_token`, the CST
/// auto-emission hook) rides exactly the two settle surfaces and nothing else: once
/// inside `InputRef::commit_token`'s body (the fourteen consume settles arrive through it)
/// and once inside `skip_and_report` beside the `adopt` skip settle. Peeks, declines,
/// `unconsume`, `settle_fatal`, `SyncTo::on_eof`, and `commit_at` never reach the
/// emitter's token channel — a hook appearing anywhere else double-emits or
/// phantom-emits, the silent wrong-tree class.
#[test]
fn settle_census_emitter_hook_rides_only_the_settles() {
  // The consume-settle home: the field-receiver form inside `InputRef::commit_token`.
  assert!(
    count(source("mod.rs"), "emitter.commit_token(") == 1
      && count(source("mod.rs"), "emitter().commit_token(") == 0,
    "SETTLE_CENSUS drift: `Emitter::commit_token` must be called exactly once in mod.rs — \
     inside `InputRef::commit_token`'s body (grep SETTLE_CENSUS)."
  );
  // The skip-settle home: `skip_and_report`, before the report's verdict.
  assert!(
    count(source("scan.rs"), "emitter().commit_token(") == 1
      && count(source("scan.rs"), "emitter.commit_token(") == 0,
    "SETTLE_CENSUS drift: `Emitter::commit_token` must be called exactly once in scan.rs — \
     inside `skip_and_report`, beside the `adopt` settle (grep SETTLE_CENSUS)."
  );
  // Nowhere else: any new caller is a settle surface the census must adjudicate first.
  for (name, src) in SOURCES {
    if *name == "mod.rs" || *name == "scan.rs" {
      continue;
    }
    assert!(
      count(src, "emitter.commit_token(") == 0 && count(src, "emitter().commit_token(") == 0,
      "SETTLE_CENSUS drift: `{name}` feeds the emitter's committed-token channel outside \
       the two censused settle surfaces (grep SETTLE_CENSUS)."
    );
  }
  // The trait surface: declared once (defaulted no-op) and blanket-forwarded once —
  // the same shape the release census locks.
  assert!(
    count(EMITTER_MOD, "fn commit_token(") == 2,
    "SETTLE_CENSUS drift: `Emitter::commit_token` must appear exactly twice in \
     `emitter/mod.rs` — the defaulted declaration and the `&mut U` blanket forward \
     (grep SETTLE_CENSUS)."
  );
}

/// SETTLE_CENSUS — `AtFrontier::adopt` is the one skip settle, with exactly one caller
/// (`skip_and_report`). Every token a scan skips — trivia and recovery garbage alike,
/// cached or freshly lexed — settles behind the frontier through this single call, so a
/// side channel that hooks skipped tokens has exactly one home too.
#[test]
fn settle_census_adopt_is_the_single_skip_settle() {
  assert!(
    count(source("scan.rs"), ".adopt(") == 1,
    "SETTLE_CENSUS drift: `AtFrontier::adopt` must have exactly one caller \
     (`skip_and_report` in scan.rs). A second skip settle splits the surface the census \
     exists to keep whole (grep SETTLE_CENSUS)."
  );
  for (name, src) in SOURCES {
    if *name == "scan.rs" {
      continue;
    }
    assert!(
      count(src, ".adopt(") == 0,
      "SETTLE_CENSUS drift: `{name}` calls `adopt` outside the scanner's skip settle \
       (grep SETTLE_CENSUS)."
    );
  }
  assert!(
    count(source("mod.rs"), "fn adopt(") == 1,
    "SETTLE_CENSUS drift: `AtFrontier::adopt` must be defined exactly once, in `mod.rs`"
  );
}

// ─────────────────────────────────────────────────────────────────────────────────────
// RELEASE_CENSUS — every emitter checkpoint ends in exactly one of `rewind` (the branch
// was abandoned) or `release` (the branch was kept), on every exit path.
//
// `Emitter::checkpoint()` has exactly four captures: `save_checkpoint` (every guard,
// attempt, session point, and raw save) and the three sync-entry `ThroughEntry`
// snapshots. The spends are two `rewind` sites (`restore_unchecked`; the sync family's
// no-match EOF arm). Everything else that lets go of a mark *keeps* the branch, and must
// say so through `release` — otherwise a mark-keyed emitter (a checkpoint stack, an
// event sink) strands one row per committed guard, forever. The keep paths funnel:
//
// - every kept `Checkpoint` goes through `InputRef::forget_kept_checkpoint` — the raw
//   commit, both transaction guards' commit and commit-on-drop arms, every stacked-guard
//   path that ends the savepoint stack's life, and the session-point suffix a rewind
//   invalidates — which pairs the lineage forget with the emitter release in one body;
// - every kept `ThroughEntry` goes through `ScanMode::on_commit` — called on each of the
//   five committing exits of `skip_until` (boundary drain, fatal lex propagation, trip,
//   stop, fatal report propagation); the sixth exit is `on_eof`, which spends the mark
//   by rewinding to it.
//
// The third keep path is a session point abandoned by dropping the handle: `Session`'s
// `Drop` settles it exactly as `commit_point` would — unpin, forget the lineage id, and
// release the emitter mark — through the assert-free primitives rather than the
// `forget_kept_checkpoint` funnel (a drop may run mid-unwind, where the funnel's debug
// asserts must not fire). The cell holds the emitter borrow precisely so its drop can
// reach it; the census locks the abandon path to its one `lineage.forget` and its one
// `emitter.release`, so neither a second direct-forget nor an unpaired forget can appear
// silently.
// ─────────────────────────────────────────────────────────────────────────────────────

/// The emitter trait source, censused for the `release` surface itself.
const EMITTER_MOD: &str = include_str!("../../emitter/mod.rs");

/// RELEASE_CENSUS — the checkpoint captures and both settle verbs are locked, file by
/// file. A new `Emitter::checkpoint()` caller must pair every exit path with a `rewind`
/// or a `release` and register itself here in the same commit.
#[test]
fn release_census_every_checkpoint_capture_is_paired() {
  // The four captures of an emitter mark.
  let captures: &[(&str, usize)] = &[
    // `save_checkpoint`, the single funnel behind save/guards/attempts/session points.
    ("mod.rs", 1),
    // `sync_through` + `sync_through_then_peek_with_emitter` entry snapshots.
    ("sync_through.rs", 2),
    // `sync_balanced`'s entry snapshot.
    ("sync_balanced.rs", 1),
  ];
  for (name, want) in captures {
    let got =
      count(source(name), ".emitter.checkpoint()") + count(source(name), "emitter().checkpoint()");
    assert!(
      got == *want,
      "RELEASE_CENSUS drift: `{name}` captures {got} emitter checkpoint(s), expected \
       {want}. Every capture must end in `rewind` or `release` on every exit path — \
       wire the new capture and update this census in the same commit \
       (grep RELEASE_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if captures.iter().any(|(n, _)| n == name) {
      continue;
    }
    let got = count(src, ".emitter.checkpoint()") + count(src, "emitter().checkpoint()");
    assert!(
      got == 0,
      "RELEASE_CENSUS drift: `{name}` gained {got} emitter-checkpoint capture(s) outside \
       the census (grep RELEASE_CENSUS)."
    );
  }

  // The two rewind spends.
  assert!(
    count(source("mod.rs"), "emitter().rewind(") == 1
      && count(source("scan.rs"), "emitter().rewind(") == 1,
    "RELEASE_CENSUS drift: `Emitter::rewind` must have exactly two callers — \
     `restore_unchecked` (mod.rs) and the sync family's no-match EOF arm (scan.rs). \
     A third rewind site is a new abandon path; census it (grep RELEASE_CENSUS)."
  );

  // The three release homes: the kept-checkpoint funnel, the scanner's kept-snapshot
  // hook, and the session cell's abandoning drop (assert-free by necessity — it may run
  // mid-unwind). `release` is never called raw anywhere else in the input layer.
  assert!(
    count(source("mod.rs"), "emitter().release(") == 1
      && count(source("scan.rs"), "emitter().release(") == 1
      && count(source("session.rs"), ".emitter.release(") == 1,
    "RELEASE_CENSUS drift: `Emitter::release` must have exactly three input-layer homes — \
     `forget_kept_checkpoint` (mod.rs), `ScanMode::on_commit` (scan.rs), and the \
     session-abandon settle in `Session::release_abandoned_points` (session.rs). Route a \
     new keep path through one of them (grep RELEASE_CENSUS)."
  );
  for (name, src) in SOURCES {
    if *name == "mod.rs" || *name == "scan.rs" || *name == "session.rs" {
      continue;
    }
    assert!(
      count(src, "emitter().release(") == 0 && count(src, ".emitter.release(") == 0,
      "RELEASE_CENSUS drift: `{name}` releases an emitter mark directly; kept \
       checkpoints go through `forget_kept_checkpoint`, kept scan snapshots through \
       `ScanMode::on_commit`, abandoned session points through `Session`'s drop \
       (grep RELEASE_CENSUS)."
    );
  }
}

/// RELEASE_CENSUS — every kept `Checkpoint` funnels through `forget_kept_checkpoint`,
/// so its lineage forget and its emitter release cannot come apart. `forget_checkpoint`
/// (lineage only) keeps exactly one caller: the funnel's own body.
#[test]
fn release_census_kept_checkpoints_funnel_through_one_body() {
  // (file, expected `forget_kept_checkpoint` mentions, who they are)
  let expected: &[(&str, usize, &str)] = &[
    (
      "mod.rs",
      3,
      "the definition + `commit_checkpoint` + `abandon_points_above` (the session-point \
       suffix a rewind invalidates)",
    ),
    (
      "transaction/mod.rs",
      2,
      "`Transaction::commit` + the commit-on-drop arm",
    ),
    (
      "stacked/mod.rs",
      8,
      "savepoint `release` + `rollback_to`'s younger-savepoint drain + `commit` (savepoints, \
       base) + whole `rollback`'s savepoint drain + both drop arms (the rollback arm's \
       savepoint drain; the commit arm's savepoints and base)",
    ),
  ];
  for (name, want, who) in expected {
    let got = count(source(name), "forget_kept_checkpoint(");
    assert!(
      got == *want,
      "RELEASE_CENSUS drift: `{name}` has {got} `forget_kept_checkpoint` mentions, \
       expected {want} ({who}). A kept checkpoint must release its emitter mark through \
       the one funnel (grep RELEASE_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if expected.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      count(src, "forget_kept_checkpoint(") == 0,
      "RELEASE_CENSUS drift: `{name}` keeps a checkpoint outside the censused funnel \
       callers (grep RELEASE_CENSUS)."
    );
  }

  // The lineage-only forget keeps exactly one caller — the funnel body — plus its
  // definition; the session cell's abandon path uses the `Lineage` primitives directly
  // (documented above: the funnel's asserts must not run mid-unwind) and pairs them with
  // its own emitter release, which the home census above counts.
  assert!(
    count(source("mod.rs"), "forget_checkpoint(") == 2,
    "RELEASE_CENSUS drift: `forget_checkpoint` (lineage-only) must be its definition \
     plus exactly one caller — `forget_kept_checkpoint`'s body. Keeping a checkpoint \
     without releasing its emitter mark strands mark-keyed emitter state \
     (grep RELEASE_CENSUS)."
  );
  for (name, src) in SOURCES {
    if *name == "mod.rs" {
      continue;
    }
    assert!(
      count(src, "forget_checkpoint(") == 0,
      "RELEASE_CENSUS drift: `{name}` forgets a checkpoint's lineage entry without \
       releasing its emitter mark — route through `forget_kept_checkpoint` \
       (grep RELEASE_CENSUS)."
    );
  }
  assert!(
    count(source("session.rs"), "lineage.forget(") == 1,
    "RELEASE_CENSUS drift: the session cell's abandon path is the one documented \
     lineage-direct forget, paired in the same loop body with its emitter release; a \
     second direct forget must not appear (grep RELEASE_CENSUS)."
  );
}

/// RELEASE_CENSUS — every path that ends the savepoint stack's life routes **each entry**
/// through the kept-checkpoint funnel.
///
/// The savepoint stack is the one place a [`Checkpoint`](super::Checkpoint) is held outside a
/// guard's own base field, and a savepoint's emitter mark can *read the same value* as the
/// base's — a capture taken with no intervening events reads the same emission length. A
/// rewind spends what sits strictly above the mark plus the newest capture at it, so one
/// restore can never settle the older aliases: each of the five life-ending paths has to walk
/// the stack itself. Counted per method body, because the law is per exit path rather than per
/// file.
#[test]
fn release_census_every_savepoint_life_end_settles_through_the_funnel() {
  let stacked = source("stacked/mod.rs");

  // (method, expected funnel calls in its body, what they settle)
  let expected: &[(&str, usize, &str)] = &[
    ("release", 1, "the released savepoint and every younger one"),
    ("rollback_to", 1, "every savepoint younger than the target"),
    ("commit", 2, "every savepoint, then the base"),
    (
      "rollback",
      1,
      "every savepoint (the base is settled by its own restore)",
    ),
    (
      "drop",
      3,
      "the rollback arm's savepoints, and the commit arm's savepoints and base",
    ),
  ];
  for (method, want, who) in expected {
    let got = count(&method_body(stacked, method), "forget_kept_checkpoint(");
    assert!(
      got == *want,
      "RELEASE_CENSUS drift: `StackedTransaction::{method}` routes {got} entries through \
       `forget_kept_checkpoint`, expected {want} ({who}). Every path that ends the savepoint \
       stack's life must settle each entry through the funnel; a savepoint checkpoint dropped \
       raw strands mark-keyed emitter bookkeeping in every build (grep RELEASE_CENSUS)."
    );
  }

  // …and no bulk removal may shortcut the walk: each of these drops the removed entries'
  // checkpoints raw, which is exactly the shape the per-entry settle replaces.
  for bulk in ["saves.truncate(", "saves.clear(", "saves.drain("] {
    assert!(
      count(stacked, bulk) == 0,
      "RELEASE_CENSUS drift: `stacked/mod.rs` removes savepoints in bulk (`{bulk}`), dropping \
       their checkpoints without settling them (grep RELEASE_CENSUS)."
    );
  }
}

/// RELEASE_CENSUS — the scanner's kept snapshots: `skip_until` has six exits; five keep
/// the scan's progress and settle the snapshot through `ScanMode::on_commit`, the sixth
/// (`on_eof`) spends the mark by rewinding. A new exit must pick one, in the same
/// commit.
#[test]
fn release_census_scanner_snapshot_settles_on_every_exit() {
  let scan = source("scan.rs");
  assert!(
    count(scan, "M::on_commit(") == 5,
    "RELEASE_CENSUS drift: `skip_until` must settle the pre-call snapshot on each of \
     its five committing exits (boundary drain, fatal lex propagation, trip, stop, \
     fatal report propagation); a sixth committing exit needs its own `M::on_commit` \
     call, and a rewinding exit belongs to `on_eof` (grep RELEASE_CENSUS)."
  );
  assert!(
    count(scan, "M::on_eof(") == 1,
    "RELEASE_CENSUS drift: `skip_until` must have exactly one rewinding exit \
     (grep RELEASE_CENSUS)."
  );
  // The trait method and its four mode impls (the balanced mode delegates to
  // `SyncThrough`'s, which is the one body that releases).
  assert!(
    count(scan, "fn on_commit(") == 5,
    "RELEASE_CENSUS drift: every `ScanMode` must define `on_commit` — a rewinding \
     mode releases its snapshot's mark, a committing mode holds none \
     (grep RELEASE_CENSUS)."
  );
}

/// RELEASE_CENSUS — the trait surface: `release` is declared once (defaulted no-op) and
/// forwarded once (the `&mut U` blanket impl — the W3 forwarding-gap class; the
/// conformance test in `tests/handler_coverage.rs` drives it).
#[test]
fn release_census_trait_declares_and_blanket_forwards() {
  assert!(
    count(EMITTER_MOD, "fn release(") == 2,
    "RELEASE_CENSUS drift: `Emitter::release` must appear exactly twice in \
     `emitter/mod.rs` — the defaulted declaration and the `&mut U` blanket forward. \
     A defaulted-not-forwarded method silently no-ops through `&mut` emitters \
     (grep RELEASE_CENSUS)."
  );
}

// ─────────────────────────────────────────────────────────────────────────────────────
// CAPTURE_WINDOW — between taking a capture (`save`, `Emitter::checkpoint`, `Checkpoint::new`)
// and the moment it has an owner able to settle it — a `Transaction`, a `StackedTransaction`, a
// pushed session point or savepoint, or the finished `Checkpoint`/`ThroughEntry` itself — nothing
// may allocate or otherwise unwind. `Checkpoint` has no `Drop` (it cannot reach the input or the
// borrowed emitter to settle itself), so an unwind inside that window drops the capture raw: no
// rollback, no commit, no `forget_kept_checkpoint`, no release — a stranded lineage entry, a
// stranded pin, and a stranded emitter mark, with nothing left that knows any of the three exist.
//
// The fix is a preflight taken before the capture: reserve the destination container's slot, or —
// where the fallible step is a caller-supplied clone rather than a container push — hoist that
// clone above the capture instead. Either way, once the capture is taken, its landing is either
// already-reserved capacity or has nothing fallible left ahead of it. Eight sites close a window
// this way:
//
// - `guard_with` (so `begin`/`begin_with` and both attempts) and `begin_stacked_with` each pin
//   their captured begin point into the lineage's pin stack — a SECOND container, distinct from
//   the one `save` itself reserves — via `reserve_pin_slot`, called before `save`;
// - `begin_point` does the same, and ALSO lands its capture on the session's own point stack,
//   reserved via `session.points.reserve(1)`, likewise before `save`;
// - `save_checkpoint` — the shared body every `save` funnels through — reserves the
//   live-checkpoint stack's slot with `reserve_open` before EITHER of its own two registrations
//   (the emitter mark, then the lineage entry that follows it);
// - `StackedTransaction::savepoint` reserves its slot in the savepoint stack before capturing;
// - `sync_through`, `sync_through_then_peek_with_emitter`, and `sync_balanced` take only an
//   emitter mark (no lineage entry, no pin, so no container to reserve): their preflight is a
//   hoist — the caller-supplied dedup-watermark clone moves above the mark, so nothing fallible
//   is left between the mark and the finished `ThroughEntry` that owns it.
//
// One capture is a documented exception rather than a ninth site: `rollback_to`'s re-save needs
// no reservation of its own, because the savepoint pops immediately above it already free the
// very slot its push lands in.
//
// A capture site added without its preflight is dropped raw by the very first unwinding
// insertion after it. Take the reservation (or the hoist) before the capture — a capture taken
// before its owner can accept it is stranded by an unwinding insertion — then extend this census
// in the same commit (grep CAPTURE_WINDOW).
// ─────────────────────────────────────────────────────────────────────────────────────

/// The lineage module source, censused for the two preflight-reservation primitives
/// `reserve_open` and `reserve_pin`.
const LINEAGE_MOD: &str = include_str!("../lineage.rs");

/// The code lines of `hay` rejoined without its `//`-prefixed lines — the same filter [`count`]
/// applies, kept as text rather than a count so a capture site's ordering can be checked
/// positionally without a comment that merely mentions a call site shifting what counts as its
/// first occurrence.
fn code_only(hay: &str) -> std::string::String {
  hay
    .lines()
    .filter(|line| !line.trim_start().starts_with("//"))
    .collect::<std::vec::Vec<_>>()
    .join("\n")
}

/// CAPTURE_WINDOW — the three lineage pin-stack captures: `guard_with` (hence `begin`/
/// `begin_with` and both attempts), `begin_stacked_with`, and `begin_point` each call
/// `reserve_pin_slot` before `save`, so the `pin_checkpoint` push that follows the capture lands
/// in already-reserved capacity. `begin_point` reserves a second, distinct container the same
/// way: the session's own point stack.
#[test]
fn capture_window_pin_stack_reservations_precede_their_save() {
  let mod_src = source("mod.rs");

  // Inventory: exactly three `self.save()` captures live in this file, one per pin-stack site.
  // A fourth capture anywhere in the censused sources is new and has not yet earned a preflight.
  let got = calls(mod_src, "save");
  assert!(
    got == 3,
    "CAPTURE_WINDOW drift: `mod.rs` has {got} `self.save()` capture(s), expected 3 \
     (`guard_with`, `begin_stacked_with`, `begin_point`). A capture taken before its owner can \
     accept it is stranded by an unwinding insertion — reserve the destination slot with \
     `reserve_pin_slot` BEFORE calling `save`, then extend this census in the same commit \
     (grep CAPTURE_WINDOW)."
  );
  for (name, src) in SOURCES {
    if *name == "mod.rs" {
      continue;
    }
    assert!(
      calls(src, "save") == 0,
      "CAPTURE_WINDOW drift: `{name}` gained a `self.save()` capture outside `mod.rs`. Reserve \
       its destination slot BEFORE the capture — a capture taken before its owner can accept it \
       is stranded by an unwinding insertion — then register the site in this census (grep \
       CAPTURE_WINDOW)."
    );
  }

  // Ordering, per site: the reservation must precede the capture it guards, not merely appear
  // somewhere in the same function.
  for method in ["guard_with", "begin_stacked_with", "begin_point"] {
    let body = code_only(&method_body(mod_src, method));
    let reserve_pos = body.find("self.reserve_pin_slot()").unwrap_or_else(|| {
      panic!(
        "CAPTURE_WINDOW drift: `InputRef::{method}` no longer reserves the pin-stack slot \
         before its capture. Take the reservation before the capture — a capture taken before \
         its owner can accept it is stranded by an unwinding insertion (grep CAPTURE_WINDOW)."
      )
    });
    let save_pos = body.find("self.save()").unwrap_or_else(|| {
      panic!(
        "CAPTURE_WINDOW: `InputRef::{method}` no longer captures via `self.save()`; update \
         this census (grep CAPTURE_WINDOW)."
      )
    });
    assert!(
      reserve_pos < save_pos,
      "CAPTURE_WINDOW drift: `InputRef::{method}` calls `reserve_pin_slot` AFTER `save` rather \
       than before. Take the reservation before the capture — a capture taken before its owner \
       can accept it is stranded by an unwinding insertion (grep CAPTURE_WINDOW)."
    );
  }

  // `begin_point`'s second container: the session point stack, reserved before the same capture.
  let begin_point_body = code_only(&method_body(mod_src, "begin_point"));
  let points_reserve_pos = begin_point_body
    .find("self.session.points.reserve(1)")
    .unwrap_or_else(|| {
      panic!(
        "CAPTURE_WINDOW drift: `InputRef::begin_point` no longer reserves the session point \
         stack's slot before its capture. Take the reservation before the capture — a capture \
         taken before its owner can accept it is stranded by an unwinding insertion (grep \
         CAPTURE_WINDOW)."
      )
    });
  let save_pos = begin_point_body
    .find("self.save()")
    .expect("checked above: begin_point captures via `self.save()`");
  assert!(
    points_reserve_pos < save_pos,
    "CAPTURE_WINDOW drift: `InputRef::begin_point` reserves the session point stack's slot \
     AFTER `save` rather than before (grep CAPTURE_WINDOW)."
  );
}

/// CAPTURE_WINDOW — `save_checkpoint`, the shared body every `save` funnels through, reserves
/// the live-checkpoint stack's slot with `reserve_open` before EITHER of its own two
/// registrations: the emitter mark it takes, then the lineage entry `open` records into the
/// slot just reserved.
#[test]
fn capture_window_save_checkpoint_reserves_before_either_registration() {
  let body = code_only(&method_body(source("mod.rs"), "save_checkpoint"));

  let reserve_pos = body
    .find("self.session.lineage.reserve_open()")
    .unwrap_or_else(|| {
      panic!(
        "CAPTURE_WINDOW drift: `InputRef::save_checkpoint` no longer reserves the \
         live-checkpoint stack's slot before its registrations. Take the reservation before the \
         capture — a capture taken before its owner can accept it is stranded by an unwinding \
         insertion (grep CAPTURE_WINDOW)."
      )
    });
  let emitter_pos = body
    .find("self.session.emitter.checkpoint()")
    .expect("CAPTURE_WINDOW: `save_checkpoint` no longer takes an emitter mark");
  let open_pos = body
    .find("self.session.lineage.open()")
    .expect("CAPTURE_WINDOW: `save_checkpoint` no longer opens a lineage entry");

  assert!(
    reserve_pos < emitter_pos && reserve_pos < open_pos,
    "CAPTURE_WINDOW drift: `InputRef::save_checkpoint` takes a registration — the emitter mark \
     or the lineage entry — before reserving the live-checkpoint stack's slot. Take the \
     reservation before the capture — a capture taken before its owner can accept it is \
     stranded by an unwinding insertion (grep CAPTURE_WINDOW)."
  );
}

/// CAPTURE_WINDOW — `StackedTransaction::savepoint` reserves its slot in the savepoint stack
/// before capturing. `rollback_to`'s re-save is the one documented exception: the savepoint pops
/// immediately above it already free the slot its push lands in, so it needs — and must not
/// grow — a reservation of its own.
#[test]
fn capture_window_savepoint_reservation_precedes_its_save() {
  let stacked_src = source("stacked/mod.rs");

  // Inventory: exactly two `self.input.save()` captures in this file — `savepoint`'s fresh mark
  // and `rollback_to`'s re-save. A third capture anywhere in the censused sources is new and has
  // not yet earned its preflight treatment.
  let got = count(stacked_src, "self.input.save()");
  assert!(
    got == 2,
    "CAPTURE_WINDOW drift: `stacked/mod.rs` has {got} `self.input.save()` capture(s), expected \
     2 (`savepoint`, `rollback_to`). A capture taken before its owner can accept it is stranded \
     by an unwinding insertion — reserve its destination slot BEFORE the capture, then extend \
     this census in the same commit (grep CAPTURE_WINDOW)."
  );
  for (name, src) in SOURCES {
    if *name == "stacked/mod.rs" {
      continue;
    }
    assert!(
      count(src, "self.input.save()") == 0,
      "CAPTURE_WINDOW drift: `{name}` gained a `self.input.save()` capture outside \
       `stacked/mod.rs` (grep CAPTURE_WINDOW)."
    );
  }

  // `savepoint`: the reservation must precede the capture.
  let savepoint_body = code_only(&method_body(stacked_src, "savepoint"));
  let reserve_pos = savepoint_body
    .find("self.saves.reserve(1)")
    .unwrap_or_else(|| {
      panic!(
        "CAPTURE_WINDOW drift: `StackedTransaction::savepoint` no longer reserves its slot \
         before its capture. Take the reservation before the capture — a capture taken before \
         its owner can accept it is stranded by an unwinding insertion (grep CAPTURE_WINDOW)."
      )
    });
  let save_pos = savepoint_body
    .find("self.input.save()")
    .expect("CAPTURE_WINDOW: `savepoint` no longer captures via `self.input.save()`");
  assert!(
    reserve_pos < save_pos,
    "CAPTURE_WINDOW drift: `StackedTransaction::savepoint` reserves its slot AFTER the capture \
     rather than before (grep CAPTURE_WINDOW)."
  );

  // `rollback_to`'s re-save is exempt by construction; a reservation appearing here would either
  // be dead weight or, worse, mask the day this exemption stops holding.
  let rollback_to_body = code_only(&method_body(stacked_src, "rollback_to"));
  assert!(
    !rollback_to_body.contains(".reserve("),
    "CAPTURE_WINDOW drift: `StackedTransaction::rollback_to` grew a reservation call. Its \
     re-save is exempt by construction — the savepoint pops immediately above it already free \
     the slot its push lands in; if that no longer holds, restructure the exemption rather than \
     adding a reservation silently (grep CAPTURE_WINDOW)."
  );
}

/// CAPTURE_WINDOW — the three sync-entry snapshots capture only an emitter mark (no lineage
/// entry, no pin, so no container to reserve): their preflight hoists the caller-supplied
/// dedup-watermark clone above the mark instead, so nothing fallible is left between the mark
/// and the `ThroughEntry` that owns it.
#[test]
fn capture_window_sync_entry_snapshots_hoist_the_fallible_clone_first() {
  let sites: &[(&str, usize)] = &[("sync_through.rs", 2), ("sync_balanced.rs", 1)];
  for (name, want) in sites {
    let code = code_only(source(name));
    let clone_positions: std::vec::Vec<usize> = code
      .match_indices("self.emitted_error_end.clone()")
      .map(|(i, _)| i)
      .collect();
    let mark_positions: std::vec::Vec<usize> = code
      .match_indices("self.session.emitter.checkpoint()")
      .map(|(i, _)| i)
      .collect();
    assert!(
      clone_positions.len() == *want && mark_positions.len() == *want,
      "CAPTURE_WINDOW drift: `{name}` has {} watermark clone(s) and {} emitter capture(s), \
       expected {want} of each — one `ThroughEntry` snapshot per sync entry point (grep \
       CAPTURE_WINDOW).",
      clone_positions.len(),
      mark_positions.len()
    );
    for (clone_pos, mark_pos) in clone_positions.iter().zip(mark_positions.iter()) {
      assert!(
        clone_pos < mark_pos,
        "CAPTURE_WINDOW drift: `{name}` takes an emitter mark before hoisting the \
         dedup-watermark clone above it. The clone is caller-supplied `Offset::clone` code that \
         may allocate; taken after the mark it becomes a fallible step between the capture and \
         its owner. Hoist the clone before the capture — a capture taken before its owner can \
         accept it is stranded by an unwinding insertion (grep CAPTURE_WINDOW)."
      );
    }
  }
}

/// CAPTURE_WINDOW — the reservation primitives themselves: `Lineage::reserve_open` and
/// `Lineage::reserve_pin` are each defined exactly once, so every call above names the one
/// reservation body this census locks the order against.
#[test]
fn capture_window_reservation_primitives_are_defined_once() {
  assert!(
    count(LINEAGE_MOD, "fn reserve_open(") == 1,
    "CAPTURE_WINDOW drift: `Lineage::reserve_open` must be defined exactly once, in \
     `lineage.rs` (grep CAPTURE_WINDOW)."
  );
  assert!(
    count(LINEAGE_MOD, "fn reserve_pin(") == 1,
    "CAPTURE_WINDOW drift: `Lineage::reserve_pin` must be defined exactly once, in `lineage.rs` \
     (grep CAPTURE_WINDOW)."
  );
}
