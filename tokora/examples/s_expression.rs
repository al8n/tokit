//! A Lisp-style S-expression parser and evaluator using [`InputRef`](tokora::InputRef).
//!
//! Demonstrates pure recursive-descent parsing with [`InputRef::next`] and
//! [`InputRef::try_expect`] — no Pratt parsing is involved. The evaluator
//! reduces the AST to an [`Atom`] value.
//!
//! Supported forms:
//!
//! | Form                       | Example             |
//! |----------------------------|---------------------|
//! | Integer literal            | `42`, `-7`          |
//! | Boolean literal            | `#t`, `#f`          |
//! | Built-in function          | `+`, `-`, `*`, `/`, `=`, `not` |
//! | Keyword atom               | `:foo`              |
//! | Quoted list                | `'(1 2 3)`          |
//! | Conditional                | `(if cond then [else])` |
//! | Function application       | `(func args...)`    |

use tokora::{
  Emitter, InputRef, Parse, ParseContext, Parser, Token as TokenT,
  error::token::UnexpectedTokenOf,
  logos::{self, Logos},
};

// ── Lexer ─────────────────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, PartialEq)]
struct LexError;

impl From<()> for LexError {
  fn from(_: ()) -> Self {
    LexError
  }
}

#[derive(Debug, Clone, Logos, PartialEq)]
#[logos(crate = logos, skip r"[ \t\r\n]+", error = LexError)]
enum Token {
  /// Integer literals; a leading `-` is part of the literal (e.g. `-3`).
  #[regex(r"-?[0-9]+", |lex| lex.slice().parse::<i64>().map_err(|_| LexError))]
  Int(i64),

  #[token("#t")]
  True,
  #[token("#f")]
  False,

  /// Keyword atoms such as `:foo` — the leading `:` is stripped.
  #[regex(r":[a-zA-Z_][a-zA-Z0-9_]*", |lex| lex.slice()[1..].to_string())]
  Keyword(String),

  // Arithmetic / comparison builtins
  #[token("+")]
  Plus,
  #[token("-")]
  Minus,
  #[token("*")]
  Star,
  #[token("/")]
  Slash,
  #[token("=")]
  Equal,

  // Special forms / builtins
  #[token("not")]
  Not,
  #[token("if")]
  If,

  // Delimiters
  #[token("(")]
  LParen,
  #[token(")")]
  RParen,
  #[token("'")]
  Quote,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum TokenKind {
  Int,
  True,
  False,
  Keyword,
  Plus,
  Minus,
  Star,
  Slash,
  Equal,
  Not,
  If,
  LParen,
  RParen,
  Quote,
}

impl core::fmt::Display for TokenKind {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      TokenKind::Int => write!(f, "integer"),
      TokenKind::True => write!(f, "#t"),
      TokenKind::False => write!(f, "#f"),
      TokenKind::Keyword => write!(f, "keyword"),
      TokenKind::Plus => write!(f, "+"),
      TokenKind::Minus => write!(f, "-"),
      TokenKind::Star => write!(f, "*"),
      TokenKind::Slash => write!(f, "/"),
      TokenKind::Equal => write!(f, "="),
      TokenKind::Not => write!(f, "not"),
      TokenKind::If => write!(f, "if"),
      TokenKind::LParen => write!(f, "("),
      TokenKind::RParen => write!(f, ")"),
      TokenKind::Quote => write!(f, "'"),
    }
  }
}

impl From<&Token> for TokenKind {
  fn from(t: &Token) -> Self {
    match t {
      Token::Int(_) => TokenKind::Int,
      Token::True => TokenKind::True,
      Token::False => TokenKind::False,
      Token::Keyword(_) => TokenKind::Keyword,
      Token::Plus => TokenKind::Plus,
      Token::Minus => TokenKind::Minus,
      Token::Star => TokenKind::Star,
      Token::Slash => TokenKind::Slash,
      Token::Equal => TokenKind::Equal,
      Token::Not => TokenKind::Not,
      Token::If => TokenKind::If,
      Token::LParen => TokenKind::LParen,
      Token::RParen => TokenKind::RParen,
      Token::Quote => TokenKind::Quote,
    }
  }
}

impl TokenT<'_> for Token {
  type Kind = TokenKind;
  type Error = LexError;

  const READ_FRONTIER_CLASS: tokora::ReadFrontierClass = tokora::ReadFrontierClass::Unbounded;

  fn kind(&self) -> TokenKind {
    TokenKind::from(self)
  }
  fn is_trivia(&self) -> bool {
    false
  }
}

type SExprLexer<'a> = tokora::lexer::LogosLexer<'a, Token>;

// ── Errors ────────────────────────────────────────────────────────────────────

#[derive(Debug)]
enum SExprError {
  Lex(LexError),
  UnexpectedToken,
  UnexpectedEot,
}

impl From<LexError> for SExprError {
  fn from(e: LexError) -> Self {
    SExprError::Lex(e)
  }
}

