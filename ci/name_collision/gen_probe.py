#!/usr/bin/env python3
"""Emit one consumer probe for one added name, against its REAL owner.

The first version of this harness exercised every spelling on a local `Gadget`. That can
only collide with a name that is blanket-implemented for every `Sized` type — which was
true of exactly one item, the one already cut. `Gadget::parse_except` cannot collide with
`Ident::parse_except`; `Gadget.peek_kind()` cannot collide with `InputRef::peek_kind`. So
the probe is generated per item, against the owner the inventory records.

Usage:
    gen_probe.py <category> <name> <owner> <spelling>

Prints the probe crate's `src/lib.rs` on stdout. Exits non-zero — never silently — on a
category or owner it does not know how to express, so an unprobed name is a loud failure
rather than a green run over nothing.

`owner` for the trait categories is the TRAIT the item is declared on, and it is looked up
in `TRAITS` below rather than defaulted. It used to be defaulted, and that is the more
dangerous of the two shapes this file was missing: a probe riding a subject that does not
implement the trait constructs no collision and reports a clean run — a false green, which
is worse than the FATAL an unexpressible shape produces.
"""

import sys

FIXTURE = r'''
#![allow(dead_code, unused_variables)]
// `unused_imports` is NOT allowed crate-wide: a stranded consumer-trait import is the one
// breadcrumb a silent steal leaves, and a blanket allow would blind the probe to the very
// signal it measures. The fixture's own imports are allowed individually instead — which
// half the probes legitimately do not use, and whose set differs between the two sides, so
// leaving them unallowed manufactures a warning that looks like a breadcrumb and is not.

#[allow(unused_imports)]
use std::cell::Cell;

#[allow(unused_imports)]
use tokora::{
  Emitter, InputRef, Lexer, ParseContext, ParseInput, ScanLookahead, Token as TokenT,
  TryParseInput, Parse, Parser, ParserContext,
  emitter::Silent,
  error::{Unclosed, UnexpectedEot, token::UnexpectedToken},
  lexer::LogosLexer,
  logos::{self, Logos},
  try_parse_input::ParseAttempt,
  types::Ident,
};

thread_local! {
  /// Bumped only by the CONSUMER's own item. If the added name takes the call, it stays 0.
  static CONSUMER_RAN: Cell<usize> = const { Cell::new(0) };
}

fn ran() {
  CONSUMER_RAN.with(|c| c.set(c.get() + 1));
}

fn consumer_calls() -> usize {
  CONSUMER_RAN.with(Cell::get)
}

thread_local! {
  /// Bumped immediately before the call under test. `CONSUMER-CALLS: 0` alone cannot
  /// distinguish "the added name took the call" from "control never reached the call
  /// site" — both are the consumer's item not running. This separates them.
  static REACHED: Cell<usize> = const { Cell::new(0) };
}

fn reached() {
  REACHED.with(|c| c.set(c.get() + 1));
}

fn reached_count() -> usize {
  REACHED.with(Cell::get)
}

#[derive(Debug, Clone, PartialEq, Logos)]
#[logos(crate = logos, skip r"[ \t\r\n]+")]
pub enum Tok {
  #[regex(r"[0-9]+", |l| l.slice().parse::<i64>().ok())]
  Num(i64),
  #[regex(r"[a-zA-Z_][a-zA-Z0-9_]*")]
  Word,
}

impl core::fmt::Display for Tok {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
  Num,
  Word,
}

impl core::fmt::Display for Kind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

impl TokenT<'_> for Tok {
  type Kind = Kind;
  type Error = PErr;
  // No default (see `Token::SCAN_LOOKAHEAD`'s own doc): every logos-backed vocabulary in this
  // crate states one, and the fixture is no exception now that #293 requires it. `Unbounded`
  // is the value every test fixture in the crate proper picks — it is always sound and needs
  // no per-pattern audit, which is exactly right for a throwaway probe token type.
  const SCAN_LOOKAHEAD: ScanLookahead = ScanLookahead::Unbounded;
  fn kind(&self) -> Kind {
    match self {
      Tok::Num(_) => Kind::Num,
      Tok::Word => Kind::Word,
    }
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

impl tokora::token::IdentifierToken<'_> for Tok {
  fn is_identifier(&self) -> bool {
    matches!(self, Tok::Word)
  }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PErr;

impl From<()> for PErr {
  fn from(_: ()) -> Self {
    PErr
  }
}
impl<O, Lang: ?Sized, Set: Clone + 'static> From<UnexpectedEot<O, Lang, Set>> for PErr {
  fn from(_: UnexpectedEot<O, Lang, Set>) -> Self {
    PErr
  }
}
impl<'a, T, K: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, K, S, Lang>> for PErr {
  fn from(_: UnexpectedToken<'a, T, K, S, Lang>) -> Self {
    PErr
  }
}
impl<'a, L: Lexer<'a>, Lang: ?Sized> tokora::emitter::FromUnclosed<'a, L, Lang> for PErr {
  fn from_unclosed<D>(_: Unclosed<D, L::Span, Lang>) -> Self {
    PErr
  }
}

pub type PLexer<'a> = LogosLexer<'a, Tok>;
pub type PCtx<'a> = ParserContext<'a, PLexer<'a>, Silent<PErr>>;

pub fn ctx() -> PCtx<'static> {
  ParserContext::new(Silent::<PErr>::new())
}

/// A parser-shaped fn item — the receiver `ParseInput`'s blanket actually accepts. A
/// non-parser-shaped value probes clean and proves nothing.
pub fn try_num<'inp>(
  inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>,
) -> Result<ParseAttempt<i64>, PErr> {
  Ok(match inp.next()? {
    Some(sp) => match sp.into_data() {
      Tok::Num(n) => ParseAttempt::Accept(n),
      _ => ParseAttempt::Decline,
    },
    None => ParseAttempt::Decline,
  })
}

pub fn parse_num<'inp>(
  inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>,
) -> Result<i64, PErr> {
  match inp.next()? {
    Some(sp) => match sp.into_data() {
      Tok::Num(n) => Ok(n),
      _ => Err(PErr),
    },
    None => Err(PErr),
  }
}

// A receiver-method probe drives through the PUBLIC `Parser` API — the way a consumer
// reaches an `InputRef` at all. `Input` is crate-private, so a probe that reached for it
// would not be testing anything a consumer can write.
'''

WITNESS = r'''
#[cfg(test)]
mod witness {
  use super::*;
  #[test]
  fn who_ran() {
    CONSUMER_RAN.with(|c| c.set(0));
    REACHED.with(|c| c.set(0));
    drive();
    println!("REACHED: {}", reached_count());
    println!("CONSUMER-CALLS: {}", consumer_calls());
  }
}
'''


# A typed CST node — the population the `CastNode` blanket impl silently enlarges.
#
# It exists on BOTH sides: `Node` is not new, so the probe compiles against the base ref, and
# the only thing the head adds is a second `cast_node` candidate for the very same type. That
# is what makes the row a measurement rather than a compile accident. Copied in shape from
# `tokora/tests/misc.rs`, which is where the minimum viable `Node` is already written down.
CST_NODE_FIXTURE = r'''
use rowan::{Language as RowanLanguage, SyntaxKind as RawKind, SyntaxNode};
use tokora::{
  cst::{Element, Node, error::NodeMismatch},
  syntax::Syntax,
  utils::{GenericArrayDeque, typenum::U0},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PK {
  Root,
  Inner,
}

impl core::fmt::Display for PK {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "{self:?}")
  }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PLang {}

impl RowanLanguage for PLang {
  type Kind = PK;
  fn kind_from_raw(raw: RawKind) -> PK {
    match raw.0 {
      0 => PK::Root,
      _ => PK::Inner,
    }
  }
  fn kind_to_raw(k: PK) -> RawKind {
    match k {
      PK::Root => RawKind(0),
      PK::Inner => RawKind(1),
    }
  }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct NoComponent;

impl core::fmt::Display for NoComponent {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    write!(f, "none")
  }
}

#[derive(Debug)]
pub struct ProbeNode(SyntaxNode<PLang>);

impl Syntax for ProbeNode {
  type Lang = PLang;
  const KIND: PK = PK::Inner;
  type Component = NoComponent;
  type COMPONENTS = U0;
  type REQUIRED = U0;

  fn possible_components() -> &'static GenericArrayDeque<Self::Component, U0> {
    const C: &GenericArrayDeque<NoComponent, U0> = &GenericArrayDeque::from_array([]);
    C
  }
  fn required_components() -> &'static GenericArrayDeque<Self::Component, U0> {
    const C: &GenericArrayDeque<NoComponent, U0> = &GenericArrayDeque::from_array([]);
    C
  }
}

impl Element<PLang> for ProbeNode {
  const KIND: PK = PK::Inner;
  fn castable(kind: PK) -> bool {
    kind == PK::Inner
  }
}

impl Node<PLang> for ProbeNode {
  fn try_cast_node(
    syntax: SyntaxNode<PLang>,
  ) -> Result<Self, tokora::cst::error::SyntaxError<Self, PLang>> {
    if Self::castable(syntax.kind()) {
      Ok(Self(syntax))
    } else {
      Err(NodeMismatch::new(syntax).into())
    }
  }
  fn syntax(&self) -> &SyntaxNode<PLang> {
    &self.0
  }
}
'''


# ── Reaching an owner that lives in a module the base ref does not have ──────────────────
#
# The indirection below is not decoration and must not be flattened to a direct import.
#
# `run.sh` grants `new-owner` only on positive evidence: a base-side diagnostic that NAMES this
# row's owner. Every owner before this one lived in a module the base ref already had, so
# `use tokora::EmitterView;` failed as `unresolved import \`tokora::EmitterView\`` — the owner in
# the message. `tokora::diagnostic` is the first WHOLE MODULE a diff has added, and there the
# same shape reports the module instead: `unresolved import \`tokora::diagnostic\``, with no
# mention of `Code` anywhere in the output. Measured, all three spellings, on the base ref
# `c14b936`:
#
#   use tokora::diagnostic::Code;   -> E0432 `tokora::diagnostic`      (1 error, no `Code`)
#   use tokora::diagnostic::*;      -> E0432 `tokora::diagnostic`      (1 error, no `Code`)
#   impl .. for tokora::diagnostic::Code -> E0433 `diagnostic` in `tokora`  (3, no `Code`)
#
# rustc suppresses the follow-on `cannot find type \`Code\`` in all three, which is the right
# thing for a compiler to do and leaves the harness with no evidence: every row scores INCONCL
# while the head side is measuring exactly what it should.
#
# Re-exporting through a local module splits the one failure into two, and the second one names
# the owner:
#
#   error[E0432]: unresolved import `tokora::diagnostic`
#   error[E0432]: unresolved import `new_module::Code`
#
# The head side resolves both hops and is unchanged. Any future diff adding a module takes this
# shape; a direct import of a type inside a new module cannot reach `new-owner` at all.
def new_module_imports(module, *names):
    """Import `names` from `module` so a base ref lacking `module` names each one when it fails."""
    tail = names[0] if len(names) == 1 else "{%s}" % ", ".join(names)
    return (
        "mod new_module {\n"
        "  pub use %s::*;\n"
        "}\n"
        "use new_module::%s;\n" % (module, tail)
    )


