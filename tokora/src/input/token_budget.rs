//! The **input-layer token budget**: a durable ceiling on the items one `Input`'s
//! lexer is allowed to produce.

/// A ceiling on the number of items one `Input` will let its lexer produce —
/// **tokens and lexer errors alike** — enforced by the driver at the single lexing
/// chokepoint and **not refunded by a rollback**.
///
/// Default: [`unlimited`](Self::unlimited). A parse that does not configure one behaves exactly as
/// it did before this type existed.
///
/// # The gate is in front of the work
///
/// The ceiling is tested **before** the lexer is invoked, at `InputRef::lex_within_boundary` — the
/// crate's single lexing site, which **both** lexing drivers (the scanner behind every consume
/// path, and the peek fill) go through. An exhausted budget therefore does not merely refuse the
/// item; it declines to run the lexer at all.
///
/// That ordering is the bound, not a refinement of it. A ceiling asked *after* the work has
/// already happened refuses a produced item, and the refusal has to be recorded somewhere for the
/// caller to see — in the poison boundary, which a rollback restores, a
/// [`set_state`](crate::InputRef::set_state) drops, and a
/// [`state_mut`](crate::InputRef::state_mut) drops. The counter survives all three; the *stop*
/// does not. A public `attempt` that drains to the refusal and declines therefore re-enters
/// against the same unchanged `spent`, and the lexer runs again — once per call, without bound,
/// for free. Measured before the preflight landed: a budget of **zero** funded one full
/// `Lexer::lex` per re-entry (256 rounds → 256 invocations, `spent` still `0`) by all three
/// routes. **A durable counter is half a bound; the other half is enforcement that a rollback
/// cannot refund.**
///
/// # What it charges
///
/// One charge per item the lexer hands back, taken at that same site the moment an item exists —
/// so a step that produced none (end of input) charges nothing. Four consequences follow, and each
/// was a decision rather than an accident:
///
/// - **a lexer error is charged.** This is the shape the budget exists for. A plain (non-limit)
///   lexer error leaves [`Lexer::check`](crate::Lexer::check) `Ok`, so the scanner emits its
///   diagnostic and keeps looking for the next valid token: one valid token followed by *N* bytes
///   of garbage is **one accepted token** while *N* scans run and *N* diagnostics accumulate
///   durably in the emitter's log. A ceiling denominated in *accepted* items cannot see that.
///   Denominated in items *produced*, the same input costs *N*;
/// - **trivia is charged.** An item produced is work performed, and a vocabulary that surfaces
///   trivia is floodable through it exactly as it is through errors;
/// - **a peek-fill is charged when it is produced, not when it is consumed.** The work happens at
///   the fill. Charging at consumption would let a caller pay nothing for a cache it filled and
///   then abandoned;
/// - **a cached token replayed after a rollback is not charged again**, because nothing lexes: the
///   cache serves it and the lexing site is never reached. The budget prices *lexing*, not
///   consumption.
///
/// # What it does not bound, stated plainly
///
/// It bounds **items produced by one `Input`**, and no more than that:
///
/// - **not distinct document tokens.** A region the cache could not retain across a rollback is
///   re-lexed, and re-lexing charges again. So a heavily-speculating parse over a perfectly valid
///   document can reach a ceiling calibrated as "tokens in the document". That is the direction
///   this crate's own module docs already warn about — a cumulative counter "counts replay as
///   input and trips on valid documents" ([`input`](crate::input)) — and it is the price of a
///   count no rollback can rewind. Calibrate against produce-events, not against a token census;
/// - **not the cost of an item.** A lexer that scans a kilobyte to decide one token spends one
///   charge for it. A bound denominated in attempt cost is a different budget with a different
///   charging site, and this crate does not have one;
/// - **not a session.** It lives on one `Input`, and a
///   [`PartialSession`](crate::input::PartialSession) builds a fresh `Input` — and therefore a
///   fresh budget — for every redrive. What stops a session is the terminal latch this budget's
///   refusal feeds (see below) together with the session's own byte
///   [`Budget`](crate::input::Budget), which is denominated in Σ attempt-lexable bytes precisely
///   because *that* is the quantity an adversarial chunker controls.
///
/// # A met ceiling is not an end of input
///
/// The gate answers *"would an item be refused?"*. A driver needs *"is there an item?"*, and the two
/// come apart at exactly the place a calibrated ceiling lands: a budget of `N` over a document of
/// `N` items. Answering the first question as though it were the second reports a fully-parsed
/// document as a **terminal** stop — a terminal-aware consumer rejects it, and a
/// [`PartialSession`](crate::input::PartialSession) latches on it permanently. So the site settles
/// it in two steps, and neither one re-opens the rollback door the gate's position closed:
///
/// 1. **the end of the source**, positionally. With the lex position already at the end there is no
///    item and cannot be one, whatever the counter says. This costs no lexer call, and it covers a
///    zero budget over an empty source and every document whose last item ends at the last byte;
/// 2. **the one-shot probe**, for the residue step 1 cannot see: a **tail the lexer skips**. After
///    the last token of `"aa  "` the lex position is `2` and the source is `4` long, so the
///    positional test says input remains — and no item does. Every lexer that discards trailing
///    whitespace or a comment tail has that shape. Only running the lexer can tell the two apart, so
///    the site runs it **once**, and latches the outcome in a private field of this struct when it
///    produced.
///
/// The latch is what keeps step 2 from being the previous defect wearing a new hat. It rides beside
/// [`spent`](Self::spent) — same cell, same four arguments: not a [`Checkpoint`](super::Checkpoint)
/// field, so a rollback does not clear it; not reached by the state re-key behind
/// [`set_state`](crate::InputRef::set_state) or [`state_mut`](crate::InputRef::state_mut); and no
/// `token_budget_mut` exists to lower it. So the ceiling funds **one** `Lexer::lex` over the life of
/// one `Input`, however often its stop is re-opened, by any route.
///
/// A probe that produced **nothing** latches nothing, deliberately. It performed no work this budget
/// is denominated in, and its answer must stay re-derivable rather than frozen — asking an
/// already-drained input where the stream ended costs exactly one lexer call each time it is asked,
/// which is what that question costs on an input with no budget configured at all. The budget prices
/// items produced; a probe that produces none is priced at what the crate already charges for an end
/// of input.
///
/// # Exhaustion is terminal
///
/// The refusal latches the poison boundary and counts a scanner trip, so it travels the pipeline a
/// lexer-side resource trip already travels: [`next`](crate::InputRef::next) folds it into
/// `Ok(None)`, [`next_or_stop`](crate::InputRef::next_or_stop) and the `*_or_stop` family surface
/// the end-of-input error already marked terminal, and the recovery gates re-raise rather than
/// recover because the scanner-trip counter moved. It has to be terminal: a `PartialSession` that
/// read the refusal as an ordinary failure would re-drive forever, each redrive against a fresh
/// budget.
///
/// The stop is re-latched on **every** refused entry, and it has to be: the boundary is a
/// per-lineage memo that a rollback restores and a state re-key drops, so a refusal that latched
/// once and then trusted the latch would go quiet the first time one of those cleared it. The
/// ceiling is re-derived from [`spent`](Self::spent) instead, which none of them touch.
///
/// The one thing the refusal cannot do is *report itself*. There is no channel for it — a
/// diagnostic would have to be built as the emitter's own error type, which needs a `From` bound
/// this crate deliberately does not add to every consume path — so the item that would have
/// exhausted the budget is refused **silently**. That also means a lexer error at exactly that
/// position is not emitted: the item the one-shot probe produced is dropped where it stands, and
/// every later refusal never lexes at all.
///
/// # It is not the lexer's counter, and not the session's
///
/// | | denominated in | lives in | survives a rollback | who charges |
/// |---|---|---|---|---|
/// | `TokenBudget` | items produced | `Input` | **yes** | the driver |
/// | [`TokenLimiter`](crate::state::token_tracker::TokenLimiter) | whatever the lexer counts | `L::State` | no — a `Checkpoint` carries the state | the lexer impl |
/// | [`Budget`](crate::input::Budget) | Σ attempt-lexable bytes | `PartialSession` | n/a (cross-attempt) | the session |
///
/// The middle row is not a defect being replaced. A refund under rollback is the *correct*
/// semantics for a bound on tokens in the **committed** stream, and the terminal-trip pipeline is
/// built around it. The rows differ in what they are bounds *on*.
///
/// # No mutator
///
/// There is no `Input::token_budget_mut` and no `InputRef::token_budget_mut`, for the same reason
/// there is no `recursion_mut` and no `emitter_mut`: the cell has exactly one writer, and it is on
/// the driver's side of the seam. Configure it once, at
/// [`InputContext::with_token_budget`](super::InputContext::with_token_budget) or
/// [`ParserContext::with_token_budget`](crate::ParserContext::with_token_budget); read it through
/// [`InputRef::token_budget`](crate::InputRef::token_budget).
///
/// # Examples
///
/// ```rust
/// use tokora::input::TokenBudget;
///
/// // The default costs nothing and refuses nothing.
/// assert_eq!(TokenBudget::default(), TokenBudget::unlimited());
/// assert_eq!(TokenBudget::unlimited().limitation(), usize::MAX);
///
/// let budget = TokenBudget::with_limitation(1_000);
/// assert_eq!(budget.limitation(), 1_000);
/// assert_eq!(budget.spent(), 0);
/// assert!(!budget.is_exhausted());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TokenBudget {
  max: usize,
  spent: usize,
  /// The **one-shot at-limit probe**, spent. See the type docs' *A met ceiling is not an end of
  /// input* section for what the probe answers and why the answer has to be latched here rather
  /// than in the poison boundary beside it.
  ///
  /// It rides in this struct on purpose: every argument that makes [`spent`](Self::spent) a durable
  /// latch — no [`Checkpoint`](super::Checkpoint) field, untouched by the state re-key, no
  /// `token_budget_mut` — is an argument about the *cell*, `Input::token_budget`, and therefore
  /// covers this flag verbatim. A separate cell would have needed its own copy of all four, and a
  /// copy is a thing that can drift.
  probed_at_limit: bool,
}

