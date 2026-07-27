//! JSON parser example demonstrating advanced tokora features.
//!
//! This example parses JSON (objects, arrays, strings, numbers, booleans, null)
//! using `peek_then_choice` for deterministic dispatch, `separated_by` for
//! comma-separated lists, and `DelimitedBy` for bracket/brace-delimited containers.
//!
//! Run: `cargo run --example json --features logos`

use std::num::ParseFloatError;

use derive_more::{Display, From, Unwrap};
use generic_arraydeque::typenum::U1;

use tokora::{
  Accumulator, Branch, Emitter, InputRef, Parse, ParseChoice, ParseContext, ParseInput, Parser,
  Token as TokenT, TryParseInput,
  cache::Peeked,
  emitter::{
    FromUnclosed, FullContainerEmitter, SeparatedEmitter, UnclosedEmitter,
    UnexpectedLeadingSeparatorEmitter, UnexpectedTrailingSeparatorEmitter,
  },
  error::{
    Unclosed, UnexpectedEot,
    syntax::{FullContainer, MissingSyntaxOf},
    token::{MissingTokenOf, SeparatedErrorOf, UnexpectedToken, UnexpectedTokenOf},
  },
  logos::{self, Logos},
  parser::{Action, expect},
  punct::{Brace, Bracket, CloseBrace, CloseBracket, Colon, Comma, OpenBrace, OpenBracket},
  span::Spanned,
  token::PunctuatorToken,
  try_parse_input::ParseAttempt,
  utils::Expected,
};

#[derive(Clone, Debug, From, PartialEq, Eq)]
enum JsonLexerError {
  ParseFloat(Spanned<ParseFloatError>),
  Other(&'static str),
}

impl Default for JsonLexerError {
  fn default() -> Self {
    JsonLexerError::Other("unknown lexer error")
  }
}

impl From<JsonLexerError> for JsonError<'_> {
  fn from(err: JsonLexerError) -> Self {
    match err {
      JsonLexerError::ParseFloat(e) => JsonError::Parse(e),
      JsonLexerError::Other(msg) => JsonError::Other(msg),
    }
  }
}

impl From<()> for JsonLexerError {
  fn from(_: ()) -> Self {
    JsonLexerError::Other("unknown lexer error")
  }
}

#[derive(Clone, From, Unwrap)]
enum JsonError<'a> {
  Parse(Spanned<ParseFloatError>),
  MissingSeparator(MissingTokenOf<'a, JsonLexer<'a>>),
  MissingElement(MissingSyntaxOf<'a, JsonLexer<'a>>),
  Separator(SeparatedErrorOf<'a, JsonLexer<'a>>),
  UnexpectedToken(UnexpectedTokenOf<'a, JsonLexer<'a>>),
  FullContainer(FullContainer),
  // Typed migration arms preserve the concrete delimiter marker the parser emitted.
  // JSON arrays and objects use distinct `Unclosed` types, so a consuming error needs
  // one conversion for each delimiter pair it parses.
  UnclosedBracket(Unclosed<Bracket>),
  UnclosedBrace(Unclosed<Brace>),
  Eot(UnexpectedEot),
  Other(&'a str),
}

impl Default for JsonError<'_> {
  fn default() -> Self {
    JsonError::Other("unknown error")
  }
}

impl From<Option<Spanned<ParseFloatError>>> for JsonError<'_> {
  fn from(opt: Option<Spanned<ParseFloatError>>) -> Self {
    match opt {
      Some(err) => JsonError::Parse(err),
      None => JsonError::Other("unknown parse float error"),
    }
  }
}

impl From<()> for JsonError<'_> {
  fn from(_: ()) -> Self {
    JsonError::Other("unknown error")
  }
}

