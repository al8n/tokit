use crate::{
  error::{MaybeIncomplete, MaybeTerminal},
  input::Cursor,
};

use super::*;

/// A trait for recovery parsers that start from the original position after backtracking.
///
/// This trait defines the interface for recovery parsers used by [`Recover`]. When the primary
/// parser fails, implementors of this trait receive the error and attempt to produce a valid
/// output by parsing from the restored checkpoint position.
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for closures with the signature:
/// ```ignore
/// FnMut(&mut InputRef, Error) -> Result<O, Error>
/// ```
///
/// # Example
///
/// ```ignore
/// use tokora::parser::{ParseInput, RecoverInput};
///
/// // Manual implementation
/// struct ErrorNodeRecovery;
///
/// impl RecoverInput<'_, MyLexer, Node, MyContext> for ErrorNodeRecovery {
///     fn recover_input(&mut self, input, err) -> Result<Node, Error> {
///         // Create error node with span from error
///         Ok(Node::Error(err.span()))
///     }
/// }
///
/// // Or use a closure (automatic implementation)
/// parser.recover(|_input, err| {
///     Ok(Node::Error(err.span()))
/// })
/// ```
///
/// See [`Recover`] for usage examples.
pub trait RecoverInput<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Try to recover from a parsing error.
  ///
  /// This method is called when the primary parser fails. The input position has been
  /// restored to where it was before the primary parser started.
  ///
  /// # Parameters
  ///
  /// - `input`: Input reference at the original starting position (after backtracking)
  /// - `err`: The error produced by the failed primary parser
  ///
  /// # Returns
  ///
  /// - `Ok(output)`: Successfully recovered with a value
  /// - `Err(error)`: Recovery failed
  fn recover_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, L, O, Ctx, Lang: ?Sized, Cmpl, F> RecoverInput<'inp, L, O, Ctx, Lang, Cmpl> for F
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  F: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn recover_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (self)(input, err)
  }
}

/// A trait for recovery parsers that continue from the error position without backtracking.
///
/// This trait defines the interface for recovery parsers used by [`InplaceRecover`]. When the
/// primary parser fails, implementors of this trait receive the error and a [`Cursor`] marking
/// where the primary parser started, and attempt to produce a valid output by parsing from the
/// current (error) position.
///
/// Unlike [`RecoverInput`], the input position is **not** restored - recovery continues from
/// where the primary parser stopped. The recovery handler receives a [`Cursor`] rather than a
/// [`Checkpoint`](crate::input::Checkpoint) because checkpoints are single-use backtracking
/// capabilities (restoring one consumes it); a recovery handler only needs the position facts
/// the cursor carries, e.g. to span from the start of the failed parse.
///
/// # Automatic Implementation
///
/// This trait is automatically implemented for closures with the signature:
/// ```ignore
/// FnMut(&mut InputRef, Cursor, Error) -> Result<O, Error>
/// ```
///
/// # Example
///
/// ```ignore
/// use tokora::parser::{ParseInput, InplaceRecoverInput};
///
/// // Manual implementation
/// struct SkipToSemicolon;
///
/// impl InplaceRecoverInput<'_, MyLexer, Stmt, MyContext> for SkipToSemicolon {
///     fn inplace_recover_input(&mut self, input, _cursor, _err) -> Result<Stmt, Error> {
///         // Skip tokens until semicolon from current position
///         while !input.peek().is_semicolon() {
///             input.next();
///         }
///         input.next(); // consume semicolon
///         Ok(Stmt::Error)
///     }
/// }
///
/// // Or use a closure (automatic implementation)
/// parser.inplace_recover(|input, _cursor, _err| {
///     // Skip to semicolon from error position
///     skip_to_semicolon(input)?;
///     Ok(Stmt::Error)
/// })
/// ```
///
/// See [`InplaceRecover`] for usage examples.
pub trait InplaceRecoverInput<'inp, L, O, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  /// Try to recover from a parsing error without backtracking.
  ///
  /// This method is called when the primary parser fails. Unlike [`RecoverInput::recover_input`],
  /// the input position has **not** been restored - it remains at the error position.
  ///
  /// # Parameters
  ///
  /// - `input`: Input reference at the current (error) position
  /// - `cursor`: A [`Cursor`] marking where the primary parser started — a position
  ///   view, not a restorable checkpoint (recovery cannot backtrack)
  /// - `err`: The error produced by the failed primary parser
  ///
  /// # Returns
  ///
  /// - `Ok(output)`: Successfully recovered with a value
  /// - `Err(error)`: Recovery failed
  fn inplace_recover_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    cursor: Cursor<'inp, '_, L>,
    err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: Completeness;
}