impl Default for TokenBudget {
  /// [`unlimited`](Self::unlimited) — so a consumer that never names this type never changes
  /// behaviour.
  ///
  /// Deliberately unlike the session [`Budget`](super::Budget), which has **no** `Default` because
  /// it is a required constructor argument and the unsafe choice there must be impossible to make
  /// by omission. This one is a field with a stated default, and the default is the pre-existing
  /// behaviour rather than a guess at a safe ceiling: a number picked here would be wrong for
  /// every grammar, and picking one would break every consumer at once.
  #[inline(always)]
  fn default() -> Self {
    Self::unlimited()
  }
}

impl TokenBudget {
  /// No ceiling: the driver charges every item and refuses none.
  ///
  /// `usize::MAX` is the sentinel, and [`is_exhausted`](Self::is_exhausted) reads it as *the
  /// absence of a ceiling* rather than as a very large one — see there for the difference and why
  /// it is not cosmetic.
  #[inline(always)]
  pub const fn unlimited() -> Self {
    Self {
      max: usize::MAX,
      spent: 0,
      probed_at_limit: false,
    }
  }

  /// A ceiling of `max` items produced. The `max + 1`-th item the lexer hands back is refused.
  ///
  /// `with_limitation(usize::MAX)` **is** [`unlimited`](Self::unlimited) — the same value, and
  /// therefore the same behaviour. There is no ceiling above it to distinguish it from.
  #[inline(always)]
  pub const fn with_limitation(max: usize) -> Self {
    Self {
      max,
      spent: 0,
      probed_at_limit: false,
    }
  }

