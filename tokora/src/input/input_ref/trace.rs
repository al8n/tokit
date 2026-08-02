//! `InputRef` tracing hooks — the `trace` feature.
//!
//! The enter / exit / leaf event emitters and the source preview they print. Kept beside
//! [`InputRef`](super::InputRef) so they can reach its `pub(super)` `depth` field. Every line
//! is handed to [`crate::trace::write_line`], which routes it out of band (stderr, or the
//! test capture buffer) — never through the emitter.

use super::{Completeness, InputRef};
use crate::{Lexer, ParseContext, source::Source};

/// How many characters of `{rest:?}` a trace preview shows before truncating. Was, and stays,
/// a plain `24` in the output — pulled out here only because [`bounded_debug`] needs the same
/// number the old inline arithmetic used, and one named constant can't drift out of sync with
/// itself the way two copies of `24` could.
const PREVIEW_WINDOW: usize = 24;

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// A short, generic preview of the source at the cursor: the current offset plus a debug
  /// window of the remaining source. The window is followed by `…` when the window cut real
  /// content off, or by a distinct failure marker when `T`'s `Debug` impl broke on its own
  /// account instead — see [`BoundedDebugOutcome`] for the difference between the two, and the
  /// `FormatFailed` arm below for why that marker is a plain-text convention for a human
  /// reader, not reserved syntax, and can collide with a value's own rendering. Cheap
  /// when the [`Source`]'s `Slice` type has a `Debug` impl that writes incrementally — see
  /// [`bounded_debug`] for what that means, which in-tree backings qualify, and the residual
  /// shapes where cost can still track the remaining source (one of them unbounded, not merely
  /// narrower).
  fn trace_preview(&self) -> std::string::String {
    let off = self.offset();
    match self.source().slice(off.clone()..) {
      Some(rest) => {
        let (mut window, outcome) = bounded_debug(&rest);
        match outcome {
          BoundedDebugOutcome::Complete => {}
          BoundedDebugOutcome::WindowTruncated => window.push('\u{2026}'),
          // Deliberately not `…`: that glyph promises more of the value exists. This says
          // the renderer itself broke instead, which is a different fact calling for a
          // different reaction from whoever reads the trace.
          //
          // This text is a convention for a human reading the trace, not a reserved syntax.
          // It is appended into the same channel as `window` itself, so a value whose own
          // `Debug` happens to render this exact text (or any text containing it) inside the
          // window is indistinguishable, in the line this function returns, from a value that
          // rendered less and then failed here. Nothing in this crate parses a trace line back
          // apart, and nothing that consumes trace output should try to either.
          BoundedDebugOutcome::FormatFailed => window.push_str(" <fmt error>"),
        }
        std::format!("@{off:?} {window}")
      }
      None => std::format!("@{off:?} <eof>"),
    }
  }

  /// Emits a leaf event naming an instrumented combinator at the current depth. Reached only
  /// through the `trace_event!` macro, so with the feature off it has no caller.
  pub(crate) fn trace_leaf(&self, name: &str) {
    crate::trace::write_line(std::format!(
      "{}\u{b7} {name}  {}",
      "  ".repeat(*self.depth),
      self.trace_preview()
    ));
  }

  /// Drops the depth by one **without emitting**, for a [`traced`](crate::traced) scope that is
  /// leaving by an unwind rather than by one of its exit arms.
  ///
  /// The arms decrement and then emit; an unwound scope prints no exit line (trace events are
  /// out of band, and there is no outcome to report), so its guard needs the decrement alone.
  /// The field is `pub(super)` to `input_ref` and `crate::trace` is a sibling module, so the
  /// guard reaches it through here.
  pub(crate) fn trace_unwind_pop(&mut self) {
    *self.depth = (*self.depth).saturating_sub(1);
  }

  /// Emits an `enter` event, then bumps the depth so nested events indent beneath it.
  pub(crate) fn trace_enter(&mut self, name: &str) {
    crate::trace::write_line(std::format!(
      "{}> {name}  {}",
      "  ".repeat(*self.depth),
      self.trace_preview()
    ));
    *self.depth += 1;
  }

  /// Emits an `ok` exit carrying the span consumed since `start`, dropping the depth first.
  pub(crate) fn trace_exit_ok(&mut self, name: &str, start: &L::Offset) {
    *self.depth = (*self.depth).saturating_sub(1);
    let end = self.offset().clone();
    crate::trace::write_line(std::format!(
      "{}< {name}  ok  {start:?}..{end:?}",
      "  ".repeat(*self.depth)
    ));
  }

  /// Emits a non-`ok` exit (`err` or `decline`), dropping the depth first.
  pub(crate) fn trace_exit(&mut self, name: &str, outcome: &str) {
    *self.depth = (*self.depth).saturating_sub(1);
    crate::trace::write_line(std::format!(
      "{}< {name}  {outcome}",
      "  ".repeat(*self.depth)
    ));
  }
}

