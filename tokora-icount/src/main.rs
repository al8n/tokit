//! Runs one repetition workload a given number of times, and prints a checksum.
//!
//! It counts nothing. Everything that counts instructions is outside the compiled artefact,
//! in valgrind — `ci/icount/measure.py` runs this binary under `--tool=callgrind` at two
//! iteration counts and differences the two totals:
//!
//! ```text
//!     Ir(hi) - Ir(lo)  =  (hi - lo) x per-iteration cost
//! ```
//!
//! That subtraction is the reason the binary has no measurement framework, no timing, and no
//! setup/teardown markers. Every fixed cost of the process — the dynamic loader, `main`'s
//! prologue, building the fixture source, the allocator's first arena, the final `println!` —
//! appears identically in both runs and cancels EXACTLY. What is left is the loop body and
//! nothing else, with no client requests to place and no framework overhead inside the
//! measured region to argue about.
//!
//! It also makes the per-iteration figure independent of the environment the process was
//! started in. `envp` and `argv` are copied by the loader at a cost proportional to their size,
//! so a runner with a longer `PATH` shifts a whole-process count; it does not shift a
//! difference of two counts from the same process image.
//!
//! # Usage
//!
//! ```text
//!     tokora-icount --list                 # one workload name per line
//!     tokora-icount <workload> <iterations>
//! ```

mod fixture;
mod workloads;

use std::process::ExitCode;

fn main() -> ExitCode {
  let args: Vec<String> = std::env::args().skip(1).collect();
  match args.as_slice() {
    [flag] if flag == "--list" => {
      for name in workloads::NAMES {
        println!("{name}");
      }
      ExitCode::SUCCESS
    }
    [name, iters] => match iters.parse::<u64>() {
      Ok(iters) => match workloads::run(name, iters) {
        // The checksum is printed rather than discarded so that a run which parsed nothing is
        // distinguishable from one that parsed everything. `ci/icount/measure.py` requires the
        // two runs to report checksums in the ratio their iteration counts are in, which is a
        // cheap proof that the loop ran the number of times it was asked to.
        Some(acc) => {
          println!("{name} {iters} {acc}");
          ExitCode::SUCCESS
        }
        None => {
          eprintln!("tokora-icount: no workload named `{name}`");
          eprintln!(
            "tokora-icount: known workloads: {}",
            workloads::NAMES.join(", ")
          );
          ExitCode::FAILURE
        }
      },
      Err(e) => {
        eprintln!("tokora-icount: `{iters}` is not an iteration count: {e}");
        ExitCode::FAILURE
      }
    },
    _ => {
      eprintln!("usage: tokora-icount --list");
      eprintln!("       tokora-icount <workload> <iterations>");
      ExitCode::FAILURE
    }
  }
}