  /// The ceiling this budget was built with.
  #[inline(always)]
  pub const fn limitation(&self) -> usize {
    self.max
  }

  /// How many items the driver has charged against it.
  ///
  /// Monotone for the life of the `Input`: nothing lowers it, and a rollback does not put it back.
  #[inline(always)]
  pub const fn spent(&self) -> usize {
    self.spent
  }

  /// Whether the ceiling has been met, so that an item the lexer would hand back now is refused.
  ///
  /// **This is the gate**, and it is the whole of why the bound holds. The driver asks it *before*
  /// it invokes the lexer, and it is answered out of [`spent`](Self::spent) — a cell no
  /// [`Checkpoint`](super::Checkpoint) carries and no state re-key touches — so it is re-derived
  /// identically on every entry no matter what a rollback put back in between. Asking after the
  /// work, off a stop that *is* rollbackable, bounds nothing: see the type docs.
  ///
  /// A met ceiling is **not** an end of input, and this predicate does not claim to be one: it
  /// says an item *would* be refused, not that one exists. Which of the two the driver is looking
  /// at is settled at the lexing site, not here — see the type docs.
  ///
  /// # `usize::MAX` is the absence of a ceiling, not a ceiling
  ///
  /// The sentinel is excluded explicitly rather than left to `spent >= max`. Left to it, an
  /// [`unlimited`](Self::unlimited) budget charged `usize::MAX` times reaches `spent == max` and
  /// begins refusing **terminally** — the one budget that promised to refuse nothing. The distance
  /// is unreachable at a 64-bit `usize`; at 32 bits it is `4_294_967_295` produce-events, and this
  /// crate charges a *re-lex* as a produce-event, so a long-lived `Input` over a speculating
  /// grammar covers it without a four-billion-token document.
  ///
  /// The comparison order is load-bearing for cost, not only for meaning: `spent >= max` is false
  /// on every entry that is not at a ceiling, so the sentinel test is never reached on the hot
  /// path and this stays **one comparison per item** for bounded and unlimited budgets alike.
  #[inline(always)]
  pub const fn is_exhausted(&self) -> bool {
    self.spent >= self.max && self.max != usize::MAX
  }

