//! The recovery state of a syntax carrier, the trait that reads it, and the parts it decomposes
//! into.
//!
//! [`Status`] is re-exported as `types::Status`, so a consumer globbing `tokora::types::*` has the
//! status type without naming this module. [`RecoveryState`], [`FromComponents`] and
//! [`Components`] are reached through this path.
//!
//! # How a name added to a globbed namespace behaves, by kind
//!
//! This table is a measured fact and is the durable result of the rounds that produced it. What it
//! is **not** is the reason for the placement above — see *The line this API is placed on*.
//!
//! Measured against `rustc 1.100.0-nightly` with the consumer's source held fixed and the library
//! gaining one item — and with **both sides exposing the item the consumer reaches for**, which is
//! what the first attempt got wrong:
//!
//! | kind       | vs an extern crate of that name  | vs a second glob            | vs a local def |
//! |------------|----------------------------------|-----------------------------|----------------|
//! | **module** | **silent redirect**              | `E0659`                     | local wins     |
//! | **type**   | **silent redirect**              | `E0659`                     | local wins     |
//! | **trait**  | `E0790`                          | **silent, first glob wins** | local wins     |
//! | fn / const | no interaction (value namespace) | `E0659`                     | local wins     |
//!
//! The `type` row was first measured as `E0599`, loud. That was wrong, and wrong in a way worth
//! recording: the probe asked for an associated item the shadowing type did not have, so it could
//! only be loud. With a dependency aliased `Status` exporting `Error`, and tokora's `Status`
//! carrying an `Error` variant with an inherent `is_error()`, `Status::Error.is_error()` compiles
//! on both revisions and **the boolean inverts**. A collision provoked by asking for something one
//! side lacks is a verdict about the probe, not about the kind.
//!
//! So **no kind that can carry this API is loud on both axes.** That is the finding, and it holds.
//!
//! # The line this API is placed on
//!
//! **tokora must not silently change what a method on a tokora type means. A name a consumer's own
//! glob shadows is that glob's cost, and `::` is its fix.**
//!
//! Those are two different axes, and almost every repair in tokora#320 is the first one.
//! `literal.is_valid()`, where `literal` is a `Lit*`, an [`Ident`](super::Ident) or a
//! [`Keyword`](super::Keyword): the consumer is holding **tokora's** type, calling what they
//! believe is their own check, and getting tokora's answer. That is tokora changing what its own
//! API means underneath them, with nothing at the call site to say so. Moving the three questions
//! onto [`RecoveryState`], withdrawing an inherent `status`, withdrawing an inherent `with_status`
//! for [`FromComponents`], and making [`Components`] a braced struct are all that one class.
//!
//! The names in this module's *path* — `Status` in `types::*`, and `recovery` itself — are the
//! second axis. Neither is a call on a tokora type. Both are a path into the consumer's own
//! namespace being shadowed by a glob **they** wrote, in code that has nothing to do with tokora's
//! carriers. That is namespace hygiene: the ordinary cost of `use ...::*`, whose remedy the
//! language already provides. `::recovery::Recovery` and `::my_status::Status` reach past any glob,
//! and it is the same one-line remedy in both cases.
//!
//! This matters because the earlier reasoning used *loud versus silent* as the deciding property,
//! and that does not survive: of these two names the one that clashes loudly is the module, and the
//! one that can clash quietly is `Status`. Sorting them that way would keep the wrong one. Sorting
//! by **whose type the call is on** puts both outside the line and leaves every carrier repair
//! inside it.
//!
//! It is also a rule nothing could follow. If a public name may not be added because some crate
//! might share it, no public module or vocabulary type can ever be added again.
//!
//! # Both clashes, measured
//!
//! Recorded so nobody re-derives them. They are evidence for the placement above, not open
//! questions.
//!
//! **`Status`, quiet.** A consumer whose own dependency is aliased `Status` and who writes
//! `use tokora::types::*;` resolves `Status::Error.is_error()` here rather than to their crate.
//! Compiled both ways against a real second crate: the boolean reads `false` before and `true`
//! after. Disambiguated by `::my_status::Status`.
//!
//! **`recovery`, loud.** A crate of that name **is published** — 0.1.6, ~16.1k downloads, last
//! updated 2025-09-14, checked against the crates.io API on **2026-08-27** — and its documentation
//! instructs `use recovery::Recovery;`. `Recovery` is not one of this module's four names, so a
//! consumer of both crates gets `E0432` at their own import line, plus `E0659` where the name is
//! then used. Measured, not assumed. Disambiguated by `::recovery::Recovery`.
//!
//! A rename of this module was drafted when the deciding property was still loud-versus-silent,
//! and withdrawn. If the check above goes stale — that crate yanked, or one published that exports
//! a name this module also has — the date is what makes the claim re-checkable instead of
//! re-argued.