// One umbrella conversion covers every delimiter pair. `from_unclosed` is generic over the
// pair, so the typed arm is chosen by the runtime name the `Unclosed` carries; the catch-all
// is mandatory — over a generic `D` no arm set is exhaustive — and must diagnose, not panic.
impl<'inp, L> FromUnclosed<'inp, L> for JsonError<'_>
where
  L: tokora::Lexer<'inp, Span = tokora::SimpleSpan>,
{
  fn from_unclosed<D>(err: Unclosed<D, tokora::SimpleSpan>) -> Self {
    let (span, name) = err.into_components();
    match name.as_ref() {
      "[]" => JsonError::UnclosedBracket(Unclosed::of(span, name)),
      "{}" => JsonError::UnclosedBrace(Unclosed::of(span, name)),
      _ => JsonError::Other("unclosed delimiter"),
    }
  }
}

// The committed dispatch drivers raise the same end-of-input carrying their `TokenKind`
// classification table. `JsonError::Eot` holds the default expected-set spelling — a
// `Set`-generic impl would overlap the derived `From<UnexpectedEot>` above — so this arm
// re-spells the diagnostic at the default set. The position survives the crossing because
// `offset`/`name`/`hint` are readable at every `Set`; only the table is dropped.
impl From<UnexpectedEot<usize, (), TokenKind>> for JsonError<'_> {
  fn from(err: UnexpectedEot<usize, (), TokenKind>) -> Self {
    JsonError::Eot(UnexpectedEot::eot(err.offset()))
  }
}

impl core::fmt::Debug for JsonError<'_> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Parse(err) => write!(f, "{err:?}"),
      Self::UnexpectedToken(err) => err.debug_fmt(f),
      Self::Eot(err) => write!(f, "{err:?}"),
      Self::Separator(err) => {
        write!(f, "{:?} separator: ", err.position())?;
        err.inner_ref().debug_fmt(f)
      }
      Self::MissingSeparator(err) => err.debug_fmt(f),
      Self::MissingElement(err) => write!(f, "{err:?}"),
      Self::FullContainer(err) => write!(f, "{err:?}"),
      Self::UnclosedBracket(err) => write!(f, "{err:?}"),
      Self::UnclosedBrace(err) => write!(f, "{err:?}"),
      Self::Other(msg) => write!(f, "{}", msg),
    }
  }
}

impl core::fmt::Display for JsonError<'_> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Parse(err) => write!(f, "parse float error: {}", err),
      Self::UnexpectedToken(err) => err.display_fmt(f),
      Self::Eot(err) => write!(f, "{}", err),
      Self::Separator(err) => {
        write!(f, "unexpected {} separator: ", err.position())?;
        err.inner_ref().display_fmt(f)
      }
      Self::MissingSeparator(err) => err.display_fmt(f),
      Self::MissingElement(err) => write!(f, "{err}"),
      Self::FullContainer(err) => write!(f, "{err}"),
      // Both typed variants are formatted from their carried delimiter name.
      Self::UnclosedBracket(err) => write!(f, "unclosed delimiter {}", err.name_ref()),
      Self::UnclosedBrace(err) => write!(f, "unclosed delimiter {}", err.name_ref()),
      Self::Other(msg) => write!(f, "{}", msg),
    }
  }
}

impl core::error::Error for JsonError<'_> {}

#[derive(Debug, Logos, Clone, Unwrap)]
#[logos(
  crate = logos,
  skip r"[ \t\r\n\f]+", error = JsonLexerError
)]
enum Token<'a> {
  #[token("false", |_| false)]
  #[token("true", |_| true)]
  Bool(bool),

  #[token("{")]
  BraceOpen,

  #[token("}")]
  BraceClose,

  #[token("[")]
  BracketOpen,

  #[token("]")]
  BracketClose,

  #[token(":")]
  Colon,

  #[token(",")]
  Comma,

  #[token("null")]
  Null,

  #[regex(r"-?(?:0|[1-9]\d*)(?:\.\d+)?(?:[eE][+-]?\d+)?", |lex| lex.slice().parse::<f64>().map_err(|e| JsonLexerError::ParseFloat(Spanned::new(lex.span().into(), e))))]
  Number(f64),

  #[regex(r#""([^"\\\x00-\x1F]|\\(["\\bnfrt/]|u[a-fA-F0-9]{4}))*""#, |lex| lex.slice())]
  String(&'a str),
}