# The subject an `Emitter`-declared row rides.
#
# `Silent<PErr>` is the fixture's OWN emitter — `PCtx` is built on it — so this is a type tokora
# really implements `Emitter` for rather than one picked for convenience. A subject that does not
# implement the trait constructs no collision and reports a clean run, which is the failure
# `trait_subject` exists to refuse.
EMITTER_SUBJECT_FIXTURE = r'''
pub fn emitter_value() -> Silent<PErr> {
  Silent::<PErr>::new()
}
'''


# A `Lexer` subject: the fixture's own logos-backed lexer, which tokora implements `Lexer` for
# through the adapter's blanket impl. `Lexer::new` is a trait method, so the fixture spells it
# through the trait to keep the value's construction independent of whatever inherent
# constructors the adapter happens to carry.
LEXER_SUBJECT_FIXTURE = r'''
pub fn lexer_value() -> PLexer<'static> {
  <PLexer<'static> as Lexer<'static>>::new("1 2")
}
'''


# ── Which subject a trait probe rides ────────────────────────────────────────────────────
#
# This was two hardcoded traits and an `else`:
#
#     recvr = "try_num" if owner == "TryParseInput" else "parse_num"
#
# so EVERY trait that was not `TryParseInput` silently got `parse_num` — a receiver that
# implements `ParseInput` and nothing else. For a name declared on any other trait the probe
# would then collide with nothing and report a clean run over an experiment that never
# happened, which is a false green and strictly worse than refusing. The two-line comment
# above the hardcode said exactly that and the code did not obey it.
#
# So the subject is looked up, and an absent trait is FATAL. Each field answers one question,
# and a probe that needs a field the trait does not supply refuses rather than defaulting:
#
#   recvr    a VALUE satisfying the trait's bound, for a `self`-taking method.
#   self_ty  a TYPE satisfying it, for a receiver-less associated function — path resolution
#            has no receiver to walk, so the collision is `Type::name(..)` and needs a type.
#   scope    how a consumer brings the trait into scope. `None` means the shared fixture
#            already imports it. A trait the release ADDS cannot be imported by name: that
#            import does not resolve on the base side and the row goes INCONCL, measuring
#            nothing — so a new trait is reached through a module glob, which is a legal
#            consumer spelling on both sides.
#   fixture  extra source the subject needs, or `None`.
#
# ── THE TRUST BOUNDARY: `recvr` IS NOT POLICED BY THE RUN ─────────────────────────────────
#
# Read this before adding or editing a row. Nothing below is checked by anything; the table is
# an assertion its author makes, and for some row shapes it is the ONLY protection there is.
#
# The run cannot verify that `recvr` names a value the trait is actually implemented for. It
# has no witness to key on: a subject that does NOT implement the trait constructs no collision
# and reports a clean run, which is byte-identical to the clean run a correct subject produces
# whenever the consumer's item wins at an earlier pick. Both score `ok*`, both sides agree, and
# both leave the same `CONSUMER-CALLS` witness — there is no column in which they differ.
#
# That is not a hypothesis. It was MEASURED (issue #225) for the shape it bites hardest:
#
#   A `&mut self` trait method on a NON-DEREF subject. The consumer is `impl<T> ConsumerComb
#   for T`, so it supplies a candidate at every pick, and `trait_method`'s three spellings
#   declare `self` or `&self` — both EARLIER than `&mut`, and on a non-deref value there is no
#   later step to fall through to. The consumer's item therefore claims the call at pick one or
#   two and tokora's is never a candidate, no matter WHAT the receiver is. Swapping
#   `emitter_value()` for a receiver that implements nothing produced the identical `7u8` from
#   the consumer's item and the identical witness on both sides — a byte-identical `ok*`. The
#   row cannot be falsified by running it.
#
# So for such rows the blessed record is TRUSTED, NOT VERIFIED, and the written justification in
# `no_collision.txt` — read by a human who agrees with it — is the entire mechanism. The only
# consumer that would genuinely compete declares `&mut self`, collides at the same pick, and is
# E0034-loud; `trait_method` cannot generate it. This is a property of METHOD RESOLUTION, not a
# gap in the templates, so no amount of generator work converts it into machine detection —
# see issue #225's ruling, which declines exactly that work and records why.
#
# What follows for an author: pick `recvr` from a type tokora demonstrably implements the trait
# for (the fixture's own types, not a convenient one), say in the row's `no_collision.txt` entry
# WHY the subject satisfies the bound, and never read a green `ok*` on a `&mut`-self row as
# evidence that the subject was right. `EMITTER_SUBJECT_FIXTURE` above is the worked example.

# ── The subject for `tokora::types::recovery`, and WHY IT IS NOT `Ident` ─────────────────
#
# Both traits are implemented for `Ident` and for `Keyword`, and `Ident` is the one already in
# the shared fixture's import list — so it is the obvious pick and it is the WRONG one, which
# is a fact rather than a preference. `Ident` has carried inherent `is_valid`, `is_error` and
# `is_missing` since long before the base ref, and an inherent item outranks every trait item
# at the same step of the receiver walk. Measured, on the `later_pick_discarded` spelling: the
# probe compiled, ran, reported `CONSUMER-CALLS: 0` and scored `new-owner` — over a call that
# went to `Ident`'s OWN pre-existing method, with `RecoveryState` reported unused in the same
# log. Three false verdicts in one run, in the exact shape this harness exists to refuse.
#
# `Keyword` declares no inherent `status`, `is_valid`, `is_error` or `is_missing` — check that
# before moving this subject, because the check is what the choice rests on — so tokora's trait
# item is the only candidate of that name on it besides the consumer's. `run.sh`'s fifth witness
# now REFUSES the `Ident` shape rather than trusting this note, but the note is what tells the
# next author which way to move when it fires.
#
# The import is deliberately NOT wrapped in `#[allow(unused_imports)]`, and that is load-bearing
# rather than an oversight: `unused import: `new_module::RecoveryState`` IS the fifth witness's
# evidence, and an allow is a witness that cannot fire. `trait_scope`'s `scope` field carries
# that allow for its own documented reason, which is why these two traits reach the probe
# through `fixture` instead.
RECOVERY_CARRIER = r'''
use tokora::types::Keyword;

/// A carrier tokora implements both `types::recovery` traits for, and which declares no
/// inherent item of any probed name. Built directly: `Keyword::new` is public and const, and a
/// parse would hand the value back through a grammar rather than to the line making the call.
pub fn recovery_carrier() -> Keyword<&'static str> {
  Keyword::new(tokora::SimpleSpan::new(0, 1), "probe")
}
'''

RECOVERY_STATE_FIXTURE = (
    new_module_imports("tokora::types::recovery", "RecoveryState") + RECOVERY_CARRIER
)

FROM_COMPONENTS_FIXTURE = (
    new_module_imports("tokora::types::recovery", "FromComponents") + RECOVERY_CARRIER
)


