//! SETTLE_CENSUS / RELEASE_CENSUS / CAPTURE_WINDOW / RESUME_CENSUS / FRONT_CENSUS — the source
//! censuses of every place a committed token settles, every place an emitter checkpoint is spent
//! or forgotten, every place one is taken behind the preflight reservation that keeps an unwind
//! from stranding it, every place the (state, offset) pair a fresh lexer resumes from is built,
//! and every place the front of the token stream is reached.
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
//! **Every 1:1 consume settle goes through `commit_token`** — the nineteen sites:
//! `next()`'s and `next_or_stop()`'s cached and fresh-lex arms (4), `consume_cached_one` and
//! `consume_cached_to`'s loop body (2 — `consume_all_cached` drains *through*
//! `consume_cached_to`, per token), the `try_expect`/`try_expect_map`/
//! `try_expect_and_then`/`try_expect_or_stop`/`try_expect_map_or_stop` cached and accept
//! arms (10), the by-value `commit_probed` settle of a probed closer (1),
//! `SyncThrough::on_stop` (1), and the complete-input trivia skip's lexing phase (1).
//!
//! …**through one of its two entrances.** The body is a fallible clamp followed by an infallible
//! `settle_committed_token`, and the split is load-bearing: `commit_token` is handed a token its
//! caller already removed, so its clamp runs in the window where the token is out of the stream
//! and the position has not moved. `commit_front` is the other entrance, for a caller that removes
//! the token itself and can therefore clamp *first* — the trivia skip's resident phase, which
//! crosses a run of tokens rather than one and closes that window by order instead of by a scope.
//! Both entrances reach the side channel through the one infallible half, so the emitter hook
//! still has a single home, and the census locks the half to exactly those two callers.
//!
//! **The one skip settle is `AtFrontier::adopt`**, called only by `skip_and_report`: a
//! token a scan skips settles behind the frontier — not into the input's span — exactly
//! once, whichever origin fed it. It is the settle surface's second (and last) member.
//!
//! **The span funnel is not a settle.** `set_span_after_consume` is also written by
//! four non-token paths, and they must **never** grow a settle hook: `settle_fatal`
//! writes a *rejected lexer error's* span; `SyncTo::on_eof` and the complete-input trivia skip's
//! own end-of-input arm write the lexer's span at exhaustion; `commit_at` batch-writes a whole
//! skipped run's frontier (each skipped token already settled via `adopt`). Peeks, declines, `unconsume`/`hold_front`, the
//! front fetches (`take_front`, `take_front_if`), the peek fill's back push
//! (`cache_append`), and the position-write surgeries (`set_state`, restores) touch no
//! settle at all — a fetch is not a commit, and every caller of one still calls
//! `commit_token` itself.
//!
//! **The front of the stream has five primitives** (FRONT_CENSUS): `front`, `has_front`,
//! `take_front`, `take_front_if`, and `hold_front`. Every consume, peek, probe, put-back
//! and position read reaches the front through them, never through `Ctx::Cache` directly
//! — a path that talks to the cache sees a zero-capacity one as empty and re-lexes a token
//! that is parked right there. The `self.pending` counts the census pins per file are the
//! checklist: `mod.rs` 16 (the five primitives and the three cold parked halves they gate
//! into, the `park` write and the head guard above it, `resume` and its cold parked arm,
//! `cursor`, `offset`, the re-key, the restore), `peek/mod.rs` 4 (one accounting read and
//! the three window exits), `scan.rs` 1 (the fetch's origin probe), everywhere else 0.
//!
//! **Every hot-path probe of the slot is gated on `Cache::RETAINS_FRONT`**, through
//! `InputRef::can_park()`. Only a cache that can *refuse* a front push into an empty cache
//! ever parks anything, so under a retaining cache the slot is statically `None` and the
//! probe folds away at monomorphization. The gate counts are censused beside the slot
//! counts — `mod.rs` 8, `peek/mod.rs` 4, `scan.rs` 1 — so a new reader of the slot has to
//! decide its gating rather than inherit an ungated probe; the touches that are
//! deliberately ungated are the cold ones (the parked halves reached through a gate, the
//! `park` write, and the two drops in the re-key and the restore).
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
  // Added by 0.8.0's recursion budget and unregistered until R23 — the one non-test neighbour the
  // list had fallen behind. Every count it contributes is zero (it settles nothing, reaches no
  // front, captures no checkpoint), which is exactly what registering it pins: a settle or a front
  // read added here would now move a count instead of arriving unseen.
  ("descent.rs", include_str!("descent.rs")),
  // Added by the emitter-forwarding wall: the handle's `emit_*` / `cst_*` surface. Every count
  // it contributes is **zero**, and that is exactly what registering it pins — the four members
  // the input layer owns (`checkpoint`, `rewind`, `release`, `commit_token`) are deliberately not
  // forwarded, so a forwarder added for one of them would move a count here instead of arriving
  // unseen.
  ("emit.rs", include_str!("emit.rs")),
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
    // The complete-input trivia skip's LEXING phase: a token it lexed and accepted is a 1:1
    // consume like any other, and settles through the ordinary entrance. Its RESIDENT phase does
    // not appear here — it removes the token itself, so it clamps first and calls `commit_front`,
    // which is counted below.
    ("skip_while.rs", 1),
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

  // The settle has **two entrances and one body**, and the law above only stays checkable while
  // that is true. `commit_token` clamps and then calls the infallible half; `commit_front` — for a
  // caller that removes the token itself and therefore clamps BEFORE the removal — calls the same
  // half directly. Both are defined once, in `mod.rs`, and the half has exactly those two callers:
  // a third would be a settle that reached the side channel without joining either entrance.
  assert!(
    count(source("mod.rs"), "fn settle_committed_token(") == 1
      && count(source("mod.rs"), "fn commit_front(") == 1
      && calls(source("mod.rs"), "settle_committed_token") == 2,
    "SETTLE_CENSUS drift: the token settle's infallible half must be defined once and called \
     exactly twice — by `commit_token` (clamp, then settle) and by `commit_front` (the caller \
     clamped while the token was still in the stream). A third caller is a settle outside both \
     entrances (grep SETTLE_CENSUS)."
  );
  // …and the pre-clamped entrance is reached only by the complete-input trivia skip's resident
  // phase. A second caller must state why it can clamp before its own removal.
  for (name, src) in SOURCES {
    let want = usize::from(*name == "skip_while.rs");
    let got = calls(src, "commit_front");
    assert!(
      got == want,
      "SETTLE_CENSUS drift: `{name}` has {got} `commit_front` call site(s), expected {want}. \
       The pre-clamped settle entrance belongs to the trivia skip's resident phase; a new caller \
       must show that its clamp runs while the token is still in the stream, and join this census \
       in the same commit (grep SETTLE_CENSUS)."
    );
  }
}

/// SETTLE_CENSUS — **the sequence, not the remedy and not the type.**
///
/// The defect class that kept re-appearing at new addresses is a *sequence*:
///
/// > an **irreversible step** (a token leaves durable state, or half a position is installed),
/// > then a **fallible step** (a `Clone`, a `Drop` of a replaced value, a `to_owned`, a clamp, an
/// > emitter hook, a predicate), then the **settle** that makes it observable.
///
/// Interrupted in the middle, the irreversible step has happened and the settle has not: a token
/// is gone from the stream with the position behind it, or progress is published that the
/// observer was never told about.
///
/// Two earlier rails failed to see it, and both failures are instructive. One counted calls to
/// the funnel — a remedy census cannot detect the hazard anywhere the remedy was not applied. The
/// other scanned `InputRef`'s two fields — a type census cannot see the same shape at
/// `AtFrontier`. This one is keyed on the **text between** the irreversible step and its settle,
/// wherever that pair occurs.
///
/// The check: in every body that removes a token from the front of the stream, nothing between
/// the removal and the `commit_token` that settles it may clone, drop or otherwise run caller
/// code. Moves and destructuring are fine — they cannot unwind.
/// Every two-space-indented `fn` in `src`, with its body — the derivation the check below runs
/// over, so the set of inspected bodies comes from the source rather than from a hand-list.
fn all_method_bodies(src: &str) -> std::vec::Vec<(std::string::String, std::string::String)> {
  let mut out = std::vec::Vec::new();
  let lines: std::vec::Vec<&str> = src.lines().collect();
  for (i, line) in lines.iter().enumerate() {
    let Some(rest) = line.strip_prefix("  ") else {
      continue;
    };
    if rest.starts_with(' ') {
      continue;
    }
    let rest = rest
      .trim_start_matches("pub ")
      .trim_start_matches("pub(crate) ")
      .trim_start_matches("pub(super) ")
      .trim_start_matches("const ")
      .trim_start_matches("unsafe ");
    let Some(after) = rest.strip_prefix("fn ") else {
      continue;
    };
    let name: std::string::String = after
      .chars()
      .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
      .collect();
    if name.is_empty() {
      continue;
    }
    // Body: the lines strictly INSIDE the braces — the signature is excluded deliberately. A
    // census marker naming a method (`replace_position(`, `live_pop_through(`) otherwise matches
    // the declaration of the very method it names, and each self-match enters the roster as a
    // body that holds no window at all. Three did, padding a count that is supposed to be exact.
    let mut body = std::string::String::new();
    let mut started = false;
    for l in &lines[i..] {
      if !started {
        if let Some(brace) = l.find('{') {
          started = true;
          let tail = &l[brace + 1..];
          if !tail.trim().is_empty() {
            body.push_str(tail);
            body.push('\n');
          }
        }
        continue;
      }
      if *l == "  }" {
        break;
      }
      body.push_str(l);
      body.push('\n');
    }
    out.push((name, body));
  }
  out
}

/// The region of a body a settle rail is entitled to police.
///
/// Most bodies are policed whole. A body may instead declare a phase boundary — the token
/// `CENSUS_PHASE_BOUNDARY` in a comment — and then only the text below it is censused. That is not an escape
/// hatch: it is how `restore_unchecked` states, checkably, that everything running caller code
/// happens while the input is still wholly on the branch being abandoned, and that nothing below
/// the line does. Both group-C rails honour the same marker, so a body cannot be strict for one
/// and lax for the other.
fn censused_region(raw_body: &str) -> std::string::String {
  // A token that cannot occur in prose ABOUT the boundary. Spelled `PHASE 2`, the first match
  // landed in the paragraph explaining the split, which sits above phase 1 — so the "policed"
  // region silently grew to include the phase the split exists to exclude.
  match raw_body.find("CENSUS_PHASE_BOUNDARY") {
    Some(at) => code_only(&raw_body[at..]),
    None => code_only(raw_body),
  }
}