/// Reachable both ways: as `types::Status`, which is where a glob finds it, and through this
/// module beside the rest of the recovery API.
///
/// ```rust
/// use tokora::types::Status;
/// use tokora::types::recovery::{Components, FromComponents, RecoveryState, Status as Same};
/// # const _: () = { let _: fn(Status) -> Same = |s| s; };
/// ```
///
/// The traits and [`Components`] are **not** in `types::*`, so they are named rather than globbed:
///
/// ```compile_fail,E0432
/// use tokora::types::RecoveryState;
/// ```
/// The recovery state of a syntax carrier: whether the parser found the construct, found
/// something malformed where it should have been, or found nothing at all.
///
/// [`Ident`](super::Ident), [`Keyword`](super::Keyword) and every `Lit*` type hold one of these.
/// The rule they are all keeping — that a carrier reports its recovery state through this field
/// and never through its payload — is stated once, on
/// [`ErrorNode`](crate::error::ErrorNode), under *Two Shapes of Implementor*.
///
/// # Why it is a value a consumer can hold
///
/// It is not decoration on the carrier: it is the third thing every carrier is made of, beside
/// the span and the payload. So it appears in
/// [`IntoComponents::Components`](crate::utils::IntoComponents::Components) — which promises a
/// *complete* decomposition — and it is accepted back by [`FromComponents`], the inverse over
/// the same associated type. Those two are inverses, which is what makes the round trip an identity in all
/// three states; without the status in both, a decompose-and-rebuild would report every
/// recovered node as valid syntax, the same laundering tokora#303 removed from `Ident::map`.
///
/// A consumer does not have to interpret it to carry it, and mostly should not: pass it through
/// [`FromComponents`], or ask it one of the three questions below.
///
/// # Non-exhaustive
///
/// A fourth recovery state is admissible in a future version, so a `match` on this enum needs a
/// wildcard arm. [`is_valid`](Self::is_valid) is not `!is_error() && !is_missing()` for that
/// reason — ask the question you mean.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub enum Status {
  /// The parser found the construct, spelled out in the source.
  Valid,
  /// The parser found something where the construct should have been, and it was malformed —
  /// the state [`ErrorNode::error`](crate::error::ErrorNode::error) declares.
  Error,
  /// The parser found nothing where the construct was required — the state
  /// [`ErrorNode::missing`](crate::error::ErrorNode::missing) declares.
  Missing,
}

impl Status {
  /// Returns `true` if this is the malformed-content state.
  #[inline(always)]
  pub const fn is_error(self) -> bool {
    matches!(self, Self::Error)
  }

  /// Returns `true` if this is the absent-content state.
  #[inline(always)]
  pub const fn is_missing(self) -> bool {
    matches!(self, Self::Missing)
  }

  /// Returns `true` if this is the found-in-the-source state.
  #[inline(always)]
  pub const fn is_valid(self) -> bool {
    matches!(self, Self::Valid)
  }
}