TRAITS = {
    "ParseInput": {
        "recvr": "parse_num",
        "self_ty": None,
        "scope": None,
        "fixture": None,
    },
    "TryParseInput": {
        "recvr": "try_num",
        "self_ty": None,
        "scope": None,
        "fixture": None,
    },
    "CastNode": {
        # No receiver value: `CastNode` declares no `self`-taking item, so supplying one
        # would be a guess. If it ever grows one, this is the line to fill in — with a
        # `ProbeNode` value built from a green tree, not with whatever is nearest.
        "recvr": None,
        "self_ty": "ProbeNode",
        "scope": "use tokora::cst::*;",
        "fixture": CST_NODE_FIXTURE,
    },
    "Emitter": {
        # A VALUE, and the fixture's own: `emitter_value()` hands back the `Silent<PErr>` that
        # `PCtx` is built on. `self_ty` stays absent — every item `Emitter` declares takes a
        # receiver, so a receiver-less path row would be a shape the trait does not have, and
        # filling the field in "just in case" is how a probe ends up riding a guess.
        "recvr": "emitter_value()",
        "self_ty": None,
        # `Emitter` is a PRE-EXISTING trait the shared fixture already imports by name, so no
        # glob is needed — the import resolves on both sides.
        "scope": None,
        "fixture": EMITTER_SUBJECT_FIXTURE,
    },
    # ── `Diagnose` / `DiagnoseExt` — TOKORA IMPLEMENTS NEITHER, FOR ANY TYPE ─────────────
    #
    # These two are the first traits in this harness with NO implementor inside tokora, and
    # that is the whole content of their rows. `tokora::diagnostic` publishes a contract for a
    # CONSUMER's error types to implement; tokora's own errors do not implement it, and the
    # module's header says so in as many words. `DiagnoseExt` is blanket over `D: Diagnose`, so
    # it reaches exactly as far.
    #
    # There is therefore no receiver, anywhere in this crate, on which tokora's items are
    # candidates — so every row here AGREES on both sides and is justified by name in
    # `no_collision.txt`. Read the README's bound before reading the verdict column: a subject
    # that does not implement the trait produces a clean run that means nothing, and here that
    # is the FINDING rather than a mis-chosen subject. `UnexpectedEot` is picked deliberately as
    # one of tokora's own error types — the natural first candidate for a `Diagnose` impl — so
    # the row says "even there, tokora's item is not a candidate" rather than "some unrelated
    # value does not implement it".
    #
    # Reaching `Diagnose::code` at all costs a consumer two source changes they make
    # themselves: implementing the trait for their own type, and importing the trait. Neither
    # can happen to a call site that already exists because tokora published a name.
    #
    # RE-OPEN CONDITION, and it is not optional: the first time tokora implements `Diagnose`
    # for one of its own types, `recvr` becomes a value of THAT type and these rows are
    # re-probed. Until then the entry below is trusted, not verified, and the justification in
    # `no_collision.txt` is the whole of its protection.
    #
    # `scope` stays absent for the same reason it is absent on `Emitter`, arrived at from the
    # other direction: an import of a trait the base ref does not have makes the base side fail
    # to compile, which destroys the before-state and scores every row INCONCL. A pre-existing
    # glob cannot reach these names either, because the module is new and the crate root does
    # not re-export them.
    #
    # `self_ty` stays absent because neither trait declares a receiver-less item, so no
    # `trait_assoc_fn` row can arise; filling it in "just in case" is how a probe ends up
    # riding a guess.
    # ── `Lexer`, `State`, `Token` — the three traits #282's read frontier touches ────────
    #
    # All three are PRE-EXISTING, so each can be brought into scope by name on both sides and
    # `scope` follows the `Emitter` precedent: `Lexer` and `Token` are already in the shared
    # fixture's import list, `State` is not.
    #
    # `Lexer`'s subject is the fixture's OWN lexer — `PLexer<'static>` is `LogosLexer<'static,
    # Tok>`, and tokora implements `Lexer` for it through the adapter's blanket impl over
    # `T: FromLogos`, which `Tok` satisfies by deriving `Logos` and implementing `Token`. It is
    # not a convenient nearby value: it is the one type in this fixture the trait is
    # demonstrably implemented for. The subject is built rather than obtained from a parse,
    # for the reason the error subjects are: the receiver has to be a value on the line where
    # the call is made, and an `InputRef` never hands its lexer out.
    "Lexer": {
        "recvr": "lexer_value()",
        # `Lexer` declares no receiver-less item, so a `trait_assoc_fn` row cannot arise and
        # filling `self_ty` in "just in case" is how a probe ends up riding a guess.
        "self_ty": None,
        "scope": None,
        "fixture": LEXER_SUBJECT_FIXTURE,
    },
    # `State`'s subject is `()`, and that is tokora's own implementor rather than a stand-in:
    # `impl State for ()` is in `state/mod.rs`, and it is what a `logos` vocabulary with no
    # `extras` actually uses — `Tok` above declares none, so `PLexer::State` IS `()`.
    "State": {
        "recvr": "()",
        "self_ty": None,
        "scope": "use tokora::State;",
        "fixture": None,
    },
    # `Token`'s subject is the fixture's own `Tok`, which implements it a few lines above.
    # `recvr` stays absent: the only item this release adds to `Token` is an associated CONST,
    # which is path-resolved and needs the TYPE, and a receiver supplied for a shape the row
    # cannot have is the same guess the other rows refuse.
    "Token": {
        "recvr": None,
        "self_ty": "Tok",
        "scope": None,
        "fixture": None,
    },
    "Diagnose": {
        "recvr": "UnexpectedEot::eot(7usize)",
        "self_ty": None,
        "scope": None,
        "fixture": None,
    },
    "DiagnoseExt": {
        "recvr": "UnexpectedEot::eot(7usize)",
        "self_ty": None,
        "scope": None,
        "fixture": None,
    },
    # ── `ErrorContainer` — a PRE-EXISTING trait gaining two DEFAULTED items ─────────────────
    #
    # `Option<E>` is one of the four types tokora implements `ErrorContainer` for, and the one
    # that needs no construction and carries no inherent item of its own to confound the
    # receiver walk (`Vec::clear` would). `PErr` is the fixture's own error type, so the impl
    # being exercised is tokora's `impl<E> ErrorContainer<E> for Option<E>` at a real `E`.
    #
    # `scope` IS needed, and it is not the `Emitter` case: `ErrorContainer` is pre-existing, so
    # the import resolves on both sides — but the shared fixture does not import it, and without
    # the import tokora's item is a candidate nowhere and every row is a clean run over an
    # experiment that never happened.
    #
    # `self_ty` is the same subject in the type namespace, and it is filled in because #284 adds
    # the trait's first RECEIVER-LESS item since it shipped: `from_errors` declares no `self`, so
    # its row is a `trait_assoc_fn` and needs the path form. It used to be absent on the reading
    # that `new`/`with_capacity` were the only receiver-less items and both pre-existing — true
    # when it was written, and exactly the kind of statement that stops being true the moment the
    # trait gains a constructor. A `None` here is FATAL rather than a skipped row, so the harness
    # would have reported the gap rather than swallowing it.
    #
    # READ THE HEADER ABOVE BEFORE READING A VERDICT HERE. `clear` takes `&mut self`, so it sits
    # at a strictly LATER pick than either consumer item `trait_method` can generate (`self` and
    # `&self`) — the consumer wins on both sides, the row AGREES, and agreement here is the
    # foreknown outcome rather than evidence of safety. That is #225's ruling; the rows are
    # justified by name in `no_collision.txt` rather than read as green. `from_errors` is the
    # other shape and is expected `loud`: a path call walks no receiver chain, so tokora's item
    # and the consumer's are both applicable at the one pick and rustc refuses to choose.
    "ErrorContainer": {
        "recvr": "Option::<PErr>::None",
        "self_ty": "Option::<PErr>",
        "scope": "use tokora::error::ErrorContainer;",
        "fixture": None,
    },
    # ── `TrackerExt` / `RecursionTrackerExt` — tokora#265's split of the combined checks ────
    #
    # Both are NEW traits: `Tracker` and `RecursionTracker` are PRE-EXISTING (the four
    # primitives — `increase_token`/`increase_recursion`/`decrease_recursion`/`check` and
    # `increase`/`decrease`/`check` respectively), but the *combined* update-and-check
    # operations used to be `Tracker`'s own provided methods and now live on these two
    # blanket-implemented extension traits instead — see `TrackerExt`'s own doc for why
    # (a type could override a provided method and disambiguate to the wrong trait's `check`;
    # a blanket impl makes that unrepresentable). So `scope` follows the `CastNode` precedent,
    # not `Emitter`'s: a NEW trait cannot be imported by name — that import does not resolve on
    # the base ref and the row goes INCONCL — so each is reached through a glob over its own
    # module instead, which is NOT new (only the trait inside it is), so the glob itself
    # resolves unchanged on both sides.
    #
    # `recvr` is each trait's own module-mate rather than a shared subject reached across a
    # second module: `RecursionLimiter` (`impl RecursionTracker for RecursionLimiter`) sits in
    # `state::recursion_tracker` beside `RecursionTrackerExt` itself, and `Limiter`
    # (`impl Tracker for Limiter`) sits in `state::tracker` beside `TrackerExt`. Either row
    # could have ridden `Limiter` instead — it implements `Tracker`, `TokenTracker` AND
    # `RecursionTracker` all at once, constructed with `Limiter::new()` — but that would pull a
    # second glob into `RecursionTrackerExt`'s scope for no gain: one type per trait, from the
    # trait's own module, is the simpler, more direct implementor and matches the standing
    # precedent (`Lexer`'s subject is "the one type in this fixture the trait is demonstrably
    # implemented for", `State`'s is "tokora's own implementor rather than a stand-in").
    #
    # No `let` binding, mutable or otherwise, and none is needed: `recvr` is spliced directly
    # into `{recvr}.{name}(ARGS)`, so `Limiter::new()` / `RecursionLimiter::new()` is a fresh
    # temporary at the call site, and Rust lets a `&mut self` method borrow a temporary's place
    # for that one statement — the identical shape `emitter_value()` and `lexer_value()` already
    # ride. `fixture` therefore stays absent: no helper function or setup is needed beyond the
    # glob import, and both constructors are `pub const fn new() -> Self` with no arguments.
    #
    # READ THE HEADER ABOVE BEFORE READING A VERDICT HERE. Every item both traits declare takes
    # `&mut self`, with no other trait-shaped alternative on either subject, so this is the
    # identical foreknown-vacuous shape `Emitter`'s row is: `trait_method`'s three consumer
    # spellings declare `self` or `&self`, both strictly EARLIER in the receiver walk than
    # `&mut self`, and neither subject is a deref target — so the consumer's blanket item wins
    # at pick one or two on every row, no matter what the receiver is. #225's ruling applies; a
    # green verdict here is trusted on the written justification, not on having exercised
    # tokora's item.
    #
    # `self_ty` stays absent for both: neither trait declares a receiver-less item (every method
    # on both takes `&mut self`), so no `trait_assoc_fn` row can arise, and filling it in "just
    # in case" is how a probe ends up riding a guess.
    "TrackerExt": {
        "recvr": "Limiter::new()",
        "self_ty": None,
        "scope": "use tokora::state::tracker::*;",
        "fixture": None,
    },
    "RecursionTrackerExt": {
        "recvr": "RecursionLimiter::new()",
        "self_ty": None,
        "scope": "use tokora::state::recursion_tracker::*;",
        "fixture": None,
    },
    # ── `RecoveryState` / `FromComponents` — a NEW trait in a NEW module ────────────────────
    #
    # The first owners here that the same diff introduces AND that live in a module the base ref
    # does not have. Both facts matter and they are answered in different places:
    #
    #   * the module is new, so a direct `use tokora::types::recovery::RecoveryState;` fails on
    #     base as `unresolved import `tokora::types::recovery`` — the MODULE, with the owner
    #     suppressed — and every row scores INCONCL for want of a diagnostic naming the owner.
    #     `new_module_imports` splits that into two errors, the second `unresolved import
    #     `new_module::RecoveryState``. Measured on `a218439`, exactly the `tokora::diagnostic`
    #     shape that function was written for.
    #   * the owner is new, so the BASE side cannot compile and no row here can reach `loud`,
    #     `silent` or `ok*` — those all need a before-state. The three verdicts a new owner can
    #     reach are `new-owner` (head takes the call), `new-owner-err` (head refuses to choose)
    #     and `new-owner-nc*` (the consumer wins an earlier pick, justified by pick order).
    #
    # Which one each spelling lands on is decided by ONE fact: every item on both traits takes
    # `&self` or no receiver at all, and the `trait_method` spellings are named for a tokora
    # method taken BY VALUE. Against `&self` there is no LATER pick for the consumer to sit at —
    # `same_pick` (`self`) is one pick EARLIER and wins outright; `later_pick_*` (`&self`) is the
    # SAME pick and rustc refuses to choose. So no spelling here can produce a silent steal, and
    # that is a property of the receiver kinds rather than of these names. It is the mirror of
    # `Emitter::commit_lexer_error`, one receiver step over: there tokora's `&mut self` is later
    # than every consumer spelling, here tokora's `&self` is earlier than or equal to all of them.
    #
    # `FromComponents::from_components` takes no receiver, so it is a `trait_assoc_fn` and its
    # rule is path resolution rather than a walk: the consumer's `impl<T> ConsumerAssoc for T`
    # and tokora's trait are both applicable to `Keyword` and rustc reports E0034. That is the
    # `loud` verdict that category's docstring predicts, reached on the one side a new owner has.
    #
    # `self_ty` carries the turbofish because the template spells `{ty}::{name}()` and a bare
    # `Keyword<&'static str>::from_components()` is not a path.
    "RecoveryState": {
        "recvr": "recovery_carrier()",
        # `RecoveryState` declares no receiver-less item, so no `trait_assoc_fn` row can arise
        # and filling this in "just in case" is how a probe ends up riding a guess.
        "self_ty": None,
        # The import is in `fixture`, not here: `trait_scope` wraps `scope` in
        # `#[allow(unused_imports)]`, and that allow would suppress the fifth witness's evidence.
        "scope": None,
        "fixture": RECOVERY_STATE_FIXTURE,
    },
    "FromComponents": {
        # No receiver value: `FromComponents` declares exactly one item and it takes none.
        "recvr": None,
        "self_ty": "Keyword::<&'static str>",
        "scope": None,
        "fixture": FROM_COMPONENTS_FIXTURE,
    },
}


