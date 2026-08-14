#![allow(dead_code)]

//! Whether this process's local addresses may be read as offsets into a native stack.
//!
//! Two probes in this suite difference the addresses of two locals: `stack_per_level` reports the
//! difference as bytes of stack per nesting level, and `pratt_limit_unit_sink` asserts on it to
//! corroborate that the recursion tracker released every frame. Both readings hold only on one
//! contiguous native stack.
//!
//! # Why this is shared rather than written twice
//!
//! It already was written twice, and the copies drifted into the same defect: each decided the
//! question from `TOKORA_SANITIZER`, a variable only `ci/sanitizer.sh` exports.
//! `RUSTFLAGS=-Zsanitizer=address cargo test` leaves it unset, so a genuinely instrumented process
//! took the native-stack path — and `stack_per_level` printed fake-stack allocation spacing as
//! bytes per nesting level, because monotone addresses with a constant stride pass every other
//! check it has. Fixing one copy would have left the other, so there is one copy.
//!
//! # The shape of the decision
//!
//! Refusal is the default and evidence lifts it, not the other way round:
//!
//! * **Positive detections** — the compiler's own `sanitize=` cfgs for this build, recorded by
//!   `build.rs`; a build whose flags the compiler would not describe; `ci/sanitizer.sh`'s
//!   announcement; Miri; and a direct observation that frames are not being reused. Any of these
//!   refuses, and they refuse a *printed measurement* and an *assertion* alike.
//! * **The affirmation** — [`OPT_IN`]. Nothing in a stable-compiled test can prove the absence of
//!   instrumentation reaching it by a route `build.rs` cannot see, so a figure is printed only when
//!   an operator says the build is native. This is what makes the absence of a detection stop
//!   reading as the presence of a native stack.
//!
//! The affirmation cannot override a detection: [`measurement_refusal`] returns the detections
//! first and the missing affirmation only when there are none.

/// The operator's affirmation that this build is uninstrumented, required before any figure is
/// printed. Not a boolean flag: the value names what is being asserted.
pub const OPT_IN: &str = "TOKORA_STACK_PROBE";

/// The only accepted value of [`OPT_IN`].
pub const OPT_IN_VALUE: &str = "native";

/// The announcement `ci/sanitizer.sh` exports. Still read — it is the one signal that survives a
/// route `build.rs` cannot see — but no longer the only one, and no longer trusted in reverse.
pub const ANNOUNCE: &str = "TOKORA_SANITIZER";

/// AddressSanitizer's runtime configuration, which is where the fake stack is switched on.
pub const ASAN_OPTIONS: &str = "ASAN_OPTIONS";

/// The one `ASAN_OPTIONS` flag [`frames_reused`] can observe.
const FAKE_STACK_FLAG: &str = "detect_stack_use_after_return";

/// Everything the decision rests on, as plain data.
///
/// The live values are gathered by [`here`]; taking them as an argument is what gives every branch
/// a positive control that neither mutates the process environment nor requires an instrumented
/// build to run. The previous version of this logic had controls that injected `Some("address")`
/// straight into the helper, so they exercised the helper and never the lookup — which is why the
/// lookup could be wrong for three rounds while its controls stayed green.
#[derive(Clone, Copy, Debug)]
pub struct StackEvidence<'a> {
  /// The program is being interpreted, so a frame's locals are a virtual allocation.
  pub miri: bool,
  /// `sanitize=` as the compiler reported it for this build's flags, via `build.rs`.
  pub compiler_sanitizers: Option<&'a str>,
  /// This build has rustflags the compiler would not produce a cfg list for.
  pub build_flags_opaque: bool,
  /// [`ANNOUNCE`]'s value, if the runner set one.
  pub announced_sanitizer: Option<&'a str>,
  /// Two sequential calls to one function reported the same address for the same local.
  pub frames_reused: bool,
  /// [`OPT_IN`] is set to [`OPT_IN_VALUE`].
  pub affirmed_native: bool,
}

impl StackEvidence<'_> {
  /// The all-clear. Controls start here and break exactly one field, so each one names the single
  /// reason it expects to see.
  pub const fn native() -> Self {
    Self {
      miri: false,
      compiler_sanitizers: None,
      build_flags_opaque: false,
      announced_sanitizer: None,
      frames_reused: true,
      affirmed_native: true,
    }
  }
}

