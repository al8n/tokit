#!/usr/bin/env python3
"""Mechanical public-surface delta between two rustdoc-JSON dumps.

An earlier version read **only the crate root's direct item list**, so
nested public modules were invisible — it missed `tokora::parser::{dispatch_take,
try_dispatch_take}` and the new public module `tokora::cst::kinds`, both reachable by
a legal consumer glob (`use tokora::parser::*`). This version walks the whole public
module tree, following `module` items and `use` re-exports (named and glob), and
reports every module path a new name lands in.

Usage:
    surface_diff.py BASE.json PROTO.json --features "std,logos,trace,rowan"

The `--features` string is **required** and is echoed into the output: a rustdoc dump
is a fact about one feature point (the lesson V21 exists to teach).

Emits four categories:
  MODULE          a new public module (its own glob namespace)
  GLOB NAME       a name newly reachable from `use <path>::*` (E0659 risk), with
                  every module path it is reachable from
  TRAIT-ITEM      a new method declared on a trait, and whether the trait is new
  INHERENT        a new item in an inherent impl, SPLIT into receiver methods and
                  associated functions — they resolve by different rules

Writes `surface_names.json` next to the proto dump so `census_v3.py` consumes the
name set instead of hand-copying it.
"""

import json
import os
import sys
from collections import defaultdict


def load(path):
    with open(path) as f:
        d = json.load(f)
    return d, {str(k): v for k, v in d["index"].items()}


def get(idx, i):
    return idx.get(str(i))


def kind_of(item):
    inner = item.get("inner")
    return next(iter(inner.keys()), "?") if isinstance(inner, dict) else "?"


def namespace(idx, mod_id, _memo=None, _stack=None):
    """{name: (kind, submodule_id_or_None)} — a module's *resolved* namespace.

    Resolves `use` re-exports, including globs from private modules (which is how
    `WhileHead`/`WhileKind` reach the root: `pub use decision_adapters::{..}` inside
    `parse_input`, then `pub use parse_input::*` at the root). Reading a module's
    direct `items` alone misses those.
    """
    if _memo is None:
        _memo = {}
    if _stack is None:
        _stack = set()
    key = str(mod_id)
    if key in _memo:
        return _memo[key]
    if key in _stack:
        return {}
    _stack.add(key)

    out = {}
    item = get(idx, mod_id)
    inner = item.get("inner", {}) if item else {}
    if isinstance(inner, dict) and "module" in inner:
        for cid in inner["module"].get("items", []):
            child = get(idx, cid)
            if child is None:
                continue
            cinner = child.get("inner", {})
            if isinstance(cinner, dict) and "use" in cinner:
                u = cinner["use"]
                tid = u.get("id")
                tgt = get(idx, tid) if tid is not None else None
                if u.get("is_glob"):
                    if tgt and "module" in tgt.get("inner", {}):
                        for nm, val in namespace(idx, tid, _memo, _stack).items():
                            out.setdefault(nm, val)
                    continue
                nm = u.get("name")
                if nm:
                    sub = tid if (tgt and "module" in tgt.get("inner", {})) else None
                    out.setdefault(nm, (kind_of(tgt) if tgt else "use", sub))
                continue
            nm = child.get("name")
            if not nm:
                continue
            sub = cid if "module" in cinner else None
            out.setdefault(nm, (kind_of(child), sub))

    _stack.discard(key)
    _memo[key] = out
    return out


def walk_modules(d, idx):
    """path -> {name: kind} for every public module reachable from the crate root."""
    out = defaultdict(dict)
    seen = set()
    memo = {}

    def visit(mod_id, path):
        key = (str(mod_id), path)
        if key in seen:
            return
        seen.add(key)
        # register the path even when the module is empty — `cst::kinds` holds only a
        # `#[macro_export]` macro (which lands at the crate root), and an empty new
        # module is still a new glob namespace.
        _ = out[path]
        for nm, (kind, sub) in namespace(idx, mod_id, memo).items():
            out[path].setdefault(nm, kind)
            if sub is not None:
                visit(sub, path + "::" + nm)

    visit(d["root"], "tokora")
    return out


def trait_methods(idx):
    out = defaultdict(set)
    for _tid, it in idx.items():
        inner = it.get("inner")
        if not isinstance(inner, dict) or "trait" not in inner:
            continue
        tname = it.get("name")
        for cid in inner["trait"].get("items", []):
            sub = get(idx, cid)
            if sub and sub.get("name"):
                out[tname].add(sub["name"])
    return out


def inherent_items(idx):
    out = defaultdict(set)
    for _iid, it in idx.items():
        inner = it.get("inner")
        if not isinstance(inner, dict) or "impl" not in inner:
            continue
        im = inner["impl"]
        if im.get("trait") is not None:
            continue
        forty = im.get("for", {})
        owner = None
        if isinstance(forty, dict):
            rp = forty.get("resolved_path")
            if isinstance(rp, dict):
                owner = rp.get("path")
        for cid in im.get("items", []):
            sub = get(idx, cid)
            if sub and sub.get("name"):
                out[sub["name"]].add((owner, _has_self(sub)))
    return out