def trait_subject(name, owner, need):
    """The trait's record, or a FATAL that names the trait and the missing field."""
    rec = TRAITS.get(owner)
    if rec is None:
        sys.exit(
            f"gen_probe: UNKNOWN TRAIT {owner!r} (item {name!r}) — refusing to guess a "
            f"subject. A probe riding a subject that does not implement {owner} collides "
            f"with nothing and reports a clean run over an experiment that never happened. "
            f"Add {owner!r} to TRAITS in gen_probe.py."
        )
    if rec.get(need) is None:
        sys.exit(
            f"gen_probe: trait {owner!r} has no {need!r} entry, which item {name!r} needs. "
            f"Fill it in in gen_probe.py rather than letting this row pass unprobed."
        )
    return rec


def trait_scope(rec):
    """The consumer's import of the trait under test, if it needs one.

    `unused_imports` is not allowed crate-wide (see the fixture header), and this one is
    genuinely unused on the base side — the trait it names does not exist there yet. That is
    the fixture's own documented case for an individual allow: a warning whose set differs
    between the two sides is an artefact of generation, not a breadcrumb. It is also not
    load-bearing here, because this text does not name the probed item, so it can never be
    what turns a SILENT row into a `warned` one.
    """
    scope = rec.get("scope")
    return "" if not scope else f"  #[allow(unused_imports)]\n  {scope}\n"


# ── The pratt error types, as probe subjects ─────────────────────────────────────────────
#
# `RecursionLimitReached` and `NonAssociativeChain` are the two error types the recursion
# bound introduced, and they are the first owners in this harness that the SAME diff also
# introduces. Read `run.sh`'s `new-owner` verdict before reading a row for one of them: the
# probe below names the owner, so it cannot compile on a base ref where the owner does not
# exist, and that absence is the finding rather than a broken probe.
#
# The subject is BUILT rather than obtained from a tripped parse. The receiver has to be a
# value on the line where the call is made, and a parse that trips hands the error back
# through the grammar's own `From` — which for the fixture's `PErr` has already discarded it.
# Both types are plain `Copy` values with a public constructor, so building one directly is
# both simpler and closer to what a consumer holding one would have.
#
# Each record answers: which concrete type the consumer's extension trait is implemented for,
# how a path call spells it, what the probe must import, and how to build one.
#
# BOTH are listed even though #147's inventory routed every shared name to
# `RecursionLimitReached`. `of`, `offset`, `offset_ref` and `map_offset` exist on both types, and
# `inherent_owners` records ONE owner per name — so which of the two a row rides is decided by
# rustdoc's iteration order, not by anything in this file. An entry for only the winner would be
# a template that works until that order changes and then reports FATAL over names it has
# already probed. The two share `error_subject_method` / `error_subject_assoc_fn`, so the second
# entry is one record rather than a second code path, and the path that serves it is the one
# this diff exercises.
ERROR_SUBJECTS = {
    "RecursionLimitReached": {
        "ty": "RecursionLimitReached<usize, ()>",
        "path": "RecursionLimitReached::<usize, ()>",
        "imports": (
            "use tokora::error::RecursionLimitReached;\n"
            "use tokora::state::recursion_tracker::{RecursionLimiter, RecursionTracker};\n"
        ),
        # Two `increase`s against a limitation of 1, which is the type's own doc example.
        "build": (
            "  let mut limiter = RecursionLimiter::with_limitation(1);\n"
            "  limiter.increase();\n"
            "  limiter.increase();\n"
            "  let exceeded = RecursionTracker::check(&limiter)\n"
            "    .expect_err(\"a limiter two levels over its limitation must report exceeded\");\n"
            "  let subject: RecursionLimitReached<usize, ()> =\n"
            "    RecursionLimitReached::of(7usize, exceeded);\n"
        ),
        # `of`'s own two arguments — the same values `build` above feeds it to construct
        # `subject`, split out because `error_subject_assoc_fn` probes `of` ITSELF and needs
        # them without a `let subject = ...` wrapped around the call. Two independently spelled
        # fields rather than one parsed back out of the other: both read "the type's own doc
        # example, two `increase`s over a limitation of 1", so a change to one is a prompt to
        # check the other.
        "of_setup": (
            "  let mut limiter = RecursionLimiter::with_limitation(1);\n"
            "  limiter.increase();\n"
            "  limiter.increase();\n"
            "  let exceeded = RecursionTracker::check(&limiter)\n"
            "    .expect_err(\"a limiter two levels over its limitation must report exceeded\");\n"
        ),
        "of_args": "7usize, exceeded",
        # The real return type of every OTHER inherent item these two types declare — what
        # `error_subject_method` substitutes for the `used` spelling's `let` binding instead of
        # the fixed `u8` every other inherent-method row rides. `offset`/`offset_ref` read the
        # same `O = usize` both owners fix here; `exceeded`/`depth`/`limitation` exist only on
        # this owner. `map_offset` is deliberately absent: it is the one name that takes an
        # argument and consumes `self`, so `error_subject_method` builds its call directly
        # rather than from this table.
        "accessors": {
            "offset": "usize",
            "offset_ref": "&usize",
            "exceeded": "tokora::state::recursion_tracker::RecursionLimitExceeded",
            "depth": "usize",
            "limitation": "usize",
        },
    },
    "NonAssociativeChain": {
        "ty": "NonAssociativeChain<usize, ()>",
        "path": "NonAssociativeChain::<usize, ()>",
        "imports": "use tokora::error::NonAssociativeChain;\n",
        "build": (
            "  let subject: NonAssociativeChain<usize, ()> = NonAssociativeChain::of(6usize);\n"
        ),
        "of_setup": "",
        "of_args": "6usize",
        "accessors": {
            "offset": "usize",
            "offset_ref": "&usize",
        },
    },
}


def error_subject_method(name, owner, spelling):
    """A consumer extension trait on one of the two pratt error types.

    `&self`, not `&mut self`: every method these types declare takes `&self` or `self`, and a
    consumer's receiver has to be reachable by the same autoref step tokora's is, or the two are
    not competing for the same pick.

    The call has to reach the REAL method's arity and `used`-binding type, not the fixed
    zero-arg/`u8` shape every other inherent-method row rides: `map_offset` consumes `self` and
    takes a closure, and the five plain accessors return `usize`, `&usize` or
    `RecursionLimitExceeded` — never `u8`. Built against the wrong shape, the call fails with
    E0061 or E0308 before rustc ever reaches the collision, which reads as INCONCL rather than
    the `new-owner` finding it should be. (`map_offset`'s consumer trait below still declares
    `&self`, not the real method's by-value `self` — harmlessly: the real, by-value `map_offset`
    is found at the FIRST autoref step, before the trait's `&self` candidate at the next step is
    ever considered, so which receiver kind the trait declares cannot change which one is
    picked.)
    """
    # Unpacked into locals BEFORE the f-string, not indexed inside it. Same reason the
    # parameter type in `trait_method` is: a quote inside an f-string expression is a
    # SyntaxError before Python 3.12, and this file has to parse under whatever `python3` is
    # on the runner. A gate that cannot be parsed is not a gate.
    rec = ERROR_SUBJECTS[owner]
    imports, ty, build = rec["imports"], rec["ty"], rec["build"]
    if name == "map_offset":
        # The identity closure keeps `U = O`, so the result is the same type `subject` already
        # is. The parameter is typed explicitly rather than left for inference — see
        # `trait_method`'s `peek_then_head` for why a bare closure parameter is not something
        # this file leaves to chance.
        args, returns = "|x: usize| x", ty
    else:
        returns = rec["accessors"].get(name)
        if returns is None:
            sys.exit(f"gen_probe: no call-shape template for method {name!r} on {owner!r}")
        args = ""
    call = (
        f"let _v: {returns} = subject.{name}({args});"
        if spelling == "used"
        else f"subject.{name}({args});"
    )
    return FIXTURE + f"""
{imports}
pub trait ConsumerExt {{
  fn {name}(&self) -> u8;
}}

impl ConsumerExt for {ty} {{
  fn {name}(&self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
{build}  reached();
  {call}
}}
""" + WITNESS


def error_subject_assoc_fn(name, owner, spelling):
    """The path-resolved half of the same two types — `Type::name(args)`, no receiver.

    `of` is the only associated function either type declares without a receiver — everything
    else on them takes `&self` or `self` and rides `error_subject_method` instead — so this
    always probes the constructor itself, at its real arity: two arguments on
    `RecursionLimitReached` (`at`, `exceeded`), one on `NonAssociativeChain` (`at`). A call built
    with neither, as this used to be, fails with E0061 before rustc ever reaches the collision.
    """
    rec = ERROR_SUBJECTS[owner]
    imports, ty, path = rec["imports"], rec["ty"], rec["path"]
    if name != "of":
        sys.exit(f"gen_probe: no call-shape template for associated fn {name!r} on {owner!r}")
    setup, args = rec["of_setup"], rec["of_args"]
    call = (
        f"let _v: {ty} = {path}::{name}({args});"
        if spelling == "used"
        else f"{path}::{name}({args});"
    )
    return FIXTURE + f"""
{imports}
pub trait ConsumerAssoc {{
  fn {name}() -> u8;
}}

impl ConsumerAssoc for {ty} {{
  fn {name}() -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  use ConsumerAssoc as _;
{setup}  reached();
  {call}
}}
""" + WITNESS


