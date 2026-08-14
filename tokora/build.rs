use std::env::{self, var};

fn main() {
  // Don't rerun this on changes other than build.rs, as we only depend on
  // the rustc version.
  println!("cargo:rerun-if-changed=build.rs");

  sanitizers();

  // Check for `--features=tarpaulin`.
  let tarpaulin = var("CARGO_FEATURE_TARPAULIN").is_ok();

  if tarpaulin {
    use_feature("tarpaulin");
  } else {
    // Always rerun if these env vars change.
    println!("cargo:rerun-if-env-changed=CARGO_TARPAULIN");
    println!("cargo:rerun-if-env-changed=CARGO_CFG_TARPAULIN");

    // Detect tarpaulin by environment variable
    if env::var("CARGO_TARPAULIN").is_ok() || env::var("CARGO_CFG_TARPAULIN").is_ok() {
      use_feature("tarpaulin");
    }
  }

  // Rerun this script if any of our features or configuration flags change,
  // or if the toolchain we used for feature detection changes.
  println!("cargo:rerun-if-env-changed=CARGO_FEATURE_TARPAULIN");
}

fn use_feature(feature: &str) {
  println!("cargo:rustc-cfg={}", feature);
}

/// Records whether this build is sanitizer-instrumented, as reported by the compiler.
///
/// # Why the tests cannot answer this themselves
///
/// Two integration tests read the address of a local and difference two of them —
/// `stack_per_level`'s bytes-per-nesting-level figure and `pratt_limit_unit_sink`'s frame-release
/// witness. Differencing addresses is a stack measurement only while they are offsets into one
/// contiguous native stack, and under a sanitizer they need not be: ASan's
/// `detect_stack_use_after_return` machinery can relocate a frame's locals onto a heap-backed fake
/// stack, and even with that off it inserts redzones that change every frame's size.
///
/// The fact wanted is `cfg(sanitize = "address")`, which needs `feature(cfg_sanitize)` — a
/// stable-compiled integration test cannot carry a feature gate. So both tests used to decide it
/// from `TOKORA_SANITIZER`, which nothing but `ci/sanitizer.sh` exports.
/// `RUSTFLAGS=-Zsanitizer=address cargo test`, the way anyone would actually reach for a
/// sanitizer, leaves it unset — and an unset variable read as "native stack" makes the probe print
/// heap-allocation spacing as bytes per nesting level. **Absence of an announcement is not
/// evidence of absence of instrumentation.**
///
/// A build script needs no feature gate to get the real answer: run the compiler with the flags
/// this build uses and read the cfgs it reports back. The answer then comes from rustc rather than
/// from a pattern match on flag text, so it covers every spelling of the flag and any sanitizer
/// added later. It is a property of the produced binary, so it is exported as a compile-time
/// value rather than left to be re-read from an environment the binary may not be running in.
///
/// # What it does when it cannot answer
///
/// It says so, and the readers refuse on that too. `TOKORA_BUILD_FLAGS_OPAQUE` is set whenever
/// there are rustflags and the compiler would not produce a cfg list for them; a build whose flags
/// cannot be interrogated is a build whose instrumentation cannot be excluded.
///
/// The compiler is only asked when there are rustflags at all, so an ordinary downstream build
/// pays nothing for this.
fn sanitizers() {
  // Without these the recorded answer outlives the flags it describes: nothing else in this
  // script depends on the environment, so `rerun-if-changed=build.rs` alone would keep a
  // sanitizer build's answer for the next uninstrumented one, and the reverse.
  println!("cargo:rerun-if-env-changed=CARGO_ENCODED_RUSTFLAGS");
  println!("cargo:rerun-if-env-changed=RUSTFLAGS");

  let encoded = var("CARGO_ENCODED_RUSTFLAGS").unwrap_or_default();
  let flags: Vec<&str> = encoded.split('\u{1f}').filter(|f| !f.is_empty()).collect();
  if flags.is_empty() {
    return;
  }

  let rustc = var("RUSTC").unwrap_or_else(|_| String::from("rustc"));
  let mut cmd = std::process::Command::new(rustc);
  cmd.arg("--print").arg("cfg");
  if let Ok(target) = var("TARGET") {
    cmd.arg("--target").arg(target);
  }
  cmd.args(&flags);

  match cmd.output() {
    Ok(out) if out.status.success() => {
      let text = String::from_utf8_lossy(&out.stdout);
      let mut found: Vec<&str> = text
        .lines()
        .filter_map(|line| line.trim().strip_prefix("sanitize=\""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .collect();
      found.sort_unstable();
      found.dedup();
      if !found.is_empty() {
        println!(
          "cargo:rustc-env=TOKORA_BUILD_SANITIZERS={}",
          found.join(",")
        );
      }
    }
    _ => println!("cargo:rustc-env=TOKORA_BUILD_FLAGS_OPAQUE=1"),
  }
}
