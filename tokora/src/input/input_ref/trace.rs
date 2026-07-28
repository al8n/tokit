//! `InputRef` tracing hooks — the `trace` feature.
//!
//! The enter / exit / leaf event emitters and the source preview they print. Kept beside
//! [`InputRef`](super::InputRef) so they can reach its `pub(super)` `depth` field. Every line
//! is handed to [`crate::trace::write_line`], which routes it out of band (stderr, or the
//! test capture buffer) — never through the emitter.

use super::{Completeness, InputRef};
use crate::{Lexer, ParseContext, source::Source};

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
{
  /// A short, generic preview of the source at the cursor: the current offset plus a debug
  /// window of the remaining source, truncated. Cheap and works for every [`Source`].
  fn trace_preview(&self) -> std::string::String {
    let off = self.offset();
    match self.source().slice(off.clone()..) {
      Some(rest) => {
        let dump = std::format!("{rest:?}");
        let mut window: std::string::String = dump.chars().take(24).collect();
        if dump.chars().count() > 24 {
          window.push('\u{2026}');
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