impl<'inp, L, O, Ctx, Lang: ?Sized, Cmpl, F> InplaceRecoverInput<'inp, L, O, Ctx, Lang, Cmpl> for F
where
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: Completeness,
  F: FnMut(
    &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    Cursor<'inp, '_, L>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>,
{
  #[inline(always)]
  fn inplace_recover_input(
    &mut self,
    input: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
    cursor: Cursor<'inp, '_, L>,
    err: <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    (self)(input, cursor, err)
  }
}

/// A parser that provides error recovery by trying an alternative parser with backtracking.
///
/// This combinator implements **error recovery with checkpoint restoration**. If the main
/// parser fails, the input position is reset to where it was before parsing, and a recovery
/// parser is tried instead.
///
/// This is useful for:
/// - **Resilient parsing**: Continue parsing after errors to find more issues
/// - **Fallback strategies**: Try alternative interpretations when the primary fails
/// - **Error correction**: Insert synthetic nodes to maintain parse tree structure
///
/// # Type Parameters
///
/// - `P`: The primary parser to try first
/// - `R`: The recovery parser to use if primary fails
/// - `O`: Output type (both parsers must produce the same type)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Basic Recovery
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Try to parse a valid expression, fall back to error node
/// let parser = parse_expression()
///     .recover(parse_error_node());
///
/// // Input: "1 + 2"      → Ok(BinaryOp(Add, 1, 2))
/// // Input: "@ invalid" → Ok(ErrorNode(...))  // recovery parser used
/// ```
///
/// ## Statement-Level Recovery
///
/// ```ignore
/// // Parse statement, recover by skipping to next semicolon
/// let parser = parse_statement()
///     .recover(
///         skip_to(|tok| matches!(tok, Token::Semicolon))
///             .then_ignore(any())  // consume semicolon
///             .map(|_| Statement::Error)
///     );
///
/// // Input: "let x = 1;"      → Ok(LetStmt { .. })
/// // Input: "### bad ;"       → Ok(Statement::Error)
/// //                               ^ skips to semicolon
/// ```
///
/// ## Multiple Recovery Strategies
///
/// ```ignore
/// // Try multiple parsers with recovery fallbacks
/// let parser = parse_function()
///     .recover(parse_struct())
///     .recover(parse_error_item());
///
/// // Tries: function → struct → error item
/// ```
///
/// ## Collecting Errors While Recovering
///
/// ```ignore
/// // Use with greedy emitter to collect all errors
/// let parser = parse_item()
///     .recover_with(|_err, state| {
///         // Error already emitted by failed parser
///         // Return placeholder and continue
///         Ok(Item::Error(state.span()))
///     });
/// ```
///
/// # How It Works
///
/// 1. **Save checkpoint**: Record current input position and state
/// 2. **Try primary parser**: Attempt to parse with the main parser
/// 3. **On success**: Return the parsed value
/// 4. **On failure**:
///    - **Restore checkpoint**: Reset input to saved position
///    - **Try recovery parser**: Attempt recovery from the original position
///    - **Return recovery result**: May succeed or fail
///
/// # Comparison with InplaceRecover
///
/// | Feature | `Recover` | `InplaceRecover` |
/// |---------|-----------|------------------|
/// | **Backtracking** | ✅ Restores position on error | ❌ Continues from error position |
/// | **Use Case** | Try alternative from same position | Skip ahead to resynchronize |
/// | **Performance** | Saves/restores checkpoint | No checkpoint overhead |
/// | **Example** | Parse expr or error node | Skip to next statement boundary |
///
/// **When to use**:
/// - `Recover`: Try completely different parsers from the same starting position
/// - `InplaceRecover`: Continue parsing from where the error occurred (e.g., skip tokens)
///
/// # Performance
///
/// - **Memory**: O(1) for checkpoint (just a position marker)
/// - **Runtime**: Two parser attempts in worst case
/// - **Backtracking**: Resets to saved position, no token re-buffering needed
///
/// # Error Handling
///
/// - Errors from the **primary parser are discarded** (recovery masks them)
/// - Errors from the **recovery parser are propagated**
/// - For error collection, use an emitter that accumulates errors
///
/// # See Also
///
/// - [`InplaceRecover`] - Error recovery without backtracking
/// - [`PeekThenChoice`] - Deterministic choice (no error recovery)
///
/// # Caveats
///
/// When the primary parser fails, `Recover` rolls the input back to the
/// pre-attempt state — position, lexer state, **and** diagnostics: emissions
/// made by the failed attempt are rewound from the
/// [`Emitter`](crate::Emitter)'s log, so a successful recovery leaves no
/// spurious errors behind. The failure itself reaches the [`RecoverInput`]
/// implementation as the error value; whether to emit a diagnostic for it is
/// the recoverer's decision.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Recover<P, R, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  recoverer: R,
  _m: PhantomData<O>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _l: PhantomData<L>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, R, O, L, Ctx, Lang: ?Sized, Cmpl> Recover<P, R, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `Recover` parser.
  #[inline(always)]
  pub(crate) const fn new(parser: P, recoverer: R) -> Self {
    Self {
      parser,
      recoverer,
      _m: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _l: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, R, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for Recover<P, R, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  R: RecoverInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: MaybeIncomplete + MaybeTerminal,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // Speculate through `try_attempt`: it saves a checkpoint before the primary runs, and
    // on `Ok` keeps the progress while dropping the checkpoint's lineage id — closing the
    // success-path leak where a bare `save` left an orphan id on the live stack for every
    // valid parse. On `Err` it restores that checkpoint (rewinding to the pre-parse state,
    // see [`restore`](InputRef::restore)) and hands the error back, so the recoverer runs
    // from the restored position exactly as the raw save/restore pair did.
    match inp.try_attempt(|input| self.parser.parse_input(input)) {
      Ok(output) => Ok(output),
      // The never-recoverable law and its terminal dual: an `Incomplete` (more input may fix
      // this) and a terminal scanner stop (no input ever will) both ride the `Err` channel
      // untouched. Recovery fabricates a value from a *malformed* construct; neither an
      // unfinished construct nor a tripped limit is that, so re-raise verbatim rather than
      // invoking the recoverer — see [`MaybeIncomplete`](crate::error::MaybeIncomplete) and
      // [`MaybeTerminal`](crate::error::MaybeTerminal).
      Err(e) if e.is_incomplete() || e.is_terminal() => Err(e),
      Err(e) => self.recoverer.recover_input(inp, e),
    }
  }
}

/// A parser that provides error recovery without backtracking, continuing from error position.
///
/// This combinator implements **error recovery without checkpoint restoration**. If the main
/// parser fails, the recovery parser starts from **where the error occurred**, not from the
/// original starting position.
///
/// This is useful for:
/// - **Panic mode recovery**: Skip tokens until a synchronization point
/// - **Resynchronization**: Find the next safe point to continue parsing
/// - **Performance**: Avoid checkpoint overhead when you don't need backtracking
///
/// # Type Parameters
///
/// - `P`: The primary parser to try first
/// - `R`: The recovery parser starting from the error position
/// - `O`: Output type (both parsers must produce the same type)
/// - `L`: Lexer type
/// - `Ctx`: Parse context
/// - `Lang`: Language marker type (default `()`)
///
/// # Examples
///
/// ## Skip to Synchronization Point
///
/// ```ignore
/// use tokora::parser::ParseInput;
///
/// // Parse statement, skip to semicolon on error
/// let parser = parse_statement()
///     .inplace_recover(
///         skip_to(|tok| matches!(tok, Token::Semicolon))
///             .then_ignore(any())
///             .map(|_| Statement::Error)
///     );
///
/// // Input: "let x = 1;"     → Ok(LetStmt { .. })
/// // Input: "bad ### ; ok"   → Ok(Statement::Error)
/// //             ^^^ ^
/// //        error here, skip to semicolon from error position
/// ```
///
/// ## Block-Level Recovery
///
/// ```ignore
/// // Parse block item, skip to next closing brace on error
/// let parser = parse_block_item()
///     .inplace_recover(
///         skip_to(|tok| matches!(tok, Token::RBrace | Token::Semicolon))
///             .map(|_| Item::Error)
///     );
///
/// // Continues parsing from where error occurred
/// ```
///
/// ## Forward-Only Error Recovery
///
/// ```ignore
/// // Parser that never backtracks, only moves forward
/// let parser = parse_token()
///     .inplace_recover(
///         any()  // Consume whatever token caused the error
///             .map(|tok| Value::Error(tok))
///     );
///
/// // Always makes progress, never goes back
/// ```
///
/// # How It Works
///
/// 1. **Try primary parser**: Attempt to parse with the main parser
/// 2. **On success**: Return the parsed value
/// 3. **On failure**:
///    - **Keep current position**: Don't restore any checkpoint
///    - **Try recovery parser**: Start recovery from current (error) position
///    - **Return recovery result**: May succeed or fail
///
/// # Comparison with Recover
///
/// | Feature | `Recover` | `InplaceRecover` |
/// |---------|-----------|------------------|
/// | **Backtracking** | ✅ Restores position | ❌ No backtracking |
/// | **Recovery starts from** | Original position | Error position |
/// | **Checkpoint overhead** | Yes (save/restore) | No |
/// | **Use Case** | Try alternatives | Skip to sync point |
///
/// **When to use**:
/// - `Recover`: When recovery needs to start from the beginning (alternative interpretation)
/// - `InplaceRecover`: When recovery should continue from error point (skip ahead)
///
/// # Performance
///
/// - **Memory**: O(1) - no checkpoint storage
/// - **Runtime**: Two parser attempts in worst case (like `Recover`)
/// - **No backtracking overhead**: More efficient when you don't need to reset position
///
/// # Error Handling
///
/// - Errors from the **primary parser are discarded**
/// - Errors from the **recovery parser are propagated**
/// - Recovery parser sees input from where the primary parser stopped
///
/// # The never-recoverable law
///
/// An [`Incomplete`](crate::error::Incomplete) error is re-raised untouched — before the
/// recovery parser runs — exactly as [`Recover`] and
/// [`skip_then_retry`](crate::ParseInput::skip_then_retry) do: recovery synthesizes progress
/// over a *malformed* construct, but an incomplete one is merely unfinished, so continuing past
/// it would fabricate output from input that has not finished arriving. See
/// [`MaybeIncomplete`](crate::error::MaybeIncomplete).
///
/// # See Also
///
/// - [`Recover`] - Error recovery with backtracking
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct InplaceRecover<P, R, O, L, Ctx, Lang: ?Sized = (), Cmpl = Complete> {
  parser: P,
  recoverer: R,
  _m: PhantomData<O>,
  _ctx: PhantomData<Ctx>,
  _lang: PhantomData<Lang>,
  _l: PhantomData<L>,
  _cmpl: PhantomData<Cmpl>,
}

impl<P, R, O, L, Ctx, Lang: ?Sized, Cmpl> InplaceRecover<P, R, O, L, Ctx, Lang, Cmpl> {
  /// Creates a new `InplaceRecover` parser.
  #[inline(always)]
  pub(crate) const fn new(parser: P, recoverer: R) -> Self {
    Self {
      parser,
      recoverer,
      _m: PhantomData,
      _ctx: PhantomData,
      _lang: PhantomData,
      _l: PhantomData,
      _cmpl: PhantomData,
    }
  }
}

impl<'inp, P, R, L, O, Ctx, Lang, Cmpl> ParseInput<'inp, L, O, Ctx, Lang, Cmpl>
  for InplaceRecover<P, R, O, L, Ctx, Lang, Cmpl>
where
  P: ParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
  R: InplaceRecoverInput<'inp, L, O, Ctx, Lang, Cmpl>,
  L: Lexer<'inp>,
  Ctx: ParseContext<'inp, L, Lang>,
  <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: MaybeIncomplete + MaybeTerminal,
  Lang: ?Sized,
  Cmpl: Completeness,
{
  #[inline(always)]
  fn parse_input(
    &mut self,
    inp: &mut InputRef<'inp, '_, L, Ctx, Lang, Cmpl>,
  ) -> Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error> {
    // The in-place path never backtracks: hand the recovery handler a position view
    // (the cursor where the primary parser started), not a restorable checkpoint.
    let cursor = inp.cursor().clone();
    match self.parser.parse_input(inp) {
      Ok(output) => Ok(output),
      // The never-recoverable law and its terminal dual: an `Incomplete` (more input may fix
      // this) and a terminal scanner stop (no input ever will) both ride the `Err` channel
      // untouched. Recovery fabricates a value from a *malformed* construct; neither an
      // unfinished construct nor a tripped limit is that, so re-raise verbatim rather than
      // invoking the recoverer — see [`MaybeIncomplete`](crate::error::MaybeIncomplete) and
      // [`MaybeTerminal`](crate::error::MaybeTerminal).
      Err(e) if e.is_incomplete() || e.is_terminal() => Err(e),
      Err(e) => self.recoverer.inplace_recover_input(inp, cursor, e),
    }
  }
}

// The no-growth regression needs a lexer that actually runs (so `save`/`next` push and
// commit real checkpoints), which pins it to `logos` + `std` — the same set the
// `live_checkpoints_len` accessor is gated to.
#[cfg(all(test, feature = "logos", feature = "std"))]
mod tests {
  use super::*;
  use crate::{
    Emitter, ParseContext, Token,
    cache::DefaultCache,
    emitter::{Silent, Verbose},
    error::token::UnexpectedToken,
    input::Input,
    lexer::LogosLexer,
    span::Spanned,
  };

  #[derive(Debug, Clone, PartialEq)]
  struct LexErr;

  impl From<()> for LexErr {
    fn from(_: ()) -> Self {
      LexErr
    }
  }

  // `LexErr` never carries an incomplete or terminal signal, so it takes the blanket `false`
  // default for both discrimination hooks.
  impl crate::error::MaybeIncomplete for LexErr {}
  impl crate::error::MaybeTerminal for LexErr {}

  #[derive(Debug, Clone, PartialEq, Eq, crate::logos::Logos)]
  #[logos(crate = crate::logos, skip r"[ \t\r\n]+")]
  enum Tok {
    #[regex(r"[0-9]+")]
    Num,
  }

  impl core::fmt::Display for Tok {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "number")
    }
  }

  #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
  struct NumKind;

  impl core::fmt::Display for NumKind {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
      write!(f, "number")
    }
  }

  impl Token<'_> for Tok {
    type Kind = NumKind;
    type Error = LexErr;

    fn kind(&self) -> NumKind {
      NumKind
    }

    fn is_trivia(&self) -> bool {
      false
    }
  }

  type Lex<'a> = LogosLexer<'a, Tok>;
  type Ctx<'a> = (Silent<LexErr>, DefaultCache<'a, Lex<'a>>);
  // The emitter's error type, spelled as the trait's own projection so the manual
  // `ParseInput`/`RecoverInput` impls below match their trait signatures exactly (it
  // resolves to `LexErr`, but the compatibility check wants the projection, not the
  // resolved type).
  type EmitErr<'a> =
    <<Ctx<'a> as ParseContext<'a, Lex<'a>, ()>>::Emitter as Emitter<'a, Lex<'a>, ()>>::Error;

  /// The primary parser: consume one token and succeed. It always returns `Ok` (even at
  /// end of input, where it consumes nothing), so every application takes `Recover`'s
  /// success path — the path that must not leak its checkpoint.
  struct ConsumeOne;

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), Ctx<'inp>, ()> for ConsumeOne {
    fn parse_input(
      &mut self,
      inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, ()>,
    ) -> Result<(), EmitErr<'inp>> {
      let _ = inp.next()?;
      Ok(())
    }
  }

  /// The recovery parser: unreachable here, since the primary always succeeds.
  struct NeverRecover;

  impl<'inp> RecoverInput<'inp, Lex<'inp>, (), Ctx<'inp>, ()> for NeverRecover {
    fn recover_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, Ctx<'inp>, ()>,
      _err: EmitErr<'inp>,
    ) -> Result<(), EmitErr<'inp>> {
      unreachable!("the primary always succeeds on this input, so recovery never runs")
    }
  }

  #[test]
  fn recover_success_does_not_grow_lineage_stack() {
    // One input, one parse session: a `Recover`-wrapped sub-parser applied repeatedly.
    // Every application takes the success path, which keeps (commits) its progress — so the
    // live-checkpoint lineage stack must return to its baseline length after each iteration.
    // Before the fix, `Recover`'s `Ok` branch left the `save()`d id on the stack, so this
    // count grew by one per successful parse without bound.
    let src = "1 ".repeat(128);
    let mut input = Input::<Lex<'_>, Ctx<'_>, ()>::new(src.as_str());
    let mut emitter = Silent::<LexErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let baseline = inp.live_checkpoints_len();

    let mut rec = Recover::<_, _, (), Lex<'_>, Ctx<'_>, ()>::new(ConsumeOne, NeverRecover);

    for i in 0..100 {
      rec
        .parse_input(&mut inp)
        .expect("the primary parser always succeeds");
      assert_eq!(
        inp.live_checkpoints_len(),
        baseline,
        "iteration {i}: a committed Recover success must not leave its checkpoint id on the \
         lineage stack (found {} live, baseline {baseline})",
        inp.live_checkpoints_len(),
      );
    }
  }

  // ── The never-recoverable law: Recover re-raises an Incomplete, untouched ──────

  // An error type that can carry an incomplete signal (`Incomplete`), a terminal scanner stop
  // (`Terminal`), or an ordinary failure (`Other`). `is_incomplete()` and `is_terminal()` are what
  // `Recover` keys the never-recoverable law (and its terminal dual) off of.
  #[derive(Debug, Clone, PartialEq)]
  enum RecErr {
    Incomplete,
    Terminal,
    Other,
  }

  impl From<()> for RecErr {
    fn from(_: ()) -> Self {
      RecErr::Other
    }
  }

  // The two conversions the collecting `Verbose` emitter asks of an error type (`FromEmitterError`):
  // the lexer's own error, and an unexpected token. Neither is an incomplete signal.
  impl From<LexErr> for RecErr {
    fn from(_: LexErr) -> Self {
      RecErr::Other
    }
  }

  impl<'a, T, Kind: Clone, S, Lang: ?Sized> From<UnexpectedToken<'a, T, Kind, S, Lang>> for RecErr {
    fn from(_: UnexpectedToken<'a, T, Kind, S, Lang>) -> Self {
      RecErr::Other
    }
  }

  // The exact construction path W5's frontier rules use (`SurfaceIncomplete` builds the emitter
  // error as `Error::from(Incomplete::new(off))`); pairing it with `MaybeIncomplete` below keeps
  // construct-and-detect coherent, which is what makes the never-recoverable law hold end to end.
  impl From<crate::error::Incomplete<usize>> for RecErr {
    fn from(_: crate::error::Incomplete<usize>) -> Self {
      RecErr::Incomplete
    }
  }

  // The terminal twin of the incomplete conversion: a terminal-marked end-of-input value (the way
  // `try_expect_or_stop` and the delimited close build one) maps to `Terminal`, a genuine one to
  // `Other`. Pairing it with `MaybeTerminal` keeps construct-and-detect coherent, which is what makes
  // the terminal dual of the never-recoverable law hold end to end.
  impl From<crate::error::UnexpectedEot<usize>> for RecErr {
    fn from(e: crate::error::UnexpectedEot<usize>) -> Self {
      if e.is_terminal() {
        RecErr::Terminal
      } else {
        RecErr::Other
      }
    }
  }

  impl crate::error::MaybeTerminal for RecErr {
    fn is_terminal(&self) -> bool {
      matches!(self, RecErr::Terminal)
    }
  }

  impl crate::error::MaybeIncomplete for RecErr {
    fn is_incomplete(&self) -> bool {
      matches!(self, RecErr::Incomplete)
    }
  }

  type RCtx<'a> = (Silent<RecErr>, DefaultCache<'a, Lex<'a>>);
  type REmitErr<'a> =
    <<RCtx<'a> as ParseContext<'a, Lex<'a>, ()>>::Emitter as Emitter<'a, Lex<'a>, ()>>::Error;

  /// A primary parser that fails outright with a chosen error, without consuming input.
  struct FailWith(RecErr);

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), RCtx<'inp>, ()> for FailWith {
    fn parse_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, RCtx<'inp>, ()>,
    ) -> Result<(), REmitErr<'inp>> {
      Err(self.0.clone())
    }
  }

  /// A recoverer that records whether it was invoked and otherwise succeeds.
  struct FlagRecover<'f> {
    invoked: &'f core::cell::Cell<bool>,
  }

  impl<'inp> RecoverInput<'inp, Lex<'inp>, (), RCtx<'inp>, ()> for FlagRecover<'_> {
    fn recover_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, RCtx<'inp>, ()>,
      _err: REmitErr<'inp>,
    ) -> Result<(), REmitErr<'inp>> {
      self.invoked.set(true);
      Ok(())
    }
  }

  // The law, value-asserted: when the primary fails with an `Incomplete`, `Recover` re-raises it
  // verbatim on the `Err` channel and NEVER invokes the recoverer.
  #[test]
  fn recover_reraises_incomplete_and_never_invokes_recoverer() {
    let mut input = Input::<Lex<'_>, RCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Silent::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = Recover::<_, _, (), Lex<'_>, RCtx<'_>, ()>::new(
      FailWith(RecErr::Incomplete),
      FlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(RecErr::Incomplete),
      "an Incomplete is re-raised untouched on the Err channel"
    );
    assert!(
      !invoked.get(),
      "the recoverer must never run for an Incomplete"
    );
  }

  // The W5 partial-input incomplete: the value the frontier rules actually surface is built via
  // `From<Incomplete>` (not the hand-written `RecErr::Incomplete` literal above). Prove that value
  // reports itself incomplete and that `Recover` re-raises it untouched — the never-recoverable law
  // wired to the real surfacing mechanism.
  #[test]
  fn recover_reraises_a_from_incomplete_built_error() {
    let surfaced: RecErr = crate::error::Incomplete::new(7usize).into();
    assert!(
      surfaced.is_incomplete(),
      "an error built the way the frontier rules build it must report itself incomplete"
    );

    let mut input = Input::<Lex<'_>, RCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Silent::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = Recover::<_, _, (), Lex<'_>, RCtx<'_>, ()>::new(
      FailWith(surfaced.clone()),
      FlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(surfaced),
      "the partial-mode incomplete is re-raised untouched"
    );
    assert!(
      !invoked.get(),
      "the recoverer never runs for a partial-mode incomplete"
    );
  }

  // The scoping guard: an ordinary (non-incomplete) failure still recovers as before — the law
  // does not swallow every error, only the incomplete sentinel.
  #[test]
  fn recover_still_recovers_an_ordinary_error() {
    let mut input = Input::<Lex<'_>, RCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Silent::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = Recover::<_, _, (), Lex<'_>, RCtx<'_>, ()>::new(
      FailWith(RecErr::Other),
      FlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(r, Ok(()), "an ordinary error is recovered");
    assert!(invoked.get(), "the recoverer runs for an ordinary error");
  }

  // ── The terminal dual: Recover re-raises a terminal scanner stop, untouched ──────
  //
  // A terminal stop is not a malformed construct — no amount of skipping or re-attempting clears a
  // tripped limit — so, like an Incomplete, it must re-raise on the `Err` channel rather than reach
  // the recoverer, which would otherwise synthesize a value from a recovery that should never have
  // begun.

  // The law, value-asserted: when the primary fails with a terminal stop, `Recover` re-raises it
  // verbatim and NEVER invokes the recoverer.
  #[test]
  fn recover_reraises_terminal_and_never_invokes_recoverer() {
    let mut input = Input::<Lex<'_>, RCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Silent::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = Recover::<_, _, (), Lex<'_>, RCtx<'_>, ()>::new(
      FailWith(RecErr::Terminal),
      FlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(RecErr::Terminal),
      "a terminal stop is re-raised untouched on the Err channel"
    );
    assert!(
      !invoked.get(),
      "the recoverer must never run for a terminal stop"
    );
  }

  // The value the attempt/decline leaves actually surface is built via `From<UnexpectedEot>` with
  // the terminal marker set (the way `try_expect_or_stop` and the delimited close build it). Prove
  // that value reports itself terminal — while a genuine end of input does not — and that `Recover`
  // re-raises it untouched: the terminal dual wired to the real surfacing mechanism.
  #[test]
  fn recover_reraises_a_from_terminal_built_error() {
    let surfaced: RecErr = crate::error::UnexpectedEot::eot_of(7usize)
      .into_terminal()
      .into();
    assert_eq!(
      surfaced,
      RecErr::Terminal,
      "a terminal-marked end-of-input value maps to the terminal variant"
    );
    assert!(
      surfaced.is_terminal(),
      "an error built the way the leaves build it must report itself terminal"
    );
    let genuine: RecErr = crate::error::UnexpectedEot::eot_of(7usize).into();
    assert!(
      !genuine.is_terminal(),
      "a genuine end of input is not a terminal stop"
    );

    let mut input = Input::<Lex<'_>, RCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Silent::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = Recover::<_, _, (), Lex<'_>, RCtx<'_>, ()>::new(
      FailWith(surfaced.clone()),
      FlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(r, Err(surfaced), "the terminal stop is re-raised untouched");
    assert!(
      !invoked.get(),
      "the recoverer never runs for a terminal stop"
    );
  }

  // ── The same law, binding the in-place path: InplaceRecover re-raises an Incomplete ──────
  //
  // The in-place path never backtracks, so it cannot undo a recovery it should never have begun —
  // which is precisely why the check must happen *before* the handler runs. These run over the
  // collecting `Verbose` emitter so the "no emissions" half of the law is observable: the handler
  // records a diagnostic whenever it runs, so an untouched pass-through must leave the log empty.

  type VCtx<'a> = (Verbose<RecErr>, DefaultCache<'a, Lex<'a>>);
  type VEmitErr<'a> =
    <<VCtx<'a> as ParseContext<'a, Lex<'a>, ()>>::Emitter as Emitter<'a, Lex<'a>, ()>>::Error;

  /// The `Verbose`-context twin of `FailWith`: fails outright with a chosen error, consuming
  /// nothing, so every application takes the recovery path.
  struct VFailWith(RecErr);

  impl<'inp> ParseInput<'inp, Lex<'inp>, (), VCtx<'inp>, ()> for VFailWith {
    fn parse_input(
      &mut self,
      _inp: &mut InputRef<'inp, '_, Lex<'inp>, VCtx<'inp>, ()>,
    ) -> Result<(), VEmitErr<'inp>> {
      Err(self.0.clone())
    }
  }

  /// An in-place recoverer that records that it ran — and emits while doing so, so a run is
  /// visible on the diagnostic channel too — then succeeds.
  struct InplaceFlagRecover<'f> {
    invoked: &'f core::cell::Cell<bool>,
  }

  impl<'inp> InplaceRecoverInput<'inp, Lex<'inp>, (), VCtx<'inp>, ()> for InplaceFlagRecover<'_> {
    fn inplace_recover_input(
      &mut self,
      inp: &mut InputRef<'inp, '_, Lex<'inp>, VCtx<'inp>, ()>,
      _cursor: Cursor<'inp, '_, Lex<'inp>>,
      err: VEmitErr<'inp>,
    ) -> Result<(), VEmitErr<'inp>> {
      self.invoked.set(true);
      let span = *inp.span();
      <Verbose<RecErr> as Emitter<'inp, Lex<'inp>>>::emit_error(
        inp.emitter(),
        Spanned::new(span, err),
      )?;
      Ok(())
    }
  }

  /// The input reference the in-place law tests run on, over the verbose (collecting) context.
  type VIr<'inp, 'closure> = InputRef<'inp, 'closure, Lex<'inp>, VCtx<'inp>, ()>;

  /// The number of diagnostics the input's emitter is holding.
  fn vdiags(inp: &mut VIr<'_, '_>) -> usize {
    inp.emitter().errors().values().map(|g| g.len()).sum()
  }

  // The law, value-asserted on the in-place path: an `Incomplete` is re-raised verbatim, the
  // recoverer never runs, and nothing is emitted.
  #[test]
  fn inplace_recover_reraises_incomplete_and_never_invokes_recoverer() {
    let mut input = Input::<Lex<'_>, VCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = InplaceRecover::<_, _, (), Lex<'_>, VCtx<'_>, ()>::new(
      VFailWith(RecErr::Incomplete),
      InplaceFlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(RecErr::Incomplete),
      "an Incomplete is re-raised untouched on the Err channel"
    );
    assert!(
      !invoked.get(),
      "the recoverer must never run for an Incomplete"
    );
    assert_eq!(vdiags(&mut inp), 0, "and nothing is emitted on its behalf");
  }

  // The W5 partial-input incomplete: the value the frontier rules actually surface is built via
  // `From<Incomplete>`, not a hand-written literal. The in-place path re-raises that one too.
  #[test]
  fn inplace_recover_reraises_a_from_incomplete_built_error() {
    let surfaced: RecErr = crate::error::Incomplete::new(7usize).into();
    assert!(
      surfaced.is_incomplete(),
      "an error built the way the frontier rules build it must report itself incomplete"
    );

    let mut input = Input::<Lex<'_>, VCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = InplaceRecover::<_, _, (), Lex<'_>, VCtx<'_>, ()>::new(
      VFailWith(surfaced.clone()),
      InplaceFlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(surfaced),
      "the partial-mode incomplete is re-raised untouched"
    );
    assert!(
      !invoked.get(),
      "the recoverer never runs for a partial-mode incomplete"
    );
    assert_eq!(vdiags(&mut inp), 0, "and nothing is emitted on its behalf");
  }

  // The scoping guard, in-place flavour: an ordinary failure still recovers exactly as before.
  #[test]
  fn inplace_recover_still_recovers_an_ordinary_error() {
    let mut input = Input::<Lex<'_>, VCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = InplaceRecover::<_, _, (), Lex<'_>, VCtx<'_>, ()>::new(
      VFailWith(RecErr::Other),
      InplaceFlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(r, Ok(()), "an ordinary error is recovered in place");
    assert!(invoked.get(), "the recoverer runs for an ordinary error");
    assert_eq!(vdiags(&mut inp), 1, "and its diagnostic is on the log");
  }

  // The terminal dual on the in-place path: a terminal scanner stop is re-raised verbatim, the
  // recoverer never runs, and nothing is emitted. The in-place path cannot undo a recovery it should
  // never have begun, so re-raising before the handler runs is the only correct order.
  #[test]
  fn inplace_recover_reraises_terminal_and_never_invokes_recoverer() {
    let mut input = Input::<Lex<'_>, VCtx<'_>, ()>::new("1 2 3");
    let mut emitter = Verbose::<RecErr>::new();
    let mut inp = input.as_ref(&mut emitter);

    let invoked = core::cell::Cell::new(false);
    let mut rec = InplaceRecover::<_, _, (), Lex<'_>, VCtx<'_>, ()>::new(
      VFailWith(RecErr::Terminal),
      InplaceFlagRecover { invoked: &invoked },
    );

    let r = rec.parse_input(&mut inp);
    assert_eq!(
      r,
      Err(RecErr::Terminal),
      "a terminal stop is re-raised untouched on the Err channel"
    );
    assert!(
      !invoked.get(),
      "the recoverer must never run for a terminal stop"
    );
    assert_eq!(vdiags(&mut inp), 0, "and nothing is emitted on its behalf");
  }
}