impl<'inp> From<UnexpectedTokenOf<'inp, SExprLexer<'inp>>> for SExprError {
  fn from(_: UnexpectedTokenOf<'inp, SExprLexer<'inp>>) -> Self {
    SExprError::UnexpectedToken
  }
}

// The token-level bundle the entry points require: one `Set`-generic end-of-input impl covers
// both the default expected-set spelling and the `Kind`-table one the committed dispatch
// drivers raise, and one generic `FromUnclosed` covers every delimiter pair.
impl<O, Lang: ?Sized, Set: Clone + 'static> From<tokora::error::UnexpectedEot<O, Lang, Set>>
  for SExprError
{
  fn from(_: tokora::error::UnexpectedEot<O, Lang, Set>) -> Self {
    SExprError::UnexpectedEot
  }
}

impl<'inp, L, Lang: ?Sized> tokora::emitter::FromUnclosed<'inp, L, Lang> for SExprError
where
  L: tokora::Lexer<'inp>,
{
  fn from_unclosed<D>(_: tokora::error::Unclosed<D, L::Span, Lang>) -> Self {
    SExprError::UnexpectedEot
  }
}

// ── AST ───────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum BuiltIn {
  Add,
  Sub,
  Mul,
  Div,
  Eq,
  Not,
}

#[derive(Debug, Clone, PartialEq)]
enum Atom {
  Num(i64),
  Bool(bool),
  Keyword(String),
  BuiltIn(BuiltIn),
}

impl core::fmt::Display for Atom {
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Atom::Num(n) => write!(f, "{n}"),
      Atom::Bool(true) => write!(f, "#t"),
      Atom::Bool(false) => write!(f, "#f"),
      Atom::Keyword(k) => write!(f, ":{k}"),
      Atom::BuiltIn(BuiltIn::Add) => write!(f, "+"),
      Atom::BuiltIn(BuiltIn::Sub) => write!(f, "-"),
      Atom::BuiltIn(BuiltIn::Mul) => write!(f, "*"),
      Atom::BuiltIn(BuiltIn::Div) => write!(f, "/"),
      Atom::BuiltIn(BuiltIn::Eq) => write!(f, "="),
      Atom::BuiltIn(BuiltIn::Not) => write!(f, "not"),
    }
  }
}

#[derive(Debug, Clone)]
enum Expr {
  Constant(Atom),
  If {
    cond: Box<Expr>,
    then: Box<Expr>,
    otherwise: Option<Box<Expr>>,
  },
  /// A quoted list: `'(expr ...)`.  Not evaluated — returned as-is.
  Quote(Vec<Expr>),
  Application(Box<Expr>, Vec<Expr>),
}

// ── Parser ────────────────────────────────────────────────────────────────────

/// Parses a single S-expression.
fn parse_expr<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SExprLexer<'inp>, Ctx>,
) -> Result<Expr, SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  match inp.next()? {
    None => Err(SExprError::UnexpectedEot),
    Some(tok) => match tok.into_data() {
      // ── Atoms ──
      Token::Int(n) => Ok(Expr::Constant(Atom::Num(n))),
      Token::True => Ok(Expr::Constant(Atom::Bool(true))),
      Token::False => Ok(Expr::Constant(Atom::Bool(false))),
      Token::Keyword(k) => Ok(Expr::Constant(Atom::Keyword(k))),

      // ── Built-in functions (used as first-class values) ──
      Token::Plus => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Add))),
      Token::Minus => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Sub))),
      Token::Star => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Mul))),
      Token::Slash => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Div))),
      Token::Equal => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Eq))),
      Token::Not => Ok(Expr::Constant(Atom::BuiltIn(BuiltIn::Not))),

      // ── '( expr... ) ──
      Token::Quote => {
        if inp
          .try_expect(|t| matches!(t.data, Token::LParen))?
          .is_none()
        {
          return Err(SExprError::UnexpectedToken);
        }
        Ok(Expr::Quote(parse_list(inp)?))
      }

      // ── ( if cond then [else] )  or  ( func args... ) ──
      Token::LParen => {
        if inp.try_expect(|t| matches!(t.data, Token::If))?.is_some() {
          // Conditional: (if cond then [else])
          let cond = Box::new(parse_expr(inp)?);
          let then = Box::new(parse_expr(inp)?);
          let otherwise = if inp
            .try_expect(|t| matches!(t.data, Token::RParen))?
            .is_some()
          {
            // No else branch
            None
          } else {
            let e = Box::new(parse_expr(inp)?);
            if inp
              .try_expect(|t| matches!(t.data, Token::RParen))?
              .is_none()
            {
              return Err(SExprError::UnexpectedToken);
            }
            Some(e)
          };
          Ok(Expr::If {
            cond,
            then,
            otherwise,
          })
        } else {
          // Application: (func arg...)
          let func = Box::new(parse_expr(inp)?);
          let args = parse_list(inp)?;
          Ok(Expr::Application(func, args))
        }
      }

      _ => Err(SExprError::UnexpectedToken),
    },
  }
}