def _has_self(item):
    """Does this inherent item take a receiver?

    Methods and associated functions resolve by DIFFERENT rules — a method call walks the
    receiver's autoderef/autoref chain, a path call `Type::name(..)` does not — so an
    inventory that merges them hides a whole mechanism. It did: `Ident::{parse_except,
    try_parse_except}` were counted among the 'inherent items' and never probed as paths.
    """
    inner = item.get("inner")
    if not isinstance(inner, dict):
        return False
    fn = inner.get("function") or {}
    sig = fn.get("sig") or {}
    for name, _ty in sig.get("inputs", []) or []:
        return name == "self"
    return False


def main():
    if len(sys.argv) < 5 or sys.argv[3] != "--features":
        sys.exit(
            'usage: surface_diff.py BASE.json PROTO.json --features "std,logos,..."'
        )
    base_path, proto_path, features = sys.argv[1], sys.argv[2], sys.argv[4]

    bd, bidx = load(base_path)
    pd, pidx = load(proto_path)

    print("# feature point: " + features)
    print("# base:  " + base_path)
    print("# proto: " + proto_path)
    print()

    bmods, pmods = walk_modules(bd, bidx), walk_modules(pd, pidx)

    new_mods = sorted(set(pmods) - set(bmods))
    print("== NEW PUBLIC MODULES (each opens its own glob namespace) ==")
    for m in new_mods or ["  (none)"]:
        print("  " + m)
    print()

    name_paths = defaultdict(list)
    for path, names in pmods.items():
        for nm, kind in names.items():
            if nm not in bmods.get(path, {}):
                name_paths[(nm, kind)].append(path)

    print("== NEW GLOB-REACHABLE NAMES (`use <path>::*` -> E0659 risk) ==")
    glob_names = set()
    glob_details = {}
    for (nm, kind), paths in sorted(name_paths.items()):
        paths = [p for p in paths if p not in new_mods]
        if not paths:
            continue
        glob_names.add(nm)
        # The MODULE the name is reachable from and the NAMESPACE it occupies. A probe
        # that globs the crate root for a name living in `tokora::parser`, or that
        # references a function in the type namespace, constructs no collision at all and
        # reports a clean run over an experiment that never happened.
        glob_details[nm] = {"kind": kind, "path": sorted(paths)[0]}
        print("  %-26s %-10s via %s" % (nm, kind, ", ".join(sorted(paths))))
    print()

    btm, ptm = trait_methods(bidx), trait_methods(pidx)
    print("== NEW TRAIT-DECLARED METHODS ==")
    trait_names = set()
    trait_owners = {}
    for t in sorted(ptm, key=lambda x: (x is None, x)):
        for m in sorted(ptm[t] - btm.get(t, set())):
            tag = "EXISTING trait" if t in btm else "NEW trait"
            trait_names.add(m)
            trait_owners[m] = str(t)
            print("  %-26s on %s   [%s]" % (m, t, tag))
    print()

    bim, pim = inherent_items(bidx), inherent_items(pidx)
    print("== NEW INHERENT ITEMS ==")
    inherent_names = set()
    method_names, assoc_names = set(), set()
    inherent_owners = {}
    for nm in sorted(pim):
        for owner, has_self in sorted(pim[nm] - bim.get(nm, set()), key=str):
            inherent_names.add(nm)
            inherent_owners[nm] = str(owner)
            if has_self:
                method_names.add(nm)
                kind = "method"
            else:
                assoc_names.add(nm)
                kind = "ASSOCIATED FN (path-resolved)"
            print("  %-26s on %-28s %s" % (nm, owner, kind))
    print()
    print(
        "#   of which %d receiver method(s), %d associated function(s)"
        % (len(method_names), len(assoc_names))
    )

    print(
        "# totals: %d glob-reachable, %d trait-method, %d inherent, %d new modules"
        % (len(glob_names), len(trait_names), len(inherent_names), len(new_mods))
    )

    out = {
        "feature_point": features,
        "glob_names": sorted(glob_names),
        "glob_details": glob_details,
        "new_modules": sorted(new_mods),
        "trait_method_names": sorted(trait_names),
        "inherent_names": sorted(inherent_names),
        "inherent_method_names": sorted(method_names),
        "inherent_assoc_fn_names": sorted(assoc_names),
        "inherent_owners": inherent_owners,
        "trait_method_owners": trait_owners,
    }
    dest = os.path.join(
        os.path.dirname(os.path.abspath(proto_path)), "surface_names.json"
    )
    with open(dest, "w") as f:
        json.dump(out, f, indent=2)
    print("# machine-readable name set -> " + dest)


if __name__ == "__main__":
    main()