/// Reads the recovery state of a syntax carrier: [`Ident`](super::Ident),
/// [`Keyword`](super::Keyword) and every `Lit*` type implement it.
///
/// # Why the three questions live on a trait
///
/// `is_valid`, `is_error` and `is_missing` are names a consumer may already have on their own
/// extension trait, meaning something of their own — "this literal's payload parses as a number",
/// say. Rust prefers an **inherent** method over a trait's at an unqualified call site, so
/// shipping these as inherent methods would have made `literal.is_valid()` keep compiling and
/// quietly stop calling the consumer's check, with no diagnostic anywhere. tokora#320 widened
/// [`IntoComponents`](crate::utils::IntoComponents) rather than withdrawing it precisely because
/// widening fails loudly, and the same standard applied here rules inherent predicates out: a
/// silent change of meaning on upgrade is the one outcome that standard exists to prevent.
///
/// On a trait the clash cannot be silent. A consumer who imports both gets an ambiguity error
/// naming both candidates and picks with `RecoveryState::is_valid(&x)` or their own equivalent; a
/// consumer who imports neither, or only their own, keeps exactly the behaviour they had.
///
/// # `status` is on this trait too, and nowhere else
///
/// It was briefly an inherent `const fn` on each carrier, on the reasoning that a *return* type
/// which did not exist before could not be displaced silently: a consumer whose own `status()`
/// returned something else would get a type error. **That reasoning is wrong**, and the
/// counterexample is one line:
///
/// ```rust,ignore
/// literal.status().is_valid()
/// ```
///
/// A differing return type is only loud if the caller *stores* the value. When the next step is a
/// method both types offer — and [`Status`] offers exactly the names a consumer's semantic status
/// is most likely to carry — the whole chain still typechecks and the meaning changes underneath
/// it. Since `new` marks any caller-supplied payload [`Status::Valid`], a literal their own check
/// would have rejected then passes.
///
/// So the test a name has to survive is not *does the signature differ* but **does the whole call
/// chain still typecheck with a different meaning**, and an inherent accessor of any name fails
/// it whenever the returned types share a method. There is no inherent reader left to displace
/// anything: `x.status()` and `Carrier::status(&x)` both resolve here.
///
/// # What that costs
///
/// A trait method cannot be `const fn`, so the recovery state is **not readable in a const
/// context** by any spelling. Nothing in this crate read it in one, and the state is a parse
/// result, which is not const-constructible in practice.
///
/// Two ways to keep const were available and both are worse. A `pub` field is reachable by
/// nothing method-call syntax can displace — but a public field is a public *setter*, and
/// `x.status = Status::Valid` would launder a recovery placeholder into valid syntax by
/// assignment, which is the whole defect this type exists to close. A receiver-less associated
/// function is unreachable by method syntax, but `Carrier::status(&x)` in path position displaces
/// a consumer's trait item the same way, so it trades a certainty for an improbability — and the
/// reasoning it would rest on is the reasoning that was just wrong.
pub trait RecoveryState {
  /// The recovery state this value is in.
  fn status(&self) -> Status;

  /// Returns `true` if the parser found this construct spelled out in the source.
  #[inline]
  fn is_valid(&self) -> bool {
    self.status().is_valid()
  }

  /// Returns `true` if this value is a malformed-content placeholder.
  #[inline]
  fn is_error(&self) -> bool {
    self.status().is_error()
  }

  /// Returns `true` if this value is an absent-content placeholder.
  #[inline]
  fn is_missing(&self) -> bool {
    self.status().is_missing()
  }
}

/// The parts a recovery carrier decomposes into: a span, a payload, and the recovery state.
///
/// This is [`IntoComponents::Components`](crate::utils::IntoComponents::Components) for
/// [`Ident`](super::Ident), [`Keyword`](super::Keyword) and every `Lit*` type, the return of
/// `Keyword`'s inherent `into_components`, and the argument of
/// [`FromComponents::from_components`]. One shape in all three
/// places, so the decomposition and its inverse cannot disagree about what a part is.
///
/// # Why a struct and not a three-tuple
///
/// The three-tuple came first, and it was defended as safe on the grounds that widening the old
/// `(Span, Payload)` changes the **arity**, so every existing destructuring breaks loudly. That
/// is false, and one line defeats it:
///
/// ```rust,ignore
/// let (_, .., value) = literal.into_components();
/// value.is_valid()
/// ```
///
/// `..` matches zero elements against the old pair and one against the triple, so `value` is the
/// payload before and the status after. Both compile whenever the payload also answers
/// `is_valid`, and since `new` assigns [`Status::Valid`], a payload the caller's own check
/// rejects then reports valid. `(.., b)` and `t.1` are the same defect in other spellings.
///
/// That was the third exemption of its kind on this branch to be refuted — after "the return type
/// differs" and "the argument type is a `Status`" — and all three failed the same way: each
/// estimated *what a consumer could have written* and estimated it too small. So this is not a
/// better-argued exemption. It is a shape that **no** tuple pattern can match, whatever it binds
/// and however it spells the rest, because a braced struct is not a tuple. `let (a, b)`,
/// `(_, b)`, `(a, ..)`, `(_, .., b)`, `(.., b)`, a trailing comma, `ref` bindings, a `match` arm,
/// `.0` and `.1` were each compiled against both shapes: all ten build on 0.9 and all ten are
/// rejected here, with `E0308` for the patterns and `E0609` for the index accesses.
///
/// It is also the better API. Three positional parts whose third is a status is exactly the shape
/// that made `..` dangerous; a named field says which is which.
///
/// # Every pre-existing tuple pattern, refused
///
/// Each of these is the source a 0.9 consumer wrote against `(Span, Payload)`, and each is now a
/// compile error with the code named — so a stale destructuring cannot survive the upgrade in any
/// spelling, whatever it binds and however it elides the rest.
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let (span, payload) = lit.into_components();
/// ```
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let (_, payload) = lit.into_components();
/// ```
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let (span, ..) = lit.into_components();
/// ```
///
/// The one that made a three-tuple unsafe — `..` matches zero elements against a pair and one
/// against a triple, so this bound the payload before and the status after:
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let (_, .., value) = lit.into_components();
/// ```
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let (.., value) = lit.into_components();
/// ```
///
/// ```compile_fail,E0308
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// match lit.into_components() { (_, payload) => { let _ = payload; } }
/// ```
///
/// Index access is refused too, with its own code:
///
/// ```compile_fail,E0609
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let _ = lit.into_components().1;
/// ```
///
/// ```compile_fail,E0609
/// # use tokora::{SimpleSpan, types::LitDecimal, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let _ = lit.into_components().0;
/// ```
///
/// What replaces them binds by name, so no elision can slide a binding onto the status:
///
/// ```rust
/// # use tokora::{SimpleSpan, types::{LitDecimal, recovery::Components}, utils::IntoComponents};
/// # let lit = LitDecimal::<&str>::new(SimpleSpan::new(0, 2), "42");
/// let Components { span, payload, status } = lit.into_components();
/// assert_eq!(payload, "42");
/// assert!(status.is_valid());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Components<Span, Payload> {
  /// Where the construct was, or would have been.
  pub span: Span,
  /// The spelling the carrier holds. For a recovery placeholder this is whatever
  /// [`ErrorNode`](crate::error::ErrorNode) produced, and it is **not** the recovery channel —
  /// read `status`.
  pub payload: Payload,
  /// Whether the parser found the construct, found something malformed, or found nothing.
  pub status: Status,
}

