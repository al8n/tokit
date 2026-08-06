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
  Emitter, InputRef, Lexer, ParseContext, ParseInput, Token as TokenT, TryParseInput,
  Parse, Parser, ParserContext,
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
        # NOT `mut`: three of the seven items consume `self` and the other four take `&self`, so a
        # `mut` binding would be an `unused_mut` warning on every row — a head-side diagnostic
        # this template manufactured, in a harness whose verdicts are decided by which side gains
        # one.
        "build": (
            "  let subject: Cst<'static, Mini<'static>, Silent<PErr>> = mini_cst();\n"
        ),
        "calls": {
            "with_trivia_policy": ("TriviaPolicy::AsEmitted", "Cst<'static, Mini<'static>, Silent<PErr>>"),
            "trivia_policy": ("", "TriviaPolicy"),
            "error_kind": ("", "u16"),
            "gap_kind": ("", "u16"),
            "inner_ref": ("", "&Silent<PErr>"),
            "finish": ("MINI_ROOT", "(Result<GreenNode, FinishError>, Silent<PErr>)"),
            "finish_partial": ("MINI_ROOT", "(Result<GreenNode, FinishError>, Silent<PErr>)"),
        },
        # `Cst` declares no receiver-less associated function: `from_sink` is `pub(crate)` and the
        # drivers are the only minters. An empty table rather than an absent key, so the refusal
        # below reads "this owner has no such shape" instead of a KeyError.
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