# A spent lossless driver — the only door onto a `Cst`, and therefore the only way to build the
# subject its rows ride.
#
# `Cst::from_sink` is `pub(crate)` on purpose ("the drivers are the only minters"), so a probe
# cannot short-circuit to one; it has to run a real lossless parse. That drags in a lexer of its
# own, because `parse_lossless` is walled at compile time to tokens declaring `SURFACES_TRIVIA`
# and the shared fixture's `Tok` is a trivia-SKIPPING logos lexer — driving `Cst` off `PLexer`
# fails post-monomorphization (E0080) on both sides and every row would read INCONCL.
#
# Copied in shape from `parse_lossless`'s own doc example in `tokora/src/cst/driver.rs`, which is
# where the minimum viable trivia-surfacing lexer is already written down and compiled. The error
# type is the fixture's `PErr` rather than a second one, so the `From` impls it already carries
# are the ones this lexer needs.
CST_HANDLE_FIXTURE = r'''
#[allow(unused_imports)]
use rowan::GreenNode;

#[allow(unused_imports)]
use tokora::{
  SimpleSpan,
  cache::DefaultCache,
  cst::{Cst, CstProfile, FinishError, KindValidator, TriviaPolicy, parse_lossless},
};

const MINI_SRC: &str = "ab c";
const MINI_ROOT: u16 = 1;
const MINI_TOK: u16 = 10;
const MINI_ERR: u16 = 90;
const MINI_GAP: u16 = 91;

#[derive(Debug, Clone, Copy)]
pub struct MiniTok(u8);

impl TokenT<'_> for MiniTok {
  type Kind = u8;
  type Error = PErr;
  const SURFACES_TRIVIA: bool = true;
  // See the identical note on `Tok`'s impl above: required since #293, no default, and
  // `Unbounded` is the crate-wide fixture convention.
  const SCAN_LOOKAHEAD: ScanLookahead = ScanLookahead::Unbounded;
  fn kind(&self) -> u8 {
    self.0
  }
  fn is_trivia(&self) -> bool {
    self.0 == b' '
  }
}

pub struct Mini<'a> {
  src: &'a str,
  tok_start: usize,
  pos: usize,
  state: (),
}

impl<'inp> Lexer<'inp> for Mini<'inp> {
  type State = ();
  type Source = str;
  type Token = MiniTok;
  type Span = SimpleSpan;
  type Offset = usize;
  fn new(src: &'inp str) -> Self {
    Self { src, tok_start: 0, pos: 0, state: () }
  }
  fn with_state(src: &'inp str, state: ()) -> Self {
    Self { src, tok_start: 0, pos: 0, state }
  }
  fn check(&self) -> Result<(), PErr> {
    Ok(())
  }
  fn state(&self) -> &() {
    &self.state
  }
  fn state_mut(&mut self) -> &mut () {
    &mut self.state
  }
  fn into_state(self) {
    self.state
  }
  fn source(&self) -> &'inp str {
    self.src
  }
  fn span(&self) -> SimpleSpan {
    SimpleSpan::new(self.tok_start, self.pos)
  }
  fn slice(&self) -> &'inp str {
    &self.src[self.tok_start..self.pos]
  }
  fn lex(&mut self) -> Option<Result<MiniTok, PErr>> {
    let byte = *self.src.as_bytes().get(self.pos)?;
    self.tok_start = self.pos;
    self.pos += 1;
    Some(Ok(MiniTok(byte)))
  }
  fn bump(&mut self, n: &usize) {
    self.pos += *n;
    self.tok_start = self.pos;
  }
}

fn mini_drain<'inp, Ctx>(inp: &mut InputRef<'inp, '_, Mini<'inp>, Ctx>) -> Result<usize, PErr>
where
  Ctx: ParseContext<'inp, Mini<'inp>>,
  Ctx::Emitter: Emitter<'inp, Mini<'inp>, (), Error = PErr>,
{
  let mut n = 0;
  while inp.next()?.is_some() {
    n += 1;
  }
  Ok(n)
}

/// The spent driver, by the only door there is.
pub fn mini_cst() -> Cst<'static, Mini<'static>, Silent<PErr>> {
  let profile = CstProfile::new(
    |_: &MiniTok| MINI_TOK,
    KindValidator::new(|k| matches!(k, MINI_ROOT | MINI_TOK | MINI_ERR | MINI_GAP)),
    MINI_ERR,
    MINI_GAP,
  );
  let (cst, _parsed) = parse_lossless(
    MINI_SRC,
    (),
    Silent::<PErr>::new(),
    profile,
    DefaultCache::<Mini<'_>>::new(),
    mini_drain,
  );
  cst
}
'''


# ── Inherent subjects whose OWNER this same diff introduces ──────────────────────────────
#
# `EmitterView` and `Cst` are minted by the diff that adds their items, so the BASE side cannot
# resolve the owner at all and the only verdict a row here can reach is `new-owner` — which
# `run.sh` grants on four witnesses, the last of which is that the HEAD side reached
# `witness=0`: compiled, ran, and let tokora's item take the call. A head that fails to compile
# scores INCONCL instead, "a broken probe on the side that was supposed to prove the owner is
# new".
#
# So for these owners the call is built at the item's REAL arity and the `used` spelling binds
# the item's REAL return type. That is #158's lesson, applied to the two owners this release
# mints: a fixed zero-argument `-> u8` call put 9 of `RecursionLimitReached`'s 14 rows in
# INCONCL because the head side never compiled.
#
# The rule INVERTS for a PRE-EXISTING owner (`InputRef`, `ParserContext`, `ParseState`): there
# the BASE side must compile and it has only the consumer's own item, so the binding stays the
# consumer's `u8` and a head-side E0308 is the honest `loud` verdict. Do not move an owner
# between the two shapes without moving it between these two rules.
#
# WHICH names can reach these tables is bounded, not merely observed. `surface_diff.py` assigns
# each name ONE owner, the alphabetically LAST of those declaring it — `inherent_owners[nm]` is
# overwritten inside `for owner, _ in sorted(...)`. `Cst` < `EmitterView` < `InputRef` <
# `ParseState`, so these two owners can only ever receive the names they declare ALONE: every
# forwarder `EmitterView` shares with `InputRef` or `ParseState` is routed to those instead. The
# tables below are complete for their owners rather than a subset that happens to work today,
# and a name outside them is a loud refusal naming what to add.
INHERENT_SUBJECTS = {
    "EmitterView": {
        "imports": "use tokora::EmitterView;\n",
        "fixture": "",
        # `EmitterView::new(&mut E)` is public and grants nothing (see its own doc): a caller who
        # can build one already held the `&mut E` the view exists to withhold. That is exactly
        # what makes it usable here — the subject is a REAL view over a real emitter, not a
        # stand-in. `L` is phantom in the view, so it is pinned by the annotation rather than by
        # the argument.
        "ty": "EmitterView<'_, 'static, PLexer<'static>, Silent<PErr>>",
        "setup": "  let mut inner = Silent::<PErr>::new();\n",
        "build": (
            "  let mut subject: EmitterView<'_, 'static, PLexer<'static>, Silent<PErr>> =\n"
            "    EmitterView::new(&mut inner);\n"
        ),
        "calls": {
            "bound_source": ("", "Option<tokora::source::SourceIdentity>"),
            "reborrow": ("", "EmitterView<'_, 'static, PLexer<'static>, Silent<PErr>>"),
        },
        "assoc_calls": {
            "new": ("&mut inner", "EmitterView<'_, 'static, PLexer<'static>, Silent<PErr>>"),
        },
    },
    "Cst": {
        "imports": "",
        "fixture": CST_HANDLE_FIXTURE,
        "ty": "Cst<'static, Mini<'static>, Silent<PErr>>",
        "setup": "",
        # `mut`, and the reason it was not used to be is handled elsewhere. Three of the eight
        # items consume `self` and the other five take `&self`, so nothing here needs a `&mut`
        # receiver and the binding used to be left immutable to avoid an `unused_mut` warning —
        # a head-side diagnostic the template would have manufactured, in a harness whose
        # verdicts are decided by which side gains one. `drive()` already carries
        # `#[allow(unused_mut)]`, so that warning cannot fire and the immutability bought nothing.
        #
        # What it COST became visible the first time a row was added to `Cst` after `Cst` itself
        # shipped: the consumer's extension item is generated with a `&mut self` receiver, so on
        # the BASE side — where that item is the only candidate — an immutable binding is E0596
        # and the base build fails. For an owner the diff introduces that is expected (the base
        # side cannot compile anyway), which is why it went unnoticed; for a pre-existing owner
        # it destroys the before-state and every row scores INCONCL.
        "build": (
            "  let mut subject: Cst<'static, Mini<'static>, Silent<PErr>> = mini_cst();\n"
        ),
        # The second element is the type the `used` spelling BINDS, and for seven of these eight
        # it is the item's real return type — the shape this table's header prescribes for an
        # owner the same diff introduces, where the base side cannot compile at all and only the
        # head side's binding has to be right.
        #
        # `resource_trips` is the first row added to `Cst` AFTER `Cst` shipped, so for its diff
        # the owner is PRE-EXISTING and the header's rule inverts (the same inversion it already
        # states for `InputRef` / `ParserContext` / `ParseState`). The base side must compile, and
        # there the only `resource_trips` in scope is the consumer's `ConsumerExt::resource_trips
        # -> u8`; binding the real `usize` would make the BASE build fail, which scores INCONCL —
        # `new-owner` is unavailable because rustc resolves `Cst` on both sides. So the binding is
        # the CONSUMER's `u8`, the base side compiles and runs the consumer's item, and the head
        # side's E0308 against tokora's inherent `-> usize` is the honest `loud` verdict: adding
        # the name is source-breaking for a consumer extension trait, and breaks it *loudly*.
        #
        # Any further row added to `Cst` from here on takes this same shape, not the seven above.
        "calls": {
            "with_trivia_policy": ("TriviaPolicy::AsEmitted", "Cst<'static, Mini<'static>, Silent<PErr>>"),
            "trivia_policy": ("", "TriviaPolicy"),
            "error_kind": ("", "u16"),
            "gap_kind": ("", "u16"),
            "inner_ref": ("", "&Silent<PErr>"),
            "finish": ("MINI_ROOT", "(Result<GreenNode, FinishError>, Silent<PErr>)"),
            "finish_partial": ("MINI_ROOT", "(Result<GreenNode, FinishError>, Silent<PErr>)"),
            "resource_trips": ("", "u8"),
        },
        # `Cst` declares no receiver-less associated function: `from_sink` is `pub(crate)` and the
        # drivers are the only minters. An empty table rather than an absent key, so the refusal
        # below reads "this owner has no such shape" instead of a KeyError.
        "assoc_calls": {},
    },
    # ── The `tokora::diagnostic` vocabulary types ────────────────────────────────────────
    #
    # `Code`, `Label` and `Location` are minted by the same diff as their items, so they take
    # this table's shape and not the pre-existing-owner one: the base side cannot name the type
    # at all, and only the head side's binding has to be right. Every call below is at the item's
    # real arity and every `used` binding is the item's real return type.
    #
    # Their `imports` go through `new_module_imports` rather than naming the type directly, and
    # that is what makes the base-side diagnostic mention the owner at all — read its header
    # before touching them.
    #
    # All three are `Copy` and every accessor is `&self`, so the receiver walk finds tokora's
    # inherent item at the `&T` step while the generated consumer's `&mut self` item sits one
    # step later — tokora's takes the call, `witness=0`, which is the one shape `run.sh` accepts
    # as evidence.
    #
    # WHICH names reach which owner: `surface_diff.py` records ONE owner per name, the
    # alphabetically LAST that declares it, so `Code` < `Label` < `Location` decides the shared
    # ones. `new` is declared by all three and routes to `Location`; it is written into all
    # three tables anyway, for the reason `ERROR_SUBJECTS` gives — an entry for only today's
    # winner is a template that reports FATAL over a name it has already probed the day that
    # order moves.
    #
    # `diagnostic::Severity` needs no entry and has none, and the reason is worth writing down
    # because it is a property of the harness rather than of the type: owners are identified by
    # UNQUALIFIED name, and `emitter::Severity` already declares `as_str` on the base side, so
    # `diagnostic::Severity::as_str` is absent from the inventory — masked by a same-named type
    # in another module. Its analysis is `Code::as_str`'s exactly (a new owner no call site can
    # predate), so nothing is lost; a future item on it that `emitter::Severity` does NOT also
    # declare will surface and land here.
    "Code": {
        "imports": new_module_imports("tokora::diagnostic", "Code"),
        "fixture": "",
        "ty": "Code",
        "setup": "",
        "build": '  let mut subject: Code = Code::new("probe::name-collision::row");\n',
        "calls": {
            "as_str": ("", "&'static str"),
        },
        "assoc_calls": {
            "new": ('"probe::name-collision::row"', "Code"),
        },
    },
    "Label": {
        "imports": "use tokora::SimpleSpan;\n" + new_module_imports("tokora::diagnostic", "Label", "Location"),
        "fixture": "",
        "ty": "Label",
        "setup": "",
        "build": (
            "  let mut subject: Label =\n"
            '    Label::new(Location::new(0, SimpleSpan::new(0, 1)), "probe");\n'
        ),
        "calls": {
            "location": ("", "Location"),
            "text": ("", "&'static str"),
        },
        "assoc_calls": {
            "new": ('Location::new(0, SimpleSpan::new(0, 1)), "probe"', "Label"),
        },
    },
    # `Probe` is minted by the same diff as its three items, so it takes this table's
    # new-owner shape: the base side cannot name the type at all, and only the head side's
    # binding has to be right. Its module (`tokora::state`) is NOT new, so a plain
    # `use tokora::Probe;` already names the owner in the base-side diagnostic and the
    # `new_module_imports` indirection the `diagnostic` types need buys nothing here.
    #
    # It is `Copy` and both accessors are `&self`, so the receiver walk is `Code`'s exactly:
    # tokora's inherent item is found at the `&T` step, one step before the generated
    # consumer's `&mut self` item, and takes the call.
    #
    # `new` routes HERE rather than to `Location`, because `surface_diff.py` keeps the
    # alphabetically last owner declaring a name and `Location` < `Probe`. It is written into
    # this table for that reason and stays in the other three for the reason their header gives.
    "Probe": {
        "imports": "use tokora::Probe;\n",
        "fixture": "",
        "ty": "Probe",
        "setup": "",
        "build": "  let mut subject: Probe = Probe::new(3, 7);\n",
        "calls": {
            "scanned_from": ("", "usize"),
            "probed_to": ("", "usize"),
        },
        "assoc_calls": {
            "new": ("3, 7", "Probe"),
        },
    },
    "Location": {
        "imports": "use tokora::SimpleSpan;\n" + new_module_imports("tokora::diagnostic", "Location"),
        "fixture": "",
        "ty": "Location",
        "setup": "",
        "build": "  let mut subject: Location = Location::new(0, SimpleSpan::new(0, 1));\n",
        "calls": {
            "is_entire": ("", "bool"),
            "source": ("", "u32"),
            "span": ("", "Option<SimpleSpan>"),
        },
        "assoc_calls": {
            "entire": ("0", "Location"),
            "new": ("0, SimpleSpan::new(0, 1)", "Location"),
        },
    },
    # `TokenBudget` is minted by the same diff as its items, so it takes this table's
    # new-owner shape like every entry above it. Unlike `TokenBudgetTally` (see the bespoke
    # branch in `inherent_method` below), it is the plain `Copy` configuration half — the one
    # a caller builds directly and hands to a `with_token_budget` door — so it needs no special
    # reaching: `unlimited()` and `with_limitation(n)` are its own public constructors.
    #
    # `limitation` is declared on BOTH `TokenBudget` and `TokenBudgetTally`, and
    # `surface_diff.py` keeps the alphabetically LAST owner — `TokenBudget` < `TokenBudgetTally`
    # — so the shared name routes to the tally, not here. This table's `calls` is therefore
    # empty today; it is filled in anyway (a real, working `build`/`setup`, an explicit empty
    # `calls`) rather than left to a KeyError, for the reason `Cst`'s empty `assoc_calls` gives.
    "TokenBudget": {
        "imports": "use tokora::input::TokenBudget;\n",
        "fixture": "",
        "ty": "TokenBudget",
        "setup": "",
        "build": "  let mut subject: TokenBudget = TokenBudget::with_limitation(1_000);\n",
        "calls": {},
        "assoc_calls": {
            "unlimited": ("", "TokenBudget"),
            "with_limitation": ("1_000", "TokenBudget"),
        },
    },
    # `Status` is minted by the same diff as its three items, so it takes this table's new-owner
    # shape like every entry above it: the base side cannot name the type at all, and only the
    # head side's binding has to be right. Its module (`tokora::types`) is NOT new — only the
    # `Status` name within it is — so a plain `use tokora::types::Status;` already names the
    # owner in the base-side diagnostic and the `new_module_imports` indirection the
    # `diagnostic` types need buys nothing here, the same reasoning `Probe`'s entry gives.
    # `recovery`, the submodule `Status` is re-exported from, is new too, but the owner probed
    # here is reached through the pre-existing `types` re-export, not through `recovery` itself.
    #
    # It is `Copy` and all three items take `self` BY VALUE — not `&self` or `&mut self`, one
    # step EARLIER than any other receiver kind this table carries. The receiver walk is
    # therefore stronger than `Code`'s or `Probe`'s (found at the `&T` step): tokora's item is
    # found at the FIRST candidate, `T` itself, before the generated consumer's `&mut self`
    # extension method is even a candidate.
    "Status": {
        "imports": "use tokora::types::Status;\n",
        "fixture": "",
        "ty": "Status",
        "setup": "",
        # `Status::Error`, not `::Valid` — the module's own doc header measures exactly this
        # variant (`Status::Error.is_error()`, "the boolean reads false before and true after"),
        # so the subject here reproduces the same construction rather than a fresh guess.
        "build": "  let mut subject: Status = Status::Error;\n",
        "calls": {
            "is_valid": ("", "bool"),
            "is_error": ("", "bool"),
            "is_missing": ("", "bool"),
        },
        # `Status` declares no receiver-less associated function — a consumer does not build one
        # from a constructor, it is handed back through `RecoveryState::status`. An empty table
        # rather than an absent key, for the reason `Cst`'s empty `assoc_calls` gives.
        "assoc_calls": {},
    },
}