/// Every reason this build's local addresses are not native stack offsets. Empty means none found.
///
/// This is the set that refuses an **assertion** as well as a measurement: each entry is a
/// positive finding, not the absence of one, so applying it where the assertion would otherwise
/// run cannot silently skip an uninstrumented build.
pub fn instrumentation_refusals(e: StackEvidence<'_>) -> Vec<String> {
  let mut why = Vec::new();
  if e.miri {
    why.push(String::from(
      "miri: frame locals are separate virtual allocations, not offsets into a contiguous native \
       stack, so their differences are allocation spacing",
    ));
  }
  if let Some(list) = e.compiler_sanitizers {
    why.push(format!(
      "the compiler reports sanitize={list} for this build: a sanitizer inserts redzones around \
       every stack local, and may relocate a frame's locals onto a heap-backed fake stack, so the \
       address of a local is neither where its frame sits nor how large it is"
    ));
  }
  if e.build_flags_opaque {
    why.push(String::from(
      "the compiler would not report a cfg list for this build's rustflags, so build.rs could not \
       exclude instrumentation; a build that cannot be interrogated is not a build known to be \
       native",
    ));
  }
  if let Some(san) = e.announced_sanitizer.filter(|s| !s.is_empty()) {
    why.push(format!(
      "{ANNOUNCE}={san}: the runner announced a sanitizer-instrumented build"
    ));
  }
  if !e.frames_reused {
    why.push(String::from(
      "two sequential calls to one function reported different addresses for the same local, so \
       frames are not being reused out of a contiguous stack — the signature of a heap-backed \
       fake stack",
    ));
  }
  why
}

/// [`instrumentation_refusals`], plus the missing affirmation when nothing was detected.
///
/// The extra clause is the whole fix: a printed figure is silent damage when it is wrong, so it
/// needs a positive statement that the build is native rather than the mere absence of a signal
/// that it is not.
pub fn measurement_refusals(e: StackEvidence<'_>) -> Vec<String> {
  let mut why = instrumentation_refusals(e);
  if why.is_empty() && !e.affirmed_native {
    why.push(format!(
      "{OPT_IN}={OPT_IN_VALUE} is not set: nothing a stable-compiled test can read proves this \
       build is uninstrumented, and an unproven native stack is not a native stack"
    ));
  }
  why
}

/// [`instrumentation_refusals`] as one message.
pub fn instrumentation_refusal(e: StackEvidence<'_>) -> Option<String> {
  join(instrumentation_refusals(e))
}

/// [`measurement_refusals`] as one message.
pub fn measurement_refusal(e: StackEvidence<'_>) -> Option<String> {
  join(measurement_refusals(e))
}

fn join(why: Vec<String>) -> Option<String> {
  if why.is_empty() {
    None
  } else {
    Some(why.join("; "))
  }
}

/// Reads one local's address out of a frame of its own.
///
/// `black_box` is what forces the local to exist on the stack at all, and `inline(never)` is what
/// keeps the frame from being merged into the caller's — without both, the two calls in
/// [`frames_reused`] can collapse to one address for reasons that have nothing to do with the
/// stack layout being measured.
#[inline(never)]
fn frame_address() -> usize {
  let anchor = 0u8;
  let p: *const u8 = core::hint::black_box(&anchor);
  p as usize
}

/// Whether sequential calls to one function reuse one frame.
///
/// On a native stack the caller's stack pointer is unchanged between the calls, so the callee's
/// local lands at the same address every time. ASan's `detect_stack_use_after_return` machinery
/// moves a frame with address-taken locals onto a heap-backed fake stack and quarantines it on
/// return, so each call lands somewhere else — and Miri gives each frame its own virtual
/// allocation, likewise.
///
/// This is the one detector that observes the property rather than asking the build about it, so
/// it is what fires if instrumentation ever reaches this code by a route `build.rs` cannot see.
pub fn frames_reused() -> bool {
  let a = frame_address();
  let b = frame_address();
  let c = frame_address();
  a == b && b == c
}