/// Collects zero or more expressions until `)` (which is consumed).
fn parse_list<'inp, Ctx>(
  inp: &mut InputRef<'inp, '_, SExprLexer<'inp>, Ctx>,
) -> Result<Vec<Expr>, SExprError>
where
  Ctx: ParseContext<'inp, SExprLexer<'inp>>,
  Ctx::Emitter: Emitter<'inp, SExprLexer<'inp>, Error = SExprError>,
{
  let mut elems = Vec::new();
  loop {
    if inp
      .try_expect(|t| matches!(t.data, Token::RParen))?
      .is_some()
    {
      break;
    }
    elems.push(parse_expr(inp)?);
  }
  Ok(elems)
}

fn eval(expr: Expr) -> Result<Atom, String> {
  match expr {
    Expr::Constant(a) => Ok(a),

    Expr::Quote(elems) => {
      // Evaluate each element and format as a keyword :(a b c)
      let inner = elems
        .into_iter()
        .map(eval)
        .collect::<Result<Vec<_>, _>>()?
        .iter()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(" ");
      Ok(Atom::Keyword(format!("({inner})")))
    }

    Expr::If {
      cond,
      then,
      otherwise,
    } => match eval(*cond)? {
      Atom::Bool(true) => eval(*then),
      Atom::Bool(false) => match otherwise {
        Some(e) => eval(*e),
        None => Ok(Atom::Bool(false)), // no else branch → #f
      },
      other => Err(format!("if: condition must be boolean, got {other}")),
    },

    Expr::Application(func, args) => match eval(*func)? {
      Atom::BuiltIn(bi) => {
        let vals = args.into_iter().map(eval).collect::<Result<Vec<_>, _>>()?;
        apply(bi, vals)
      }
      other => Err(format!("application: {other} is not a function")),
    },
  }
}

fn apply(bi: BuiltIn, args: Vec<Atom>) -> Result<Atom, String> {
  fn num(a: &Atom) -> Result<i64, String> {
    match a {
      Atom::Num(n) => Ok(*n),
      other => Err(format!("expected number, got {other}")),
    }
  }

  match bi {
    BuiltIn::Add => {
      let s: i64 = args
        .iter()
        .map(num)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .sum();
      Ok(Atom::Num(s))
    }

    BuiltIn::Sub => match args.as_slice() {
      [] => Err("-: needs at least one argument".into()),
      [a] => Ok(Atom::Num(-num(a)?)),
      [first, rest @ ..] => {
        let s: i64 = rest
          .iter()
          .map(num)
          .collect::<Result<Vec<_>, _>>()?
          .into_iter()
          .sum();
        Ok(Atom::Num(num(first)? - s))
      }
    },

    BuiltIn::Mul => {
      let p: i64 = args
        .iter()
        .map(num)
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .product();
      Ok(Atom::Num(p))
    }

    BuiltIn::Div => match args.as_slice() {
      [a, b] => {
        let b = num(b)?;
        if b == 0 {
          return Err("division by zero".into());
        }
        Ok(Atom::Num(num(a)? / b))
      }
      _ => Err("/: needs exactly 2 arguments".into()),
    },

    BuiltIn::Eq => {
      if args.len() < 2 {
        return Err("=: needs at least 2 arguments".into());
      }
      Ok(Atom::Bool(args[1..].iter().all(|a| a == &args[0])))
    }

    BuiltIn::Not => match args.as_slice() {
      [Atom::Bool(b)] => Ok(Atom::Bool(!*b)),
      [other] => Err(format!("not: expected boolean, got {other}")),
      _ => Err("not: needs exactly 1 argument".into()),
    },
  }
}

fn main() {
  let cases: &[(&str, &str)] = &[
    ("(+ 1 2)", "3"),
    ("(* 3 (+ 2 2))", "12"),
    ("(- 10 3 2)", "5"), // variadic sub: 10 - 3 - 2
    ("(if #t 1 2)", "1"),
    ("(if #f 1 2)", "2"),
    ("(if #f 1)", "#f"), // no else-branch → #f
    ("(not #t)", "#f"),
    ("(not #f)", "#t"),
    ("(= 1 1)", "#t"),
    ("(= 1 2)", "#f"),
    ("(* (+ 1 2) (- 5 3))", "6"),
    ("(if (= 1 1) (+ 3 4) 0)", "7"),
    (
      "((if (= (+ 3 (/ 9 3))
         (* 2 3))
     *
     /)
  456 123)",
      "56088",
    ),
    ("'(1 2 3)", ":(1 2 3)"), // quoted list → keyword
  ];

  for (src, expected) in cases {
    let expr: Expr = Parser::new().apply(parse_expr).parse_str(src).unwrap();
    let result = eval(expr).unwrap();
    println!("{src:42}  =  {result}  (expected {expected})");
    assert_eq!(result.to_string(), *expected, "mismatch for `{src}`");
  }

  println!("All assertions passed.");
}

#[cfg(test)]
mod tests {
  #[test]
  fn test_example() {
    super::main();
  }
}