/// The outcome of trying to capture up to [`PREVIEW_WINDOW`] characters of `T`'s `Debug`
/// rendering through [`bounded_debug`]'s sink.
///
/// Two independent facts a caller needs kept apart, not folded into one bool: whether the
/// returned prefix might be missing real content the window cut off
/// ([`WindowTruncated`](Self::WindowTruncated)), and whether `T`'s own `Debug` impl finished
/// without failing on its own account (it did not, exactly when
/// [`FormatFailed`](Self::FormatFailed)). Conflating these is the bug this type exists to rule
/// out — see [`bounded_debug`]'s doc comment for how the two are told apart and why that is
/// reliable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundedDebugOutcome {
  /// The write ran to completion and the sink never refused it: `prefix` is the complete
  /// rendering.
  Complete,
  /// The sink refused a write once its window was already full. `prefix` is a genuine prefix
  /// of the untruncated dump: real content, but not all of it.
  ///
  /// Also reported — indistinguishably — when a `Debug` impl swallows that refusal and then
  /// fails again afterward for a reason of its own. See "A residual this mechanism cannot
  /// resolve" on [`bounded_debug`]'s doc comment for why that case cannot be told apart from
  /// an ordinary truncation.
  WindowTruncated,
  /// `write!` returned `Err` and the sink itself never refused anything, so `T`'s `Debug` impl
  /// failed on its own account. `prefix` is whatever it wrote before failing — which can be
  /// its complete intended output — with no evidence either way that more was coming.
  FormatFailed,
}