/// SETTLE_CENSUS — the **second** window shape: a restore in progress.
///
/// The twelve-window census next door guards one shape — *a token leaves durable state, then it
/// is settled* — and it is keyed on token-removal markers. It is easy to cite as "the settle
/// window census", and doing so is a mistake: the enumeration named five groups and that check
/// sees group A/B only. Group C is a different shape entirely:
///
/// > **a restore mutates lineage or position, then the rest of the restore completes.**
///
/// Interrupted in the middle, the checkpoint has been invalidated — session points settled, the
/// lineage popped through it — while position, cache and emitter still sit on the abandoned
/// branch. The mark is then stranded and every later rollback reasons from a lineage that
/// disagrees with the input. That is a serious defect this project shipped, because group C's
/// fixes were verified by individual cells and cells do not generalize: a new instance appeared
/// at a site the enumeration had already named and marked fixed.
///
/// So: same discipline, second arm. Once a restore has begun mutating, nothing in the rest of the
/// body may run caller code — no clone, no `to_owned`. The deliberate trailing `drop` of a
/// replaced pair is not caller code *in* the window; it is what the window exists to defer.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn settle_census_nothing_fallible_once_a_restore_has_begun() {
  // A restore opens its window at the first step that cannot be taken back.
  // The FACT writes — the steps that move the input onto the restored branch. Deliberately not
  // "any mutation": clearing a cache or abandoning a superseded session point changes nothing an
  // observer can see (a memo re-lexes; an abandoned point is being discarded anyway), so those
  // are teardown, not facts, and a body may run them freely before it starts installing.
  const MUTATIONS: &[&str] = &[
    "live_pop_through(",
    "replace_position(",
    "install_position(",
    "restore_cache_pushes(",
    "emitter().rewind(",
    "rekey_offset_facts(",
    "install_rekey(",
    "core::mem::replace(self.emitted_error_end",
    "core::mem::replace(self.poison_boundary",
    // The state replacement is a FACT WRITE, and leaving it off this list was the exact cost of
    // sharpening the rule. While the rail said "no caller code after the first mutation" it did
    // not matter what a mutation was; once it said "between the first and last FACT write", every
    // omission from this list became a blind spot with no other symptom. `set_state` wrote the
    // state and then ran an `L::Offset::clone`, and the rail read clean.
    "core::mem::replace(self.state",
  ];

  // Two shapes of caller code, not one. The `.clone()` family is the obvious one. The second is
  // an in-place field assignment: `*place = value` installs the new value and then drops the
  // displaced one, and the displaced one here is an `L::Offset`, an `Option<L::Offset>` or a
  // whole `CachedToken` — token, span AND `L::State`. Every restore in this crate wrote at least
  // one fact that way, which is precisely how three of them ran caller `Drop` code mid-restore
  // while a census keyed only on `.clone()` reported them clean.
  const HAZARDS: &[&str] = &[
    ".clone()",
    ".to_owned()",
    "*self.pending = ",
    "*self.poison_boundary = ",
    "*self.emitted_error_end = ",
    "*self.state = ",
    "*self.span = ",
    // A clone nobody wrote. The position funnel takes `MaybeRef<L::Span>`, so passing `(&span)`
    // infers `MaybeRef::Borrowed` and `clamped_span` then clones the span to own it — caller code
    // appearing inside a restore purely from an inferred conversion, invisible to a rail that
    // greps for `.clone()`. A `[high]` was exactly this, in a body the window rail had
    // just passed twice. Pass the span BY MOVE; an owned `MaybeRef` clones nothing.
    "((&",
    // A drop nobody wrote. `pop_back()`, `pop_front()`, `take()` and `pop()` all RETURN an owned
    // value; writing the call as a statement discards it, and discarding it IS the `Drop` call.
    // `clear()` is the same thing N times over. The values here own `L::Span` and `L::State` —
    // and, for a session point, a whole `Checkpoint` — so each is caller code, and none of it
    // appears as `.clone()`, `drop(` or an assignment. A later `[high]` was `pop_back();` in
    // a body two rails had just passed. Stage the value, or hoist the eviction above the phase
    // boundary.
    ".pop_back();",
    ".pop_front();",
    ".take();",
    ".pop();",
    ".clear();",
    // Evicting CALLEES. The markers above read the body's own text, so a body that hands the
    // eviction to someone else is invisible to them — moving `reconcile_cache_geometry` below the
    // boundary was not caught until these existed, because its `clear`/`pop_front` live in a free
    // function this rail never reads. Calling one of these IS a drop site, wherever it appears.
    // No trailing `(` — this one is called through a turbofish
    // (`reconcile_cache_geometry::<L, ...>(`), so a marker ending in `(` never matches it. The
    // teeth test is what found that; the rail read clean against a body that had moved the
    // eviction below the boundary.
    "reconcile_cache_geometry",
    "abandon_points_above(",
    "unwind_clear_stream(",
    // A FALLIBLE callee: the combined re-key wrapper clones the committed offset. Calling it is
    // safe only as the first fact a body writes — which is why it is both a fact write above and
    // a hazard here. A body that writes something else first and then re-keys is running caller
    // code with its own surgery half-done.
    "rekey_offset_facts(",
  ];

  // Windows that are open and known, each with the reason and the disclosure it belongs to. An
  // entry whose body no longer opens a window is itself a failure.
  // EMPTY, and that is an outcome rather than a starting point. This carried one
  // entry — `restore_entry`, whose post-restore cursor read was disclosed in the CHANGELOG as a
  // known limitation — on the reasoning that removing it meant deriving the cursor from the
  // entry, which is only sound while the store is drained. The value it needed turned out not to
  // need deriving at all: the entry's own span end IS the resumed position once the stream is
  // drained, both callers drain it, and a `debug_assert` at the site now states that where it can
  // be checked. Hoisting the read above the first mutation removed the window outright.
  const EXEMPT: &[(&str, &str, &str)] = &[];

  let mut exempted = 0usize;
  let mut roster = std::vec::Vec::new();

  for (file, src) in SOURCES {
    for (name, body) in all_method_bodies(src) {
      let body = censused_region(&body);
      let lines: std::vec::Vec<&str> = body.lines().collect();
      let fact = |i: &usize| MUTATIONS.iter().any(|m| lines[*i].contains(m));
      let Some(open) = (0..lines.len()).find(fact) else {
        continue;
      };
      // The region is first fact write THROUGH last, not first-to-end-of-body. A body that
      // finishes installing and then tears something down — `rekey_offset_facts` clears its cache
      // last, on purpose — has nothing left to tear, and a rail that ran to the closing brace
      // would call that a defect. The endpoints are included: a hazard sharing a line with a fact
      // write is exactly the borrow-inference case.
      let close = (0..lines.len()).rfind(fact).unwrap_or(open);
      if let Some((_, _, why)) = EXEMPT
        .iter()
        .find(|(f, m, _)| f == file && *m == name.as_str())
      {
        let _ = why;
        exempted += 1;
        continue;
      }
      roster.push(std::format!("{file}::{name}"));
      // Strictly BELOW the opening fact write. A marker that is both a fact write and a hazard —
      // `rekey_offset_facts`, which is safe as a body's FIRST write and unsafe as a later one —
      // would otherwise flag the body that uses it correctly. The two bodies where a hazard on
      // the opening line matters (`restore_entry`, `restore_unchecked`) are both policed whole by
      // the rail below, so nothing is lost by starting one line down.
      let window = lines[open + 1..=close].join("\n");
      for hazard in HAZARDS {
        assert!(
          !window.contains(hazard),
          "SETTLE_CENSUS drift: `{file}::{name}` runs `{hazard}` after the restore has begun \
           mutating. The checkpoint is already invalidated at that point, so a panic there \
           strands its emitter mark and leaves the lineage disagreeing with the input. Hoist it \
           above the first mutation, or take the value by reference as `restore_unchecked` does \
           for the cursor (grep SETTLE_CENSUS)."
        );
      }
    }
  }

  // ── Bodies whose window is the WHOLE body ────────────────────────────────────────────────
  //
  // The rail above opens each window at the first irreversible step, which is right for a body
  // that starts clean. It is wrong for one that is ALREADY inside somebody else's window: by the
  // time `restore_entry` is called, the scan has held an emitter mark since entry, and the rewind
  // at the end of this body is what settles it. A fallible step anywhere in the body — before the
  // first mutation as readily as after — skips that rewind and leaves `ScanScope::drop` to
  // RELEASE the mark instead, which keeps the abandoned scan's diagnostics forever.
  //
  // This is not hypothetical and it is not a tightening for its own sake: the first fix attempted
  // for `restore_entry` hoisted its clone to the top of the body, ABOVE the first
  // mutation, and the window rail above was satisfied by it. The behaviour was still wrong —
  // `r9_restore_entry_is_atomic_at_every_offset_clone` reported four diagnostics surviving — and
  // the operation had to leave the body altogether. A rail that only measures from the first
  // mutation cannot express that, so these bodies get the stricter one.
  const WHOLE_BODY: &[(&str, &str)] = &[
    ("mod.rs", "restore_entry"),
    // A checkpoint rollback is all-or-nothing by definition, so its atomic unit is the whole
    // function and not the stretch after its first mutation. Three consecutive fixes corrected an
    // ordering in this body and each left a later operation exposed — the window rail was
    // satisfied every time, because a tail is exactly what it cannot see.
    ("mod.rs", "restore_unchecked"),
  ];
  for (file, method) in WHOLE_BODY {
    let body = SOURCES
      .iter()
      .find(|(f, _)| f == file)
      .and_then(|(_, src)| {
        all_method_bodies(src)
          .into_iter()
          .find(|(n, _)| n == method)
          .map(|(_, b)| b)
      })
      .unwrap_or_else(|| {
        panic!(
          "SETTLE_CENSUS drift: `{file}::{method}` is gone; if the body was \
         renamed or inlined, move this entry with it (grep SETTLE_CENSUS)"
        )
      });
    let body = censused_region(&body);
    for hazard in HAZARDS {
      assert!(
        !body.contains(hazard),
        "SETTLE_CENSUS drift: `{file}::{method}` runs `{hazard}`. This body settles an emitter \
         mark that has been outstanding since scan entry, so EVERY fallible step in it — not \
         just the ones after its first mutation — can skip the rewind and turn a release into \
         permanent diagnostics from an abandoned branch. Move the operation to the capture, as \
         `ThroughEntry::rewind_to` did (grep SETTLE_CENSUS)."
      );
    }
  }

  // The exact roster, not a count and not a floor: a total catches an arrival, but only names
  // catch a swap — a window leaving the set while an unclassified one joins it holds the count
  // still. Every name here has been read and carries no fallible step after its first mutation.
  const EXPECTED: &[&str] = &[
    "mod.rs::commit_position",
    // `commit_token` is deliberately ABSENT, and its departure is the change that put
    // `settle_committed_token` here: the body is now a clamp followed by a call, and neither is a
    // fact write, so it holds no window of its own. The window moved into the half it calls.
    // The two INFALLIBLE install halves, both lit up by classifying the state replacement as a
    // fact write. Each is nothing but moves — `mem::replace`/`take` — so neither can run caller
    // code before its last fact lands; `install_rekey`'s trailing cache clear is teardown after
    // the writes, not between them. They belong on the roster because they are where the facts
    // move, and they pass because that is all they do.
    "mod.rs::install_position",
    "mod.rs::install_rekey",
    "mod.rs::rekey_offset_facts",
    // The clamp-then-install wrapper. Its region is the single `install_position` line, which is
    // the point: the fallible clamp above it is outside the region by construction.
    "mod.rs::replace_position",
    "mod.rs::restore_entry",
    "mod.rs::restore_unchecked",
    // The two public state-surgery entry points, inspected here for the first time. They were
    // never exempted by this rail — they were INVISIBLE to it, because nothing they call was a
    // marker. That is the more dangerous form of a blind spot: an exemption is at least written
    // down and countable, while an unlisted marker leaves no trace at all.
    "mod.rs::set_state",
    // The token settle's infallible half: `install_position`, the side-channel notification, and
    // the trailing drop of the pair it replaced. Its region is the single `install_position` line,
    // and everything after it is the deferral the window exists for.
    "mod.rs::settle_committed_token",
    "mod.rs::state_mut",
    // `unwind_clear_stream` is deliberately ABSENT. It drops the parked entry and clears the
    // cache and installs nothing at all, so there is no restored branch for an interrupted drop
    // to half-land on — it is teardown end to end, and the rail's own rule (no caller code
    // BETWEEN fact writes) has nothing to say about a body with no fact writes. It was on this
    // roster while the rule was the looser "after the first mutation", and the implicit-drop
    // marker promptly reported its `clear()` as a defect it is not.
  ];
  roster.sort_unstable();
  assert!(
    roster
      .iter()
      .map(std::string::String::as_str)
      .eq(EXPECTED.iter().copied())
      && exempted == EXEMPT.len(),
    "SETTLE_CENSUS drift: restore windows are {roster:?} with {exempted} exempt; expected \
     {EXPECTED:?} and {}. A restore path was added, removed, renamed or newly exempted — read \
     it, confirm nothing fallible runs after its first mutation, and update this roster in the \
     same commit (grep SETTLE_CENSUS).",
    EXEMPT.len()
  );
}

