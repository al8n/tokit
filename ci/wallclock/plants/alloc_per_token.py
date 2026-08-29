#!/usr/bin/env python3
"""Plant a COARSE regression the wall-clock gate must catch: a heap allocation per token.

Applied by `ci/wallclock/run.sh --plant alloc_per_token` to the HEAD side of the extracted work
tree only. It is never applied to the repository and no commit carries it.

# What it plants

`InputRef::next` is the per-token entry point the parsers in `tokora-benches` drain through. This
adds a `Vec` allocation and its matching free on one call in `RATE + 1` — nothing written, nothing
read, the pointer handed to a black box so the pair cannot be elided. That is the canonical coarse
regression: an allocator round trip that appeared inside a hot loop, which is the shape a refactor
produces when a value that used to be borrowed starts being owned.

Measured on this machine, one `Vec::<u64>::with_capacity(n)` and its drop costs 18 ns at n = 8 and
27-30 ns at n = 64 or 512, against a 1 ns control. The default `RATE` of 3 therefore adds roughly
7 ns per call.

# What it measured, and the half of the reading that is not the magnitude

A DATED RECORD of one run, on a developer machine under load ~36 — its absolute times are
worthless and its ratios are not, because both sides met the same machine within seconds of each
other. Two rounds a side, 100 ms warm-up and 200 ms measurement an id.

**26 of the 46 ids failed, from +12.17% to +82.81%.** `input/scan/next_drain` went 674 878 ->
1 232 724 ns, and the arithmetic closes: its 128 KiB fixture of `var1 = 2 + val3 ;` lines is about
22 bytes a line, six tokens plus six runs of trivia, so ~71 000 `next` calls a parse; 557 846 ns
over 71 000 is 7.8 ns a call, against the 7 ns predicted from the allocator microbenchmark and the
rate.

**The other twenty ids did not move at all**, between -3.4% and +2.9%, and which twenty is the
interesting part. `cst/*` never parses — it builds a tree from pre-made events — and every one of
the `parser/*` combinator drivers, along with `input/dispatch/fused_*`, reaches its tokens through
`try_expect` and the scan family rather than through `next`. So the gate did not merely fire: it
said WHERE, and the split falls exactly along whether an id enters the function the plant is in. A
plant that moved every row equally would be measuring the harness. The rate is a knob, `WALLGATE_PLANT_RATE`, because one plant size proves
the plant and not the property: raise it until the gate loses the regression and you have measured
what the threshold means instead of asserting it.

# Why this, and not the cache-hostile access that was tried first

The first version read one byte per token from an 8 MiB static array at a scattered index, on the
theory that a main-memory round trip is the cost an instruction count is least able to price. It
measured **nothing**, and why is worth more than the plant was:

  * The array was `static HAYSTACK: [u8; SPAN] = [0; SPAN]`, so it lived in `.bss` — and a
    zero-initialised read-only page is backed by the kernel's **shared zero page**. All 2048 of
    those pages were one physical page, permanently in L1. A standalone A/B put the
    zero-initialised array at **0.33 ns a read** against **1.36 ns** for an identical array
    initialised to ones: the plant was reading one cache line, not eight megabytes.
  * Distinct pages would not have made it a cliff either. The index did not depend on the value
    loaded, so the misses overlapped — the point of an out-of-order window is that a dozen
    independent misses cost barely more than one. Making the chase **dependent**, the next address
    computed from the byte just loaded, cost 2.0 ns for one hop and 24.5 ns for eight: about 3 ns
    a hop, not the ~80 ns a DRAM round trip costs.
  * And the dependent chase was cheap because 8 MiB **fits in the cache** of the machine it ran on,
    and would very likely fit a runner's L3 too. A static array big enough to guarantee a miss is
    a static array big enough to be an absurd binary.

So a cache cliff is not a thing one plants in five lines. A working set that outgrew its cache is
a property of a data structure's size, not of an added instruction, and planting one honestly
would mean changing a type under `tokora/src/` — which is exactly what this mechanism exists to
avoid. An allocation per element is the other regression the coarse layer is for, it is the one
that actually happens, and it is sized by an allocator rather than by a guess about somebody
else's cache hierarchy.

# It fails loudly rather than planting nothing

The anchor is asserted to match exactly once. A plant that silently matched nothing would run the
gate against unmodified code and report the floor, which reads exactly like a gate that works —
and, as the cold-line attempt shows, a plant that matched and then did nothing reads the same way.
The gate's own number is the only thing that separates those from a plant that works, which is why
what a plant produces is reported and not just whether it exited non-zero.
"""

import os
import pathlib
import sys

ANCHOR = """  #[allow(clippy::should_implement_trait)]
  pub fn next(
    &mut self,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
  {
"""

# `std::vec::Vec` rather than `alloc::vec::Vec`: `tokora/src/lib.rs` aliases `extern crate alloc
# as std` when the `std` feature is off, so this one path resolves in both configurations. The
# tree this is applied to is only ever built by `cargo bench -p tokora-benches`, which pins `std`.
PLANT = """    // ── WALLGATE PLANT: ci/wallclock/plants/alloc_per_token.py ──────────────────────────
    // Not shipped code. Applied to one side of one measurement, in a temporary tree.
    {
      #[inline(never)]
      fn wallgate_alloc() {
        const WORDS: usize = 64;
        let scratch: std::vec::Vec<u64> = std::vec::Vec::with_capacity(WORDS);
        // Observing the pointer is what stops LLVM removing an allocation nothing reads: a
        // malloc/free pair with no other use is a pair it is allowed to delete outright.
        core::hint::black_box(scratch.as_ptr());
      }
      const RATE: usize = __RATE__;
      static TICK: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
      let n = TICK.load(core::sync::atomic::Ordering::Relaxed);
      TICK.store(n.wrapping_add(1), core::sync::atomic::Ordering::Relaxed);
      if n & RATE == 0 {
        wallgate_alloc();
      }
    }
"""

TARGET = "tokora/src/input/input_ref/mod.rs"


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: alloc_per_token.py <extracted-tree>")
    rate = os.environ.get("WALLGATE_PLANT_RATE", "3").strip()
    if not rate.isdigit():
        sys.exit(f"::error::WALLGATE_PLANT_RATE={rate!r} is not a non-negative integer")
    path = pathlib.Path(sys.argv[1]) / TARGET
    text = path.read_text(encoding="utf-8")
    hits = text.count(ANCHOR)
    if hits != 1:
        sys.exit(
            f"::error::{TARGET}: the plant's anchor matched {hits} times, expected exactly 1. "
            "`InputRef::next`'s signature moved; re-anchor the plant rather than letting it "
            "measure unmodified code."
        )
    path.write_text(
        text.replace(ANCHOR, ANCHOR + PLANT.replace("__RATE__", rate)), encoding="utf-8"
    )
    print(f"plant: alloc_per_token applied to {TARGET}, one allocation per {int(rate) + 1} calls")


if __name__ == "__main__":
    main()
