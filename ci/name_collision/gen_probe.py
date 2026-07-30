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


def inherent_method(name, owner, spelling):
    """A consumer extension trait declaring `name` on the OWNER tokora added it to."""
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
    sys.exit(f"gen_probe: no template for inherent method owner {owner!r} (name {name!r})")


def inherent_assoc_fn(name, owner, spelling):
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
    recvr = "try_num" if owner == "TryParseInput" else "parse_num"
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
    return FIXTURE + f'''
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
  reached();
  {call.replace("ARGS", args)}
}}
''' + WITNESS


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