/// SETTLE_CENSUS — the settle window, over **every window**, not every body.
///
/// The set of windows is derived from the source. Two earlier revisions of this check failed in
/// ways worth keeping written down, because both looked correct:
///
/// - it carried a hand-list of three bodies while the enumeration said eight call sites shared
///   the window, so five consume paths were unguarded;
/// - it then derived the bodies but exempted `skip_until` **whole**, for a real false positive in
///   one of its three windows — silently dropping the other two, which are the scanner's own
///   hand-over and skip rails and its highest-risk code. Its guards did not catch
///   that: the exemption count was still exactly one, and a *floor* on the inspected count cannot
///   see a missing member. Only a total can.
///
/// So the unit here is the window — one removal paired with the settle that accounts for it —
/// exemptions are keyed to a single removal marker inside a single body, and the counts are
/// EXACT. An exact count fails both when a window disappears and when one appears unclassified,
/// which is what the floor was meant to do and could not. If it needs updating because a consume
/// path was added, that is the census working, and the update is a one-line diff under review.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn settle_census_nothing_fallible_between_a_removal_and_its_settle() {
  // A window opens when a token comes out of durable state...
  const REMOVALS: &[&str] = &[
    "take_front()",
    "take_front_if(",
    "take_front_parked()",
    ".hand_over()",
    ".take_recorded()",
  ];
  // ...and closes when that token is accounted for: settled, put back, or placed in the scope
  // slot that owns it across caller code.
  const SETTLES: &[&str] = &[
    "commit_token(",
    // The pre-clamped entrance to the same settle. `commit_token`'s own body is a clamp followed
    // by this call, and a caller that removes the token itself hoists the clamp above the removal
    // and lands here directly — which is precisely a window this rail must be able to close, so
    // the marker has to name it. (`settle_committed_token(` does not contain `commit_token(`.)
    "settle_committed_token(",
    "unconsume(",
    "hold_front(",
    "TokenSlot::Held(",
    "settle_stop(",
    "skip_one::",
  ];
  const HAZARDS: &[&str] = &[".clone()", "drop(", ".to_owned()"];

  // The primitives DEFINE a removal; they do not open a window over one.
  const PRIMITIVES: &[&str] = &[
    "take_front",
    "take_front_if",
    "take_front_parked",
    "take_front_if_parked",
    "hand_over",
    "take_recorded",
  ];

  // Exemptions are per WINDOW — (file, method, removal marker) — never per body. A body may hold
  // both a false positive and a real window, which is exactly how the scanner's two rails went
  // uninspected.
  const EXEMPT: &[(&str, &str, &str, &str)] = &[
    (
      "scan.rs",
      "skip_until",
      "take_front_parked()",
      "the parked fetch and the `TokenSlot::Held` placement are in different arms of one \
       `if/else`; the arm carrying a state clone is the LEXING arm, reached only when both \
       fetches returned nothing. A line-order rail reads the arms as a sequence",
    ),
    (
      "scan.rs",
      "skip_until",
      "take_front()",
      "the cache fetch, same `if/else` as above",
    ),
  ];

  let mut inspected = 0usize;
  let mut exempted = 0usize;

  for (file, src) in SOURCES {
    for (name, body) in all_method_bodies(src) {
      if PRIMITIVES.contains(&name.as_str()) {
        continue;
      }
      let body = code_only(&body);
      let lines: std::vec::Vec<&str> = body.lines().collect();

      // Every removal in this body, each its own window.
      for (at, line) in lines.iter().enumerate() {
        let Some(marker) = REMOVALS.iter().find(|m| line.contains(**m)) else {
          continue;
        };
        if let Some((_, _, _, why)) = EXEMPT
          .iter()
          .find(|(f, m, k, _)| f == file && *m == name.as_str() && k == marker)
        {
          let _ = why;
          exempted += 1;
          continue;
        }
        // The settle that closes it: the nearest one at or after this line. Same-line is the
        // common case for a nested call, and then the window is that single line.
        let Some(close) = (at..lines.len()).find(|i| SETTLES.iter().any(|s| lines[*i].contains(s)))
        else {
          panic!(
            "SETTLE_CENSUS drift: `{file}::{name}` takes a token out of the front of the stream \
             at line {at} of its body and never accounts for it — no settle, no put-back, no \
             scope slot (grep SETTLE_CENSUS)."
          );
        };
        inspected += 1;
        let window = lines[at..=close].join("\n");
        for hazard in HAZARDS {
          assert!(
            !window.contains(hazard),
            "SETTLE_CENSUS drift: `{file}::{name}` runs `{hazard}` between `{marker}` and the \
             settle that accounts for the token. That is caller code in the window where the \
             token has already left the stream and the position has not yet moved — a panic \
             there loses the token silently. Hoist it above the removal, or hand the value back \
             and run it after the settle (grep SETTLE_CENSUS)."
          );
        }
      }
    }
  }

  // EXACT, not a floor. A floor is satisfied by a derivation that omits a window, which is the
  // defect this check itself shipped.
  // 13 since the trivia skip left the scanner: `commit_front` is the thirteenth, and it is the
  // one window in the crate that is closed by ORDER rather than by a scope — its clamp runs in
  // its caller, above the removal, so the text between the removal and the settle is two moves.
  assert!(
    inspected == 13 && exempted == EXEMPT.len(),
    "SETTLE_CENSUS drift: {inspected} window(s) inspected and {exempted} exempt; expected 13 and \
     {}. A consume path was added, removed, or newly exempted — classify it and update these \
     numbers in the same commit. Do NOT relax this to a floor: a floor cannot tell a missing \
     window from a smaller codebase, which is how the scanner's two rails went uninspected \
     (grep SETTLE_CENSUS).",
    EXEMPT.len()
  );
}

/// SETTLE_CENSUS — **the hazard, not the remedy.** No code anywhere may write half of the
/// committed position.
///
/// The span and the lexer state that produced it are one fact. Written separately they can tear:
/// an unwind between the halves, or a caller that simply forgets the second line, leaves a
/// committed span paired with the lexer state of somewhere else — and a host that resumes then
/// lexes from the new offset under the old state. For a stateful lexer that is silent stream
/// corruption.
///
/// This census counts **raw assignments to either half**, which is the pattern the pair funnel
/// exists to prevent. Counting calls to the funnel instead would have been useless: a rail that
/// counts the remedy cannot see the hazard anywhere the remedy was not applied, which is how an
/// unrouted site goes unnoticed. Two raw writes are legitimate and both are named:
///
/// - `replace_position`'s own two `core::mem::replace`s — the one body that writes the pair;
/// - `set_state`'s single `*self.state =` — a deliberate position *surgery* on the state alone,
///   which does not advance the span and so cannot tear the pair.
///
/// Anything else is a half-pair write and fails here. The span half has no legitimate raw writer
/// at all: the span-only setters were deleted once the pair funnel existed, so "advance the span
/// without supplying the state" is no longer expressible.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn settle_census_no_half_pair_position_write() {
  for (name, src) in SOURCES {
    let raw_span = count(src, "*self.span = ") + count(src, "*ir.span = ");
    assert!(
      raw_span == 0,
      "SETTLE_CENSUS drift: `{name}` writes the committed span directly ({raw_span} site(s)). \
       A span advance must carry the lexer state that produced it — route it through \
       `commit_position`/`replace_position`, which take both halves (grep SETTLE_CENSUS)."
    );
    // No exemption. This used to allow `mod.rs` exactly one raw `*self.state = ` — `set_state`'s
    // "deliberate surgery, moves no span". That reasoning was sound about the SPAN pair and was
    // then honoured against every other hazard: the write it waved through drops the displaced
    // `L::State`, which is caller code, and it sat after a re-key that had already cleared the
    // cache, the parked slot and the poison latch. `set_state` now moves the state in with
    // `mem::replace` before the re-key and drops the displaced one at the end, so the site the
    // exemption existed for no longer needs it and the count is a flat zero everywhere.
    let raw_state = count(src, "*self.state = ") + count(src, "*ir.state = ");
    assert!(
      raw_state == 0,
      "SETTLE_CENSUS drift: `{name}` has {raw_state} raw `state` write(s), expected none. \
       `*place = value` drops the displaced state in place — caller code — so a state write \
       belongs in `replace_position`'s pair funnel, or in `set_state`'s `mem::replace` whose \
       displaced value is dropped once the surgery is whole (grep SETTLE_CENSUS)."
    );
  }
  // The span half stays singular: exactly one `mem::replace(self.span` in the crate, inside
  // `replace_position`. The state half is allowed a second — `set_state`'s state-only surgery —
  // and that is the whole of the narrowed exemption: it establishes "a state may move without a
  // span" and nothing else. Any THIRD state move, or a second span move, is a pair being torn.
  assert!(
    count(source("mod.rs"), "core::mem::replace(self.span") == 1
      && count(source("mod.rs"), "core::mem::replace(self.state") == 2,
    "SETTLE_CENSUS drift: the span move must live in `replace_position` alone, and the state \
     move in `replace_position` plus `set_state`'s state-only surgery — no third site"
  );
}

/// SETTLE_CENSUS — the position **pair**. A span and the lexer state that produced it are one
/// fact, and writing them separately can tear: an unwind between the halves leaves a position no
/// execution reaches, and the committing modes have no snapshot that could restore it. Every site
/// that writes both routes through `commit_position`, which computes both halves before writing
/// either and drops the values it replaces last.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn settle_census_position_pair_writes_through_one_body() {
  // (file, expected `commit_position` call sites, who they are)
  let expected: &[(&str, usize, &str)] = &[
    (
      "mod.rs",
      2,
      "`commit_at` (a scan's batch frontier write) and `settle_fatal` (a rejected error's span \
       with the lexer state that reached it). `commit_token` uses `replace_position` instead, \
       because it must notify the committed-token observer before the replaced pair's drops run",
    ),
    (
      "scan.rs",
      1,
      "`SyncTo::on_eof` (the lexer's span and state at exhaustion)",
    ),
    (
      "skip_while.rs",
      1,
      "the complete-input trivia skip's own end-of-input settle — the lexer's span and state at \
       exhaustion, the same pair `SyncTo::on_eof` writes for the scanned route",
    ),
  ];
  for (name, want, who) in expected {
    let got = calls(source(name), "commit_position");
    assert!(
      got == *want,
      "SETTLE_CENSUS drift: `{name}` has {got} `commit_position` call sites, expected \
       {want} ({who}). A new site that writes the span and the lexer state together must \
       route through this body and be listed here, in the same commit — writing the halves \
       separately is how the pair tears (grep SETTLE_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if expected.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      calls(src, "commit_position") == 0,
      "SETTLE_CENSUS drift: `{name}` writes the committed position outside the two censused \
       files (grep SETTLE_CENSUS)."
    );
  }
  assert!(
    count(source("mod.rs"), "fn commit_position(") == 1
      && count(source("mod.rs"), "fn replace_position(") == 1,
    "SETTLE_CENSUS drift: the pair's two bodies must each be defined exactly once, in `mod.rs`"
  );
  // The `#[must_use]` form, for callers doing more than a position write: they install the pair,
  // finish the rest of the restore, and only then let the replaced values' drops run.
  assert!(
    calls(source("mod.rs"), "replace_position") == 2,
    "SETTLE_CENSUS drift: `replace_position` has {} call sites, expected 2 — \
     `commit_position` (the immediate-drop case) and `restore_entry` (which restores more and \
     must drop the replaced pair last). `restore_unchecked` is deliberately NOT among them: it \
     needs the fallible clamp and the infallible install in different phases, so it calls \
     `clamped_span` and `install_position` separately — and `commit_token` is no longer among \
     them for the SAME reason, so that `commit_front` can hoist the clamp above its own removal \
     (grep SETTLE_CENSUS).",
    calls(source("mod.rs"), "replace_position")
  );
  // The pair's two moves stay in ONE body even though the funnel now has two entrances: the
  // split put the `mem::replace`s in `install_position` and left `replace_position` as the
  // clamp-then-install wrapper. Two callers, one place where the pair actually moves.
  assert!(
    count(source("mod.rs"), "fn install_position(") == 1
      && calls(source("mod.rs"), "install_position") == 3,
    "SETTLE_CENSUS drift: `install_position` must be defined once and called exactly three times \
     — by `replace_position` (the clamp-then-install path), by `restore_unchecked` and by \
     `settle_committed_token` (both of which clamp in their own earlier phase). A fourth caller \
     is a position write that skipped the clamp (grep SETTLE_CENSUS)."
  );
}