impl core::fmt::Display for Token<'_> {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Token::Bool(b) => write!(f, "{}", b),
      Token::BraceOpen => write!(f, "{{"),
      Token::BraceClose => write!(f, "}}"),
      Token::BracketOpen => write!(f, "["),
      Token::BracketClose => write!(f, "]"),
      Token::Colon => write!(f, ":"),
      Token::Comma => write!(f, ","),
      Token::Null => write!(f, "null"),
      Token::Number(n) => write!(f, "{}", n),
      Token::String(s) => write!(f, "\"{}\"", s),
    }
  }
}

impl Token<'_> {
  #[inline]
  fn is_value_start(&self) -> bool {
    matches!(
      self,
      Token::Bool(_)
        | Token::Null
        | Token::Number(_)
        | Token::String(_)
        | Token::BraceOpen
        | Token::BracketOpen
    )
  }
}

impl<'inp> PunctuatorToken<'inp> for Token<'inp> {
  #[inline]
  fn comma() -> Option<Self::Kind> {
    Some(TokenKind::Comma)
  }

  #[inline]
  fn colon() -> Option<Self::Kind> {
    Some(TokenKind::Colon)
  }

  #[inline]
  fn open_brace() -> Option<Self::Kind> {
    Some(TokenKind::BraceOpen)
  }

  #[inline]
  fn close_brace() -> Option<Self::Kind> {
    Some(TokenKind::BraceClose)
  }

  #[inline]
  fn open_bracket() -> Option<Self::Kind> {
    Some(TokenKind::BracketOpen)
  }

  #[inline]
  fn close_bracket() -> Option<Self::Kind> {
    Some(TokenKind::BracketClose)
  }
}

#[derive(Debug, Display, PartialEq, Eq, Clone, Copy, Hash)]
enum TokenKind {
  #[display("bool")]
  Bool,

  #[display("{{")]
  BraceOpen,
  #[display("}}")]
  BraceClose,
  #[display("[")]
  BracketOpen,
  #[display("]")]
  BracketClose,
  #[display(":")]
  Colon,
  #[display(",")]
  Comma,
  #[display("null")]
  Null,
  #[display("number")]
  Number,
  #[display("string")]
  String,
}

impl From<&Token<'_>> for TokenKind {
  fn from(token: &Token<'_>) -> Self {
    match token {
      Token::Bool(_) => TokenKind::Bool,
      Token::BraceOpen => TokenKind::BraceOpen,
      Token::BraceClose => TokenKind::BraceClose,
      Token::BracketOpen => TokenKind::BracketOpen,
      Token::BracketClose => TokenKind::BracketClose,
      Token::Colon => TokenKind::Colon,
      Token::Comma => TokenKind::Comma,
      Token::Null => TokenKind::Null,
      Token::Number(_) => TokenKind::Number,
      Token::String(_) => TokenKind::String,
    }
  }
}

impl<'inp> TokenT<'inp> for Token<'inp> {
  type Kind = TokenKind;

  type Error = JsonLexerError;

  #[inline]
  fn kind(&self) -> Self::Kind {
    TokenKind::from(self)
  }

  #[inline]
  fn is_trivia(&self) -> bool {
    false
  }
}

impl From<Comma> for TokenKind {
  #[inline]
  fn from(_: Comma) -> Self {
    TokenKind::Comma
  }
}

impl From<Colon> for TokenKind {
  #[inline]
  fn from(_: Colon) -> Self {
    TokenKind::Colon
  }
}

impl From<OpenBrace> for TokenKind {
  #[inline]
  fn from(_: OpenBrace) -> Self {
    TokenKind::BraceOpen
  }
}

impl From<CloseBrace> for TokenKind {
  #[inline]
  fn from(_: CloseBrace) -> Self {
    TokenKind::BraceClose
  }
}

impl From<OpenBracket> for TokenKind {
  #[inline]
  fn from(_: OpenBracket) -> Self {
    TokenKind::BracketOpen
  }
}

