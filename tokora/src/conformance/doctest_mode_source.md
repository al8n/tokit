# use tokora::Source;
# use tokora::conformance::{RefillDriver, SameAllocation};
# // A source that carries a fact its `Slice` does not: `keywords` decides whether the word `kw`
# // lexes as a keyword or an identifier, and two of these can be equal at `as_slice` and lex
# // differently. Indexing hands back stored prefixes, which is legal and is caller code.
# #[derive(Debug)]
# struct ModeSource {
#   text: &'static str,
#   keywords: bool,
#   prefixes: Vec<ModeSource>,
# }
# impl ModeSource {
#   fn leaf(text: &'static str, keywords: bool) -> Self {
#     Self { text, keywords, prefixes: Vec::new() }
#   }
#   fn new(text: &'static str, keywords: bool, prefix_mode: bool) -> Self {
#     let prefixes = (0..=text.len()).map(|k| Self::leaf(&text[..k], prefix_mode)).collect();
#     Self { text, keywords, prefixes }
#   }
# }
# impl Source<usize> for ModeSource {
#   type Slice<'a> = &'a str where Self: 'a;
#   fn is_empty(&self) -> bool { self.text.is_empty() }
#   fn len(&self) -> usize { self.text.len() }
#   fn as_slice(&self) -> &str { self.text }
#   fn slice<R>(&self, range: R) -> Option<&str>
#   where
#     R: core::ops::RangeBounds<usize>,
#   {
#     self.text.get((range.start_bound().map(|s| *s), range.end_bound().map(|s| *s)))
#   }
#   fn is_boundary(&self, index: usize) -> bool { self.text.is_char_boundary(index) }
# }
# impl core::ops::Index<core::ops::RangeTo<usize>> for ModeSource {
#   type Output = Self;
#   fn index(&self, range: core::ops::RangeTo<usize>) -> &Self { &self.prefixes[range.end] }
# }
# // A driver of the caller's own, holding the buffers their own chunking would hold.
# struct OwnChunks(Vec<ModeSource>);
# impl<'inp> RefillDriver<'inp, ModeSource> for &'inp OwnChunks {
#   fn buffer(&self, _idx: usize, _src: &'inp ModeSource, k: usize) -> Option<&'inp ModeSource> {
#     self.0.get(k)
#   }
# }
# // Exactly what `Harness::run_refill` asks of the driver it is handed, and nothing else.
# fn takes_a_driver<'a, S, D>(_: D)
# where
#   S: Source<usize> + ?Sized,
#   D: RefillDriver<'a, S>,
# {
# }