/// SETTLE_CENSUS — the committed-token side channel (`Emitter::commit_token`, the CST
/// auto-emission hook) rides exactly the two settle surfaces and nothing else: once
/// inside `InputRef::commit_token`'s body (the fourteen consume settles arrive through it)
/// and once inside `skip_and_report` beside the `adopt` skip settle. Peeks, declines,
/// `unconsume`, `settle_fatal`, `SyncTo::on_eof`, and `commit_at` never reach the
/// emitter's token channel — a hook appearing anywhere else double-emits or
/// phantom-emits, the silent wrong-tree class.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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

/// LEXER_ERROR_CENSUS — the **evidence** door onto a recording sink's gap-coverage law
/// (`Emitter::commit_lexer_error`) has exactly one caller in this crate: the input layer's own
/// deduped reporting site, `emit_lexer_error_deduped` in `mod.rs`. It carries the lexer's span
/// over bytes the layer just refused, and that span is the only thing that licenses a gap tile.
///
/// The census exists because the door was once `emit_lexer_error` — the *caller-facing* method —
/// and a caller-chosen span with nothing consumed for it could therefore excuse an uncovered
/// byte of the sink's own buffer, foreign spans included. So this locks two things at once: the
/// producer is the layer and nothing else, and the method stays **off** the two forwarding
/// surfaces (`emit.rs`, the handle's; `view.rs`, the callback's) beside `commit_token`, whose
/// withholding this exactly mirrors.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn lexer_error_census_the_evidence_door_is_the_input_layers_alone() {
  // The one producer: inside `emit_lexer_error_deduped`, on the crate-private `emitter()`.
  assert!(
    count(source("mod.rs"), "commit_lexer_error(") == 1,
    "LEXER_ERROR_CENSUS drift: `Emitter::commit_lexer_error` must be called exactly once in \
     mod.rs — inside `emit_lexer_error_deduped` (grep LEXER_ERROR_CENSUS)."
  );
  // Nowhere else in the input layer, and in particular NOT on either forwarding surface: a
  // forwarder here hands a caller the gap-licensing door back.
  for (name, src) in SOURCES {
    if *name == "mod.rs" {
      continue;
    }
    assert!(
      count(src, "commit_lexer_error(") == 0,
      "LEXER_ERROR_CENSUS drift: `{name}` reaches the emitter's gap-evidence channel. Only \
       the input layer's own deduped report may — a caller-chosen span licenses nothing \
       (grep LEXER_ERROR_CENSUS)."
    );
  }
  // The callback surface withholds it too, for the same reason it withholds `commit_token`:
  // the view is what a downstream crate can wrap under the orphan rules.
  const VIEW: &str = include_str!("../../emitter/view.rs");
  assert!(
    count(VIEW, "commit_lexer_error") == 0,
    "LEXER_ERROR_CENSUS drift: `EmitterView` forwards the gap-evidence door. A wrapper built \
     over the view could then license gaps in a sink's buffer with a foreign lexer error \
     (grep LEXER_ERROR_CENSUS)."
  );
  // The trait surface: declared once (defaulted to the diagnostic door) and blanket-forwarded
  // once — the same shape the settle and release censuses lock.
  assert!(
    count(EMITTER_MOD, "fn commit_lexer_error(") == 2,
    "LEXER_ERROR_CENSUS drift: `Emitter::commit_lexer_error` must appear exactly twice in \
     `emitter/mod.rs` — the defaulted declaration and the `&mut U` blanket forward \
     (grep LEXER_ERROR_CENSUS)."
  );
}