impl From<CloseBracket> for TokenKind {
  #[inline]
  fn from(_: CloseBracket) -> Self {
    TokenKind::BracketClose
  }
}

type JsonLexer<'a> = tokora::lexer::LogosLexer<'a, Token<'a>>;

// Example of using map combinator to extract token values
#[derive(Debug, Clone)]
pub enum JsonValue<'a> {
  Null,
  Bool(bool),
  Number(f64),
  String(&'a str),
  List(Vec<JsonValue<'a>>),
  Object(Vec<(&'a str, JsonValue<'a>)>),
}

impl<'inp> JsonValue<'inp> {
  fn decide<Ctx>(
    mut peeked: Peeked<'_, 'inp, JsonLexer<'inp>, U1>,
    _: &mut Ctx::Emitter,
  ) -> Result<Action, <Ctx::Emitter as Emitter<'inp, JsonLexer<'inp>>>::Error>
  where
    Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  {
    Ok(match peeked.pop_front() {
      None => Action::Stop,
      Some(tok) => {
        let tok = tok
          .as_maybe_ref()
          .map(|t| t.token().copied(), |t| t.token())
          .into_inner();
        match tok.data().is_value_start() {
          true => Action::Continue,
          _ => Action::Stop,
        }
      }
    })
  }
}

fn boolean<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<bool, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>,
{
  expect(|t: &Token<'inp>| {
    if matches!(t, Token::Bool(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Bool))
    }
  })
  .map(Token::unwrap_bool)
  .parse_input(inp)
}

fn null<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<(), JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>,
{
  expect(|t: &Token<'inp>| {
    if matches!(t, Token::Null) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Null))
    }
  })
  .ignored()
  .parse_input(inp)
}

fn number<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<f64, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>,
{
  expect(|t: &Token<'inp>| {
    if matches!(t, Token::Number(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::Number))
    }
  })
  .map(Token::unwrap_number)
  .parse_input(inp)
}

fn string<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<&'inp str, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>,
{
  expect(|t: &Token<'inp>| {
    if matches!(t, Token::String(_)) {
      Ok(())
    } else {
      Err(Expected::one(TokenKind::String))
    }
  })
  .map(Token::unwrap_string)
  .parse_input(inp)
}

fn list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<Vec<JsonValue<'inp>>, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: SeparatedEmitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>
    + FullContainerEmitter<'inp, JsonLexer<'inp>>
    + UnclosedEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
{
  try_json_value
    .separated_by_comma()
    .delimited::<Bracket>()
    .collect()
    .parse_input(inp)
}

fn field<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<(&'inp str, JsonValue<'inp>), JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: SeparatedEmitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>
    + FullContainerEmitter<'inp, JsonLexer<'inp>>
    + UnclosedEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
{
  string
    .then_ignore(Colon::parse)
    .then(json_value::<Ctx>)
    .parse_input(inp)
}

fn object<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<Vec<(&'inp str, JsonValue<'inp>)>, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: SeparatedEmitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>
    + FullContainerEmitter<'inp, JsonLexer<'inp>>
    + UnclosedEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
{
  field
    .separated_by_comma_while::<_, U1>(JsonValue::decide::<Ctx>)
    .delimited::<Brace>()
    .collect()
    .parse_input(inp)
}

