// `Dialect` is user-implemented and not blanket, so it is genuinely the root
// obligation when a type is handed to a dialect-generic signature without one.
#![allow(dead_code)]

use tokora::Dialect;

struct NotADialect;

fn needs_dialect<'inp, D: Dialect<'inp>>() {}

fn main() {
  needs_dialect::<NotADialect>();
}