/// SETTLE_CENSUS — `AtFrontier::adopt` is the one skip settle, with exactly one caller
/// (`skip_and_report`). Every token a scan skips — trivia and recovery garbage alike,
/// cached or freshly lexed — settles behind the frontier through this single call, so a
/// side channel that hooks skipped tokens has exactly one home too.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn release_census_every_checkpoint_capture_is_paired() {
  // The four captures of an emitter mark.
  let captures: &[(&str, usize)] = &[
    // `save_checkpoint`, the single funnel behind save/guards/attempts/session points.
    ("mod.rs", 1),
    // `sync_through` + `sync_through_then_peek_with_emitter` entry snapshots.
    ("sync_through.rs", 2),
    // `sync_balanced`'s entry snapshot.
    ("sync_balanced.rs", 1),
    // The scanner's own entry capture for the COMMITTING modes, taken only under
    // `Cmpl::PARTIAL && !M::HOLDS_ENTRY` — their `Snapshot` is `()`, so the four facts their
    // `Incomplete` exit restores have nowhere else to live. It never monomorphizes on the
    // complete path, and it is settled by `on_incomplete` (abandon) or `keep_entry` (keep) on
    // every exit including the unwind edge.
    ("scan.rs", 1),
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

  // The two rewind spends, both in `mod.rs`: `restore_unchecked` for a `Checkpoint`, and
  // `restore_entry` for a rewinding scan's entry snapshot — the one body behind `on_eof`, behind
  // `on_incomplete`, and therefore behind the seventh return exit and the rewinding unwind edge
  // alike, so none of the four can drift from the others. `scan.rs` holds none: it names the
  // restore, it does not spell it.
  assert!(
    count(source("mod.rs"), "emitter().rewind(") == 2
      && count(source("scan.rs"), "emitter().rewind(") == 0,
    "RELEASE_CENSUS drift: `Emitter::rewind` must have exactly two callers, both in `mod.rs` — \
     `restore_unchecked` (a `Checkpoint`) and `restore_entry` (a scan's entry snapshot). A third \
     rewind site is a new abandon path; census it (grep RELEASE_CENSUS)."
  );

  // The three release homes: the kept-checkpoint funnel, the scanner's kept-snapshot
  // hook, and the session cell's abandoning drop (assert-free by necessity — it may run
  // mid-unwind). `release` is never called raw anywhere else in the input layer.
  // `scan.rs` holds three keep bodies. Two are per kind of scan capture: `ScanMode::on_commit`
  // for a rewinding mode's entry snapshot, and `ScanScope::keep_entry` for the committing modes'
  // Partial-only entry — used by the five keeping return exits and by the committing unwind edge,
  // which keeps its diagnosed prefix and therefore its emissions. The third is the scope's
  // last-resort settle: an exit takes the snapshot and then runs caller code to spend it, and if
  // THAT unwinds nothing else knows a mark is outstanding, so the scope releases the copy it kept
  // beside the snapshot. Release rather than rewind, because a half-run restore no longer
  // describes a cursor to rewind to.
  assert!(
    count(source("mod.rs"), "emitter().release(") == 1
      && count(source("scan.rs"), "emitter().release(") == 3
      && count(source("session.rs"), ".emitter.release(") == 1,
    "RELEASE_CENSUS drift: `Emitter::release` must have exactly five input-layer homes — \
     `forget_kept_checkpoint` (mod.rs), `ScanMode::on_commit`, `ScanScope::keep_entry` and the \
     scope's stranded-mark settle (scan.rs), and the session-abandon settle in \
     `Session::release_abandoned_points` (session.rs). Route a new keep path through one of them \
     (grep RELEASE_CENSUS)."
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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

/// RELEASE_CENSUS — the scanner's kept snapshots. `skip_until` has seven return exits: five
/// keep the scan's progress and settle the snapshot through `ScanMode::on_commit`, one
/// (`on_eof`) spends the mark by rewinding, and one (`on_incomplete`) abandons. The
/// **unwind edge** is the eighth, and its keeping arm settles through `on_commit` too — one
/// per disposition, not one per mode, which is the distinction that made `SyncBalanced`'s
/// interrupted stop take the wrong arm. A new exit must pick a disposition, in the same commit.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn release_census_scanner_snapshot_settles_on_every_exit() {
  let scan = source("scan.rs");
  assert!(
    count(scan, "M::on_commit(") == 6,
    "RELEASE_CENSUS drift: `skip_until` must settle the pre-call snapshot on each of \
     its five keeping return exits (boundary drain, fatal lex propagation, trip, stop, \
     fatal report propagation) plus the unwind edge's keeping arm; a further keeping exit \
     needs its own `M::on_commit` call, and an abandoning one belongs to `on_eof` / \
     `on_incomplete` (grep RELEASE_CENSUS)."
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn capture_window_sync_entry_snapshots_hoist_the_fallible_clone_first() {
  // `scan.rs` carries the fourth: the scanner's Partial-only capture for a committing mode,
  // which obeys the same hoist and is born directly into the `ScanScope` that owns it.
  let sites: &[(&str, usize)] = &[
    ("sync_through.rs", 2),
    ("sync_balanced.rs", 1),
    ("scan.rs", 1),
  ];
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
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
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

/// RESUME_CENSUS — the (state, offset) pair a fresh lexer resumes from is built in exactly
/// one body, from exactly one bundled source per arm.
///
/// The primary pin is structural, not textual: `Resume`'s fields are private, it has no
/// public constructor, and `lex_within_boundary`/`scan_with` accept nothing else — so a
/// driver holding a lexer built from one state beside an offset read from another no longer
/// type-checks. This census pins the residual that types cannot: that no *second* pairing
/// site appears, and that no arm of `resume` reads its two halves from two places.
///
/// Scoped to the input layer. The conformance kit's resume-faithfulness check builds its own
/// lexer with `L::with_state` + `bump`; it reads both facts from one captured item and lives
/// outside this module tree by design, so it is reviewed-and-exempt rather than counted here.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn resume_census_one_pairing_site() {
  let m = source("mod.rs");
  assert!(
    count(m, "L::with_state(") == 1,
    "RESUME_CENSUS drift: a lexer is constructed outside `resume_from`, so the \
     (state, offset) pair can be assembled from two independently-read facts. Route the \
     construction through a `Resume` constructor, then update this census in the same commit \
     (grep RESUME_CENSUS)."
  );
  assert!(
    count(m, "self.resume_from(") == 4,
    "RESUME_CENSUS drift: `resume_from` has exactly four callers — the cache-back, parked and \
     committed arms of `resume`, plus `resume_at_frontier`. A new arm must read BOTH halves of \
     the pair from one value, and say so here (grep RESUME_CENSUS)."
  );

  // Every arm of `resume` reads its offset from the same value it reads its state from, so
  // the committed state appears exactly once — in the arm that has nothing retained to read.
  let body = method_body(m, "resume");
  assert!(
    count(&body, "self.state.clone()") == 1,
    "RESUME_CENSUS drift: only the nothing-retained arm of `resume` may read the committed \
     state; any other arm reading it pairs a retained token's offset with a state from before \
     that token (grep RESUME_CENSUS)."
  );
  assert!(
    !body.contains("self.offset()"),
    "RESUME_CENSUS drift: `resume` must not read `offset()` — that is the second, \
     independently-read fact the one-value pairing exists to rule out (grep RESUME_CENSUS)."
  );

  // The scan takes the pair as ONE value — a `ResumeParts`, whose only constructor is
  // `Resume::parts_mut` — so a driver still cannot hand the lexing entry points a (lexer, offset)
  // pair it assembled itself.
  assert!(
    count(m, "parts: ResumeParts<") == 1,
    "RESUME_CENSUS drift: `scan_with` is the only `ResumeParts` consumer; a second lexing driver \
     must join it here (grep RESUME_CENSUS)."
  );
  let parts_literals: &[(&str, usize, &str)] = &[
    (
      "mod.rs",
      2,
      "the constructor in `Resume::parts_mut` + `scan_with`'s destructure",
    ),
    ("peek/mod.rs", 1, "the peek fill's destructure"),
  ];
  for (name, want, who) in parts_literals {
    let got = count(source(name), "ResumeParts {");
    assert!(
      got == *want,
      "RESUME_CENSUS drift: `{name}` names the `ResumeParts` literal {got} time(s), expected \
       {want} ({who}). The struct is the mutable view of a `Resume` and its fields are private to \
       this module tree; a new literal is a second place the two halves can be paired \
       (grep RESUME_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if parts_literals.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      count(src, "ResumeParts {") == 0,
      "RESUME_CENSUS drift: `{name}` builds a `ResumeParts` outside `Resume::parts_mut` and the \
       two lexing loops (grep RESUME_CENSUS)."
    );
  }

  // `parts_mut` is the one surface that can advance the two halves apart. Its callers are the
  // drivers that run a scan plus the peek fill; each hands the view straight to a lexing loop and
  // reads the `Resume` back only after that loop has written its position through.
  let parts_mut_sites: &[(&str, usize, &str)] = &[
    ("mod.rs", 2, "`next` + `next_or_stop`"),
    ("scan.rs", 1, "`skip_until`'s lexer arm"),
    (
      "try_expect.rs",
      6,
      "`try_expect_or_stop`, `probe_close`, and the four `*_on_input` bodies",
    ),
    ("peek/mod.rs", 1, "the peek fill"),
    (
      "skip_while.rs",
      1,
      "the complete-input trivia skip's lexing phase",
    ),
  ];
  let mut parts_mut_total = 0;
  for (name, want, who) in parts_mut_sites {
    let got = count(source(name), ".parts_mut(");
    parts_mut_total += got;
    assert!(
      got == *want,
      "RESUME_CENSUS drift: `{name}` calls `Resume::parts_mut` {got} time(s), expected {want} \
       ({who}) (grep RESUME_CENSUS)."
    );
  }
  assert!(
    parts_mut_total == 11,
    "RESUME_CENSUS drift: `Resume::parts_mut` has {parts_mut_total} callers across the input \
     layer, expected 11 — the ten scan drivers and the peek fill (grep RESUME_CENSUS)."
  );
  for (name, src) in SOURCES {
    if parts_mut_sites.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      count(src, ".parts_mut(") == 0,
      "RESUME_CENSUS drift: `{name}` splits a `Resume` into its halves outside the censused \
       lexing drivers (grep RESUME_CENSUS)."
    );
  }

  // The two-borrow lexing entry point is the residual the types no longer carry: `Resume` is
  // unforgeable, but `lex_within_boundary` takes the halves as plain `&mut`, so nothing but this
  // count stops a third caller from passing a lexer beside an offset it chose separately.
  let lex_sites: &[(&str, usize, &str)] = &[
    ("mod.rs", 2, "the definition + `scan_with`'s loop"),
    ("peek/mod.rs", 1, "the peek fill's loop"),
  ];
  for (name, want, who) in lex_sites {
    let got = count(source(name), "lex_within_boundary(");
    assert!(
      got == *want,
      "RESUME_CENSUS drift: `{name}` names `lex_within_boundary` {got} time(s), expected {want} \
       ({who}). It takes the lexer and the position as two separate `&mut` — which is what keeps \
       the position in a register — so its callers must reach them through `Resume::parts_mut` \
       and nowhere else (grep RESUME_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if lex_sites.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      count(src, "lex_within_boundary(") == 0,
      "RESUME_CENSUS drift: `{name}` lexes through `lex_within_boundary` outside the two censused \
       loops (grep RESUME_CENSUS)."
    );
  }

  // Nowhere else builds one: the fields are private to this module, so a `Resume` literal in
  // a sibling would be a second pairing site with the same reach.
  for (name, src) in SOURCES {
    if *name == "mod.rs" {
      continue;
    }
    assert!(
      count(src, "Resume {") == 0,
      "RESUME_CENSUS drift: `{name}` constructs a `Resume` outside `resume_from` \
       (grep RESUME_CENSUS)."
    );
  }
}

/// FRONT_CENSUS — the front of the token stream (the parked slot, else the cache front) is
/// reached through five primitives and nowhere else.
///
/// A consume path that talks to `Ctx::Cache` directly sees a zero-capacity cache as empty and
/// re-lexes a token that is parked right there — which is how the scanner's "an unconsumed token
/// lives at the front" promise was false under `()` for every put-back in the crate. The counts
/// below are the checklist; the module docs carry the per-file breakdown.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn front_census_one_front_surface() {
  let m = source("mod.rs");
  // (needle, expected count in `mod.rs`, the primitive that owns it)
  let homes: &[(&str, usize, &str)] = &[
    ("self.cache_mut().pop_front()", 1, "take_front"),
    ("self.cache_mut().pop_front_if(", 1, "take_front_if"),
    ("self.cache_mut().push_front(", 1, "hold_front"),
    ("self.cache().front()", 1, "front"),
    ("self.cache.push_back(", 1, "cache_append"),
  ];
  for (needle, want, home) in homes {
    let got = count(m, needle);
    assert!(
      got == *want,
      "FRONT_CENSUS drift: `mod.rs` has {got} `{needle}` site(s), expected {want} — it belongs \
       to `{home}` and nowhere else (grep FRONT_CENSUS)."
    );
  }

  // The parked slot itself: one count per file, so a bypass in a sibling fails here rather than
  // compiling quietly.
  // `mod.rs` gained three readers with the scanner's unwind edge: `has_front_parked` (which is
  // how `scan.rs` now asks, so its own count fell to zero), `unwind_clear_stream` (the rewinding
  // arm's stream restore), and `settle_fatal`'s store-empty guard — the invariant the unwind
  // settle leans on, asserted at the one site that could break it.
  let pending: &[(&str, usize)] = &[("mod.rs", 19), ("peek/mod.rs", 4)];
  for (name, want) in pending {
    let got = count(source(name), "self.pending");
    assert!(
      got == *want,
      "FRONT_CENSUS drift: `{name}` touches `self.pending` {got} time(s), expected {want}. The \
       parked slot is reached through the five front primitives; a new reader must justify \
       itself here, in the same commit (grep FRONT_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if pending.iter().any(|(n, _)| n == name) {
      continue;
    }
    assert!(
      count(src, "self.pending") == 0,
      "FRONT_CENSUS drift: `{name}` reaches the parked slot directly instead of through the \
       `front`/`has_front`/`take_front`/`take_front_if`/`hold_front` primitives \
       (grep FRONT_CENSUS)."
    );
  }

  // Everywhere but `mod.rs`: no direct front access, in either the field or the accessor
  // spelling — a submodule can write both.
  for (name, src) in SOURCES {
    if *name == "mod.rs" {
      continue;
    }
    for needle in [
      "cache.pop_front(",
      "cache_mut().pop_front(",
      "cache.pop_front_if(",
      "cache_mut().pop_front_if(",
      "cache.push_front(",
      "cache_mut().push_front(",
      "cache.front()",
      "cache().front()",
    ] {
      assert!(
        count(src, needle) == 0,
        "FRONT_CENSUS drift: `{name}` reaches the cache's front directly (`{needle}`) instead \
         of through the five front primitives (grep FRONT_CENSUS)."
      );
    }
  }

  // The peek fill is the one back push, and it is not a put-back; every one of its window exits
  // must put the parked token in front of the cached run, or a window comes back missing the
  // token at the front of the stream.
  let peek = source("peek/mod.rs");
  assert!(
    count(peek, "self.cache_append(") == 1,
    "FRONT_CENSUS drift: the back push belongs to the peek fill alone (grep FRONT_CENSUS)."
  );
  let exits = count(peek, "self.cache.peek::<W>(");
  let prepends = count(peek, "Maybe::Ref(parked.as_ref())");
  // FOUR exits, THREE prepends, and that is not a hole. Two of the exits — the overflow-free
  // fill and the one that rotates a staged region into place — are two tails of the *same*
  // block in `peek_lex_fill`, and the one prepend there precedes the branch between them, so it
  // heads the window on both. The other two exits (the resident-head arm and the latched
  // boundary) carry one each. What a count cannot say is "precedes", so the ordering is checked
  // below rather than assumed: a prepend that drifted *after* a copy would keep this count
  // exactly and produce the window this census exists to refuse.
  assert!(
    exits == 4 && prepends == 3,
    "FRONT_CENSUS drift: the peek fill has {exits} window exit(s) and {prepends} parked \
     prepend(s), expected 4 and 3 — every exit must head its window with the parked token, or a \
     window at a latched boundary comes back empty over a live, consumable token \
     (grep FRONT_CENSUS)."
  );
  let fill = method_body(peek, "peek_lex_fill");
  let prepend_at = fill
    .find("Maybe::Ref(parked.as_ref())")
    .expect("FRONT_CENSUS: the peek fill's lexing half must prepend the parked token");
  let first_copy = fill
    .find("self.cache.peek::<W>(")
    .expect("FRONT_CENSUS: the peek fill's lexing half must copy the cache into the window");
  assert!(
    prepend_at < first_copy && count(&fill, "Maybe::Ref(parked.as_ref())") == 1,
    "FRONT_CENSUS drift: the peek fill's lexing half prepends the parked token at or after its \
     first cache copy. Its two exits share one prepend precisely because that prepend runs \
     before the branch between them (grep FRONT_CENSUS)."
  );
}

/// FRONT_CENSUS — every hot-path probe of the parked slot is gated on
/// [`Cache::RETAINS_FRONT`](crate::cache::Cache::RETAINS_FRONT).
///
/// A cache that can retain a token never refuses a front push into an empty cache, and a refusal
/// is the only thing that parks one — so under such a cache the slot is provably `None` and the
/// gate deletes the probe at monomorphization. That is what keeps the slot free for the caches
/// that never park; an ungated probe hands every cache the zero-capacity cache's cost. Counted
/// beside the slot counts above so a new reader has to decide its gating in the same commit.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn front_census_every_probe_is_const_gated() {
  // The gate reads the const in one body and the contract-violation message names it in the other;
  // a third mention means a second place decides what the declaration implies.
  assert!(
    count(source("mod.rs"), "RETAINS_FRONT") == 2,
    "FRONT_CENSUS drift: `mod.rs` must name `RETAINS_FRONT` exactly twice — `can_park`'s read of \
     it and the `park` assert that reports a cache violating it. A new reader of the parked slot \
     gates on `Self::can_park()`, not on the const directly (grep FRONT_CENSUS)."
  );
  assert!(
    count(source("mod.rs"), "fn can_park(") == 1,
    "FRONT_CENSUS drift: `can_park` must be defined exactly once, in `mod.rs` \
     (grep FRONT_CENSUS)."
  );

  // (file, expected `Self::can_park()` gates, who they are)
  let gates: &[(&str, usize, &str)] = &[
    (
      "mod.rs",
      8,
      "`front`, `has_front`, `take_front`, `take_front_if`, `resume`, `cursor`, `offset`, and \
       `park`'s lying-cache assert",
    ),
    (
      "peek/mod.rs",
      4,
      "the window-slot accounting read and the three window exits",
    ),
    ("scan.rs", 1, "`skip_until`'s fetch"),
  ];
  for (name, want, who) in gates {
    let got = count(source(name), "Self::can_park()");
    assert!(
      got == *want,
      "FRONT_CENSUS drift: `{name}` gates {got} parked-slot probe(s) on `Self::can_park()`, \
       expected {want} ({who}). An ungated probe puts the zero-capacity cache's parked slot back \
       into every other cache's hot path (grep FRONT_CENSUS)."
    );
  }
  for (name, src) in SOURCES {
    if gates.iter().any(|(n, _, _)| n == name) {
      continue;
    }
    assert!(
      count(src, "Self::can_park()") == 0,
      "FRONT_CENSUS drift: `{name}` gates a parked-slot probe of its own; the slot is reached \
       through the five front primitives (grep FRONT_CENSUS)."
    );
  }
}

/// PEEK_FOOTPRINT — the peek fill's owned token storage is the caller's window and
/// nothing else.
///
/// `Token`, `State` and `Span` are unconstrained in size, so every `W::CAPACITY`-slot
/// store the fill builds costs `W::CAPACITY × (Token + State + Span)` of stack. Through
/// 0.7.3 it built a second one — a `GenericArray<MaybeUninit<_>, W::CAPACITY>` staging the
/// tokens lexed past the cache until the cache region had been copied into the output
/// buffer — so a cache miss reserved two full windows and a large token or lexer state
/// doubled a peek's frame. Those tokens are now staged in the output buffer itself and
/// rotated into place once the cache region is in, which is what makes the second store
/// unnecessary.
///
/// This census is what keeps it gone: a size assertion cannot see a new local, so the
/// only mechanical guard against the pattern coming back is refusing to let the fill body
/// name the things that build one. The size law itself is asserted at compile time beside
/// the oversized fixture in `peek/tests.rs` (grep PEEK_FOOTPRINT).
///
/// # It is syntactic, and that is the best available
///
/// The needles below are a string match, so a second window behind a `type` alias the
/// fill does not spell — `let mut spare = Staging::new();` — walks past them. That gap is
/// known and it is not closable here, because nothing the compiler checks can see the
/// property:
///
/// - a `const` assertion can read `size_of` of a **type**, never the frame of a
///   **function**; Rust exposes no frame size at compile time, so "this body reserves one
///   window" is not statable as a type-level fact.
/// - a runtime stack measurement was tried and rejected on the evidence recorded beside
///   the fixture: an unoptimized build materializes roughly seven window-sized copies
///   around the fill, so the term under test is under a seventh of the frame and any
///   threshold over it is a magic number a compiler release moves.
/// - `clippy::large_stack_frames` is threshold-based on the same frame, and the frame
///   legitimately scales with `W::CAPACITY × size_of::<Token>()` — it would fire on the
///   *single* correct window for a large-token monomorphization.
///
/// So this stays a syntactic guard that is honest about being one, rather than a
/// structural guard that is not structural. The `GenericArrayDeque::` and `.rotate_left(`
/// counts below are the file-scoped half, and they do bite an alias: whatever it is named,
/// a second window still has to be *constructed*, and the deque count is the shape a
/// construction in this file takes.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn peek_footprint_census_the_fill_owns_no_store() {
  let peek = source("peek/mod.rs");
  let fill = method_body(peek, "peek_with_emitter_inner");

  // Everything that can build a window-shaped store, plus `unsafe` — the fill needs
  // none of them now that the staged region lives in the buffer the caller owns, and
  // each is a way to reintroduce the second window.
  for needle in [
    "MaybeUninit",
    "GenericArray<",
    "GenericArrayDeque",
    "uninit(",
    "W::array",
    "W::deque",
    "Window::array",
    "Window::deque",
    "unsafe",
  ] {
    assert!(
      count(&fill, needle) == 0,
      "PEEK_FOOTPRINT drift: the peek fill names `{needle}`. The fill owns no token \
       storage of its own — tokens past the cache are staged in the caller's window and \
       rotated into place — so anything that reserves `W::CAPACITY` slots here is the \
       second owned window this was removed to close, and anything `unsafe` is the \
       partial-initialization bookkeeping that went with it (grep PEEK_FOOTPRINT)."
    );
  }

  // And the whole module, not just the fill: the staging array's `unsafe` was in a
  // helper beside the fill rather than in it, so a body-scoped count alone would not
  // have seen it go.
  let unsafes = count(peek, "unsafe");
  assert!(
    unsafes == 0,
    "PEEK_FOOTPRINT drift: `peek/mod.rs` names `unsafe` {unsafes} time(s), expected 0. \
     The only `unsafe` this module ever had was the staging array's partial-initialization \
     bookkeeping, which the single-window fill does not need (grep PEEK_FOOTPRINT)."
  );

  // One window per public entry point — the buffer each hands back — and no fourth.
  let built = count(peek, "GenericArrayDeque::");
  assert!(
    built == 3,
    "PEEK_FOOTPRINT drift: `peek/mod.rs` builds {built} deque(s), expected 3 — one per \
     public entry point (`peek_one`, `peek_with_emitter`, `peek_with_emitter_terminal`), \
     each the window it returns. A fourth is a store the caller never sees \
     (grep PEEK_FOOTPRINT)."
  );

  // The staged region is put back in stream order by one rotation, and that rotation is
  // the whole of the reordering: a second one would mean the fill is permuting something
  // it does not own.
  let rotations = count(peek, ".rotate_left(");
  assert!(
    rotations == 1,
    "PEEK_FOOTPRINT drift: `peek/mod.rs` rotates {rotations} time(s), expected 1 — the \
     single left-rotation that moves the staged region behind the front-of-stream region \
     (grep PEEK_FOOTPRINT)."
  );
}

// ─────────────────────────────────────────────────────────────────────────────────────
// POSTURE_CENSUS — the scan family's `# Panic unwind` sections, which are documentation and
// therefore invisible to every other gate in this repository.
//
// These paragraphs were once held together by a **byte-identity** requirement: edit one, copy it
// to the other two, confirm the three hash the same. That requirement was itself the defect. The
// paragraph it propagated was written for `sync_to` and opens "this method's `to`-shaped commit
// posture keeps the diagnosed prefix" — which is true of `sync_to` and false of the other two,
// whose own `ScanMode`s the very same sentence goes on to call *the rewinding scans*.
// `sync_through` consumes on a stop and rewinds at a no-match end of input; `sync_balanced`
// crosses the axes, keeping on a stop like the first and rewinding at end of input like the
// second. Three different postures cannot share one true sentence, so identity could only ever
// have been kept by making two of the three wrong.
//
// What byte-identity was really buying is anti-drift, and that survives the split as three checks:
//
//   (a) the ONE clause every member must share — the end-of-input-settle exclusion, which is a
//       property of the shared `ScanScope` (it disarms before the settle runs) and therefore
//       genuinely common. Dropping it from one file while editing another is the drift that
//       identity used to catch, and this is the half that still catches it;
//   (b) the four sections must stay pairwise DISTINCT, and no member may carry the retired
//       sentence. Re-unifying them is not a merge conflict, it produces no warning, and it is
//       exactly how the false claim spread the first time;
//   (c) each section must state the posture its own `ScanMode` ACTUALLY has, derived from that
//       mode's associated constants as they are written in `scan.rs`.
//
// (c) is not optional garnish, and (a) + (b) alone were a check that answers without looking:
// four paragraphs can carry the shared exclusion, avoid the retired sentence, and be pairwise
// different while every one of them describes the wrong method — permuting the four sections
// passes (a) and (b) outright. Distinctness is not correctness. (c) closes that by keying each
// file's required AND forbidden phrases to `HOLDS_ENTRY`, `COMMITS_FRONTIER_ON_STOP` and
// `REPORT_SKIPPED`, whose value triples are pairwise different across the four modes — so every
// permutation trips at least one phrase, and a future change to a constant forces the prose to
// change with it rather than silently outliving it.
//
// Both halves of (c) read TEXT, and text that talks about code reads like code — so both are read
// against the thing itself rather than against something that resembles it. On the source side,
// `scan.rs` is blanked of comments and of string, byte-string and character literals before any
// constant is taken from it, and the impl is brace-matched to its own top level: a comment spelling
// `const HOLDS_ENTRY: bool = true;`, a panic message quoting one, a neighbouring impl's constant and
// one declared inside a method body are all invisible to the reader. On the doc side a required
// phrase counts only where nothing negates it, because a substring survives its own denial —
// "**never** stops *before* the stopping token" contains "stops *before* the stopping token". A
// census a sentence can satisfy is the same unchecked claim this one exists to catch, one level up.
//
// The residual bounds worth naming:
//
//   - the constants are read from `scan.rs`'s SOURCE, because `ScanMode`'s constants cannot be
//     named without instantiating `L`/`Ctx`/`Cmpl`, and the only concrete lexers in this crate live
//     behind the logos features that this census module deliberately does not require. So the
//     coupling is textual: adding a FOURTH posture axis to `ScanMode`, or changing
//     `ScanScope::keeps_on_unwind`'s formula, is not caught by the reader itself — the
//     `keeps_on_unwind` pin in (c) covers the second, and the first is what the `POSTURE_PHRASES`
//     doc comment asks the next editor to extend. Blanking and brace-matching close what a
//     substring reader can be *fed*; what it can be *shown* — a `cfg`-gated second arm, a
//     macro-generated impl — is closed only by parsing `scan.rs` as Rust, and that is deliberately
//     not here;
//   - negation is scoped to the phrase's own clause (back to the nearest comma, semicolon or
//     sentence end), so a denial on the far side of one does not disqualify an occurrence. And only
//     the REQUIRED side is negation-aware: a forbidden phrase trips the census however it is
//     worded, including inside a true contrast ("this does not consume the stopping token"). Both
//     of those are the conservative direction — a section may not lean on a negated requirement,
//     and a forbidden posture is reworded rather than denied.
//
// Prose is compared with `///` stripped and whitespace collapsed, so rewrapping a paragraph is
// not drift and does not fail this. A new scan mode with a `# Panic unwind` section joins the
// list below in the same commit (grep POSTURE_CENSUS).
// ─────────────────────────────────────────────────────────────────────────────────────

/// The four scan-family members censused for posture, each paired with the
/// [`ScanMode`](super::scan::ScanMode) whose constants decide its posture. The postures are
/// pairwise different and each is stated on its own method.
const SYNC_POSTURE_SOURCES: &[(&str, &str)] = &[
  ("sync_to.rs", "SyncTo"),
  ("sync_through.rs", "SyncThrough"),
  ("sync_balanced.rs", "SyncBalanced"),
  ("skip_while.rs", "SkipWhile"),
];

/// The posture vocabulary: one canonical phrase per [`ScanMode`](super::scan::ScanMode) constant
/// per value — `(constant, phrase when true, phrase when false)`.
///
/// A method **requires** the phrase for the value its own mode declares and is **forbidden** the
/// phrase for the other, so no two of the four sections can be swapped: the triples
/// `SyncTo (false, true, true)`, `SyncThrough (true, false, true)`,
/// `SyncBalanced (true, true, false)` and `SkipWhile (false, true, false)` differ pairwise in at
/// least one axis, and every permutation therefore trips at least one required-or-forbidden
/// phrase. Distinctness alone could not do this — four paragraphs can be pairwise different and
/// all four wrong.
///
/// What each axis means on the unwind edge, which is what the sections describe:
///
/// - `HOLDS_ENTRY` — whether the mode holds a pre-call snapshot. `false` makes
///   `ScanScope::keeps_on_unwind` fold to `true`, so **every** unwind keeps and there is no
///   abandon arm; `true` gives the split, whose abandoning half is the mode's own no-match
///   end-of-input rewind;
/// - `COMMITS_FRONTIER_ON_STOP` — whether a stop settles *before* the token (leaving the frontier
///   as what a stop commits) or consumes it (committing at its own span instead);
/// - `REPORT_SKIPPED` — whether the prefix whose fate an unwind decides was diagnosed per token
///   or merely skipped.
const POSTURE_PHRASES: &[(&str, &str, &str)] = &[
  (
    "HOLDS_ENTRY",
    "rewinds the full pre-call state at a no-match end of input",
    "every unwind keeps",
  ),
  (
    "COMMITS_FRONTIER_ON_STOP",
    "stops *before* the stopping token",
    "consumes the stopping token",
  ),
  (
    "REPORT_SKIPPED",
    "diagnosed prefix",
    "skipped, not diagnosed",
  ),
];

/// `hay` with every comment and every string, byte-string and character literal blanked to spaces,
/// newlines kept so line structure survives.
///
/// The posture census reads `scan.rs`'s associated constants textually, and text *about* code reads
/// exactly like code to a substring search: a comment spelling `const HOLDS_ENTRY: bool = true;`, or
/// a panic message quoting one, would answer the reader before the declaration does. A census a
/// sentence can satisfy is the same unchecked claim this module exists to catch, one level up — so
/// the prose goes first and the reader sees only what the compiler sees.
///
/// A scanner for the forms that can *contain* something mistakable for code, and nothing else — not
/// a Rust parse (grep POSTURE_CENSUS).
fn without_prose(hay: &str) -> std::string::String {
  let src = hay.as_bytes();
  let mut out = std::vec::Vec::with_capacity(src.len());
  let mut i = 0;
  while i < src.len() {
    let rest = &src[i..];

    // `//`, `///` and `//!` alike, to the end of the line — the newline itself left in place.
    if rest.starts_with(b"//") {
      while i < src.len() && src[i] != b'\n' {
        out.push(b' ');
        i += 1;
      }
      continue;
    }

    // `/* .. */`, nesting the way Rust's do.
    if rest.starts_with(b"/*") {
      let mut depth = 0usize;
      while i < src.len() {
        if src[i..].starts_with(b"/*") {
          depth += 1;
          out.extend_from_slice(b"  ");
          i += 2;
        } else if src[i..].starts_with(b"*/") {
          depth = depth.saturating_sub(1);
          out.extend_from_slice(b"  ");
          i += 2;
          if depth == 0 {
            break;
          }
        } else {
          out.push(if src[i] == b'\n' { b'\n' } else { b' ' });
          i += 1;
        }
      }
      continue;
    }

    // `r"…"`, `r#"…"#` and the byte forms — but only where the `r` starts a token, so the `r` that
    // ends `for` or `ptr` cannot open a literal that swallows the rest of the file.
    let raw = if rest.starts_with(b"br") {
      2
    } else if rest.starts_with(b"r") {
      1
    } else {
      0
    };
    if raw > 0 && (i == 0 || !(src[i - 1].is_ascii_alphanumeric() || src[i - 1] == b'_')) {
      let hashes = rest[raw..].iter().take_while(|c| **c == b'#').count();
      if rest.get(raw + hashes) == Some(&b'"') {
        out.resize(out.len() + raw + hashes + 1, b' ');
        i += raw + hashes + 1;
        while i < src.len() {
          let closes = src[i] == b'"'
            && src[i + 1..]
              .iter()
              .take(hashes)
              .filter(|c| **c == b'#')
              .count()
              == hashes;
          if closes {
            out.resize(out.len() + hashes + 1, b' ');
            i += hashes + 1;
            break;
          }
          out.push(if src[i] == b'\n' { b'\n' } else { b' ' });
          i += 1;
        }
        continue;
      }
    }

    // `"…"`, and `b"…"` too since the `b` is copied as the ordinary identifier byte it is.
    if src[i] == b'"' {
      out.push(b' ');
      i += 1;
      while i < src.len() && src[i] != b'"' {
        // A backslash takes the byte after it with it, so `\"` does not close the literal.
        if src[i] == b'\\' {
          out.push(b' ');
          i += 1;
        }
        if i < src.len() {
          out.push(if src[i] == b'\n' { b'\n' } else { b' ' });
          i += 1;
        }
      }
      if i < src.len() {
        out.push(b' ');
        i += 1;
      }
      continue;
    }

    // A `'` opens a character literal or a lifetime, and this file is mostly the latter (`'inp`,
    // `'_`): consuming a lifetime as a literal would blank live code. A literal is one escape or one
    // ASCII character between quotes; anything else falls through and only the quote is copied. A
    // non-ASCII character literal falls through too, and safely — no delimiter this reader cares
    // about is non-ASCII.
    if src[i] == b'\'' {
      let end = if src.get(i + 1) == Some(&b'\\') {
        // `'\n'`, `'\''`, `'\u{7f}'` — past the escaped byte, or past the `}` that closes a `\u`.
        let mut j = i + 2;
        if src.get(j) == Some(&b'u') {
          while j < src.len() && src[j] != b'}' {
            j += 1;
          }
        }
        Some(j + 1).filter(|j| src.get(*j) == Some(&b'\''))
      } else {
        Some(i + 2).filter(|j| src.get(*j) == Some(&b'\''))
      };
      if let Some(end) = end {
        while i <= end {
          out.push(b' ');
          i += 1;
        }
        continue;
      }
    }

    out.push(src[i]);
    i += 1;
  }
  // Every byte is either copied verbatim from a region whose bounds are ASCII, or an ASCII space or
  // newline put in a blanked one, so no multi-byte sequence is ever split.
  std::string::String::from_utf8(out).expect("blanking ASCII-delimited regions preserves UTF-8")
}

/// The **top level** of `mode`'s `ScanMode` impl in already-blanked `code`: what sits directly
/// inside the impl's own braces, every nested body dropped.
///
/// Brace-matched rather than cut at the next line-initial `impl`, and taken at depth one rather than
/// over the whole block, so neither a neighbouring impl's constant nor one declared inside a method
/// body can answer for this one. An associated constant of *this* impl is what the posture is
/// derived from, and depth one is exactly where those live (grep POSTURE_CENSUS).
fn scan_mode_impl_top_level(code: &str, mode: &str) -> std::string::String {
  let header = std::format!("ScanMode<'inp, L, Ctx, Lang, Cmpl> for {mode}\n");
  let at = code.find(header.as_str()).unwrap_or_else(|| {
    panic!(
      "POSTURE_CENSUS: `scan.rs` has no `impl .. ScanMode<'inp, L, Ctx, Lang, Cmpl> for {mode}` \
       ending its header line. If the impl's generic parameters were renamed or rewrapped, teach \
       this reader the new spelling (grep POSTURE_CENSUS)."
    )
  });
  let open = at
    + code[at..].find('{').unwrap_or_else(|| {
      panic!(
        "POSTURE_CENSUS: `{mode}`'s `ScanMode` impl header opens no body (grep POSTURE_CENSUS)"
      )
    });
  let mut depth = 0usize;
  let mut top = std::vec::Vec::new();
  for &byte in &code.as_bytes()[open..] {
    match byte {
      b'{' => depth += 1,
      b'}' => {
        depth -= 1;
        if depth == 0 {
          return std::string::String::from_utf8(top)
            .expect("brace bounds are ASCII, so the slices between them stay whole");
        }
      }
      _ if depth == 1 => top.push(byte),
      _ => {}
    }
  }
  panic!(
    "POSTURE_CENSUS: `{mode}`'s `ScanMode` impl body never closes. The reader brace-matches it, so \
     an unbalanced brace inside a construct it blanks — a literal form it does not know — would \
     read like this (grep POSTURE_CENSUS)."
  )
}

/// Reads one of [`ScanMode`](super::scan::ScanMode)'s associated constants out of `scan.rs` — the
/// implementation, not the prose — resolving the delegating form
/// (`<SyncTo as ScanMode<..>>::CONST`) that the composed modes are written with.
///
/// This is what makes the posture census derived rather than self-confirming: flip a constant in
/// `scan.rs` and the phrases the four `# Panic unwind` sections are held to flip with it, so the
/// doc fails until it is rewritten (grep POSTURE_CENSUS).
fn scan_mode_const(mode: &str, name: &str, depth: usize) -> bool {
  assert!(
    depth < 4,
    "POSTURE_CENSUS: resolving `{mode}::{name}` went four delegations deep — a cycle, or a \
     restructure this reader has to be taught (grep POSTURE_CENSUS)."
  );
  let code = without_prose(source("scan.rs"));
  let flat = scan_mode_impl_top_level(&code, mode)
    .split_whitespace()
    .collect::<std::vec::Vec<_>>()
    .join(" ");
  let needle = std::format!("const {name}: bool =");
  let value_at = flat.find(needle.as_str()).unwrap_or_else(|| {
    panic!(
      "POSTURE_CENSUS: `{mode}`'s `ScanMode` impl declares no `{name}`. The posture each \
       `# Panic unwind` section is checked against is derived from this constant, so renaming or \
       removing it has to reach `POSTURE_PHRASES` in `census_tests.rs` too (grep POSTURE_CENSUS)."
    )
  });
  let rest = &flat[value_at + needle.len()..];
  let end = rest
    .find(';')
    .expect("a `const` declaration ends in a semicolon");
  match rest[..end].trim() {
    "true" => true,
    "false" => false,
    // The composed modes delegate: `<SyncTo as ScanMode<'inp, L, Ctx, Lang, Cmpl>>::CONST`.
    delegated => {
      let target = delegated
        .strip_prefix('<')
        .and_then(|d| d.split_once(" as "))
        .map(|(target, _)| target.trim())
        .unwrap_or_else(|| {
          panic!(
            "POSTURE_CENSUS: `{mode}::{name}` is neither a `bool` literal nor a \
             `<Mode as ScanMode<..>>::{name}` delegation, but `{delegated}`. This census derives \
             the documented posture from these constants, so a third spelling has to be taught \
             here (grep POSTURE_CENSUS)."
          )
        });
      assert!(
        delegated.ends_with(std::format!("::{name}").as_str()),
        "POSTURE_CENSUS: `{mode}::{name}` delegates to a DIFFERENT constant (`{delegated}`). \
         Cross-wiring two posture axes would leave the doc census checking the wrong fact about \
         this method (grep POSTURE_CENSUS)."
      );
      scan_mode_const(target, name, depth + 1)
    }
  }
}

/// A censused file's `# Panic unwind` doc section as prose: `///` markers stripped and every run
/// of whitespace collapsed to one space, so the check is about what the section *says* and not
/// about where its lines happen to wrap.
fn panic_unwind_prose(name: &str) -> std::string::String {
  let lines: std::vec::Vec<&str> = source(name).lines().collect();
  let start = lines
    .iter()
    .position(|line| line.trim_start() == "/// # Panic unwind")
    .unwrap_or_else(|| {
      panic!("POSTURE_CENSUS: `{name}` has no `/// # Panic unwind` section (grep POSTURE_CENSUS)")
    });
  // To the next level-one heading, or out of the doc block — whichever comes first. A `## `
  // subsection belongs to this section and is kept.
  let end = lines[start + 1..]
    .iter()
    .position(|line| {
      let t = line.trim_start();
      !t.starts_with("///") || t.starts_with("/// # ")
    })
    .map_or(lines.len(), |offset| start + 1 + offset);
  lines[start..end]
    .iter()
    .map(|line| line.trim_start().trim_start_matches("///"))
    .collect::<std::vec::Vec<_>>()
    .join(" ")
    .split_whitespace()
    .collect::<std::vec::Vec<_>>()
    .join(" ")
}

/// The words that reverse the claim they stand in front of. Grammatical negators only: *rather*,
/// *instead* and *unlike* draw a contrast without denying the verb they precede, and reading them as
/// denials would reject sentences that are true.
const NEGATORS: &[&str] = &[
  "cannot", "neither", "never", "no", "none", "nor", "not", "nothing",
];

/// Whether `prose` **states** `phrase` — carries it at least once with nothing negating it.
///
/// A required phrase is checked with this rather than with `contains`, because a substring
/// survives its own denial: "**never** stops *before* the stopping token" contains "stops
/// *before* the stopping token", and a census satisfied by the sentence that contradicts it is
/// not a census. The forbidden side deliberately stays a plain `contains` — see POSTURE_CENSUS on
/// why that direction is the conservative one (grep POSTURE_CENSUS).
fn states(prose: &str, phrase: &str) -> bool {
  prose
    .match_indices(phrase)
    .any(|(at, _)| negator_before(prose, at).is_none())
}

/// The negator governing an occurrence at byte `at`: the first negating word between the start of
/// that occurrence's own clause and the occurrence itself.
///
/// A clause runs back to the nearest comma or semicolon, or to the end of the previous sentence. The
/// prose is whitespace-collapsed before it gets here, so a sentence ender is always a `.`, `!` or
/// `?` followed by exactly one space — which `tests.rs` and `Self::sync_to` never look like.
fn negator_before(prose: &str, at: usize) -> Option<&str> {
  let bytes = prose.as_bytes();
  let mut start = 0;
  let mut i = at;
  while i > 0 {
    i -= 1;
    let ends_sentence = matches!(bytes[i], b'.' | b'!' | b'?') && bytes.get(i + 1) == Some(&b' ');
    if matches!(bytes[i], b',' | b';') || ends_sentence {
      start = i + 1;
      break;
    }
  }
  prose[start..at].split_whitespace().find(|word| {
    // Markdown emphasis and backticks are part of the sentence, not of the word: `**not**` and
    // `*not*` negate exactly as `not` does. Trimming only the ENDS keeps `no-match` one word.
    let word = word
      .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '\'')
      .to_ascii_lowercase();
    NEGATORS.contains(&word.as_str()) || word.ends_with("n't")
  })
}

