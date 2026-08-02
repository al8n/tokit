use crate::{
  TryParseInput,
  container::Container as ContainerT,
  emitter::{FullContainerEmitter, SeparatedEmitter},
  error::{syntax::MissingSyntaxOf, token::MissingTokenOf},
  input::Cursor,
  punct::Punctuator,
  span::Span,
  try_parse_input::{Accept, Decline},
};

use super::*;

use core::mem;

mod allow_leading;
mod allow_leading_require_trailing;
mod allow_surrounded;
mod allow_trailing;
mod at_least;
mod at_most;
mod bounded;
mod require_leading;
mod require_leading_allow_trailing;
mod require_surrounded;
mod require_trailing;
mod unbounded;

impl<'inp, F, Sep, O, L, Ctx, Lang: ?Sized, Cmpl> Separated<&mut F, Sep, O, L, Ctx, Lang, Cmpl> {
  fn parse<'closure, Container, CH, SP, EH>(
    &mut self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    continue_state_handler: &CH,
    separator_state_handler: &SP,
    end_state_handler: &EH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    // The separator-slot decision gate surfaces a terminal scanner stop as this end-of-input error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    EH: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
    CH: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
    SP: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
  {
    trace_event!(inp, "separated");
    let mut state = State::Start;
    let anchor = inp.cursor().clone();
    let mut cursor = anchor.clone();
    let mut committed = inp.span().end();
    // The terminal-latch baseline for the element-absence exits below: comparing the live latch
    // against it keeps their witness attempt-relative. One offset clone per collection.
    let latch = inp.latch_snapshot();
    let mut num_elems = 0;
    let mut full = false;

    loop {
      let mut ps = None;
      // Separator-slot decision gate. `try_expect_or_stop` (not `try_expect`) so a terminal
      // scanner stop at the separator slot surfaces as its terminal-marked end-of-input error
      // rather than folding into `Ok(None)` and ending the list cleanly — the peek-layer twin of
      // the element gate's terminal re-raise. Its terminal report is attempt-relative (it consults
      // the boundary only on the empty-cache scan path, where the lex offset equals the cursor), so
      // a prefilled cache cannot false-positive it; a genuine absence (wrong token / real EOI) still
      // declines to `Ok(None)`.
      let peek_span = match inp.try_expect_or_stop(|t| {
        if Sep::eval(&t.data.kind()) {
          true
        } else {
          ps = Some(t.span().clone());
          false
        }
      })? {
        None => match ps {
          None => return self.handle_end(state, inp, &anchor, num_elems, end_state_handler),
          Some(span) => span,
        },
        Some(tok) => {
          state = self.handle_separator(state, inp, container, separator_state_handler, tok)?;
          cursor = inp.cursor().clone();
          committed = inp.span().end();
          continue;
        }
      };

      match self.f.try_parse_input(inp) {
        // The never-recoverable gate and its two terminal witnesses: a frontier `Incomplete`
        // (const-false under `Complete`), a terminal *scanner* stop, or a *descent* budget trip from
        // the element parser re-raises untouched — never spent as a diagnostic, since no further
        // input clears any of the three. The scanner witness reads the *committed cursor*
        // ([`at_committed_boundary`]), so a boundary a prior lookahead already latched does not
        // mis-charge an ordinary element failure short of it. The descent witness is the set-once
        // session cell ([`resource_trip`]), latched before the grammar's `From` runs, so a trip
        // re-raises for a delegating error type and for a discarding one alike — `()` included.
        // Failure-arm only — a successful element does zero terminal work; both witnesses are
        // positional/session facts, so no `MaybeTerminal` bound needed.
        Err(e)
          if Cmpl::is_incomplete_error(&e)
            || inp.at_committed_boundary()
            || inp.resource_trip() =>
        {
          return Err(e);
        }
        Err(e) => {
          let span = inp.span_since(&cursor);
          inp.emitter().emit_error(Spanned::new(span, e))?;
        }
        // The decline concludes *absence*, but the element's own lookahead can latch a terminal
        // scanner stop and still return `Ok` with a short window, so that conclusion may rest on a
        // truncated view — a case the separator-slot gate above cannot see, since the latch happens
        // after it. Attempt-relative, so an inherited boundary is not mis-charged here.
        Ok(Decline) => {
          if inp.latched_during_attempt(&latch) {
            return Err(
              UnexpectedEot::eot_of(inp.span().end())
                .into_terminal()
                .into(),
            );
          }
          return self.handle_end(state, inp, &anchor, num_elems, end_state_handler);
        }
        Ok(Accept(elem)) => {
          // if the peeked token belongs to an element, check the current state
          state = self.handle_continue(
            state,
            inp,
            &anchor,
            peek_span,
            elem,
            &mut num_elems,
            &mut full,
            container,
            continue_state_handler,
          )?;
        }
      }

      // A cycle that consumed nothing re-sees the same input and would retry forever. The progress
      // metric is committed consumption (`span().end()`), never the cache-front cursor — a lookahead
      // fill (a `try_expect_or_stop` decline pushing a token back) moves that across skipped trivia
      // without consuming, reading a zero-width element as false progress. `<=`, not `==`: the
      // watermark cannot regress within a cycle, so anything not strictly ahead is a stall. `cursor`
      // stays the error-span anchor.
      let new_committed = inp.span().end();
      let new_cursor = inp.cursor().clone();
      if new_committed <= committed {
        // The stall concludes *absence* on the same truncated-view risk as the decline arm above:
        // surface a stop this attempt latched rather than ending the list cleanly.
        if inp.latched_during_attempt(&latch) {
          return Err(
            UnexpectedEot::eot_of(inp.span().end())
              .into_terminal()
              .into(),
          );
        }
        return self.handle_end(state, inp, &anchor, num_elems, end_state_handler);
      }
      committed = new_committed;
      cursor = new_cursor;
    }
  }

  pub(super) fn handle_separator<'closure, Handler, Container>(
    &mut self,
    mut state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    container: &mut Container,
    handler: &Handler,
    sep_tok: Spanned<L::Token, L::Span>,
  ) -> Result<State<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
    Handler: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
  {
    match state {
      // happy path, we found a separator after an element
      State::Element => {
        container.observe_separator(&sep_tok);
        state = State::Separator(sep_tok);
      }
      // First token is a separator, we found another leading separator
      State::Leading(_) => {
        // whatever the leading spec is, multiple leading separators are not allowed
        // so we treat the old one as a unexpected token, emit it via the emitter,
        // and let the emitter decide whether to return early
        inp
          .emitter()
          .emit_missing_element(MissingSyntaxOf::<'_, L, Lang>::of(
            sep_tok.span_ref().start(),
          ))?;

        // As we have emitted the missing element error, the state machine behaves as if an
        // element had been parsed here, so this separator opens a new separator state.
        container.observe_separator(&sep_tok);
        state = State::Separator(sep_tok);
      }
      // first token is a separator
      State::Start => {
        // we do not need to check leading spec here, as we cached the leading separator token,
        // the check will be done when we find the first element or reach the end of input
        handler.handle_start_state(inp, &sep_tok)?;
        container.observe_separator(&sep_tok);
        state = State::Leading(sep_tok);
      }
      // we are in separator state, so the next token should be an element,
      State::Separator(_) => {
        // We found consecutive separators, emit missing element error via the emitter
        inp
          .emitter()
          .emit_missing_element(MissingSyntaxOf::<'_, L, Lang>::of(
            sep_tok.span_ref().start(),
          ))?;

        container.observe_separator(&sep_tok);
        state = State::Separator(sep_tok);
      }
    }
    Ok(state)
  }

  #[allow(clippy::too_many_arguments)]
  pub(super) fn handle_continue<'closure, Container, Handler>(
    &mut self,
    mut state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    peek_span: L::Span,
    element: O,
    num_elems: &mut usize,
    full: &mut bool,
    container: &mut Container,
    handler: &Handler,
  ) -> Result<State<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    Handler: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
  {
    match state {
      // happy path, we found a separator before an element
      State::Separator(_) => {
        push_element(num_elems, full, container, element, inp, anchor)?;
        state = State::Element;
      }
      // we are in leading state,
      State::Leading(_) => {
        push_element(num_elems, full, container, element, inp, anchor)?;
        state = State::Element;
      }
      // nothing before element, parse the first element
      State::Start => {
        // let the passing handler deal with the start state
        handler.handle_start_state(inp, peek_span.start())?;

        push_element(num_elems, full, container, element, inp, anchor)?;

        state = State::Element;
      }
      // we are in element state, so the next token should be a separator,
      // so missing separator case, let's construct a missing separator error,
      // and emit it via the emitter, and let the emitter decide whether to return early
      State::Element => {
        let off = peek_span.start();
        inp
          .emitter()
          .emit_missing_separator(Sep::name(), MissingTokenOf::<'_, L, Lang>::of(off))?;

        // parse the next element
        push_element(num_elems, full, container, element, inp, anchor)?;
        state = State::Element;
      }
    }

    Ok(state)
  }

  pub(super) fn handle_end<'closure, Handler>(
    &mut self,
    state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang, Cmpl>,
    anchor: &Cursor<'inp, 'closure, L>,
    num_elems: usize,
    handler: &Handler,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Cmpl: crate::input::SurfaceIncomplete<'inp, L, Ctx, Lang>,
    F: TryParseInput<'inp, L, O, Ctx, Lang, Cmpl>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
    Handler: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang, Cmpl>,
  {
    Ok(match state {
      // we are in the start state, so no elements were found
      State::Start => handler.handle_start_state(num_elems, inp, anchor)?,
      // we are in element state, so all good, check for trailing separator, and the minimum, maximum constraints
      State::Element => handler.handle_element_state(num_elems, inp, anchor)?,
      State::Leading(spanned) => handler.handle_leading_state(num_elems, inp, anchor, spanned)?,
      // we have a trailing separator
      State::Separator(spanned) => {
        handler.handle_separator_state(num_elems, inp, anchor, spanned)?
      }
    })
  }
}