/// Rebuilds a syntax carrier from the components [`IntoComponents`](crate::utils::IntoComponents)
/// took it apart into — the exact inverse, over the same associated type.
///
/// ```rust
/// use tokora::{SimpleSpan, error::ErrorNode, types::{LitDecimal, recovery::FromComponents}};
/// use tokora::utils::IntoComponents;
///
/// let placeholder = LitDecimal::<&str>::error(SimpleSpan::new(0, 3));
/// let rebuilt = LitDecimal::<&str>::from_components(placeholder.into_components());
///
/// assert_eq!(rebuilt, placeholder);
/// ```
///
/// # Why this replaced a `with_status` constructor
///
/// The same job was briefly done by a `pub const fn with_status(span, payload, status)` inherent
/// on each of the nineteen carriers, on the argument that its `Status` parameter could not be
/// supplied by any expression written before that type existed. **That is false for a
/// type-directed expression**, and it was measured rather than argued: an unchanged
/// `unsafe { core::mem::zeroed() }`, `unsafe { core::mem::transmute::<u8, _>(0) }` or
/// `unsafe { MaybeUninit::uninit().assume_init() }` in that argument position is inferred as a
/// consumer's own status enum before the upgrade and as [`Status`] after. Both revisions compile,
/// there is no ambiguity, and dispatch moves from the consumer's trait to tokora's inherent
/// function — with `Status::Valid` at discriminant zero, a consumer's zero-valued *rejection*
/// state becomes valid syntax. The `MaybeUninit` route is worse still: it yields whatever the
/// stack held.
///
/// Bounded routes are not affected and were measured too — `Default::default()`, `x.into()`, an
/// associated constant and a generic `fn mk<T: Default>() -> T` all fail with `E0277` or `E0308`,
/// because each needs a trait impl or a named type that [`Status`] does not provide. The silent
/// routes are exactly the *unbounded* ones.
///
/// So the constructor is a trait method rather than an inherent one, which puts it where a
/// consumer's inherent item outranks it instead of the other way round, and this module is not
/// glob-re-exported, so it has to be named. It takes one argument whose type is an associated
/// type of the implementor, and it is reachable by construction to anyone who can decompose:
/// `T::from_components(x.into_components())` is the round trip, and it is total in all three
/// recovery states.
pub trait FromComponents: crate::utils::IntoComponents + Sized {
  /// Rebuilds the value from its components, carrying the recovery state across unchanged.
  fn from_components(components: Self::Components) -> Self;
}
