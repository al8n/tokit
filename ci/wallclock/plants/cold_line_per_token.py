#!/usr/bin/env python3
"""Plant a COARSE regression the wall-clock gate must catch: a cold cache line per token.

Applied by `ci/wallclock/run.sh --plant cold_line_per_token` to the HEAD side of the extracted
work tree only. It is never applied to the repository and no commit carries it.

# What it plants, and why this shape

`InputRef::next` is the per-token entry point every parser in `tokora-benches` drains through.
This adds, on each call, an increment of a counter and — every sixteenth call — one dependent
byte read from an 8 MiB static array at a scattered index. The array is far larger than any
last-level cache a GitHub runner has, and the index is a multiplicative hash of the counter, so
each read is a cache miss and a likely TLB miss with no prefetcher pattern to follow.

That is deliberately the cheapest possible instruction footprint for the most expensive possible
memory behaviour. The counter is a plain relaxed load/add/store rather than a `fetch_add`, so on
x86_64 it is three instructions and no `lock` prefix; the read behind the mask is one `movzx`.
Roughly five extra instructions a token buy roughly a hundred nanoseconds of stall every
sixteenth token — the shape an instruction count is least able to price and a clock prices
exactly. A gate that failed to see this would have no reason to exist beside the `icount` job.

# Sizing

`SPAN` is 8 MiB and `MASK` is 15, so the amortised cost is about one main-memory round trip per
sixteen tokens plus a few cycles of counter. Both constants are here rather than tuned into
invisibility: raise `MASK` to make the plant smaller and watch the gate lose it, which is the
other half of knowing what the threshold means.

# It fails loudly rather than planting nothing

The anchor is asserted to match exactly once. A plant that silently matched nothing would run
the gate against unmodified code and report the floor, which reads exactly like a gate that
works.
"""

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

PLANT = """    // ── WALLGATE PLANT: ci/wallclock/plants/cold_line_per_token.py ──────────────────────
    // Not shipped code. Applied to one side of one measurement, in a temporary tree.
    {
      fn wallgate_cold_line() {
        const SPAN: usize = 1 << 23;
        const MASK: usize = 15;
        static HAYSTACK: [u8; SPAN] = [0; SPAN];
        static TICK: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
        let n = TICK.load(core::sync::atomic::Ordering::Relaxed);
        TICK.store(n.wrapping_add(1), core::sync::atomic::Ordering::Relaxed);
        if n & MASK == 0 {
          let i = n.wrapping_mul(2_654_435_761) & (SPAN - 1);
          let _ = core::hint::black_box(HAYSTACK[i]);
        }
      }
      wallgate_cold_line();
    }
"""

TARGET = "tokora/src/input/input_ref/mod.rs"


def main():
    if len(sys.argv) != 2:
        sys.exit("usage: cold_line_per_token.py <extracted-tree>")
    path = pathlib.Path(sys.argv[1]) / TARGET
    text = path.read_text(encoding="utf-8")
    hits = text.count(ANCHOR)
    if hits != 1:
        sys.exit(
            f"::error::{TARGET}: the plant's anchor matched {hits} times, expected exactly 1. "
            "`InputRef::next`'s signature moved; re-anchor the plant rather than letting it "
            "measure unmodified code."
        )
    path.write_text(text.replace(ANCHOR, ANCHOR + PLANT), encoding="utf-8")
    print(f"plant: cold_line_per_token applied to {TARGET}")


if __name__ == "__main__":
    main()