/// Whether an `ASAN_OPTIONS` value switches the fake stack **on**.
///
/// Off is the default: ASan compiles the fake-stack machinery in but leaves it dormant until the
/// runtime is told otherwise, so `ASAN_OPTIONS` is what decides whether [`frames_reused`] can
/// observe anything at all under ASan.
///
/// Parsed the way the sanitizer's own parser reads it — any of space, comma, colon, tab, newline
/// or carriage return separates flags, and a repeated flag takes its **last** value — so this
/// answers for the string ASan itself will act on rather than for a simplified spelling of it.
/// Taken as an argument rather than read from the environment so its controls do not have to
/// mutate a process-wide variable while other tests are running.
pub fn asan_fake_stack_enabled(options: Option<&str>) -> bool {
  let Some(options) = options else {
    return false;
  };
  let mut on = false;
  for flag in options.split([' ', ',', ':', '\n', '\t', '\r']) {
    // `split_once`, so `detect_stack_use_after_return_2=1` is a different flag rather than a
    // prefix match on this one.
    if let Some((name, value)) = flag.split_once('=')
      && name == FAKE_STACK_FLAG
    {
      on = matches!(value, "1" | "true" | "True" | "TRUE" | "yes");
    }
  }
  on
}

/// Whether this run is one where a frame's locals are expected to **move** between two calls.
///
/// This is emphatically not "is this build instrumented", and the difference is what
/// `stack_per_level`'s control for [`frames_reused`] used to get wrong. Only two things relocate a
/// frame:
///
/// * **Miri**, which gives every frame's locals their own virtual allocation, and
/// * **ASan's fake stack**, which is off unless [`ASAN_OPTIONS`] turns it on.
///
/// ThreadSanitizer does not move frames at all — it instruments memory accesses — and the CI
/// sanitizer matrix runs it; neither does ASan with the fake stack left at its default. In those
/// builds [`frames_reused`] correctly reports *reused*, and a control that demands otherwise fails
/// while detection and refusal are both working exactly as intended.
///
/// The announcement is deliberately NOT consulted: [`ANNOUNCE`] is a claim that a build is
/// instrumented, not a claim about the fake stack, and reading it here would make an exported
/// variable able to force an assertion about the machine's behaviour.
pub fn frame_relocation_expected() -> bool {
  if cfg!(miri) {
    return true;
  }
  let address = option_env!("TOKORA_BUILD_SANITIZERS")
    .is_some_and(|list| list.split(',').any(|san| san == "address"));
  address && asan_fake_stack_enabled(std::env::var(ASAN_OPTIONS).ok().as_deref())
}

/// The live evidence, gathered once.
///
/// The two build facts come from `option_env!` rather than from the environment: instrumentation
/// is a property of the compiled binary, so it has to be read out of the binary. The announcement
/// and the affirmation are properties of the run, so those are read from the environment.
pub fn here() -> (Option<String>, Option<String>, bool) {
  (
    std::env::var(ANNOUNCE).ok(),
    std::env::var(OPT_IN).ok(),
    frames_reused(),
  )
}

/// This process's own [`StackEvidence`], handed to `f`.
///
/// A closure rather than a return value because the announcement is borrowed from the `String` the
/// environment lookup produced. Every reader of the live evidence goes through here, so a caller
/// that wants to ask what the decision would be with **one** field held fixed — the controls do,
/// to separate a refusal the compiler grounds from one the frame observation grounds — reuses the
/// same gathering rather than assembling a second copy of it.
pub fn with_evidence_here<R>(f: impl FnOnce(StackEvidence<'_>) -> R) -> R {
  let (announced, opt_in, reused) = here();
  f(evidence(&announced, &opt_in, reused))
}

/// [`instrumentation_refusal`] against this process.
pub fn instrumentation_refusal_here() -> Option<String> {
  with_evidence_here(instrumentation_refusal)
}

/// [`measurement_refusal`] against this process.
pub fn measurement_refusal_here() -> Option<String> {
  with_evidence_here(measurement_refusal)
}

fn evidence<'a>(
  announced: &'a Option<String>,
  opt_in: &Option<String>,
  frames_reused: bool,
) -> StackEvidence<'a> {
  StackEvidence {
    miri: cfg!(miri),
    compiler_sanitizers: option_env!("TOKORA_BUILD_SANITIZERS"),
    build_flags_opaque: option_env!("TOKORA_BUILD_FLAGS_OPAQUE").is_some(),
    announced_sanitizer: announced.as_deref(),
    frames_reused,
    affirmed_native: opt_in.as_deref() == Some(OPT_IN_VALUE),
  }
}

/// Says — on the process's **real** stderr — that a reading was not taken, and why.
///
/// Not `eprintln!`: libtest captures a passing test's output, so the one run where the reading is
/// skipped would be the one run where nobody can see that it was.
pub fn announce_skipped(what: &str, why: &str) {
  use std::io::Write as _;

  let mut err = std::io::stderr().lock();
  let _ = writeln!(err, "{what}: NO MEASUREMENT TAKEN — {why}");
  let _ = err.flush();
}