fn try_json_value<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<ParseAttempt<JsonValue<'inp>>, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: SeparatedEmitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>
    + FullContainerEmitter<'inp, JsonLexer<'inp>>
    + UnclosedEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
{
  let end = inp.source().len();
  (
    boolean.map(JsonValue::Bool),
    null.map(|_| JsonValue::Null),
    number.map(JsonValue::Number),
    string.map(JsonValue::String),
    list.map(JsonValue::List),
    object.map(JsonValue::Object),
  )
    // Use `peek_then_try_choice` here as we want to return None if the next token is not a valid start of a JSON value
    .peek_then_try_choice::<_, U1>(
      |mut peeked: Peeked<'_, 'inp, JsonLexer<'inp>, U1>, _emitter| match peeked.pop_front() {
        None => Err(JsonError::Eot(UnexpectedEot::eot(end))),
        Some(tok) => {
          let tok = tok
            .as_maybe_ref()
            .map(|t| t.token().copied(), |t| t.token())
            .into_inner();

          Ok(Some(match tok.data() {
            Token::Bool(_) => Branch::B0,
            Token::Null => Branch::B1,
            Token::Number(_) => Branch::B2,
            Token::String(_) => Branch::B3,
            Token::BracketOpen => Branch::B4,
            Token::BraceOpen => Branch::B5,
            _ => return Ok(None),
          }))
        }
      },
    )
    .try_parse_input(inp)
}

fn json_value<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, JsonLexer<'inp>, Ctx>,
) -> Result<JsonValue<'inp>, JsonError<'inp>>
where
  Ctx: ParseContext<'inp, JsonLexer<'inp>>,
  Ctx::Emitter: SeparatedEmitter<'inp, JsonLexer<'inp>, Error = JsonError<'inp>>
    + FullContainerEmitter<'inp, JsonLexer<'inp>>
    + UnclosedEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedLeadingSeparatorEmitter<'inp, JsonLexer<'inp>>
    + UnexpectedTrailingSeparatorEmitter<'inp, JsonLexer<'inp>>,
{
  let end = inp.source().len();
  (
    boolean.map(JsonValue::Bool),
    null.map(|_| JsonValue::Null),
    number.map(JsonValue::Number),
    string.map(JsonValue::String),
    list.map(JsonValue::List),
    object.map(JsonValue::Object),
  )
    // Use `peek_then_choice` here as we want to return an error if the next token is not a valid start of a JSON value
    .peek_then_choice::<_, U1>(
      |mut peeked: Peeked<'_, 'inp, JsonLexer<'inp>, U1>, _emitter| match peeked.pop_front() {
        None => Err(JsonError::Eot(UnexpectedEot::eot(end))),
        Some(tok) => {
          let tok = tok
            .as_maybe_ref()
            .map(|t| t.token().copied(), |t| t.token())
            .into_inner();
          let span = tok.span();
          match tok.data() {
            Token::Bool(_) => Ok(Branch::B0),
            Token::Null => Ok(Branch::B1),
            Token::Number(_) => Ok(Branch::B2),
            Token::String(_) => Ok(Branch::B3),
            Token::BracketOpen => Ok(Branch::B4),
            Token::BraceOpen => Ok(Branch::B5),
            tok => Err(JsonError::UnexpectedToken(
              UnexpectedToken::expected_one_of(
                *span,
                &[
                  TokenKind::Bool,
                  TokenKind::Null,
                  TokenKind::Number,
                  TokenKind::String,
                  TokenKind::BracketOpen,
                  TokenKind::BraceOpen,
                ],
              )
              .with_found((*tok).clone()),
            )),
          }
        }
      },
    )
    .parse_input(inp)
}

const SRC: &str = include_str!("sample.json");

fn main() {
  let output = Parser::new().apply(json_value).parse_str(SRC).unwrap();
  println!("{:#?}", output);
}

#[cfg(test)]
mod tests {
  use super::{JsonError, TokenKind};
  use tokora::error::UnexpectedEot;

  #[test]
  fn test_example() {
    super::main();
  }

  // The `Kind`-table spelling of end-of-input reaches `JsonError` with its position intact.
  // Run with `cargo test --example json`: a bare `cargo test` builds examples but does not
  // select their unit tests.
  #[test]
  fn kind_table_end_of_input_keeps_its_offset() {
    let raised: UnexpectedEot<usize, (), TokenKind> =
      UnexpectedEot::eot_expected_one_of(42, &[TokenKind::Bool, TokenKind::Null]);
    match JsonError::from(raised) {
      JsonError::Eot(eot) => assert_eq!(eot.offset(), 42),
      other => panic!("expected the end-of-input arm, got {other:?}"),
    }
  }
}