/// Debug-formats `value`, returning at most [`PREVIEW_WINDOW`] characters of
/// `format!("{value:?}")` alongside a [`BoundedDebugOutcome`] describing how the write ended.
/// The prefix is exactly the `window` half of the `(window, needs_ellipsis)` pair
/// `trace_preview` used to compute by building the complete dump and then discarding everything
/// past the 24th character; the outcome replaces the old `needs_ellipsis` bool with the
/// three-way distinction `trace_preview` needs to render a preview honestly.
///
/// # Telling the sink's own refusal apart from a foreign `Debug` failure
///
/// `write!(sink, "{value:?}")`'s `Result` comes back `Err` in exactly two situations: the
/// sink's own refusal once it already holds a full window, or `T`'s `Debug` impl returning
/// `Err` on its own account, for a reason that has nothing to do with the sink. Both are legal:
/// `fmt::Result` is a plain `Result`, and neither the `Debug` trait nor [`crate::Slice`]'s own
/// bound (`PartialEq + Eq + Debug`) requires every `Err` to originate from a formatter write. A
/// reader of the preview needs the two told apart: one means "there is more of this value than
/// shown", the other means "this value's renderer broke, independent of how much fit" — and
/// they call for different next steps.
///
/// The two are told apart by a fact `PreviewSink` itself records, not by inspecting `T` or
/// guessing at intent. `PreviewSink` implements only `write_str`; `Write::write_char` and
/// `write_fmt` are inherited default methods that both bottom out in it, so `write_str` is the
/// *only* place this sink can ever fail, at any nesting depth (a nested value's own
/// `Debug`/`Display` call still writes through the same `Formatter`, hence the same sink). That
/// method has exactly one `Err`-returning branch, and it sets `self.truncated = true` in the
/// same statement, before returning — there is no other `Err` anywhere in the sink. So
/// `sink.truncated == true` is necessary *and* sufficient evidence that the sink produced an
/// `Err` somewhere during the call, and `sink.truncated == false` proves it never did — meaning
/// an `Err` from `write!` while `sink.truncated` is still `false` cannot be the sink's; it was
/// synthesized by `T::fmt` itself (or something it called), independent of the window. That is
/// what makes the distinction reliable: it reads a fact recorded at the one point the sink is
/// able to fail, rather than inferring anything about `T`'s `Debug` impl.
///
/// ```text
/// sink.truncated                   -> WindowTruncated  (regardless of write!'s own result)
/// !sink.truncated &&  wrote_fully  -> Complete
/// !sink.truncated && !wrote_fully  -> FormatFailed
/// ```
///
/// `WindowTruncated` is checked first because the two facts are not mutually exclusive: the
/// ordinary case is the sink's `Err` propagating straight out of `write!` (`wrote_fully =
/// false`), but a `Debug` impl may also swallow that `Err` and report overall `Ok` anyway
/// (legal — nothing requires propagating a formatter `Result` with `?`). Either way the sink
/// already knows it cut something off, so this stays truncated no matter what `write!` itself
/// says. `FormatFailed` is what remains once `WindowTruncated` is ruled out: `write!` failed,
/// and by the proof above that failure cannot be the sink's, so `prefix` is whatever `T::fmt`
/// wrote before failing on its own account — which can be its complete, intended output, short
/// and otherwise unremarkable (see `FailsForAForeignReason` in this module's tests) — so this
/// must not be reported the same way as a real truncation, which would claim content beyond
/// `prefix` that the impl never intended to write.
///
/// This is a correctness property, independent of the cost discussion below: it holds
/// unconditionally, whether `T`'s `Debug` impl streams or front-loads.
///
/// # A residual this mechanism cannot resolve
///
/// The proof above tells `sink.truncated` apart from a foreign `Debug` failure whenever
/// exactly one of them happened. It says nothing about the case where both do, in the same
/// call: a `Debug` impl writes past the window — the sink refuses and sets `sink.truncated =
/// true` — swallows that `Err` instead of propagating it with `?`, and then fails again
/// afterward for a reason of its own, unrelated to the sink. Two facts are true there: the
/// window really did cut off genuine content, *and* the impl broke afterward. Only the first
/// is reported. With `sink.truncated` already `true`, `WindowTruncated` is checked first (as
/// above) and this case is `WindowTruncated`, indistinguishable from an impl that did nothing
/// but let the sink's own `Err` propagate.
///
/// This is not an artifact of checking `sink.truncated` before `wrote_fully`, and no
/// reordering of the two, nor any other reading of them, recovers it. Once the sink has
/// refused, these two histories:
///
/// - the impl let the sink's `Err` propagate straight out — ordinary truncation, nothing else
///   happened
/// - the impl swallowed the sink's `Err` and then failed again afterward, on its own account
///
/// leave identical evidence behind: `sink.truncated == true` and `wrote_fully == false`, in
/// both. Nothing records "and something else went wrong after the sink refused," so a fourth
/// [`BoundedDebugOutcome`] variant could not be populated honestly here — there is no fact left
/// to populate it from, only two histories that read the same. A richer sink protocol —
/// recording not just *that* it refused but everything offered to it afterward — could
/// recover this, but that is materially more machinery than a debug preview earns.
///
/// The loss is narrow in practice. `WindowTruncated`'s `prefix` is a genuine, incomplete slice
/// of the value in both histories, so the `…` a reader sees is warranted either way; what does
/// not survive is the extra fact that the impl was *also* broken. See
/// `bounded_debug_absorbs_a_foreign_failure_after_truncation_into_window_truncated` in this
/// module's tests for this exact case, pinned as `WindowTruncated`.
///
/// # Why the old shape (`format!` then `.chars().take(24)`) is the bug, and why this isn't
///
/// `format!("{rest:?}")` fully escapes and allocates the debug rendering of *all* of `rest` —
/// the entire remaining source — before a single byte of that is kept; `dump.chars().count()`
/// then walks it again. Both are `O(remaining source)`, unconditionally, on every traced event.
///
/// The fix this file would like to make is "ask for a shorter `rest` before formatting it at
/// all," but that lever isn't reachable from here. [`Source::slice`] only accepts a range in
/// `L::Offset`, and `Lexer::Offset` is deliberately left with no arithmetic and no conversion
/// to `usize` (`Default + Debug + Ord + Clone + Hash` — see the trait) so a third-party
/// `Source` is never forced to expose byte-offset structure it may not have; there is no
/// `off + 24` to compute anywhere generic code can reach. [`crate::Slice`] — what
/// `Source::slice` hands back — offers iteration (`iter`, `positioned_iter`, `len`), not
/// sub-slicing, so there is no smaller `Self::Slice` to ask for either. Adding either
/// capability would mean widening `Lexer`/`Source`/`Slice` themselves, which is a far bigger
/// and riskier change than this fix, for every caller of those public traits — not something
/// to do as a side effect of a trace-preview cost fix.
///
/// What *is* reachable generically: a `Debug` impl only ever produces output by calling
/// `Formatter::write_str`/`write_char`, and the `?` those calls are always written behind
/// means a sink that starts refusing them aborts the whole call chain immediately, no matter
/// how deep into its own loop the impl is. Everything written before that point is, byte for
/// byte, a genuine prefix of the untruncated dump — that is what "the first writes of a
/// formatting call" means, regardless of how an impl batches them internally — so the window
/// and the ellipsis decision below come from real output, never a reconstruction of it.
///
/// # What this does and does not bound
///
/// This removes the one guarantee the old code had: that a preview costs the *entire*
/// remaining source, every time, unconditionally. What replaces it is conditional, not
/// universal: **the bound holds for a `Debug` impl that writes incrementally** — one whose
/// calls to `write_str`/`write_char` interleave with whatever scanning, escaping or
/// allocation produces each piece of output, so a sink that starts refusing mid-stream stops
/// that work too, not merely the writes themselves. **It does not hold for an impl that
/// front-loads** — builds its entire rendering into an owned buffer first and hands it to the
/// formatter in a single `write_str` call — because that cost has already run by the time the
/// sink is ever invoked.
///
/// Every `Source`/`Slice` pairing this crate itself ships was audited against that line, and
/// the guarantee above is asserted for exactly these, not for [`Source`] in general. What the
/// audit found, worst first:
///
/// - **Unbounded, and the only one of the three reachable without touching this crate at
///   all.** [`Source::Slice`] is a public associated type any downstream crate can implement
///   (`Self::Slice<'source>: Slice<'source>`), and [`crate::Slice`]'s own bound (`PartialEq +
///   Eq + Debug`) carries no streaming requirement, so nothing stops a conforming third-party
///   impl from scanning, escaping and allocating its *entire* rendering into a temporary and
///   calling `write_str` once — no unsafe, no misuse of this crate, just an ordinary trait
///   impl. This sink cannot shorten work that already ran before it was ever called, so the
///   cost stays the same `O(remaining source)` this fix exists to remove. Output is still
///   correct (a front-loaded impl's single write still *starts* with a genuine prefix of the
///   untruncated dump, so the window and the ellipsis decision are unaffected) — only the cost
///   guarantee does not reach this far. No in-tree backing does this today (see the rest of
///   this list). See `bounded_debug_is_correct_but_unbounded_for_a_front_loading_debug_impl`
///   in this module's tests for a fixture that exercises exactly this shape.
/// - `[u8]`-shaped backings (`[u8]` itself, and `HipByt`/the `smol-bytes` byte types, which
///   delegate to it) go through `Formatter::debug_list`, which drives its element iterator
///   with the write failure recorded in an internal `Result` rather than by breaking its own
///   loop — so it keeps *pulling* elements after this sink starts refusing them, even though
///   it stops writing. That bounds the allocation and the escaping work to the budget (still a
///   large, measured win — see the fix's cost evidence), but the iteration stays
///   `O(remaining elements)`.
/// - `str`-shaped backings (`str` itself, and `HipStr`/`Utf8Bytes`, which delegate to it)
///   accumulate a run of characters that need no escaping before their first write, so a sink
///   can only cut in *between* such runs, not partway through accumulating one. Ordinary text
///   ends a run at the next newline, quote, backslash, control character or non-printable code
///   point — a handful of bytes — which is what turns the reported cost from scaling with the
///   remaining source into not scaling with it at all. An input engineered to have a long run
///   of nothing-needs-escaping right at the cursor — e.g. this crate's own
///   `int_run_source`/`comma_list_source` bench fixtures, which are digits and spaces for
///   ~128 KiB at a stretch — still costs `O(that run)` here. Found while fixing the reported
///   cost; not fixed by it.
/// - `bytes::Bytes` and `bstr::BStr` hand-write their `Debug` as a loop with `?` on every
///   write, so a refusing sink stops them outright: genuinely `O(1)` here, independent of
///   content.
fn bounded_debug<T: core::fmt::Debug + ?Sized>(
  value: &T,
) -> (std::string::String, BoundedDebugOutcome) {
  use core::fmt::Write as _;

  /// The sink itself: accepts characters until [`PREVIEW_WINDOW`] of them have been kept, then
  /// starts returning `Err` and keeps a record that it did.
  struct PreviewSink {
    prefix: std::string::String,
    len: usize,
    truncated: bool,
  }

  impl core::fmt::Write for PreviewSink {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
      for c in s.chars() {
        if self.len == PREVIEW_WINDOW {
          self.truncated = true;
          return Err(core::fmt::Error);
        }
        self.prefix.push(c);
        self.len += 1;
      }
      Ok(())
    }
  }

  // `* 4`: up to `PREVIEW_WINDOW` *characters* can be kept, and a `char` is up to 4 UTF-8
  // bytes — sized to avoid a reallocation on non-ASCII content, not because more than the
  // budget is ever kept.
  let mut sink = PreviewSink {
    prefix: std::string::String::with_capacity(PREVIEW_WINDOW * 4),
    len: 0,
    truncated: false,
  };
  // `write!`'s `Result` can be `Err` for a reason `sink.truncated` never sees: `T`'s `Debug`
  // impl failing on its own account, not because this sink refused anything. See "Telling the
  // sink's own refusal apart from a foreign `Debug` failure" above for the proof that
  // `sink.truncated` alone tells the two apart reliably.
  let wrote_fully = write!(sink, "{value:?}").is_ok();
  // `WindowTruncated` is checked first: it can coexist with `wrote_fully` (a `Debug` impl may
  // swallow the sink's `Err` and report `Ok` anyway), and whenever the sink has something to
  // report, its bookkeeping wins over what `write!` itself says. Only what is left —
  // `write!` failed, and the sink did not cause it — is `FormatFailed`.
  let outcome = if sink.truncated {
    BoundedDebugOutcome::WindowTruncated
  } else if wrote_fully {
    BoundedDebugOutcome::Complete
  } else {
    BoundedDebugOutcome::FormatFailed
  };
  (sink.prefix, outcome)
}