def inherent_subject(name, owner, table):
    """The owner's call shape for `name`, or a FATAL that names both."""
    rec = INHERENT_SUBJECTS[owner]
    shape = rec[table].get(name)
    if shape is None:
        sys.exit(
            f"gen_probe: no call-shape template for {name!r} on {owner!r}. {owner} is an owner "
            f"this diff INTRODUCES, so the base side cannot compile and the head side must — a "
            f"call built at the wrong arity or bound to the wrong return type scores INCONCL, "
            f"not `new-owner`. Add {name!r} to INHERENT_SUBJECTS[{owner!r}][{table!r}] in "
            f"gen_probe.py with its real arguments and return type."
        )
    return rec, shape


def inherent_subject_method(name, owner, spelling):
    """A consumer extension trait on an owner this same diff introduces."""
    rec, (args, returns) = inherent_subject(name, owner, "calls")
    imports, fixture, ty = rec["imports"], rec["fixture"], rec["ty"]
    setup, build = rec["setup"], rec["build"]
    call = (
        f"let _v: {returns} = subject.{name}({args});"
        if spelling == "used"
        else f"subject.{name}({args});"
    )
    return FIXTURE + fixture + f"""
{imports}
pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl ConsumerExt for {ty} {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

#[allow(unused_mut)]
fn drive() {{
{setup}{build}  reached();
  {call}
}}
""" + WITNESS


def inherent_subject_assoc_fn(name, owner, spelling):
    """The path-resolved half — `Type::name(args)` on an owner this same diff introduces.

    A qualified path (`<Ty>::name(..)`) rather than a turbofish: these owners carry lifetime
    parameters, and `EmitterView::<'_, 'static, ..>::new(..)` spells them in a position where the
    elided form is not accepted. The qualified form takes the type written out once, which is the
    same string the consumer's `impl` and the `used` binding use.
    """
    rec, (args, returns) = inherent_subject(name, owner, "assoc_calls")
    imports, fixture, ty = rec["imports"], rec["fixture"], rec["ty"]
    setup = rec["setup"]
    call = (
        f"let _v: {returns} = <{ty}>::{name}({args});"
        if spelling == "used"
        else f"<{ty}>::{name}({args});"
    )
    return FIXTURE + fixture + f"""
{imports}
pub trait ConsumerAssoc {{
  fn {name}() -> u8;
}}

impl ConsumerAssoc for {ty} {{
  fn {name}() -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  use ConsumerAssoc as _;
{setup}  reached();
  {call}
}}
""" + WITNESS


