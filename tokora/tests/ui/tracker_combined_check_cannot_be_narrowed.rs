// THE COMBINED CHECK IS NOT A CUSTOMIZATION POINT. `TrackerExt`'s five update-and-check
// operations are supplied by one blanket impl over every `Tracker`, so coherence refuses the
// impl below and no tracker can substitute a narrower check for `Tracker::check`.
//
// The shape is tokora#265 written out. Those five were `Tracker`'s own provided methods, and
// `Limiter` — which implements `RecursionTracker`, `TokenTracker` and `Tracker`, each with a
// `check` of its own — overrode two of them and disambiguated to the wrong trait's. Nothing
// about that was exotic: inside `impl Tracker for Limiter` a bare `self.check()` is ambiguous
// across three candidates, so a body there *must* name one, and naming the narrow one compiled
// and read as an optimisation. A recursion depth already past its maximum then answered `Ok(())`
// through `increase_token_and_check` and through the decrementing form for as long as the token
// count held, and when both limits were over the combined form reported the token error while
// `Limiter::check` reported the recursion one.
//
// Deleting those two overrides would have fixed the two call sites. This is what makes the
// composition unwritable instead: the only way an override can differ from "update, then run
// THIS trait's check" is by checking less, and a limit check that silently narrows is a limit
// that does not hold. Specialization stays available where it is meaningful — on `Tracker`'s
// four primitives, which this file implements normally.
//
// Its teeth point forward. Replace the blanket impl with per-type ones and this file starts
// compiling; a `compile_fail` case that compiles is a failing test, and it is the only signal
// there would be, because a narrowed combined check is indistinguishable from a tracker that was
// simply within its limits. `every_ui_case_file_is_registered` keeps it from being quietly
// deregistered instead.
#![allow(dead_code)]

use tokora::state::tracker::{Tracker, TrackerExt};

/// A tracker with two independent limits, so "check the counter I just touched" and "check every
/// limit" are genuinely different answers.
struct TwoLimits {
  tokens: usize,
  depth: usize,
}

impl Tracker for TwoLimits {
  type Error = ();

  fn increase_token(&mut self) {
    self.tokens += 1;
  }

  fn increase_recursion(&mut self) {
    self.depth += 1;
  }

  fn decrease_recursion(&mut self) {
    self.depth = self.depth.saturating_sub(1);
  }

  fn check(&self) -> Result<(), Self::Error> {
    if self.tokens > 8 || self.depth > 2 {
      Err(())
    } else {
      Ok(())
    }
  }
}

// The defect, as an impl: every combined operation ends by consulting only the counter it moved.
impl TrackerExt for TwoLimits {
  fn increase_token_and_decrease_recursion(&mut self) {
    self.tokens += 1;
    self.depth = self.depth.saturating_sub(1);
  }

  fn increase_token_and_decrease_recursion_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase_token_and_decrease_recursion();
    if self.tokens > 8 { Err(()) } else { Ok(()) }
  }

  fn increase_token_and_check(&mut self) -> Result<(), Self::Error> {
    self.tokens += 1;
    if self.tokens > 8 { Err(()) } else { Ok(()) }
  }

  fn increase_both(&mut self) {
    self.tokens += 1;
    self.depth += 1;
  }

  fn increase_both_and_check(&mut self) -> Result<(), Self::Error> {
    self.increase_both();
    if self.depth > 2 { Err(()) } else { Ok(()) }
  }
}

fn main() {}