#[cfg(test)]
mod tests {
  use super::{BoundedDebugOutcome, PREVIEW_WINDOW, bounded_debug};
  use core::cell::Cell;

  /// A `Debug` impl that deliberately breaks the streaming assumption [`bounded_debug`]'s
  /// bound depends on: it scans its *entire* input into an owned buffer — counting every
  /// character it touches along the way — and only then hands the finished buffer to the
  /// formatter in a single `write_str` call. `crate::Slice`'s bound (`PartialEq + Eq + Debug`)
  /// permits exactly this; nothing about implementing `Debug` requires interleaving writes
  /// with the work that produces them. This is the shape the unbounded residual documented on
  /// [`bounded_debug`] describes, made concrete.
  struct FrontLoading<'a> {
    data: &'a str,
    touched: &'a Cell<usize>,
  }

  impl core::fmt::Debug for FrontLoading<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      let mut built = std::string::String::with_capacity(self.data.len());
      for ch in self.data.chars() {
        // Recorded regardless of how much of `built` the caller ends up keeping — this is
        // exactly the work a streaming impl would stop doing once a sink starts refusing
        // writes, and exactly the work this one does not stop.
        self.touched.set(self.touched.get() + 1);
        built.push(ch);
      }
      // One write, issued only after the entire input has already been touched. The sink
      // below gets a chance to cut short the string it receives here, never the work that
      // built it.
      f.write_str(&built)
    }
  }

  /// The residual `bounded_debug`'s doc comment names as unbounded: a front-loading `Debug`
  /// impl still yields the right preview, but the early-refusing sink never gets a chance to
  /// shorten the work that produced it, because that work is already finished by the time the
  /// sink sees anything at all.
  ///
  /// Falsifying output: `touched` bounded near [`PREVIEW_WINDOW`] rather than equal to the
  /// full input length — that would mean either this fixture stopped being front-loading (a
  /// test bug) or `bounded_debug` gained a way to bound this shape after all, which would be
  /// real news worth updating the doc comment and the changelog for, not just this assertion.
  #[test]
  fn bounded_debug_is_correct_but_unbounded_for_a_front_loading_debug_impl() {
    // Far longer than the window, so truncation is actually exercised rather than the
    // shorter-than-budget case `bounded_debug` also has to get right.
    let long: std::string::String = "a".repeat(PREVIEW_WINDOW * 10);
    let touched = Cell::new(0);
    let value = FrontLoading {
      data: &long,
      touched: &touched,
    };

    let (window, outcome) = bounded_debug(&value);

    // Correctness is unaffected by front-loading: everything a `Debug` impl writes, however
    // it was produced, is a genuine prefix of the untruncated dump the moment it reaches
    // `write_str` — so the sink still recovers exactly the first `PREVIEW_WINDOW` characters
    // and the right truncation decision.
    assert_eq!(window, "a".repeat(PREVIEW_WINDOW));
    assert_eq!(
      outcome,
      BoundedDebugOutcome::WindowTruncated,
      "content far longer than the window must report truncated"
    );

    // The residual: a streaming impl would have left `touched` at PREVIEW_WINDOW + 1 (the one
    // character whose push trips the sink's refusal). This impl built its entire output
    // before ever calling the formatter, so it touched every character of `long` regardless —
    // cost the sink never had an opportunity to bound.
    assert_eq!(
      touched.get(),
      long.chars().count(),
      "a front-loading impl costs the whole input no matter what the sink does"
    );
  }

  /// A `Debug` impl that writes a few characters — fewer than [`PREVIEW_WINDOW`], so nothing
  /// about the sink can be why it fails — and then fails on its own account. Legal under the
  /// trait: `fmt::Result` is a plain `Result`, and neither `Debug` nor `crate::Slice`'s bound
  /// (`PartialEq + Eq + Debug`) requires every failure to originate from a formatter write.
  struct FailsForAForeignReason;

  impl core::fmt::Debug for FailsForAForeignReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("abc")?;
      Err(core::fmt::Error)
    }
  }

  /// The case Codex's review named: only three characters are ever offered to the sink here,
  /// so `self.len` never reaches [`PREVIEW_WINDOW`] and the sink never refuses a write —
  /// `sink.truncated` stays `false`. `write!` still fails, but for a reason of the `Debug`
  /// impl's own, and `"abc"` is that impl's *complete* intended output, not a fragment of a
  /// longer one that got cut off. An earlier version of this fix folded the write's own result
  /// into a single `truncated` bool with `||`, on the reasoning "incomplete is incomplete,
  /// whatever the cause" — reasonable-sounding, but wrong here: nothing is incomplete. The
  /// write finished exactly as the impl meant it to. Reporting it truncated would append `…` to
  /// a trace line for a value with no more content, claiming a continuation that does not
  /// exist — the same mistake as the original bug, in the opposite direction.
  /// [`BoundedDebugOutcome::FormatFailed`] is the outcome that makes neither claim: it says the
  /// renderer broke, and says nothing about how much of the value that left out.
  #[test]
  fn bounded_debug_is_format_failed_when_debug_fails_for_a_reason_the_sink_did_not_cause() {
    let (window, outcome) = bounded_debug(&FailsForAForeignReason);

    // What was written before the failure is real output, kept exactly as written — this fix
    // changes how the outcome is classified, not what makes it into the prefix.
    assert_eq!(window, "abc");
    assert_eq!(
      outcome,
      BoundedDebugOutcome::FormatFailed,
      "a Debug impl that already wrote its complete, short rendering and then failed for its \
       own reason must be reported as a format failure, not window-truncated — truncated would \
       tell a reader there is more content past \"abc\" than this impl ever intended to write"
    );
  }

  /// A `Debug` impl that writes past [`PREVIEW_WINDOW`] — so the sink genuinely refuses and
  /// records `sink.truncated = true` — then discards that `Err` instead of propagating it with
  /// `?`, and fails again anyway, for a reason that has nothing to do with the sink. Legal for
  /// the same reason [`FailsForAForeignReason`] is: `fmt::Result` is a plain `Result`, nothing
  /// obliges a `Debug` impl to propagate a formatter failure with `?`, and nothing obliges it
  /// to stop trying once one write fails.
  struct SwallowsTruncationThenFailsForItsOwnReason;

  impl core::fmt::Debug for SwallowsTruncationThenFailsForItsOwnReason {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      // Longer than the window, so the sink genuinely refuses partway through this call —
      // and that `Err` is then thrown away rather than returned, standing in for a `Debug`
      // impl that does not treat a formatter failure as fatal to its own control flow.
      let _ = f.write_str(&"a".repeat(PREVIEW_WINDOW + 5));
      // Fails anyway, for a reason of its own, independent of the sink's refusal above.
      Err(core::fmt::Error)
    }
  }

  /// The residual documented on `bounded_debug` under "A residual this mechanism cannot
  /// resolve": once the sink has refused, a `Debug` impl that swallows that refusal and then
  /// fails again on its own account is absorbed into `WindowTruncated` — the same outcome as
  /// an impl that only ever let the sink's `Err` propagate. This is not a bug to fix here; it
  /// is pinning the documented limit of what `sink.truncated` and `wrote_fully` can tell
  /// apart. A fourth, more specific outcome would need evidence neither flag carries.
  #[test]
  fn bounded_debug_absorbs_a_foreign_failure_after_truncation_into_window_truncated() {
    let (window, outcome) = bounded_debug(&SwallowsTruncationThenFailsForItsOwnReason);

    // The prefix is exactly what the sink kept before refusing — real content, the same as an
    // ordinary truncation with no failure of the impl's own behind it.
    assert_eq!(window, "a".repeat(PREVIEW_WINDOW));
    assert_eq!(
      outcome,
      BoundedDebugOutcome::WindowTruncated,
      "a Debug impl that swallows the sink's truncation Err and then fails again for its own, \
       unrelated reason is absorbed into WindowTruncated — indistinguishable from an impl that \
       only ever let the sink's Err propagate — because sink.truncated and wrote_fully carry \
       no evidence that anything happened after the sink refused; this is the documented \
       residual, not a defect"
    );
  }

  /// Recomputes the whole old `trace_preview` window computation this fix's commit replaced:
  /// format the value, take [`PREVIEW_WINDOW`] characters, and append the ellipsis if the full
  /// dump was longer than that. This is the exact three-step body `trace_preview` used to have
  /// inline, before it was extracted into `bounded_debug` plus the `match` on
  /// [`BoundedDebugOutcome`] that still lives in `trace_preview` today.
  fn reference_preview_window<T: core::fmt::Debug + ?Sized>(value: &T) -> std::string::String {
    let dump = std::format!("{value:?}");
    let mut window: std::string::String = dump.chars().take(PREVIEW_WINDOW).collect();
    if dump.chars().count() > PREVIEW_WINDOW {
      window.push('\u{2026}');
    }
    window
  }

  /// Runs `value` through `bounded_debug` plus the same ellipsis step `trace_preview` applies
  /// to its result, and through [`reference_preview_window`]'s independent reimplementation of
  /// the old logic, then fails with `case` in the message if they disagree.
  #[track_caller]
  fn assert_matches_reference<T: core::fmt::Debug + ?Sized>(case: &str, value: &T) {
    let (mut window, outcome) = bounded_debug(value);
    if outcome == BoundedDebugOutcome::WindowTruncated {
      window.push('\u{2026}');
    }
    assert_eq!(
      window,
      reference_preview_window(value),
      "case {case:?}: bounded_debug (plus trace_preview's ellipsis step) disagreed with the \
       old format!-then-take(24)-then-ellipsis logic"
    );
  }

  /// **Characterization test, not a specification.** It pins `bounded_debug`'s output — plus
  /// the ellipsis step `trace_preview` applies on top of it — as byte-identical to the pre-fix
  /// `format!("{value:?}")` + `.chars().take(24)` + conditional-ellipsis logic it replaced
  /// (reimplemented independently in [`reference_preview_window`]), across a spread of lengths
  /// and escape-heavy content. That equivalence was previously checked only by a throwaway
  /// harness that no longer exists, so nothing in the suite would have caught a regression of
  /// it; this test is that proof, moved in-tree and made permanent. It documents today's
  /// behaviour, not a contract: a future, *deliberate* change to what `bounded_debug` or
  /// `trace_preview` returns is expected to update this test alongside it, not be blocked by it.
  ///
  /// The reference is a stand-in for the old logic, and the old logic is a faithful reference
  /// only for a **streaming** `Debug` impl — one whose writes interleave with the work that
  /// produces them. `str` and `[u8]`, used throughout below, both qualify (see `bounded_debug`'s
  /// doc comment for exactly how, and for the residual cost gaps that make them worth calling
  /// out explicitly even though their *output* is unaffected). A front-loading impl legitimately
  /// costs more without differing in output — that is
  /// `bounded_debug_is_correct_but_unbounded_for_a_front_loading_debug_impl` above, not this
  /// test.
  #[test]
  fn bounded_debug_matches_the_old_format_and_take_24_logic() {
    assert_matches_reference("empty", "");
    assert_matches_reference("shorter than the window", "hi");
    assert_matches_reference("exactly at the window", &"a".repeat(22));
    assert_matches_reference("one character past the window", &"a".repeat(23));
    assert_matches_reference("far longer than the window", &"a".repeat(500));
    assert_matches_reference("newlines", "first line\nsecond line\nthird line");
    assert_matches_reference("tabs", "col1\tcol2\tcol3");
    assert_matches_reference("double quotes", "she said \"hello\" and left");
    assert_matches_reference("single quotes", "it's fine, isn't it");
    assert_matches_reference("backslashes", r"a\b\c\path\to\thing");
    assert_matches_reference("non-ASCII", "héllo wörld — 你好，世界 🎉");
    assert_matches_reference(
      "a multi-byte scalar straddling the cut",
      &std::format!("{}€{}", "a".repeat(22), "b".repeat(10)),
    );
    assert_matches_reference(
      "control characters",
      "\u{0}\u{1}\u{7}\u{8}\u{b}\u{c}\u{1b}\u{7f}",
    );
    let bytes: std::vec::Vec<u8> = (0..40).collect();
    assert_matches_reference("the &[u8] Debug convention", bytes.as_slice());

    // The two boundary cases, pinned directly against `bounded_debug`'s own return value, not
    // only cross-checked against `reference_preview_window` above: an off-by-one shared by both
    // implementations would make them agree with each other while both are wrong.
    let (window, outcome) = bounded_debug(&"a".repeat(22));
    assert_eq!(window, std::format!("\"{}\"", "a".repeat(22)));
    assert_eq!(
      outcome,
      BoundedDebugOutcome::Complete,
      "a dump exactly PREVIEW_WINDOW characters long is not truncated"
    );

    let (window, outcome) = bounded_debug(&"a".repeat(23));
    assert_eq!(window, std::format!("\"{}", "a".repeat(23)));
    assert_eq!(
      outcome,
      BoundedDebugOutcome::WindowTruncated,
      "a dump one character past PREVIEW_WINDOW is truncated"
    );
  }

  /// A `Debug` impl whose complete, correct rendering happens to be the exact text
  /// `trace_preview` appends for [`BoundedDebugOutcome::FormatFailed`]. Nothing is wrong with
  /// this impl or its output; it simply renders, as ordinary content, the same characters the
  /// marker uses.
  struct RendersTheFmtErrorMarkerVerbatim;

  impl core::fmt::Debug for RendersTheFmtErrorMarkerVerbatim {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      f.write_str("abc <fmt error>")
    }
  }

  /// Pins the collision the `FormatFailed` arm in `trace_preview` documents: the marker is
  /// appended into the same text channel as the preview, so a value's own, complete rendering
  /// can be byte-identical to the rendered line for a value that failed. `trace_preview` itself
  /// is a method on a live `InputRef`, which this module's tests do not otherwise construct, so
  /// its one-line `FormatFailed` step (`push_str(" <fmt error>")`) is reproduced directly here
  /// against [`FailsForAForeignReason`]'s own output rather than called.
  #[test]
  fn fmt_error_marker_collides_with_a_debug_impl_that_renders_the_same_text() {
    let (window, outcome) = bounded_debug(&RendersTheFmtErrorMarkerVerbatim);
    assert_eq!(window, "abc <fmt error>");
    assert_eq!(
      outcome,
      BoundedDebugOutcome::Complete,
      "this value's Debug impl writes its complete output in one call and returns Ok — nothing \
       about it fails"
    );

    // trace_preview's own FormatFailed rendering step, reproduced rather than called: append
    // the marker text to whatever prefix a foreign failure left behind.
    let (failure_prefix, failure_outcome) = bounded_debug(&FailsForAForeignReason);
    assert_eq!(failure_outcome, BoundedDebugOutcome::FormatFailed);
    let mut rendered_after_failure = failure_prefix;
    rendered_after_failure.push_str(" <fmt error>");

    assert_eq!(
      window, rendered_after_failure,
      "a value that legitimately renders \"abc <fmt error>\" as its complete output is \
       byte-identical, in the rendered trace line, to a value that rendered \"abc\" and then \
       failed to format at all — the marker is a convention for a human reader, not reserved \
       syntax, and it collides with arbitrary Debug output"
    );
  }
}