/// The tail a required-phrase failure adds when the section does carry the words but every
/// occurrence of them is negated — the case a plain `contains` used to pass.
fn negation_note(prose: &str, phrase: &str) -> std::string::String {
  prose
    .match_indices(phrase)
    .find_map(|(at, _)| negator_before(prose, at))
    .map_or_else(std::string::String::new, |negator| {
      std::format!(
        " (the words are there, but every occurrence of them is governed by \"{negator}\", which \
         states the opposite)"
      )
    })
}

/// POSTURE_CENSUS — the shared half: every scan that documents a panic unwind carves out the same
/// window, because the reason is the shared scope's, not any one mode's.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn posture_census_every_scan_carries_the_end_of_input_exclusion() {
  const EXCLUSION: &str = "**anywhere but the end-of-input settle**";
  for name in [
    "sync_to.rs",
    "sync_through.rs",
    "sync_balanced.rs",
    "skip_while.rs",
  ] {
    let prose = panic_unwind_prose(name);
    assert!(
      states(&prose, EXCLUSION),
      "POSTURE_CENSUS drift: `{name}`'s `# Panic unwind` section no longer carries the \
       exclusion `{EXCLUSION}`{}. `ScanScope` is disarmed by the end-of-input exit BEFORE that \
       exit's settle runs, so an unwind inside the settle is not an exit the scope handles — \
       true of every mode, which is why this clause is the one part of these sections that must \
       stay common. Restore it, or, if the scope's disarm ordering changed, change it in all \
       four and here (grep POSTURE_CENSUS).",
      negation_note(&prose, EXCLUSION)
    );
  }
}

