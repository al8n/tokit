/// The recovery state of a syntax carrier, and the field the `is_valid` / `is_error` /
/// `is_missing` predicates read.
///
/// `Ident`, `Keyword` and every `Lit*` type hold one of these. The rule they are all keeping —
/// that a carrier reports its recovery state through a status field and never through the
/// payload — is stated once, on the `ErrorNode` trait, under *Two Shapes of Implementor*; this
/// type is that rule's single implementation, which is why it lives here rather than in
/// `ident.rs` where tokora#266 first needed it.
///
/// What is local to this file is the mechanics:
///
/// - `new` is the door that declares a carrier valid, and the `ErrorNode` constructors are the
///   two doors that declare it recovered. There is no other way to set this field.
/// - Every operation that rebuilds a carrier — `Ident::map`, `Keyword::map`,
///   `From<Keyword> for Ident` — destructures it by name, so a rebuild that stops carrying it
///   fails to compile instead of silently reporting a placeholder as valid syntax. That is what
///   tokora#303 installed on `Keyword` before there was a status for it to catch, and what
///   tokora#301 gave it something to catch.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
#[repr(u8)]
#[non_exhaustive]
pub(super) enum Status {
  Valid,
  Error,
  Missing,
}

impl Status {
  #[inline(always)]
  pub(super) const fn is_error(self) -> bool {
    matches!(self, Self::Error)
  }

  #[inline(always)]
  pub(super) const fn is_missing(self) -> bool {
    matches!(self, Self::Missing)
  }

  #[inline(always)]
  pub(super) const fn is_valid(self) -> bool {
    matches!(self, Self::Valid)
  }
}
