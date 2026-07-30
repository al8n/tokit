//! `syntax_kinds!` — the declaration-side complement to the sink-side
//! [`KindValidator`](crate::cst::KindValidator).
//!
//! The sink validates the kinds a dialect emits. Nothing validated the *declaration*, so a
//! dialect hand-kept three things that had to agree — the `#[repr(u16)]` enum, the
//! declaration-order array its consumers index, and the predicate it handed to
//! [`CstProfile`](crate::cst::CstProfile) — and a disagreement between them is exactly the
//! shape the sink's own enforcement cannot see, because it only ever sees one side.
//!
//! [`syntax_kinds!`](crate::syntax_kinds) generates all three from the one declaration, so
//! the colliding declaration becomes inexpressible: the kind space is const-asserted clear
//! of the reserved [`TOMBSTONE`](crate::cst::event::TOMBSTONE) band at compile time, and the
//! validator is derived from `COUNT` rather than written a second time.
//!
//! This module is **not** `rowan`-gated, and neither is what it names: `CstProfile` and
//! `KindValidator` are fn-pointer and data only, so a dialect can declare its kind space —
//! and hand the sink a validator for it — in a `no_std`, no-`rowan` build. A
//! `macro_rules!` expansion cannot `cfg` on tokora's own features, so if either name were
//! gated the whole macro would be, in every consumer that cannot enable `rowan`.