/// POSTURE_CENSUS — the split half: each sync method states its **own** posture, and no two of
/// them state the same one. This is the replacement for the byte-identity requirement described
/// above, and it fails on precisely what identity used to require.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn posture_census_each_sync_states_its_own_unwind_posture() {
  // The retired sentence. True of `sync_to` alone, copied verbatim onto the two methods it names
  // in its own next clause as the rewinding scans, and load-bearing once the section around it
  // was narrowed. It is spelled out here so its return is a red test rather than a re-review.
  const RETIRED: &str = "this method's `to`-shaped commit posture";

  let prose: std::vec::Vec<(&str, std::string::String)> = SYNC_POSTURE_SOURCES
    .iter()
    .map(|(name, _)| (*name, panic_unwind_prose(name)))
    .collect();

  for (name, section) in &prose {
    const OWN: &str = "this method's own posture";
    assert!(
      states(section, OWN),
      "POSTURE_CENSUS drift: `{name}`'s `# Panic unwind` section no longer says it is stating \
       THIS method's own posture{}. The four scans settle an unwind differently — `sync_to` \
       commits, `sync_through` consumes on a stop and rewinds at a no-match end of input, \
       `sync_balanced` does one of each, `skip_while` does `sync_to`'s but reports nothing — so \
       a section that describes the family rather than its own method is describing the other \
       methods wrongly (grep POSTURE_CENSUS).",
      negation_note(section, OWN)
    );
    assert!(
      !section.contains(RETIRED),
      "POSTURE_CENSUS drift: `{name}`'s `# Panic unwind` section carries the retired claim \
       \"{RETIRED}\". It is true of `sync_to` only; `sync_through` and `sync_balanced` are the \
       rewinding scans that same sentence names. State the method's own posture instead \
       (grep POSTURE_CENSUS)."
    );
  }

  for (i, (name, section)) in prose.iter().enumerate() {
    for (other, other_section) in &prose[i + 1..] {
      assert!(
        section != other_section,
        "POSTURE_CENSUS drift: `{name}` and `{other}` now carry the SAME `# Panic unwind` \
         section. These three used to be required to match byte for byte, and that requirement \
         is what put `sync_to`'s commit posture on two methods that do not have it. Their \
         postures differ, so their paragraphs must (grep POSTURE_CENSUS)."
      );
    }
  }
}