def inherent_method(name, owner, spelling):
    """A consumer extension trait declaring `name` on the OWNER tokora added it to."""
    if owner in ERROR_SUBJECTS:
        return error_subject_method(name, owner, spelling)
    if owner in INHERENT_SUBJECTS:
        return inherent_subject_method(name, owner, spelling)
    if owner == "ParseState":
        # A PRE-EXISTING owner gaining its first inherent items, so the consumer's own `-> u8`
        # shape is the right one: the base side has to compile, and there it has only the
        # consumer's item. (See INHERENT_SUBJECTS' header for why the rule inverts for a new
        # owner.)
        #
        # `ParseState::new` is `pub(super)`, so the subject cannot be constructed — it is REACHED,
        # through the one door a consumer has: a state-carrying callback. `map_with` hands the
        # state BY VALUE, which is what makes the row a measurement: from a by-value receiver
        # tokora's `&self` items are found at the `&T` step and its `&mut self` items at the
        # `&mut T` step, where an inherent item beats the consumer's trait item. A `&mut
        # ParseState` receiver would hand the consumer's `&mut self` item the FIRST step instead
        # and every row would agree — the `InputRef::recursion` shape recorded in
        # no_collision.txt.
        call = f"let _v: u8 = st.{name}();" if spelling == "used" else f"st.{name}();"
        return FIXTURE + f"""
use tokora::ParseState;

pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl<'inp> ConsumerExt for ParseState<'_, 'inp, '_, PLexer<'inp>, PCtx<'inp>> {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

#[allow(unused_mut)]
fn drive() {{
  let _ = Parser::with_context(ctx())
    .apply(parse_num.map_with(|_o, mut st| {{
      reached();
      {call}
    }}))
    .parse_str("1 2");
}}
""" + WITNESS
    if owner == "ParserContext":
        # `ctx()` is the fixture's own `PCtx`, which exists on both sides — `ParserContext` is
        # not a new type, only `with_recursion_limiter` is a new item on it. The binding is
        # `mut` because the consumer's receiver is `&mut self`; tokora's takes `self` by value,
        # so on the head side its candidate is found one step EARLIER in the receiver walk.
        call = f"let _v: u8 = c.{name}();" if spelling == "used" else f"c.{name}();"
        return FIXTURE + f"""
pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl ConsumerExt for PCtx<'_> {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  let mut c = ctx();
  reached();
  {call}
}}
""" + WITNESS
    if owner == "InputRef":
        call = f"let _v: u8 = inp.{name}();" if spelling == "used" else f"inp.{name}();"
        return FIXTURE + f"""
pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl<'inp, 'b> ConsumerExt for InputRef<'inp, 'b, PLexer<'inp>, PCtx<'inp>> {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  fn body<'inp>(
    inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>,
  ) -> Result<(), PErr> {{
    reached();
    {call}
    Ok(())
  }}
  let _ = Parser::with_context(ctx()).apply(body).parse_str("1 2");
}}
""" + WITNESS
    if owner == "Errors":
        # A PRE-EXISTING public struct — shipped since 0.7.3 — so every consumer holding one
        # already had somewhere to hang these names, and the base side is a real before-state
        # rather than a compile failure. `Errors<PErr>` rides the fixture's own error type at
        # the crate's default container.
        #
        # The binding is `mut` for the reason the `Cst` subject records: the generated consumer
        # item always takes `&mut self`, so an immutable binding is E0596 on a base side where
        # that item is the ONLY candidate, and the probe then measures nothing.
        #
        # Why the base side does NOT reach the container: `Errors` used to carry `DerefMut`, and
        # a deref step is walked only after every pick on `Errors` itself has failed. The
        # consumer's item is found at the `&mut Errors` pick, before any of that, so what the
        # base side runs is the consumer's — which is exactly the before-state this row needs.
        call = f"let _v: u8 = e.{name}();" if spelling == "used" else f"e.{name}();"
        return FIXTURE + f"""
use tokora::error::Errors;

pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl<E> ConsumerExt for Errors<E> {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  let mut e: Errors<PErr> = Errors::new();
  reached();
  {call}
}}
""" + WITNESS
    if owner == "ParseAttempt":
        call = f"let _v: u8 = a.{name}();" if spelling == "used" else f"a.{name}();"
        return FIXTURE + f"""
pub trait ConsumerExt {{
  fn {name}(&self) -> u8;
}}

impl<O> ConsumerExt for ParseAttempt<O> {{
  fn {name}(&self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  let a = ParseAttempt::Accept(1i64);
  reached();
  {call}
}}
""" + WITNESS
    if owner == "TokenBudgetTally":
        # `TokenBudgetTally` is minted by this same diff, so this is the NEW-OWNER shape — the
        # base side cannot resolve the type at all, and the call has to be built at each item's
        # REAL arity with the `used` spelling bound to its REAL return type (see
        # `INHERENT_SUBJECTS`'s header). It is not an `INHERENT_SUBJECTS` entry, though every
        # other new owner is, because that table's shared template constructs its subject with a
        # `build` line evaluated once at the top of `drive()` and used afterward — and this type
        # has no route to a value that could survive past that point.
        #
        # By design: no `TokenBudgetTally::new` is public, and it is neither `Clone` nor `Copy` —
        # see the type's own doc, "It is per input, and it cannot be moved between inputs". A
        # template that called a constructor, or that squirreled a value out through a `Clone`,
        # would defeat the exact thing that round closed. The one door a real consumer has is
        # `InputRef::token_budget(&self) -> &TokenBudgetTally`, so the subject is REACHED inside
        # a real parse, the same shape `ParseState`'s branch above already uses for the same
        # reason — and it is a BORROW, not an owned value, so it cannot outlive the callback
        # either. Every statement below therefore runs inside `body`, not before it.
        #
        # The consumer's extension trait still declares `&mut self`, matching every other
        # template in this file, even though the borrow this accessor hands back makes that
        # candidate unreachable in principle: `subject: &TokenBudgetTally` cannot be reborrowed
        # `&mut`, so tokora's `&self` item is found at the FIRST step of the receiver walk
        # (`self: &TokenBudgetTally` matches the receiver expression's own type exactly) and the
        # consumer's item is never even a candidate, let alone a losing one. That is a stronger
        # result than the owned-value shape the other new-owner tables get — there the consumer's
        # `&mut self` item is merely a later, losing pick — and it is a direct consequence of the
        # type's own contract rather than a limitation of the probe: a caller who can only ever
        # borrow shared cannot write an extension method that would contend for the call at all.
        returns = {
            "is_exhausted": "bool",
            "limitation": "usize",
            "refused_an_item": "bool",
            "spent": "usize",
        }.get(name)
        if returns is None:
            sys.exit(
                f"gen_probe: no return-type mapping for TokenBudgetTally::{name!r} in "
                f"gen_probe.py's inherent_method — add one, at the item's real return type."
            )
        call = (
            f"let _v: {returns} = subject.{name}();"
            if spelling == "used"
            else f"subject.{name}();"
        )
        return FIXTURE + f"""
use tokora::input::TokenBudgetTally;

pub trait ConsumerExt {{
  fn {name}(&mut self) -> u8;
}}

impl ConsumerExt for TokenBudgetTally {{
  fn {name}(&mut self) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  fn body<'inp>(
    inp: &mut InputRef<'inp, '_, PLexer<'inp>, PCtx<'inp>>,
  ) -> Result<(), PErr> {{
    reached();
    let subject: &TokenBudgetTally = inp.token_budget();
    {call}
    Ok(())
  }}
  let _ = Parser::with_context(ctx()).apply(body).parse_str("1 2");
}}
""" + WITNESS
    # Not a default and not a skip: an owner with no template must stop the run, because a probe
    # that rides the wrong subject collides with nothing and reports a clean run.
    #
    # `Descent` deliberately has no entry. It is public and it is new, but it declares NO inherent
    # items at all — it is `Deref`/`DerefMut`/`Drop` and nothing else — so no inherent row can be
    # generated for it and a template here would be code no run ever executes. Its one inventory
    # row is the glob row, which `glob_name` covers. If it ever grows an inherent item, add it to
    # `ERROR_SUBJECTS`-style handling here rather than to the glob side.
    sys.exit(f"gen_probe: no template for inherent method owner {owner!r} (name {name!r})")


def inherent_assoc_fn(name, owner, spelling):
    if owner in ERROR_SUBJECTS:
        return error_subject_assoc_fn(name, owner, spelling)
    if owner in INHERENT_SUBJECTS:
        return inherent_subject_assoc_fn(name, owner, spelling)
    if owner == "RecursionLimiter":
        # A pre-existing type gaining a new associated function, so unlike the two error types
        # this one HAS a before-state: a consumer's `impl ConsumerAssoc for RecursionLimiter`
        # compiles on both sides and the two rows measure which item the path call selects.
        call = (
            f"let _v: u8 = RecursionLimiter::{name}();"
            if spelling == "used"
            else f"RecursionLimiter::{name}();"
        )
        return FIXTURE + f'''
use tokora::state::recursion_tracker::RecursionLimiter;

pub trait ConsumerAssoc {{
  fn {name}() -> u8;
}}

impl ConsumerAssoc for RecursionLimiter {{
  fn {name}() -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  use ConsumerAssoc as _;
  reached();
  {call}
}}
''' + WITNESS
    if "Ident" not in owner:
        sys.exit(f"gen_probe: no template for associated-fn owner {owner!r}")
    call = (
        f"let _v: u8 = Ident::<(), ()>::{name}();"
        if spelling == "used"
        else f"Ident::<(), ()>::{name}();"
    )
    return FIXTURE + f'''
pub trait ConsumerAssoc {{
  fn {name}() -> u8;
}}

impl ConsumerAssoc for Ident<(), ()> {{
  fn {name}() -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
  use ConsumerAssoc as _;
  reached();
  {call}
}}
''' + WITNESS