  /// Charges the one item [`is_exhausted`](Self::is_exhausted) authorized.
  ///
  /// **Precondition: `!is_exhausted()`**, established by the driver's preflight immediately before
  /// it invoked the lexer, with nothing between the two that could spend (the budget has exactly
  /// one writer and it is reached through `&mut`).
  ///
  /// The increment **saturates**, and that is a consequence of the sentinel being excluded from the
  /// gate above rather than a belt-and-braces addition: `!is_exhausted()` no longer implies
  /// `spent < max`, because at `max == usize::MAX` it holds at every value of `spent` including
  /// `usize::MAX` itself. That is the one state where a bare `+= 1` wraps, and a wrap would hand an
  /// unbounded budget a silently restarted counter. Saturating is also the *correct* answer there
  /// and not merely a safe one: the sentinel has no ceiling to overshoot, so a `spent` that stops
  /// climbing reports the only thing a saturated counter can honestly report.
  ///
  /// Infallible **on purpose**. A second ceiling test here would be a second gate, and a second
  /// gate is how the enforcement ends up somewhere other than in front of the work — the defect
  /// this shape exists to close. The debug assertion is the witness that the one gate ran; should
  /// it ever fail in a release build the effect is `spent` overshooting `max`, which makes
  /// `is_exhausted` refuse *sooner*, never later.
  #[inline(always)]
  pub(crate) const fn spend(&mut self) {
    debug_assert!(
      !self.is_exhausted(),
      "TokenBudget::spend without the driver's exhaustion preflight",
    );
    self.spent = self.spent.saturating_add(1);
  }

