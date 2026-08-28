# use core::convert::Infallible;
# use tokora::{Lexer, SimpleSpan, Token};
# use tokora::conformance::{Harness, OwnedChunks};
# #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
# struct CharKind;
# impl core::fmt::Display for CharKind {
#   fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result { f.write_str("char") }
# }
# #[derive(Clone, Debug, PartialEq)]
# struct CharTok;
# impl Token<'_> for CharTok {
#   type Kind = CharKind;
#   type Error = Infallible;
#   const SCAN_LOOKAHEAD: tokora::ScanLookahead = tokora::ScanLookahead::Unbounded;
#   fn kind(&self) -> CharKind { CharKind }
#   fn is_trivia(&self) -> bool { false }
# }
# struct CharLexer<'a> { src: &'a str, start: usize, end: usize, state: () }
# impl<'a> Lexer<'a> for CharLexer<'a> {
#   type State = ();
#   type Source = str;
#   type Token = CharTok;
#   type Span = SimpleSpan;
#   type Offset = usize;
#   fn new(src: &'a str) -> Self { Self { src, start: 0, end: 0, state: () } }
#   fn with_state(src: &'a str, state: ()) -> Self { Self { src, start: 0, end: 0, state } }
#   fn check(&self) -> Result<(), Infallible> { Ok(()) }
#   fn state(&self) -> &() { &self.state }
#   fn state_mut(&mut self) -> &mut () { &mut self.state }
#   fn into_state(self) { self.state }
#   fn source(&self) -> &'a str { self.src }
#   fn span(&self) -> SimpleSpan { SimpleSpan::new(self.start, self.end) }
#   fn slice(&self) -> &'a str { &self.src[self.start..self.end] }
#   fn lex(&mut self) -> Option<Result<CharTok, Infallible>> {
#     self.start = self.end;
#     if self.start >= self.src.len() { return None; }
#     let mut e = self.start + 1;
#     while e < self.src.len() && !self.src.is_char_boundary(e) { e += 1; }
#     self.end = e;
#     Some(Ok(CharTok))
#   }
#   fn read_frontier(&self) -> tokora::ReadFrontier<usize> { tokora::ReadFrontier::SpanEnd }
#   fn bump(&mut self, n: &usize) { self.end += *n; }
# }
