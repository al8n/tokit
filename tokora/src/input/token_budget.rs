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
/// exhausted the budget is refused **silently**, and never lexed. That also means a lexer error at
/// exactly that position is not emitted: the budget declined to authorize the work that would have
/// produced the diagnostic.
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
  #[inline(always)]
  pub const fn unlimited() -> Self {
    Self {
      max: usize::MAX,
      spent: 0,
    }
  }

  /// A ceiling of `max` items produced. The `max + 1`-th item the lexer hands back is refused.
  #[inline(always)]
  pub const fn with_limitation(max: usize) -> Self {
    Self { max, spent: 0 }
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

  /// Whether the next item would be refused.
  ///
  /// **This is the gate**, and it is the whole of why the bound holds. The driver asks it *before*
  /// it invokes the lexer, and it is answered out of [`spent`](Self::spent) — a cell no
  /// [`Checkpoint`](super::Checkpoint) carries and no state re-key touches — so it is re-derived
  /// identically on every entry no matter what a rollback put back in between. Asking after the
  /// work, off a stop that *is* rollbackable, bounds nothing: see the type docs.
  #[inline(always)]
  pub const fn is_exhausted(&self) -> bool {
    self.spent >= self.max
  }

  /// Charges the one item [`is_exhausted`](Self::is_exhausted) authorized.
  ///
  /// **Precondition: `!is_exhausted()`**, established by the driver's preflight immediately before
  /// it invoked the lexer, with nothing between the two that could spend (the budget has exactly
  /// one writer and it is reached through `&mut`). So this cannot push [`spent`](Self::spent) past
  /// [`limitation`](Self::limitation), and no `saturating_add` is needed: the increment runs only
  /// when `spent < max <= usize::MAX`.
  ///
  /// Infallible **on purpose**. A second ceiling test here would be a second gate, and a second
  /// gate is how the enforcement ends up somewhere other than in front of the work — the defect
  /// this shape exists to close. The debug assertion is the witness that the one gate ran; should
  /// it ever fail in a release build the effect is `spent` overshooting `max`, which makes
  /// `is_exhausted` refuse *sooner*, never later.
  #[inline(always)]
  pub(crate) const fn spend(&mut self) {
    debug_assert!(
      self.spent < self.max,
      "TokenBudget::spend without the driver's exhaustion preflight",
    );
    self.spent += 1;
  }
}

#[cfg(test)]
mod tests;