  /// Whether this budget has **refused an item** — the one question a host that caught an unwind
  /// and concluded can still ask.
  ///
  /// `true` means the ceiling was met, the lexer was run, it handed back an item, and that item was
  /// refused. `false` means no item of this `Input`'s was ever refused by this budget. The bit is
  /// written **at the refusal**, in front of every consumer step the refusal runs, and nothing
  /// lowers it: it is not a [`Checkpoint`](super::Checkpoint) field, the state re-key behind
  /// [`set_state`](crate::InputRef::set_state) / [`state_mut`](crate::InputRef::state_mut) does not
  /// reach it, and there is no `token_budget_mut`. Read it through
  /// [`InputRef::token_budget`](crate::InputRef::token_budget).
  ///
  /// # Why it exists
  ///
  /// A refusal has **no diagnostic channel** (see the type docs), and
  /// [`next`](crate::InputRef::next) folds a terminal stop into `Ok(None)`. The in-band carriers —
  /// the scanner-trip counter and the poison boundary — are published before any consumer code
  /// runs, so an ordinary parse always sees them. What they cannot reach is a host that catches an
  /// unwind out of *its own* code (an `L::Offset` destructor, a `Cache` method, `Lexer::span`) and
  /// then concludes without re-entering the scanner: nothing crate-side runs there again. This is
  /// the answer for that host — *was this input truncated by the budget?* — and it is durable
  /// enough to survive the unwind that reached it.
  ///
  /// # What it does not answer
  ///
  /// Three bounds, and each one limits what a `true` (or a `false`) licenses:
  ///
  /// - **it witnesses this budget only.** The other terminal scanner stop — the lexer's own limit
  ///   trip, latched by `InputRef::latch_if_limit_tripped` off
  ///   [`Lexer::check`](crate::Lexer::check) — never writes it. That tally lives in `L::State` and
  ///   a [`Checkpoint`](super::Checkpoint) carries it, so no durable bit here could speak for it.
  ///   `false` therefore means *the budget refused nothing*, not *the parse was not truncated*;
  /// - **it is session-absolute, not attempt-relative.** It says an item was refused somewhere in
  ///   the life of this `Input`, never *inside my window*, so a window opened after the first
  ///   refusal reads the same `true` whether or not anything was refused inside it. A caller
  ///   judging one [`attempt`](crate::InputRef::attempt) on its own terms wants the crate's
  ///   scanner-trip counter against a baseline taken at the attempt's start, which is what every
  ///   in-crate reader of terminality uses and what this deliberately is not. In band, on the paths
  ///   that do not unwind, the signal a caller already has is the terminal-marked
  ///   [`UnexpectedEot`](crate::error::UnexpectedEot) the `*_or_stop` family raises, read through
  ///   [`MaybeTerminal::is_terminal`](crate::error::MaybeTerminal);
  /// - **it does not survive a [`PartialSession`](super::PartialSession) redrive.** A redrive
  ///   builds a fresh `Input` and therefore a fresh budget, at zero. What carries a refusal across
  ///   attempts is the session's own terminal latch, which the refusal feeds by being terminal.
  ///
  /// # Examples
  ///
  /// ```rust
  /// use tokora::input::TokenBudget;
  ///
  /// // Nothing has been refused until something is.
  /// assert!(!TokenBudget::with_limitation(4).refused_an_item());
  /// assert!(!TokenBudget::unlimited().refused_an_item());
  /// ```
  #[inline(always)]
  pub const fn refused_an_item(&self) -> bool {
    self.probed_at_limit
  }

  /// Whether the **one-shot at-limit probe** has already been spent on an item.
  ///
  /// Read by the lexing site once the ceiling is met and the lex position is short of the end of
  /// the source — the one situation where "a met ceiling" and "an end of input" are not the same
  /// answer and nothing but running the lexer can tell them apart. `true` means a probe already ran
  /// there and *produced*, so the answer is settled: refuse, and do not lex again.
  ///
  /// The same bit as [`refused_an_item`](Self::refused_an_item), under the name the driver asks it
  /// by: a probe that produced is exactly a refusal, so "the answer is on record" and "an item was
  /// refused" are one fact with two audiences. It is delegated rather than duplicated so the field
  /// keeps one reader.
  #[inline(always)]
  pub(crate) const fn limit_probe_spent(&self) -> bool {
    self.refused_an_item()
  }

  /// Latches the one-shot probe, for a probe that **produced an item**.
  ///
  /// Monotone, and latched only on the producing outcome. A probe that produced nothing performed
  /// no work this budget is denominated in, and latching *it* would freeze an end-of-input answer
  /// that must stay re-derivable — the same reason the refusal re-latches its boundary on every
  /// entry instead of trusting the latch.
  #[inline(always)]
  pub(crate) const fn latch_limit_probe(&mut self) {
    self.probed_at_limit = true;
  }
}

#[cfg(test)]
mod tests;