/// Declares a CST kind space: a `#[repr(u16)]` enum plus the invariants tokora's CST sink
/// relies on, generated instead of hand-kept.
///
/// Generates, from one declaration:
///
/// - the `#[repr(u16)]` enum itself;
/// - `COUNT` — how many kinds the dialect declares;
/// - `ALL` — every kind in **declaration order**, the stability property consumers index
///   into and hand-maintain today;
/// - `raw()` and the checked inverse `from_raw()`, which returns `None` for anything
///   outside the declared space (including the reserved tombstone band);
/// - `validator()` — the dialect's [`KindValidator`](crate::cst::KindValidator), derived
///   from `COUNT`, so the declaration and the sink's validation cannot disagree;
/// - a `const` assertion that the kind space clears
///   [`TOMBSTONE`](crate::cst::event::TOMBSTONE) — see the note below on what that
///   assertion can and cannot catch.
///
/// # Example
///
/// ```
/// tokora::syntax_kinds! {
///   /// GraphQL CST kinds.
///   pub enum GraphqlKind {
///     Name,
///     Int,
///     Float,
///     Document,
///   }
/// }
///
/// // Declaration order is the raw numbering, and it is stable by construction.
/// assert_eq!(GraphqlKind::COUNT, 4);
/// assert_eq!(GraphqlKind::Int.raw(), 1u16);
/// assert_eq!(GraphqlKind::ALL[3], GraphqlKind::Document);
///
/// // `from_raw` is checked: outside the declared space is `None`, not a transmute.
/// assert_eq!(GraphqlKind::from_raw(1), Some(GraphqlKind::Int));
/// assert_eq!(GraphqlKind::from_raw(4), None);
/// assert_eq!(GraphqlKind::from_raw(u16::MAX), None);
///
/// // The validator admits exactly the declared space — it is derived from `COUNT`,
/// // not written a second time — and the sink's own construction-time enforcement is
/// // what reads it.
/// use tokora::cst::CstProfile;
///
/// fn map(k: &GraphqlKind) -> u16 { k.raw() }
///
/// // A profile whose synthesized kinds are inside the declared space is accepted.
/// let profile = CstProfile::new(
///   map as fn(&GraphqlKind) -> u16,
///   GraphqlKind::validator(),
///   GraphqlKind::Name.raw(),
///   GraphqlKind::Document.raw(),
/// );
/// let _ = profile;
///
/// // One outside it is refused at construction, by the generated validator.
/// let refused = std::panic::catch_unwind(|| {
///   CstProfile::new(map as fn(&GraphqlKind) -> u16, GraphqlKind::validator(), 4, 0)
/// });
/// assert!(refused.is_err(), "an undeclared kind must not reach a sink");
/// ```
///
/// An empty kind space does not compile:
///
/// ```compile_fail
/// tokora::syntax_kinds! {
///   pub enum Nothing {}
/// }
/// ```
///
/// Two things refuse it, and it is worth knowing which, because only one of them is the
/// macro's own choice. The matcher requires one or more variants; relax that and the
/// **generated `#[repr(u16)]`** refuses it instead —
/// `error[E0084]: unsupported representation for zero-variant enum` (measured, by relaxing
/// the matcher and reading what fails). Either way the refusal is worth having: a zero-kind
/// dialect would produce `COUNT == 0`, an empty `ALL`, and a `validator()` that admits
/// **nothing**, so the first `CstProfile::new` built from it would panic on its own
/// synthesized error kind.
///
/// # What stops an oversized kind space, stated exactly
///
/// The generated `const` assertion reads `COUNT <= TOMBSTONE`. At the shipped value —
/// [`TOMBSTONE`](crate::cst::event::TOMBSTONE) is `u16::MAX` — **that assertion cannot
/// fire**: `COUNT` is a `u16`, so it is `<= u16::MAX` by its type. It is a tripwire on
/// *tokora's own reserved-band constant*, not on your declaration: it fires only if a
/// future release moves the reserved band below `u16::MAX` while a dialect is already
/// declaring past it. It is kept for that reason, and it is not what stops you today.
///
/// What stops you today is the counter itself: declaring 65 536 variants makes `COUNT`
/// overflow during const evaluation, and const evaluation refuses to wrap —
/// `error[E0080]: attempt to compute u16::MAX + 1_u16, which would overflow`. So an
/// oversized kind space is a compile error either way; it is worth knowing which of the two
/// walls you would actually hit, because only one of them names the tombstone.
///
/// # Availability
///
/// No feature gate: the generated `validator()` names
/// [`KindValidator`](crate::cst::KindValidator), which is available in every configuration
/// including `no_std` without `rowan`. That is deliberate — a `macro_rules!` expansion
/// cannot `cfg` on tokora's features, so a gated `KindValidator` would gate the whole
/// macro, in exactly the consumers least able to enable `rowan`.
#[macro_export]
macro_rules! syntax_kinds {
  (
    $(#[$meta:meta])*
    $vis:vis enum $name:ident { $($(#[$vmeta:meta])* $variant:ident),+ $(,)? }
  ) => {
    $(#[$meta])*
    #[repr(u16)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    $vis enum $name {
      $($(#[$vmeta])* $variant,)+
    }

    impl $name {
      /// How many kinds this dialect declares.
      $vis const COUNT: u16 = {
        let mut n: u16 = 0;
        $( { let _ = Self::$variant; n += 1; } )+
        n
      };

      /// Every kind, in declaration order — the stability property consumers
      /// hand-maintain today, generated.
      $vis const ALL: [Self; Self::COUNT as usize] = [$(Self::$variant),+];

      /// The raw `u16` the CST event stream carries.
      #[inline(always)]
      $vis const fn raw(self) -> u16 {
        self as u16
      }

      /// The checked inverse of [`raw`](Self::raw): `None` for anything outside the
      /// declared space, including the reserved tombstone band.
      #[inline(always)]
      $vis const fn from_raw(raw: u16) -> ::core::option::Option<Self> {
        if raw < Self::COUNT {
          // A generated comparison chain over the declaration, so this stays checked —
          // there is no transmute here and no `unsafe`.
          let mut i: u16 = 0;
          $(
            if i == raw { return ::core::option::Option::Some(Self::$variant); }
            i += 1;
          )+
          let _ = i;
          ::core::option::Option::None
        } else {
          ::core::option::Option::None
        }
      }

      /// The dialect's `tokora::cst::KindValidator`: admits exactly the declared kinds,
      /// so the sink's validation and this declaration cannot disagree.
      #[inline(always)]
      $vis const fn validator() -> $crate::cst::KindValidator {
        $crate::cst::KindValidator::new(|raw| raw < Self::COUNT)
      }
    }

    // The reserved-band door, closed at declaration. `<=` because `COUNT` is one past the
    // last raw value: the last declared kind is `COUNT - 1`, which must be `< TOMBSTONE`.
    const _: () = ::core::assert!(
      $name::COUNT <= $crate::cst::event::TOMBSTONE,
      "syntax_kinds!: the declared kind space collides with tokora's reserved TOMBSTONE"
    );
  };
}
