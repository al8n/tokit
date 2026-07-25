use crate::{
  container::Container as ContainerT,
  emitter::{FullContainerEmitter, SeparatedEmitter},
  error::{
    syntax::{FullContainer, MissingSyntaxOf},
    token::MissingTokenOf,
  },
  input::Cursor,
  span::Span,
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

impl<'c, 'inp, F, Sep, Condition, O, W, L, Ctx, Lang: ?Sized>
  SeparatedWhile<&'c mut F, Sep, &'c mut Condition, O, W, L, Ctx, Lang>
{
  fn parse<'closure, Container, CH, SP, EH>(
    &mut self,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang>,
    container: &mut Container,
    continue_state_handler: &CH,
    separator_state_handler: &SP,
    end_state_handler: &EH,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    L: Lexer<'inp>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Ctx: ParseContext<'inp, L, Lang>,
    // The separator-slot and decision-window gates surface a terminal scanner stop as this error.
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    W: Window,
    EH: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
    CH: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
    SP: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
  {
    trace_event!(inp, "separated_while");
    let mut state = State::Start;
    let anchor = inp.cursor().clone();
    let mut num_elems = 0;

    loop {
      // Separator-slot decision gate: `try_expect_or_stop` (not `try_expect`) so a terminal scanner
      // stop at the separator slot surfaces as its terminal-marked end-of-input error rather than
      // folding into `Ok(None)` and ending the list cleanly. Attempt-relative (the boundary is
      // consulted only on the empty-cache scan path, where the lex offset equals the cursor).
      match inp.try_expect_or_stop(|tok| Sep::eval(&tok.data.kind()))? {
        Some(tok) => {
          state = self.handle_separator(state, inp, tok, container, separator_state_handler)?;

          continue;
        }
        None => {
          // Decision-window gate. `peek_with_emitter_terminal` reports (at no extra hot-path cost)
          // whether the fill came back short because of a terminal scanner stop — a mid-window trip
          // the condition would otherwise read as a clean end. Surface that as terminal at either
          // swallow point (an empty window, or an `Action::Stop`); `Continue` needs no check, as the
          // element parse re-lexes the frontier and surfaces the stop itself.
          let (peeked, terminal, emitter) = inp.peek_with_emitter_terminal::<W>()?;

          let front_span = match peeked.front() {
            None => {
              drop(peeked);
              if terminal {
                return Err(
                  UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                    .into_terminal()
                    .into(),
                );
              }
              return self.handle_end(state, inp, &anchor, num_elems, end_state_handler);
            }
            Some(front) => front
              .as_maybe_ref()
              .map(|t| t.token().copied(), |t| t.token())
              .into_inner()
              .span()
              .clone(),
          };

          match self.condition.decide(peeked, emitter)? {
            Action::Stop => {
              if terminal {
                return Err(
                  UnexpectedEot::eot_of(inp.cursor().as_inner().clone())
                    .into_terminal()
                    .into(),
                );
              }
              return self.handle_end(state, inp, &anchor, num_elems, end_state_handler);
            }
            Action::Continue => {
              // if the peeked token belongs to an element, check the current state
              state = self.handle_continue(
                state,
                inp,
                &anchor,
                &front_span,
                &mut num_elems,
                container,
                continue_state_handler,
              )?;
            }
          }
        }
      }
    }
  }

  pub(super) fn handle_separator<'closure, Handler, Container>(
    &mut self,
    mut state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang>,
    sep_tok: Spanned<L::Token, L::Span>,
    container: &mut Container,
    handler: &Handler,
  ) -> Result<State<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
    Handler: SeparatorStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
  {
    match state {
      // happy path, we found a separator after an element
      State::Element => {
        // Change the current state to Separator.
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

        // As we have emitted the missing element error, so the behavior of the state machine
        // should be as if we have successfully parsed an element here.
        // So we push the new separator token into the container,
        // and change the state to Separator.
        let sep = sep_tok;
        container.on_separator(sep.clone());
        state = State::Separator(sep);
      }
      // first token is a separator
      State::Start => {
        // we do not need to check leading spec here, as we cached the leading separator token,
        // the check will be done when we find the first element or reach the end of input
        let st = sep_tok;
        handler.handle_start_state(inp, &st)?;
        container.on_separator(st.clone());
        state = State::Leading(st);
      }
      // we are in separator state, so the next token should be an element,
      State::Separator(_) => {
        // We found consecutive separators, emit missing element error via the emitter
        inp
          .emitter()
          .emit_missing_element(MissingSyntaxOf::<'_, L, Lang>::of(
            sep_tok.span_ref().start(),
          ))?;

        container.on_separator(sep_tok.clone());
        state = State::Separator(sep_tok);
      }
    }
    Ok(state)
  }

  #[allow(clippy::too_many_arguments)]
  pub(super) fn handle_continue<'closure, Container, Handler>(
    &mut self,
    mut state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang>,
    anchor: &Cursor<'inp, 'closure, L>,
    peek_span: &L::Span,
    num_elems: &mut usize,
    container: &mut Container,
    handler: &Handler,
  ) -> Result<State<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    W: Window,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang> + FullContainerEmitter<'inp, L, Lang>,
    Container: ContainerT<O> + SeparatorHandler<'inp, L>,
    Handler: ContinueStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
  {
    match state {
      // happy path, we found a separator before an element
      State::Separator(_) => {
        // parse the next element
        let element = self.f.parse_input(inp)?;
        if push(num_elems, container, element).is_err() {
          let span = inp.span_since(anchor);
          inp.emitter().emit_full_container(FullContainer::of(
            span,
            *num_elems,
            container.max_capacity(),
          ))?;
        }
        state = State::Element;
      }
      // we are in leading state,
      State::Leading(_) => {
        // parse the first element
        let element = self.f.parse_input(inp)?;
        if push(num_elems, container, element).is_err() {
          let span = inp.span_since(anchor);
          inp.emitter().emit_full_container(FullContainer::of(
            span,
            *num_elems,
            container.max_capacity(),
          ))?;
        }
        state = State::Element;
      }
      // nothing before element, parse the first element
      State::Start => {
        // let the passing handler deal with the start state
        handler.handle_start_state(inp, peek_span.start())?;

        // parse the first element
        let element = self.f.parse_input(inp)?;
        if push(num_elems, container, element).is_err() {
          let span = inp.span_since(anchor);
          inp.emitter().emit_full_container(FullContainer::of(
            span,
            *num_elems,
            container.max_capacity(),
          ))?;
        }
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

        handler.handle_too_many_element(*num_elems, inp, anchor)?;

        // parse the next element
        let element = self.f.parse_input(inp)?;
        if push(num_elems, container, element).is_err() {
          let span = inp.span_since(anchor);
          inp.emitter().emit_full_container(FullContainer::of(
            span,
            *num_elems,
            container.max_capacity(),
          ))?;
        }
        state = State::Element;
      }
    }

    Ok(state)
  }

  pub(super) fn handle_end<'closure, Handler>(
    &mut self,
    state: State<L::Token, L::Span>,
    inp: &mut InputRef<'inp, 'closure, L, Ctx, Lang>,
    anchor: &Cursor<'inp, 'closure, L>,
    num_elems: usize,
    handler: &Handler,
  ) -> Result<L::Span, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    'inp: 'closure,
    L: Lexer<'inp>,
    Ctx: ParseContext<'inp, L, Lang>,
    F: ParseInput<'inp, L, O, Ctx, Lang>,
    W: Window,
    Condition: Decision<'inp, L, Ctx::Emitter, W, Lang>,
    Sep: Punctuator<'inp, L, Lang>,
    Ctx::Emitter: SeparatedEmitter<'inp, L, Lang>,
    Handler: EndStateHandler<'inp, 'closure, Sep, O, L, Ctx, Lang>,
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

#[inline(always)]
fn push<C, T>(nums: &mut usize, container: &mut C, item: T) -> Result<(), T>
where
  C: crate::container::Container<T>,
{
  container.push(item).inspect(|_| *nums += 1)
}