def trait_method(name, owner, spelling):
    """Both picks. `self` collides at the same pick; `&self` sits at a later one, which is
    where the silent class lives — and the returns here are not `#[must_use]`."""
    # The receiver must match the trait the name is DECLARED on, or the probe collides
    # with nothing and reports a clean run over an experiment that never happened.
    rec = trait_subject(name, owner, "recvr")
    recvr = rec["recvr"]
    recv, call = ("self", f"let _v: u8 = {recvr}.{name}(ARGS);")
    if spelling == "later_pick_discarded":
        recv, call = ("&self", f"{recvr}.{name}(ARGS);")
    elif spelling == "later_pick_used":
        recv, call = ("&self", f"let _v: u8 = {recvr}.{name}(ARGS);")
    args = {
        "labelled": '"n"',
        "traced": '"n"',
        "list_until": "|_t: &Tok| true",
        "separated1_by": "|_t: &Tok| true",
        # The hook's parameter is spelled out: against a fully generic consumer signature
        # a bare closure parameter cannot infer, and the probe would fail to compile on the
        # base side — proving nothing rather than proving safety.
        "peek_then_head":
            "|_h: Option<tokora::span::Spanned<&Tok, &tokora::SimpleSpan>>| Ok::<(), PErr>(())",
        "opt": "",
        # `Emitter::commit_lexer_error(&mut self, Spanned<<L::Token as Token>::Error, L::Span>)`
        # at its real arity, over the fixture's own token and span types: `Tok::Error` is `PErr`
        # and `LogosLexer`'s `Span` is `SimpleSpan`. The consumer's parameter is the generic `A`
        # this file builds for every argument-taking trait method, so the expression only has to
        # be what TOKORA's item would accept — which is the half a wrong-arity call gets wrong.
        "commit_lexer_error":
            "tokora::span::Spanned::new(tokora::SimpleSpan::new(0, 1), PErr)",
        # `Diagnose`'s nine accessors and `DiagnoseExt`'s two, at their real arity. Nine take
        # no argument beyond the receiver; `label` and `path_segment` take one `usize` index
        # each, which is the single-argument shape this table already serves for `labelled`
        # and `commit_lexer_error` — not the multi-argument support #225 declines.
        "code": "",
        "severity": "",
        "primary": "",
        "primary_label": "",
        "labels": "",
        "label": "0usize",
        "path_segments": "",
        "path_segment": "0usize",
        "help": "",
        "labels_iter": "",
        "path_segments_iter": "",
        # `ErrorContainer::clear(&mut self)` — no argument beyond the receiver.
        "clear": "",
        # #282's read frontier. Both take no argument beyond the receiver, so they are the
        # empty-argument shape this table already serves — not the multi-argument support #225
        # declines. `read_frontier` is `&self`; `take_probe` is `&mut self`, which is the
        # foreknown-vacuous shape #225 names, so read its verdict against the bound written in
        # `no_collision.txt` rather than as evidence the name is safe.
        #
        # `take_probe` is the value channel, and it replaces the `probe`/`clear_probe` pair this
        # branch first shipped: two independently defaulted members could be half-implemented, so
        # reading now consumes and there is one method where there were two. It keeps
        # `clear_probe`'s row analysis because the shape is identical — `&mut self`, no argument,
        # subject `()`. `probed_to`, which `probe` was itself a rename of, is an INHERENT method
        # on `Probe`: a different template and a different row.
        "read_frontier": "",
        "take_probe": "",
        # tokora#265's `TrackerExt` (five) and `RecursionTrackerExt` (one). Every one of the
        # six takes `&mut self` and NO further argument — read each signature in
        # `state/tracker/mod.rs` / `state/recursion_tracker/mod.rs` and there is nothing after
        # `&mut self` in any of them — so this is the same empty-argument shape `clear`,
        # `read_frontier` and `take_probe` already ride above, not the multi-argument support
        # #225 declines: nothing here asks the generator to invent a call it cannot express.
        "increase_token_and_decrease_recursion": "",
        "increase_token_and_decrease_recursion_and_check": "",
        "increase_token_and_check": "",
        "increase_both": "",
        "increase_both_and_check": "",
        "increase_and_check": "",
        # `RecoveryState`'s four. `status` takes `&self` and nothing else; the other three are
        # DEFAULTED methods on it with the same signature — read `types/recovery.rs` and there
        # is nothing after `&self` in any of them. The same empty-argument shape `clear`,
        # `read_frontier` and `take_probe` already ride, not the multi-argument support #225
        # declines.
        "status": "",
        "is_valid": "",
        "is_error": "",
        "is_missing": "",
    }.get(name)
    if args is None:
        # The pointer matters more than the refusal. An author who lands here reads "add a
        # template" and starts extending — which is the search issue #225 declines by name.
        sys.exit(
            f"gen_probe: no argument template for trait method {name!r} on {owner!r}. "
            "DECLINED, not missing: multi-argument support is deliberately unbuilt — see issue "
            "#225's ruling BEFORE extending this. Its grounds: for the shapes that reach here "
            "today every row the extension could produce is foreknown-vacuous (a `&mut self` "
            "trait method on a non-deref subject loses every pick to the consumer's blanket "
            "item, so the row would agree while measuring nothing), and generating cells whose "
            "outcome is known in advance is 'extend the generator until the probe goes green' "
            "wearing a work order. The reopen condition is a traced-trait method whose shape "
            "these templates cannot express AND whose pick analysis is NOT foreknown-vacuous. "
            "Until you hold one: justify the row in no_collision.txt, or state in the PR why "
            "the name cannot collide. Do not delete the row."
        )
    # The parameter type is built outside the f-string on purpose. An escaped quote inside
    # an f-string expression is a SyntaxError before Python 3.12 (PEP 701 relaxed it), and
    # this file must parse under the `python3` CI happens to have — which is 3.9 on stock
    # macOS. A gate that cannot be parsed is not a gate.
    _pty = "&'static str" if name in ("labelled", "traced") else "A"
    params = "" if args == "" else f", _a: {_pty}"
    generic = "" if name in ("labelled", "traced") or args == "" else "<A>"
    return FIXTURE + (rec["fixture"] or "") + f'''
pub trait ConsumerComb {{
  fn {name}{generic}({recv}{params}) -> u8;
}}

impl<T> ConsumerComb for T {{
  fn {name}{generic}({recv}{params}) -> u8 {{
    ran();
    7
  }}
}}

fn drive() {{
{trait_scope(rec)}  reached();
  {call.replace("ARGS", args)}
}}
''' + WITNESS


def trait_assoc_fn(name, owner, spelling):
    """A trait associated function that declares NO receiver — `Type::name(..)`.

    This shape had no template at all. `trait_method` hardwires `{recvr}.{name}(..)`, and a
    receiver call cannot typecheck for an item that takes no receiver, so `CastNode::cast_node`
    could only be refused: 3 FATAL rows and a genuinely new public name left unguarded.

    The rule being probed is a different one, which is the point of the split. A method call
    walks the receiver's autoderef/autoref chain and can pick tokora's item SILENTLY at a later
    step. A path call walks no chain: with the consumer's trait and tokora's both applicable to
    the same type, rustc reports E0034 and refuses to choose. So the expected verdict here is
    `loud`, and a `silent` one would be news.

    Both applicable is the whole construction, and each half is load-bearing:

      * the consumer's `impl<T> ConsumerAssoc for T` covers the subject type — and everything
        else, which is what a consumer's own extension trait looks like from tokora's side;
      * `self_ty` must satisfy tokora's trait, or tokora's item is not a candidate, there is
        no second candidate, both sides agree and the row scores UNPROBED. Verified by
        removing the trait's `scope` import: the head side then compiles clean and the row
        goes UNPROBED rather than green.
    """
    rec = trait_subject(name, owner, "self_ty")
    ty = rec["self_ty"]
    call = f"let _v: u8 = {ty}::{name}();" if spelling == "used" else f"{ty}::{name}();"
    # The glob and the collision live in a child module so the glob cannot shadow the shared
    # fixture's own imports; `use super::*` is what the witness module already relies on.
    return FIXTURE + (rec["fixture"] or "") + f'''
mod clash {{
  use super::*;
{trait_scope(rec)}
  pub trait ConsumerAssoc {{
    fn {name}() -> u8;
  }}

  impl<T> ConsumerAssoc for T {{
    fn {name}() -> u8 {{
      ran();
      7
    }}
  }}

  pub fn go() {{
    reached();
    {call}
  }}
}}

fn drive() {{
  clash::go();
}}
''' + WITNESS


def trait_assoc_const(name, owner, spelling):
    """An associated CONST on a trait — `Type::NAME`, resolved by path.

    Split out of `trait_assoc_item`'s blanket refusal because this half IS expressible, and
    refusing it left a genuinely new public name unprobed. The rule is `trait_assoc_fn`'s, one
    namespace over: a path carries no receiver, so there is no autoderef chain to pick a later
    candidate off. With the consumer's trait and tokora's both applicable to the same type,
    rustc reports E0034 and refuses to choose — so `loud` is the expected verdict here and a
    `silent` one would be news.

    The consumer's const cannot call `ran()` — a const initialiser runs at compile time and a
    thread-local bump is not const — so the witness is taken at the USE site instead: `go()`
    reads the const, compares it against the consumer's own value, and bumps only on a match.
    A resolution that reached tokora's item instead would not compile anyway (its type is not
    `u8`), which is the second reason this shape cannot be a silent steal.
    """
    rec = trait_subject(name, owner, "self_ty")
    ty = rec["self_ty"]
    read = f"let v: u8 = {ty}::{name};" if spelling == "used" else f"let v: u8 = {ty}::{name}; let _ = v;"
    return FIXTURE + (rec["fixture"] or "") + f'''
mod clash {{
  use super::*;
{trait_scope(rec)}
  pub trait ConsumerConst {{
    const {name}: u8;
  }}

  impl<T> ConsumerConst for T {{
    const {name}: u8 = 7;
  }}

  pub fn go() {{
    reached();
    {read}
    if v == 7 {{
      ran();
    }}
  }}
}}

fn drive() {{
  clash::go();
}}
''' + WITNESS


def trait_assoc_item(name, owner, spelling):
    """An associated TYPE or CONST on a trait. There is no template, and saying so is the
    point: it used to be filed with the methods and fail with a message about a missing
    argument template, which named the wrong problem and would have sent the reader looking
    for a signature that does not exist."""
    sys.exit(
        f"gen_probe: {name!r} on {owner!r} is an associated TYPE or CONST, not a function. "
        f"It resolves by rules of its own and this harness has no probe for it — write one, "
        f"or state in the PR why the name cannot collide. Do not delete the row."
    )


def glob_name(name, owner, spelling):
    """Two competing globs over the same name.

    `owner` is `<module-path>:<kind>` from the inventory. Both halves are load-bearing and
    both were wrong once: globbing the crate root for a name that lives in
    `tokora::parser` brings nothing in, and referencing a FUNCTION in the type namespace
    (`&name`) collides with nothing because functions live in the value namespace. Either
    mistake yields a probe that compiles clean on both sides and has tested nothing.
    """
    path, _, kind = owner.rpartition(":")
    if not path or not kind:
        sys.exit(f"gen_probe: glob probe for {name!r} needs '<path>:<kind>', got {owner!r}")
    if kind in ("struct", "trait", "enum", "union", "type_alias", "module"):
        decl, use = f"pub struct {name};", f"pub fn uses(_x: &{name}) {{}}"
    elif kind in ("function", "constant", "static"):
        # A const puts the name in the VALUE namespace, which is where a function lives.
        decl, use = f"pub const {name}: u8 = 0;", f"pub fn uses() -> u8 {{ {name} }}"
    else:
        sys.exit(f"gen_probe: glob probe has no template for namespace kind {kind!r} ({name})")
    return FIXTURE + f"""
mod other {{
  {decl}
}}

mod clash {{
  use super::other::*;
  use {path}::*;

  {use}
}}

fn drive() {{
  ran();
}}
""" + WITNESS


def glob_macro(name, owner, spelling):
    """A macro name under two competing globs — the `tokio::select!` shape.

    `#[macro_export]` always lands at a crate root, so the competing exporter has to be a
    SECOND CRATE. The first version of this template defined a macro and globbed nothing,
    so all four macro names reported clean while never being probed at all.
    """
    path, _, _kind = owner.rpartition(":")
    return FIXTURE + f"""
mod clash {{
  use otherlib::*;
  use {path or 'tokora'}::*;

  pub fn uses() -> usize {{
    {name}!(anything)
  }}
}}

fn drive() {{
  ran();
}}
""" + WITNESS


def otherlib(name):
    """The competing crate for a macro clash."""
    return f"""//! Stands in for `tokio` / `futures`: another crate exporting `{name}!`.
#[macro_export]
macro_rules! {name} {{
  ($($tt:tt)*) => {{ 0usize }};
}}
"""


DISPATCH = {
    "inherent_method": inherent_method,
    "inherent_assoc_fn": inherent_assoc_fn,
    "trait_method": trait_method,
    "trait_assoc_fn": trait_assoc_fn,
    "trait_assoc_const": trait_assoc_const,
    "trait_assoc_item": trait_assoc_item,
    "glob_name": glob_name,
    "glob_macro": glob_macro,
}


def main():
    if len(sys.argv) == 3 and sys.argv[1] == "--otherlib":
        sys.stdout.write(otherlib(sys.argv[2]))
        return
    if len(sys.argv) != 5:
        sys.exit("usage: gen_probe.py <category> <name> <owner> <spelling>")
    category, name, owner, spelling = sys.argv[1:5]
    fn = DISPATCH.get(category)
    if fn is None:
        # Never skip. An unhandled category is how the first version passed over 29 names.
        sys.exit(f"gen_probe: UNHANDLED CATEGORY {category!r} — refusing to emit nothing")
    sys.stdout.write(fn(name, owner, spelling))


if __name__ == "__main__":
    main()