/// POSTURE_CENSUS — the binding half: each section must state the posture its own `ScanMode`
/// **actually has**, read out of `scan.rs`.
///
/// The two checks above answer without looking at behaviour. "Says `this method's own posture`",
/// "does not carry the retired sentence" and "pairwise distinct" are all satisfied by four
/// paragraphs that are different from one another and wrong about all four methods — which is
/// exactly the failure mode this family kept producing. Distinctness is not correctness. This
/// check is the one that looks (grep POSTURE_CENSUS).
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn posture_census_each_sync_states_the_posture_its_scan_mode_has() {
  // The derivation the `HOLDS_ENTRY` axis rests on, pinned in place. `HOLDS_ENTRY = false` means
  // "every unwind keeps" only because THIS formula folds to `true` when it is; rewrite
  // `keeps_on_unwind` and the mapping below would go on demanding a sentence the scope no longer
  // implements, which is the same shape of unchecked claim this census exists to close.
  const KEEPS_ON_UNWIND: &str = "!M::HOLDS_ENTRY || (self.stop_decided && self.frontier.is_some())";
  assert!(
    without_prose(source("scan.rs")).contains(KEEPS_ON_UNWIND),
    "POSTURE_CENSUS drift: `ScanScope::keeps_on_unwind` is no longer \
     `{KEEPS_ON_UNWIND}`. The posture phrases keyed on `HOLDS_ENTRY` are derived from that \
     formula — `false` folds it to `true`, which is what \"every unwind keeps\" states — so a \
     change here invalidates `POSTURE_PHRASES` and every `# Panic unwind` section keyed on it \
     (grep POSTURE_CENSUS)."
  );

  for &(file, mode) in SYNC_POSTURE_SOURCES {
    let section = panic_unwind_prose(file);
    for &(constant, when_true, when_false) in POSTURE_PHRASES {
      let value = scan_mode_const(mode, constant, 0);
      let other = !value;
      let (required, forbidden) = if value {
        (when_true, when_false)
      } else {
        (when_false, when_true)
      };
      assert!(
        states(&section, required),
        "POSTURE_CENSUS drift: `{mode}::{constant}` is `{value}` in `scan.rs`, but `{file}`'s \
         `# Panic unwind` section never says so — it is missing \"{required}\"{}. Either the \
         section drifted off the behaviour, or the constant changed and the doc did not follow \
         it. Fix whichever is wrong; do not relax the phrase (grep POSTURE_CENSUS).",
        negation_note(&section, required)
      );
      assert!(
        !section.contains(forbidden),
        "POSTURE_CENSUS drift: `{file}`'s `# Panic unwind` section carries \"{forbidden}\", the \
         phrase for `{constant} = {other}` — and `{mode}::{constant}` is `{value}`. This is a \
         section describing a posture its own method does not have, which is precisely how the \
         `to`-shaped claim reached the two rewinding scans (grep POSTURE_CENSUS)."
      );
    }
  }
}

/// POSTURE_CENSUS — the callback/source vocabulary, not just the posture phrases.
///
/// The three checks above read what a section says about ITS OWN POSTURE — commit-vs-rewind,
/// stop-before-vs-consume, diagnosed-vs-skipped — and never look at the *list of callbacks* the
/// opening sentence blames a panic on. A section can pass every posture phrase and still name a
/// source its own method cannot produce: `skip_while` named "the expected-tokens closure" while
/// its only route to one passes an internal `|| None` that is never even called, because
/// `skip_and_report`'s sole `exp()` call site — `M::REPORT_SKIPPED.then(|| .. exp() ..)` — sits
/// behind a bool that is `false` for `SkipWhile`. `REPORT_SKIPPED` is exactly that gate, read out
/// of `scan.rs` as the other checks do, so this closes the gap with the same derivation rather
/// than a hand-authored per-file list: flip `REPORT_SKIPPED` on a mode and the requirement on its
/// section's callback list flips with it.
///
/// This does not attempt the general case (an arbitrary callback/source named in arbitrary prose).
/// It closes the one vocabulary item this finding was about, the way it can actually be closed —
/// derived from a real source-level gate, not asserted by fiat. `sync_balanced`'s "the classifier"
/// is a real, method-specific panic source with no `ScanMode` constant to derive a requirement
/// from (the classifier is `sync_balanced`'s own parameter, not part of the `ScanMode` trait), so
/// it is out of scope for a derived check and is not asserted here.
#[test]
#[cfg_attr(
  miri,
  ignore = "reads crate source and string-matches: no UB surface, and miri interprets every byte"
)]
fn posture_census_expected_tokens_closure_is_named_only_where_report_skipped() {
  const PHRASE: &str = "the expected-tokens closure";
  for &(file, mode) in SYNC_POSTURE_SOURCES {
    let section = panic_unwind_prose(file);
    let report_skipped = scan_mode_const(mode, "REPORT_SKIPPED", 0);
    if report_skipped {
      assert!(
        states(&section, PHRASE),
        "POSTURE_CENSUS drift: `{mode}::REPORT_SKIPPED` is `true` in `scan.rs`, so \
         `skip_and_report` calls `exp` on every token this method skips, but `{file}`'s \
         `# Panic unwind` section never names \"{PHRASE}\" as a panic source{}. A caller-supplied \
         `exp` that actually runs is a real panic source and belongs in the list \
         (grep POSTURE_CENSUS).",
        negation_note(&section, PHRASE)
      );
    } else {
      assert!(
        !section.contains(PHRASE),
        "POSTURE_CENSUS drift: `{file}`'s `# Panic unwind` section names \"{PHRASE}\" as a panic \
         source, but `{mode}::REPORT_SKIPPED` is `false` in `scan.rs` — `skip_and_report`'s one \
         call to `exp` sits inside `M::REPORT_SKIPPED.then(..)` and is never reached for this \
         mode, so this method has no such source to panic out of. This is the exact shape of the \
         inherited defect this census closes: a section naming a callback its own method cannot \
         invoke (grep POSTURE_CENSUS)."
      );
    }
  }
}
