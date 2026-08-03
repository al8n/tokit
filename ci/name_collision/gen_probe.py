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


def inherent_method(name, owner, spelling):
    """A consumer extension trait declaring `name` on the OWNER tokora added it to."""
    if owner in ERROR_SUBJECTS:
        return error_subject_method(name, owner, spelling)
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
    }.get(name)
    if args is None:
        sys.exit(f"gen_probe: no argument template for trait method {name!r} on {owner!r}")
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
