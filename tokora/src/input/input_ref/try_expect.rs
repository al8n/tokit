use super::*;

use crate::{
  error::UnexpectedEot,
  token::{PunctuatorToken, PunctuatorTokenExt, SpannedPunctuatorToken},
};

/// The four-way outcome of probing the close position of a delimited list — the
/// structural fix for the fold where a close classifier built on
/// [`try_expect`](InputRef::try_expect) reads `Ok(None)` as "no closer here" and the
/// driver then emits `Unclosed`.
///
/// `try_expect`'s `Ok(None)` also covers a `Scan::Tripped` (a resource-limit trip)
/// and a latched poison boundary — terminal stops whose own diagnostic already
/// explains the halt, so an added `Unclosed` is spurious. [`probe_close`] keeps the
/// four cases apart and the driver maps each to its own action (commit /
/// unexpected-token / `Unclosed` / propagate).
///
/// [`probe_close`]: InputRef::probe_close
pub(crate) enum CloseStatus<'inp, L>
where
  L: Lexer<'inp>,
{
  /// The closer is at hand, ready to commit by value with
  /// [`commit_probed`](InputRef::commit_probed) — see [`ClosePayload`] for the origin
  /// split. Either way the **committed cursor has not advanced**: the probe never
  /// consumes, so the caller commits at its own program point (immediately for the
  /// immediate-commit drivers, or deferred past an end-of-list pass for
  /// `separated`/`separated_while`), and the commit re-lexes nothing in any cache
  /// capacity — including the blackhole `()`.
  Close(ClosePayload<'inp, L>),
  /// A non-closer token sits where the closer belongs. It is left **unconsumed**
  /// (the downstream parse still sees it); the owned clone drives the
  /// unexpected-token / expected-close diagnostic.
  WrongToken(Spanned<L::Token, L::Span>),
  /// Genuine end of input with the opener still open — the one and only `Unclosed`
  /// path.
  Eof,
  /// A terminal scanner stop: a resource-limit trip on this scan, or an
  /// already-latched poison boundary. The halt is already diagnosed, so the caller
  /// propagates it and adds no `Unclosed` on top.
  Tripped,
}

/// How the probed closer is held between classification and commit, split by origin so the
/// probe stays **cursor-neutral** until the caller's real commit point — critical for the
/// deferred (`separated`/`separated_while`) drivers, whose `handle_end` runs *between* the
/// probe and the commit and spans the elements off [`cursor`](InputRef::cursor) (which reads
/// the cache front). Popping at probe time would advance `cursor` over the closer early —
/// over-including it in the elements span, and, if `handle_end` errors, dropping the popped
/// closer while later cached tokens survive (recovery would then skip the closer).
pub(crate) enum ClosePayload<'inp, L>
where
  L: Lexer<'inp>,
{
  /// Carried out of the **scan** path by value (its post-token lexer state included). The scan
  /// never advanced the committed cursor, so the token is already out of the input and neutral;
  /// [`commit_probed`](InputRef::commit_probed) settles it by value with no re-lex — the
  /// cache-independent path a blackhole `()` needs (a pushed-back closer would be dropped).
  Scanned(CachedTokenOf<'inp, L>),
  /// Left at the **front of the stream** — the cache front, or the parked slot when the cache
  /// refuses to hold it — classified by peek, *not* popped, so `cursor` stays put and the closer
  /// survives an intervening error for recovery. [`commit_probed`](InputRef::commit_probed) takes
  /// it and settles it at the commit point — the same front-take-commit `try_expect` uses, which
  /// never re-lexes.
  AtFront,
}

macro_rules! try_expect_punct {
  ($($punct:ident $(:$alias:ident)? :$punct_char:literal),+$(,)?) => {
    paste::paste! {
      $(
        #[doc = "Tries to advance to the next valid token if it is " $punct " (" $punct_char "). Otherwise leaves the input unchanged."]
        ///
        /// `Ok(None)` also covers a terminal stop (limit trip / latched poison
        /// boundary); when a decline commits the caller to a different parse, use
        /// [`try_expect_or_stop`](Self::try_expect_or_stop).
        #[inline(always)]
        pub fn [< try_expect_ $punct >](
          &mut self,
        ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L::Token: crate::token::PunctuatorToken<'inp>,
        {
          self.try_expect(|t| t.data.[<is_ $punct>]())
        }

        #[doc = "Advances to the next valid token and expects it to be " $punct " (" $punct_char ")."]
        #[inline(always)]
        pub fn [< expect_ $punct >](
          &mut self,
        ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
        where
          L::Token: PunctuatorToken<'inp>,
          <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
            + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
        {
          match self.next_or_stop()? {
            Some(spanned) => {
              <Spanned<L::Token, L::Span> as SpannedPunctuatorToken<'inp, L, Lang>>::[< expect_ $punct >](spanned).map_err(Into::into)
            },
            None => Err(UnexpectedEot::eot_of(self.span().end()).into()),
          }
        }

        $(
          #[doc = "Tries to advance to the next valid token if it is " $alias " (" $punct_char "). Otherwise leaves the input unchanged."]
          ///
          /// `Ok(None)` also covers a terminal stop (limit trip / latched poison
          /// boundary); when a decline commits the caller to a different parse, use
          /// [`try_expect_or_stop`](Self::try_expect_or_stop).
          #[inline(always)]
          pub fn [< try_expect_ $alias >](
            &mut self,
          ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
          where
            L::Token: PunctuatorToken<'inp>,
          {
            self.[< try_expect_ $punct >]()
          }

          #[doc = "Advances to the next valid token and expects it to be " $alias " (" $punct_char ")."]
          #[inline(always)]
          pub fn [< expect_ $alias >](
            &mut self,
          ) -> Result<Spanned<L::Token, L::Span>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
          where
            L::Token: PunctuatorToken<'inp>,
            <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>
              + From<UnexpectedToken<'inp, L::Token, <L::Token as Token<'inp>>::Kind, L::Span, Lang>>,
          {
            self.[< expect_ $punct >]()
          }
        )?
      )*
    }
  };
}

impl<'inp, L, Ctx, Lang: ?Sized, Cmpl> InputRef<'inp, '_, L, Ctx, Lang, Cmpl>
where
  L: Lexer<'inp>,
  L::State: Clone,
  Ctx: ParseContext<'inp, L, Lang>,
  Cmpl: SurfaceIncomplete<'inp, L, Ctx, Lang>,
{
  try_expect_punct!(
    // Delimiters
    open_angle:less_than:"<",
    close_angle:greater_than:">",
    open_brace:"{",
    close_brace:"}",
    open_paren:"(",
    close_paren:")",
    open_bracket:"[",
    close_bracket:"]",

    // ASCII Punctuation
    at:"@",
    asterisk:"*",
    ampersand: "&",
    apostrophe:"'",
    backtick:"`",
    backslash:"\\",
    caret:"^",
    comma:",",
    colon:":",
    dot:".",
    dollar:"$",
    double_quote:"\"",
    equal:"=",
    exclamation:bang:"!",
    hash:"#",
    hyphen:minus:"-",
    pipe:"|",
    plus:"+",
    percent:"%",
    question:"?",
    slash:"/",
    semicolon:";",
    tilde:"~",
    underscore:"_",

    // Multi-character Punctuators
    arrow:thin_arrow:"->",
    fat_arrow:"=>",
    pipe_arrow:pipe_forward:"|>",

    // Equal related
    colon_equal:colon_assign:":=",
    logical_equal: "==",
    logical_not_equal: "!=",
    strict_equal: "===",
    strict_not_equal: "!==",
    less_than_or_equal: "<=",
    greater_than_or_equal: ">=",
    strict_less_than_or_equal: "<==",
    strict_greater_than_or_equal: ">==",

    plus_equal:add_assign: "+=",
    hyphen_equal:sub_assign: "-=",
    asterisk_equal:mul_assign: "*=",
    exponentiation_equal:exponentiation_assign: "**=",
    slash_equal:div_assign: "/=",
    backslash_equal: "\\=",

    percent_equal:rem_assign: "%=",

    ampersand_equal:bitand_assign: "&=",
    pipe_equal:bitor_assign: "|=",
    caret_equal:xor_assign: "^=",

    shl_equal:shl_assign: "<<=",
    shr_equal:shr_assign: ">>=",
    sar_equal:sar_assign: ">>>=",

    shl: "<<",
    shr: ">>",
    sar: ">>>",

    increment: "++",
    decrement: "--",
    exponentiation: "**",

    logical_and: "&&",
    logical_or: "||",

    double_colon:"::",
    spread: "...",
    null_coalesce: "??",
    optional_chain: "?.",

    // Trivia
    tab:"\t",
    newline:"\n",
    carriage_return:"\r",
    crlf:"\r\n",
    space:" ",
  );

  /// Advances to the next valid token and expects it to satisfy the predicate.
  ///
  /// Emits any lexer errors encountered. If a valid token is found, calls `pred`.
  /// If `pred` returns `true`, the token is consumed and returned.
  /// Otherwise, the token remains in the cache and `Ok(None)` is returned.
  ///
  /// `Ok(None)` also covers a terminal stop (limit trip / latched poison
  /// boundary); when a decline commits the caller to a different parse, use
  /// [`try_expect_or_stop`](Self::try_expect_or_stop).
  #[inline]
  pub fn try_expect<F>(
    &mut self,
    mut pred: F,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    trace_event!(self, "try_expect");
    if !self.has_front() {
      return self.try_expect_on_input(pred);
    }

    // pop from cache if matching
    Ok(self.take_front_if(|t| pred(t.token)).map(|tok| {
      let (lexed, state) = tok.into_components();
      let (span, tok) = lexed.into_components();
      self.commit_token(&tok, &span, state);

      Spanned::new(span, tok)
    }))
  }

  /// Tries to advance to the next valid token if it satisfies `pred` — like
  /// [`try_expect`](Self::try_expect), except that a **terminal stop is an error,
  /// never a decline**.
  ///
  /// `Ok(None)` here means the thing attempted is **definitely absent**: the next
  /// valid token failed `pred` (it stays unconsumed, at the cache front), or the
  /// input has genuinely ended. A terminal stop — a resource-limit trip on this
  /// scan, or an already-latched poison boundary — is *not* evidence of absence,
  /// so it surfaces as the same end-of-input error the committed `expect_*` forms
  /// raise there, after the trip's own diagnostic has gone to the emitter
  /// (deduplicated; a fatal emitter's rejection still propagates from the scan
  /// itself). This is the primitive an attempt/decline caller should build on
  /// when a decline commits it to a different parse — see the `try_*` delimited
  /// shapes.
  #[inline]
  pub fn try_expect_or_stop<F>(
    &mut self,
    mut pred: F,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    trace_event!(self, "try_expect_or_stop");
    if self.has_front() {
      // A cached token is a REAL token (the cache never holds errors): a decline
      // against it is definite absence — identical to `try_expect`'s cache arm.
      return Ok(self.take_front_if(|t| pred(t.token)).map(|tok| {
        let (lexed, state) = tok.into_components();
        let (span, tok) = lexed.into_components();
        self.commit_token(&tok, &span, state);
        Spanned::new(span, tok)
      }));
    }
    // E4: an already-latched poison boundary at the cursor is a terminal stop,
    // not proof of absence — surface the committed form's end-of-input error,
    // marked terminal so recovery re-raises it.
    if self.reached_boundary(self.offset()) {
      return Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      );
    }
    let mut resume = self.resume();
    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        if pred(tok.as_ref()) {
          self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
          Ok(Some(tok))
        } else {
          let (span, tok) = tok.into_components();
          let ct = CachedToken::new(Spanned::new(span, tok), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(None) // E1: definite absence — the token stays at the cache front.
        }
      }
      // E3: a fresh trip whose diagnostic a recovering emitter accepted. (A fatal
      // emitter never reaches this arm — its rejection propagated out of
      // `scan_with` above.) Marked terminal so recovery re-raises it.
      Scan::Tripped => Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      ),
      Scan::Eof => {
        // E5 belt-and-braces: an exhaustion produced by refusing to cross a
        // pre-latched boundary is terminal, not genuine end of input. (Under
        // Partial non-final, `scan_with` already surfaced Incomplete for every
        // non-boundary exhaustion, so Eof implies boundary there — this check
        // makes that explicit in Complete mode too.) One cold compare.
        if self.reached_boundary(resume.at()) {
          Err(
            UnexpectedEot::eot_of(self.span().end())
              .into_terminal()
              .into(),
          )
        } else {
          Ok(None) // E2: genuine end of input — the documented decline.
        }
      }
    }
  }

  /// Classifies the close position of a delimited list, distinguishing all four
  /// outcomes a delim driver must tell apart — see [`CloseStatus`].
  ///
  /// `pred` decides whether the token at the cursor is the closer. The probe is
  /// **cursor-neutral** — it never advances the committed cursor, for any outcome. On
  /// [`Close`](CloseStatus::Close) it hands back a [`ClosePayload`] recording where the
  /// closer lives: carried out by value from the scan path, or left at the cache front
  /// (peeked, not popped) on the cache path. The caller settles it later with
  /// [`commit_probed`](Self::commit_probed) at its own program point — immediately, or
  /// deferred past an end-of-list pass — and that commit re-lexes nothing, in any cache
  /// capacity (including the blackhole `()`, where pushing a scanned closer back would be
  /// dropped and force a re-scan). On [`WrongToken`](CloseStatus::WrongToken) the token is
  /// left in place for the downstream parse.
  ///
  /// This is the structural counterpart to a close classifier built on `try_expect`,
  /// whose `Ok(None)` folds a genuine end of input together with a terminal stop:
  /// here a resource-limit trip or a latched poison boundary surfaces as
  /// [`Tripped`](CloseStatus::Tripped) — the same Tripped/Eof split
  /// [`try_expect_or_stop`](Self::try_expect_or_stop) draws — so it is never misread
  /// as EOF and never grows a spurious `Unclosed`. A fatal emitter's rejection of the
  /// trip diagnostic still propagates from the scan itself as `Err`.
  #[inline]
  pub(crate) fn probe_close<F>(
    &mut self,
    mut pred: F,
  ) -> Result<CloseStatus<'inp, L>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    trace_event!(self, "probe_close");
    if self.has_front() {
      // A cached token is a REAL token (the cache never holds errors). Classify the front by
      // PEEK — do NOT pop it here: the probe stays cursor-neutral so a deferred commit
      // (`separated`/`separated_while`, which run `handle_end` before committing) spans the
      // elements correctly and keeps the closer in the cache if that pass errors. It is popped
      // and settled later by `commit_probed`. `Spanned<&_, &_>` is `Copy`, so `pred` and the
      // owned clone read the same peeked reference.
      let peeked = self.front().expect("a front token is present").token;
      return Ok(if pred(peeked) {
        CloseStatus::Close(ClosePayload::AtFront)
      } else {
        CloseStatus::WrongToken(peeked.cloned())
      });
    }
    // A latched poison boundary at the cursor is a terminal stop, not proof of
    // absence — mirrors `try_expect_or_stop`'s E4.
    if self.reached_boundary(self.offset()) {
      return Ok(CloseStatus::Tripped);
    }
    let mut resume = self.resume();
    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        if pred(tok.as_ref()) {
          // Close (scan origin): carry the scanned closer OUT by value (token + post-token
          // state) rather than push it to a cache the blackhole `()` would drop. The scan never
          // advanced the committed cursor, so this is already neutral; `commit_probed` settles
          // it by value with no second scan, in every cache capacity.
          let (span, token) = tok.into_components();
          Ok(CloseStatus::Close(ClosePayload::Scanned(CachedToken::new(
            Spanned::new(span, token),
            resume.into_lexer().into_state(),
          ))))
        } else {
          // WrongToken: unchanged — best-effort push-back, owned clone for the
          // diagnostic. (The push-back is still a no-op under `()`; that is the
          // downstream parse's re-scan of the wrong token, not the close commit's — see
          // the out-of-scope note in the spec.)
          let wrong = Spanned::new(tok.span_ref().clone(), tok.data().clone());
          let (span, token) = tok.into_components();
          let ct = CachedToken::new(Spanned::new(span, token), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(CloseStatus::WrongToken(wrong))
        }
      }
      // A fresh trip whose diagnostic a recovering emitter accepted. (A fatal emitter
      // never reaches this arm — its rejection propagated out of `scan_with` above.)
      Scan::Tripped => Ok(CloseStatus::Tripped),
      Scan::Eof => {
        // An exhaustion produced by refusing to cross a pre-latched boundary is
        // terminal, not genuine end of input — mirrors `try_expect_or_stop`'s E5.
        if self.reached_boundary(resume.at()) {
          Ok(CloseStatus::Tripped)
        } else {
          Ok(CloseStatus::Eof)
        }
      }
    }
  }

  /// Commits a closer classified by [`probe_close`](Self::probe_close), advancing the cursor
  /// over it **without re-lexing** — dispatching on the [`ClosePayload`] origin. Both origins
  /// converge on the one [`commit_token`](Self::commit_token) settle + the per-site state
  /// write, so the closer is committed **exactly once**, cache-independently:
  ///
  /// - [`Scanned`](ClosePayload::Scanned): the token carried out of the scan is settled by
  ///   value — the path a blackhole `()` needs (a pushed-back closer would be dropped and
  ///   re-lexed).
  /// - [`AtFront`](ClosePayload::AtFront): the closer was left at the front of the stream by the
  ///   probe; take it now (`try_expect`'s front arm, which never re-lexes) and settle it. Taking
  ///   it here — not at probe time — keeps `cursor` neutral until this commit point, so the
  ///   deferred (`separated`) drivers span `handle_end` correctly and an error before this call
  ///   leaves the closer at the front for recovery.
  ///
  /// The caller runs this immediately (immediate-commit drivers) or deferred past the
  /// end-of-list pass (`separated`/`separated_while`).
  #[inline]
  pub(crate) fn commit_probed(
    &mut self,
    payload: ClosePayload<'inp, L>,
  ) -> Spanned<L::Token, L::Span> {
    let carried = match payload {
      ClosePayload::Scanned(carried) => carried,
      // The probe left the closer at the front of the stream; take it at the commit point (never
      // a re-lex — a retained token is a fully lexed token).
      ClosePayload::AtFront => self
        .take_front()
        .expect("commit_probed(AtFront): the probed closer is still at the front"),
    };
    let (lexed, state) = carried.into_components();
    let (span, tok) = lexed.into_components();
    self.commit_token(&tok, &span, state);
    Spanned::new(span, tok)
  }

  /// Advances to the next valid token and expects it to satisfy the predicate.
  ///
  /// Emits any lexer errors encountered. If a valid token is found, calls `pred`.
  /// If `pred` returns `Some(output)`, the token is consumed and `(output, token)` is returned.
  /// If `pred` returns `None`, the token remains in the cache and `Ok(None)` is returned.
  ///
  /// `Ok(None)` also covers a terminal stop (limit trip / latched poison
  /// boundary); when a decline commits the caller to a different parse, use
  /// [`try_expect_or_stop`](Self::try_expect_or_stop).
  #[inline]
  pub fn try_expect_map<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> Option<O>,
  {
    trace_event!(self, "try_expect_map");
    if !self.has_front() {
      return self.try_expect_map_on_input(pred);
    }

    let mut output = None;
    Ok(
      self
        .take_front_if(|t| match pred(t.token().copied()) {
          Some(out) => {
            output = Some(out);
            true
          }
          None => false,
        })
        .map(|tok| {
          let (lexed, state) = tok.into_components();
          let (span, tok) = lexed.into_components();
          self.commit_token(&tok, &span, state);
          (output.unwrap(), Spanned::new(span, tok))
        }),
    )
  }

  /// Tries to advance to the next valid token, mapping it through `pred` — like
  /// [`try_expect_map`](Self::try_expect_map), except that a **terminal stop is an error,
  /// never a decline**.
  ///
  /// This is the map-shaped twin of [`try_expect_or_stop`](Self::try_expect_or_stop): `Ok(None)`
  /// means the thing attempted is **definitely absent** (the next valid token mapped to `None` and
  /// stays at the cache front, or the input has genuinely ended), while a terminal stop — a
  /// resource-limit trip on this scan, or an already-latched poison boundary — surfaces as the same
  /// terminal-marked end-of-input error the committed forms raise. It is the primitive a map-shaped
  /// attempt (the token-pratt LHS/RHS classifier) should build on when a decline commits it to a
  /// different parse.
  #[inline(always)]
  pub fn try_expect_map_or_stop<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> Option<O>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    trace_event!(self, "try_expect_map_or_stop");
    if !self.has_front() {
      return self.try_expect_map_or_stop_on_input(pred);
    }

    // A cached token is a REAL token: a decline against it is definite absence —
    // identical to `try_expect_map`'s cache arm.
    let mut output = None;
    Ok(
      self
        .take_front_if(|t| match pred(t.token().copied()) {
          Some(out) => {
            output = Some(out);
            true
          }
          None => false,
        })
        .map(|tok| {
          let (lexed, state) = tok.into_components();
          let (span, tok) = lexed.into_components();
          self.commit_token(&tok, &span, state);
          (output.unwrap(), Spanned::new(span, tok))
        }),
    )
  }

  /// Advances to the next valid token and expects it to satisfy the predicate.
  ///
  /// Emits any lexer errors encountered. If a valid token is found, calls `pred`.
  /// If `pred` returns `Some(Ok(output))`, the token is consumed and `(output, token)` is returned.
  /// If `pred` returns `Some(Err(error))`, the token is consumed and `Err(error)` is returned.
  /// If `pred` returns `None`, the token remains in the cache and `Ok(None)` is returned.
  ///
  /// `Ok(None)` also covers a terminal stop (limit trip / latched poison
  /// boundary); when a decline commits the caller to a different parse, use
  /// [`try_expect_or_stop`](Self::try_expect_or_stop).
  pub fn try_expect_and_then<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(
      Spanned<&L::Token, &L::Span>,
    ) -> Option<Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>>,
  {
    trace_event!(self, "try_expect_and_then");
    if !self.has_front() {
      return self.try_expect_and_then_on_input(pred);
    }

    let mut output = None;
    if let Some(tok) = self.take_front_if(|t| match pred(t.token().copied()) {
      Some(res) => {
        output = Some(res);
        true
      }
      None => false,
    }) {
      let (lexed, state) = tok.into_components();
      let (span, tok) = lexed.into_components();
      self.commit_token(&tok, &span, state);

      return match output {
        Some(res) => res.map(|o| Some((o, Spanned::new(span, tok)))),
        None => Ok(None),
      };
    }

    Ok(None)
  }

  #[inline]
  fn try_expect_and_then_on_input<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(
      Spanned<&L::Token, &L::Span>,
    ) -> Option<Result<O, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>>,
  {
    // A sticky limit trip latches a poison boundary: at or past the durable
    // frontier, stop without rebuilding a lexer, mirroring the short-circuit in
    // `next()`; strictly before it, lexing proceeds (replaying a drained prefix). A
    // scan that finds no matching token yields `Ok(None)`, the poisoned outcome too.
    if self.reached_boundary(self.offset()) {
      return Ok(None);
    }

    let mut resume = self.resume();

    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => match pred(tok.as_ref()) {
        Some(output) => {
          self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
          output.map(|o| Some((o, tok)))
        }
        None => {
          let (span, tok) = tok.into_components();
          // put back the token into cache as it was peeked
          let ct = CachedToken::new(Spanned::new(span, tok), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(None)
        }
      },
      Scan::Tripped | Scan::Eof => Ok(None),
    }
  }

  #[inline]
  fn try_expect_on_input<F>(
    &mut self,
    mut pred: F,
  ) -> Result<Option<Spanned<L::Token, L::Span>>, <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error>
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> bool,
  {
    // A sticky limit trip latches a poison boundary: at or past the durable
    // frontier, stop without rebuilding a lexer, mirroring the short-circuit in
    // `next()`; strictly before it, lexing proceeds (replaying a drained prefix). A
    // scan that finds no matching token yields `Ok(None)`, the poisoned outcome too.
    if self.reached_boundary(self.offset()) {
      return Ok(None);
    }

    let mut resume = self.resume();

    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        if pred(tok.as_ref()) {
          self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
          Ok(Some(tok))
        } else {
          let (span, tok) = tok.into_components();
          // put back the token into cache as it was peeked
          let ct = CachedToken::new(Spanned::new(span, tok), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(None)
        }
      }
      Scan::Tripped | Scan::Eof => Ok(None),
    }
  }

  #[inline]
  fn try_expect_map_on_input<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> Option<O>,
  {
    // A sticky limit trip latches a poison boundary: at or past the durable
    // frontier, stop without rebuilding a lexer, mirroring the short-circuit in
    // `next()`; strictly before it, lexing proceeds (replaying a drained prefix). A
    // scan that finds no matching token yields `Ok(None)`, the poisoned outcome too.
    if self.reached_boundary(self.offset()) {
      return Ok(None);
    }

    let mut resume = self.resume();

    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => {
        if let Some(out) = pred(tok.as_ref()) {
          self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
          Ok(Some((out, tok)))
        } else {
          let (span, tok) = tok.into_components();
          // put back the token into cache as it was peeked
          let ct = CachedToken::new(Spanned::new(span, tok), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(None)
        }
      }
      Scan::Tripped | Scan::Eof => Ok(None),
    }
  }

  /// The cache-empty scan path of [`try_expect_map_or_stop`](Self::try_expect_map_or_stop), split out to mirror `try_expect_map`'s shape and forced inline alongside it into the token-pratt hot loop.
  #[inline(always)]
  fn try_expect_map_or_stop_on_input<O, F>(
    &mut self,
    mut pred: F,
  ) -> Result<
    Option<(O, Spanned<L::Token, L::Span>)>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error,
  >
  where
    F: FnMut(Spanned<&L::Token, &L::Span>) -> Option<O>,
    <Ctx::Emitter as Emitter<'inp, L, Lang>>::Error: From<UnexpectedEot<L::Offset, Lang>>,
  {
    // An already-latched poison boundary at the cursor is a terminal stop, not proof
    // of absence — mirrors `try_expect_or_stop`'s E4.
    if self.reached_boundary(self.offset()) {
      return Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      );
    }
    let mut resume = self.resume();
    match self.scan_with(resume.parts_mut(), &AtCursor)? {
      Scan::Token(tok) => match pred(tok.as_ref()) {
        Some(output) => {
          self.commit_token(tok.data(), tok.span_ref(), resume.into_lexer().into_state());
          Ok(Some((output, tok)))
        }
        None => {
          let (span, t) = tok.into_components();
          let ct = CachedToken::new(Spanned::new(span, t), resume.lexer().state().clone());
          self.hold_front(ct, Origin::Lexer);
          Ok(None) // definite absence — the token stays at the cache front.
        }
      },
      // A fresh trip whose diagnostic a recovering emitter accepted, marked terminal so
      // recovery re-raises it — mirrors `try_expect_or_stop`'s E3.
      Scan::Tripped => Err(
        UnexpectedEot::eot_of(self.span().end())
          .into_terminal()
          .into(),
      ),
      Scan::Eof => {
        // An exhaustion produced by refusing to cross a pre-latched boundary is terminal,
        // not genuine end of input — mirrors `try_expect_or_stop`'s E5.
        if self.reached_boundary(resume.at()) {
          Err(
            UnexpectedEot::eot_of(self.span().end())
              .into_terminal()
              .into(),
          )
        } else {
          Ok(None)
        }
      }
    }
  }
}
