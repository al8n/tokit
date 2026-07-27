#!/usr/bin/env python3
"""mdbook preprocessor: turn the guide's rustdoc intra-doc links into links the book can follow.

The guide chapters are the single source for both rustdoc and this book, so their API links are
written as rustdoc intra-doc links (`[`InputRef`](crate::InputRef)`). rustdoc resolves and
validates those under `-D warnings`; mdbook would render them as dead links. This rewrites them:

  crate::guide::chNN_x, super::chNN_x  ->  chNN_x.md            (sibling chapter, in-book)
  everything else                      ->  a docs.rs URL

Every other target must appear in DOCS_RS_MAP below. An unmapped `crate::` target FAILS THE BUILD
rather than shipping a dead link -- so a new link in the guide forces a new map entry here.

The map is not guesswork: each value was resolved against real `cargo doc` output and accepted
only if it matched an href rustdoc itself emitted for that link, which is why re-exports point at
their canonical module (`crate::InputRef` -> `input/struct.InputRef.html`, *not*
`struct.InputRef.html`) and required trait methods use `#tymethod.` rather than `#method.`.

To refresh after an API move: RUSTDOCFLAGS= cargo doc -p tokora --all-features --no-deps, then
compare the hrefs under target/doc/tokora/guide/*/index.html with the values below.
"""

import json
import re
import sys

DOCS_RS = "https://docs.rs/tokora/latest/tokora/"

# rustdoc-verified: crate:: path -> page (+anchor) under https://docs.rs/tokora/latest/tokora/.
# An absolute http(s) value is used verbatim.
DOCS_RS_MAP = {
  "crate::Accumulator::collect": "trait.Accumulator.html#method.collect",
  "crate::Accumulator::collect_with": "trait.Accumulator.html#method.collect_with",
  "crate::Balance": "input/enum.Balance.html",
  "crate::Branch": "struct.Branch.html",
  "crate::Cache": "cache/trait.Cache.html",
  "crate::Check": "trait.Check.html",
  "crate::Complete": "input/struct.Complete.html",
  "crate::Completeness": "input/trait.Completeness.html",
  "crate::DelimClass": "input/trait.DelimClass.html",
  "crate::DropPolicy": "input/trait.DropPolicy.html",
  "crate::Emitter": "emitter/trait.Emitter.html",
  "crate::Emitter::checkpoint": "emitter/trait.Emitter.html#method.checkpoint",
  "crate::Emitter::commit_token": "emitter/trait.Emitter.html#method.commit_token",
  "crate::Emitter::emit_error": "emitter/trait.Emitter.html#tymethod.emit_error",
  "crate::Emitter::emit_skipped_region": "emitter/trait.Emitter.html#method.emit_skipped_region",
  "crate::Emitter::emit_warning": "emitter/trait.Emitter.html#method.emit_warning",
  "crate::Emitter::release": "emitter/trait.Emitter.html#method.release",
  "crate::Emitter::rewind": "emitter/trait.Emitter.html#tymethod.rewind",
  "crate::ErrorOf": "type.ErrorOf.html",
  "crate::FatalContext": "type.FatalContext.html",
  "crate::Hole": "input/struct.Hole.html",
  "crate::Hole::skipped": "input/struct.Hole.html#method.skipped",
  "crate::Hole::span": "input/struct.Hole.html#method.span",
  "crate::InputRef": "input/struct.InputRef.html",
  "crate::InputRef::attempt": "input/struct.InputRef.html#method.attempt",
  "crate::InputRef::begin": "input/struct.InputRef.html#method.begin",
  "crate::InputRef::begin_point": "input/struct.InputRef.html#method.begin_point",
  "crate::InputRef::begin_stacked": "input/struct.InputRef.html#method.begin_stacked",
  "crate::InputRef::begin_with": "input/struct.InputRef.html#method.begin_with",
  "crate::InputRef::commit_point": "input/struct.InputRef.html#method.commit_point",
  "crate::InputRef::cursor": "input/struct.InputRef.html#method.cursor",
  "crate::InputRef::emitter": "input/struct.InputRef.html#method.emitter",
  "crate::InputRef::next": "input/struct.InputRef.html#method.next",
  "crate::InputRef::peek": "input/struct.InputRef.html#method.peek",
  "crate::InputRef::points": "input/struct.InputRef.html#method.points",
  "crate::InputRef::pratt": "input/struct.InputRef.html#method.pratt",
  "crate::InputRef::pratt_with_min_precedence": "input/struct.InputRef.html#method.pratt_with_min_precedence",
  "crate::InputRef::rollback_point": "input/struct.InputRef.html#method.rollback_point",
  "crate::InputRef::set_final": "input/struct.InputRef.html#method.set_final",
  "crate::InputRef::skip_while": "input/struct.InputRef.html#method.skip_while",
  "crate::InputRef::slice": "input/struct.InputRef.html#method.slice",
  "crate::InputRef::span": "input/struct.InputRef.html#method.span",
  "crate::InputRef::sync_balanced": "input/struct.InputRef.html#method.sync_balanced",
  "crate::InputRef::try_attempt": "input/struct.InputRef.html#method.try_attempt",
  "crate::InputRef::try_expect": "input/struct.InputRef.html#method.try_expect",
  "crate::Lexer": "lexer/trait.Lexer.html",
  "crate::Lexer#state-faithfulness-and-cheapness": "lexer/trait.Lexer.html#state-faithfulness-and-cheapness",
  "crate::Lexer#the-lexer-contract": "lexer/trait.Lexer.html#the-lexer-contract",
  "crate::Lexer::State": "lexer/trait.Lexer.html#associatedtype.State",
  "crate::Lexer::bump": "lexer/trait.Lexer.html#tymethod.bump",
  "crate::Lexer::check": "lexer/trait.Lexer.html#tymethod.check",
  "crate::Lexer::lex": "lexer/trait.Lexer.html#tymethod.lex",
  "crate::Lexer::new": "lexer/trait.Lexer.html#tymethod.new",
  "crate::Lexer::slice": "lexer/trait.Lexer.html#tymethod.slice",
  "crate::Lexer::span": "lexer/trait.Lexer.html#tymethod.span",
  "crate::Located": "struct.Located.html",
  "crate::Parse": "parser/trait.Parse.html",
  "crate::Parse::parse": "parser/trait.Parse.html#method.parse",
  "crate::Parse::parse_bstr": "parser/trait.Parse.html#method.parse_bstr",
  "crate::Parse::parse_bytes": "parser/trait.Parse.html#method.parse_bytes",
  "crate::Parse::parse_hipstr": "parser/trait.Parse.html#method.parse_hipstr",
  "crate::Parse::parse_slice": "parser/trait.Parse.html#method.parse_slice",
  "crate::Parse::parse_str": "parser/trait.Parse.html#method.parse_str",
  "crate::Parse::parse_with_state": "parser/trait.Parse.html#tymethod.parse_with_state",
  "crate::ParseChoice": "trait.ParseChoice.html",
  "crate::ParseChoice::dispatch_on_kind": "trait.ParseChoice.html#method.dispatch_on_kind",
  "crate::ParseChoice::peek_then_choice": "trait.ParseChoice.html#method.peek_then_choice",
  "crate::ParseChoice::peek_then_try_choice": "trait.ParseChoice.html#method.peek_then_try_choice",
  "crate::ParseContext": "trait.ParseContext.html",
  "crate::ComposableParseContext": "trait.ComposableParseContext.html",
  "crate::ParseInput": "trait.ParseInput.html",
  "crate::ParseInput::and_then": "trait.ParseInput.html#method.and_then",
  "crate::ParseInput::by_ref": "trait.ParseInput.html#method.by_ref",
  "crate::ParseInput::filter": "trait.ParseInput.html#method.filter",
  "crate::ParseInput::filter_map": "trait.ParseInput.html#method.filter_map",
  "crate::ParseInput::fold_while": "trait.ParseInput.html#method.fold_while",
  "crate::ParseInput::ignore_then": "trait.ParseInput.html#method.ignore_then",
  "crate::ParseInput::ignored": "trait.ParseInput.html#method.ignored",
  "crate::ParseInput::inplace_recover": "trait.ParseInput.html#method.inplace_recover",
  "crate::ParseInput::located": "trait.ParseInput.html#method.located",
  "crate::ParseInput::map": "trait.ParseInput.html#method.map",
  "crate::ParseInput::padded": "trait.ParseInput.html#method.padded",
  "crate::ParseInput::parse_input": "trait.ParseInput.html#tymethod.parse_input",
  "crate::ParseInput::recover": "trait.ParseInput.html#method.recover",
  "crate::ParseInput::repeated_while": "trait.ParseInput.html#method.repeated_while",
  "crate::ParseInput::separated_by_comma_while": "trait.ParseInput.html#method.separated_by_comma_while",
  "crate::ParseInput::separated_while": "trait.ParseInput.html#method.separated_while",
  "crate::ParseInput::skip_then_retry": "trait.ParseInput.html#method.skip_then_retry",
  "crate::ParseInput::sliced": "trait.ParseInput.html#method.sliced",
  "crate::ParseInput::spanned": "trait.ParseInput.html#method.spanned",
  "crate::ParseInput::then": "trait.ParseInput.html#method.then",
  "crate::ParseInput::then_ignore": "trait.ParseInput.html#method.then_ignore",
  "crate::ParseInput::then_value": "trait.ParseInput.html#method.then_value",
  "crate::ParseInput::validate": "trait.ParseInput.html#method.validate",
  "crate::ParseInputUnwrapExt::unwrap": "trait.ParseInputUnwrapExt.html#method.unwrap",
  "crate::ParseState": "struct.ParseState.html",
  "crate::ParseTokenChoice": "trait.ParseTokenChoice.html",
  "crate::ParseTokenChoice::fused_dispatch_on_kind": "trait.ParseTokenChoice.html#method.fused_dispatch_on_kind",
  "crate::Parser": "parser/struct.Parser.html",
  "crate::Parser::apply": "parser/struct.Parser.html#method.apply",
  "crate::Parser::new": "parser/struct.Parser.html#method.new",
  "crate::Partial": "input/struct.Partial.html",
  "crate::SurfaceIncomplete": "input/trait.SurfaceIncomplete.html",
  "crate::Require": "trait.Require.html",
  "crate::SavepointId": "input/struct.SavepointId.html",
  "crate::SimpleSpan": "span/struct.SimpleSpan.html",
  "crate::Slice": "slice/trait.Slice.html",
  "crate::SliceOf": "lexer/type.SliceOf.html",
  "crate::Source": "source/trait.Source.html",
  "crate::Span": "span/trait.Span.html",
  "crate::StackedTransaction": "input/struct.StackedTransaction.html",
  "crate::StackedTransaction::release": "input/struct.StackedTransaction.html#method.release",
  "crate::StackedTransaction::rollback_to": "input/struct.StackedTransaction.html#method.rollback_to",
  "crate::StackedTransaction::savepoint": "input/struct.StackedTransaction.html#method.savepoint",
  "crate::State": "state/trait.State.html",
  "crate::State::check": "state/trait.State.html#tymethod.check",
  "crate::Token": "token/trait.Token.html",
  "crate::Token::Kind": "token/trait.Token.html#associatedtype.Kind",
  "crate::Token::SURFACES_TRIVIA": "token/trait.Token.html#associatedconstant.SURFACES_TRIVIA",
  "crate::Token::is_trivia": "token/trait.Token.html#tymethod.is_trivia",
  "crate::Transaction": "input/struct.Transaction.html",
  "crate::Transaction::commit": "input/struct.Transaction.html#method.commit",
  "crate::Transaction::rollback": "input/struct.Transaction.html#method.rollback",
  "crate::TryParseInput": "try_parse_input/trait.TryParseInput.html",
  "crate::TryParseInput::repeated": "try_parse_input/trait.TryParseInput.html#method.repeated",
  "crate::TryParseInput::separated": "try_parse_input/trait.TryParseInput.html#method.separated",
  "crate::TryParseInput::separated_by_comma": "try_parse_input/trait.TryParseInput.html#method.separated_by_comma",
  "crate::Window": "trait.Window.html",
  "crate::cache::Peeked": "cache/type.Peeked.html",
  "crate::conformance": "conformance/index.html",
  "crate::conformance::Harness": "conformance/struct.Harness.html",
  "crate::conformance::Harness::lossless": "conformance/struct.Harness.html#method.lossless",
  "crate::conformance::Harness::new": "conformance/struct.Harness.html#method.new",
  "crate::conformance::Harness::over": "conformance/struct.Harness.html#method.over",
  "crate::conformance::Harness::run": "conformance/struct.Harness.html#method.run",
  "crate::conformance::Harness::run_partial": "conformance/struct.Harness.html#method.run_partial",
  "crate::container::Container": "container/trait.Container.html",
  "crate::cst": "cst/index.html",
  "crate::cst::Element": "cst/trait.Element.html",
  "crate::cst::FinishError": "cst/enum.FinishError.html",
  "crate::cst::FinishError::UncoveredGap": "cst/enum.FinishError.html#variant.UncoveredGap",
  "crate::cst::Node": "cst/trait.Node.html",
  "crate::cst::Sink": "cst/struct.Sink.html",
  "crate::cst::Sink::finish": "cst/struct.Sink.html#method.finish",
  "crate::cst::Sink::finish_partial": "cst/struct.Sink.html#method.finish_partial",
  "crate::cst::Sink::inner_ref": "cst/struct.Sink.html#method.inner_ref",
  "crate::cst::Sink::new": "cst/struct.Sink.html#method.new",
  "crate::cst::SyntaxTreeBuilder": "cst/struct.SyntaxTreeBuilder.html",
  "crate::cst::SyntaxTreeBuilder::checkpoint": "cst/struct.SyntaxTreeBuilder.html#method.checkpoint",
  "crate::cst::SyntaxTreeBuilder::finish": "cst/struct.SyntaxTreeBuilder.html#method.finish",
  "crate::cst::SyntaxTreeBuilder::finish_node": "cst/struct.SyntaxTreeBuilder.html#method.finish_node",
  "crate::cst::SyntaxTreeBuilder::new": "cst/struct.SyntaxTreeBuilder.html#method.new",
  "crate::cst::SyntaxTreeBuilder::start_node": "cst/struct.SyntaxTreeBuilder.html#method.start_node",
  "crate::cst::SyntaxTreeBuilder::start_node_at": "cst/struct.SyntaxTreeBuilder.html#method.start_node_at",
  "crate::cst::SyntaxTreeBuilder::token": "cst/struct.SyntaxTreeBuilder.html#method.token",
  "crate::cst::Token": "cst/trait.Token.html",
  "crate::cst::TriviaPolicy": "cst/enum.TriviaPolicy.html",
  "crate::cst::cast": "cst/cast/index.html",
  "crate::cst::event": "cst/event/index.html",
  "crate::cst::event::CompletedMarker": "cst/event/struct.CompletedMarker.html",
  "crate::cst::event::EventMark": "cst/event/struct.EventMark.html",
  "crate::cst::event::Marker": "cst/event/struct.Marker.html",
  "crate::cst::event::TOMBSTONE": "cst/event/constant.TOMBSTONE.html",
  "crate::delimiter::Delimiter": "delimiter/trait.Delimiter.html",
  "crate::delimiter::TypedDelimiter": "delimiter/trait.TypedDelimiter.html",
  "crate::emitter": "emitter/index.html",
  "crate::emitter::ComposableEmitter": "emitter/trait.ComposableEmitter.html",
  "crate::emitter::CstEmitter": "emitter/trait.CstEmitter.html",
  "crate::emitter::CstEmitter::cst_mark": "emitter/trait.CstEmitter.html#method.cst_mark",
  "crate::emitter::CstEmitter::cst_start_at": "emitter/trait.CstEmitter.html#method.cst_start_at",
  "crate::emitter::Diagnostic": "emitter/struct.Diagnostic.html",
  "crate::emitter::DiagnosticKind": "emitter/enum.DiagnosticKind.html",
  "crate::emitter::Diagnostics": "emitter/struct.Diagnostics.html",
  "crate::emitter::Fatal": "emitter/struct.Fatal.html",
  "crate::emitter::FromEmitterError": "emitter/trait.FromEmitterError.html",
  "crate::emitter::FullContainerEmitter": "emitter/trait.FullContainerEmitter.html",
  "crate::emitter::Ignored": "emitter/type.Ignored.html",
  "crate::emitter::MissingLeadingSeparatorEmitter": "emitter/trait.MissingLeadingSeparatorEmitter.html",
  "crate::emitter::MissingTrailingSeparatorEmitter": "emitter/trait.MissingTrailingSeparatorEmitter.html",
  "crate::emitter::PrattEmitter": "emitter/trait.PrattEmitter.html",
  "crate::emitter::SeparatedEmitter": "emitter/trait.SeparatedEmitter.html",
  "crate::emitter::Severity": "emitter/enum.Severity.html",
  "crate::emitter::Silent": "emitter/struct.Silent.html",
  "crate::emitter::TooFewEmitter": "emitter/trait.TooFewEmitter.html",
  "crate::emitter::TooManyEmitter": "emitter/trait.TooManyEmitter.html",
  "crate::emitter::UnexpectedLeadingSeparatorEmitter": "emitter/trait.UnexpectedLeadingSeparatorEmitter.html",
  "crate::emitter::UnexpectedTrailingSeparatorEmitter": "emitter/trait.UnexpectedTrailingSeparatorEmitter.html",
  "crate::emitter::UnclosedEmitter": "emitter/trait.UnclosedEmitter.html",
  "crate::emitter::Verbose": "emitter/struct.Verbose.html",
  "crate::emitter::Verbose::diagnostics": "emitter/struct.Verbose.html#method.diagnostics",
  "crate::emitter::Verbose::errors": "emitter/struct.Verbose.html#method.errors",
  "crate::emitter::Verbose::labels": "emitter/struct.Verbose.html#method.labels",
  "crate::emitter::Verbose::skipped_regions": "emitter/struct.Verbose.html#method.skipped_regions",
  "crate::emitter::Verbose::warnings": "emitter/struct.Verbose.html#method.warnings",
  "crate::error::ErrorNode": "error/trait.ErrorNode.html",
  "crate::error::Incomplete": "error/struct.Incomplete.html",
  "crate::error::IncompleteSyntax": "error/struct.IncompleteSyntax.html",
  "crate::error::IncompleteSyntax::new": "error/struct.IncompleteSyntax.html#method.new",
  "crate::error::IncompleteSyntax::push": "error/struct.IncompleteSyntax.html#method.push",
  "crate::error::Invalid": "error/struct.Invalid.html",
  "crate::error::Malformed": "error/struct.Malformed.html",
  "crate::error::MaybeIncomplete": "error/trait.MaybeIncomplete.html",
  "crate::error::MaybeIncomplete::is_incomplete": "error/trait.MaybeIncomplete.html#method.is_incomplete",
  "crate::error::Unclosed": "error/struct.Unclosed.html",
  "crate::error::Undelimited": "error/struct.Undelimited.html",
  "crate::error::UnexpectedEnd": "error/struct.UnexpectedEnd.html",
  "crate::error::UnexpectedEnd::eolhs_of": "error/struct.UnexpectedEnd.html#method.eolhs_of",
  "crate::error::UnexpectedEnd::eorhs_of": "error/struct.UnexpectedEnd.html#method.eorhs_of",
  "crate::error::UnexpectedEoLhs": "error/type.UnexpectedEoLhs.html",
  "crate::error::UnexpectedEoRhs": "error/type.UnexpectedEoRhs.html",
  "crate::error::UnexpectedEot": "error/type.UnexpectedEot.html",
  "crate::error::UnknownLexeme": "error/struct.UnknownLexeme.html",
  "crate::error::Unopened": "error/struct.Unopened.html",
  "crate::error::Unterminated": "error/struct.Unterminated.html",
  "crate::error::syntax": "error/syntax/index.html",
  "crate::error::syntax::FullContainer": "error/syntax/struct.FullContainer.html",
  "crate::error::syntax::MissingSyntax": "error/syntax/struct.MissingSyntax.html",
  "crate::error::syntax::TooFew": "error/syntax/struct.TooFew.html",
  "crate::error::syntax::TooMany": "error/syntax/struct.TooMany.html",
  "crate::error::token": "error/token/index.html",
  "crate::error::token::MissingToken": "error/token/struct.MissingToken.html",
  "crate::error::token::SeparatedError": "error/token/struct.SeparatedError.html",
  "crate::error::token::UnexpectedToken": "error/token/struct.UnexpectedToken.html",
  "crate::error::token::UnexpectedTokenOf": "error/token/type.UnexpectedTokenOf.html",
  "crate::fuzz": "fuzz/index.html",
  "crate::fuzz::Case": "fuzz/struct.Case.html",
  "crate::fuzz::run_case": "fuzz/fn.run_case.html",
  "crate::fuzz::run_seeds": "fuzz/fn.run_seeds.html",
  "crate::input": "input/index.html",
  "crate::input#the-sans-io-resumption-loop": "input/index.html#the-sans-io-resumption-loop",
  "crate::input::Checkpoint": "input/struct.Checkpoint.html",
  "crate::keyword": "macro.keyword.html",
  "crate::labelled": "parser/fn.labelled.html",
  "crate::lexer::LogosLexer": "lexer/struct.LogosLexer.html",
  "crate::lexer::SliceOf": "lexer/type.SliceOf.html",
  # `pub use logos_0_16 as logos;` is an extern-crate re-export: rustdoc emits no page for it, so
  # point at the crate the prose actually means.
  "crate::logos": "https://docs.rs/logos/latest/logos/",
  "crate::parse_partial": "input/fn.parse_partial.html",
  "crate::parser": "parser/index.html",
  "crate::parser::Action": "parser/enum.Action.html",
  "crate::parser::Any": "parser/struct.Any.html",
  "crate::parser::Any::new": "parser/struct.Any.html#method.new",
  "crate::parser::DelimiterHandler": "parser/trait.DelimiterHandler.html",
  "crate::parser::DispatchOnKind": "parser/struct.DispatchOnKind.html",
  "crate::parser::DispatchOnKind#performance-keep-token-kind-discriminants-dense": "parser/struct.DispatchOnKind.html#performance-keep-token-kind-discriminants-dense",
  "crate::parser::Empty": "parser/struct.Empty.html",
  "crate::parser::FusedDispatchOnKind": "parser/struct.FusedDispatchOnKind.html",
  "crate::parser::NoCst": "parser/struct.NoCst.html",
  "crate::parser::ParsePrattLHS": "parser/trait.ParsePrattLHS.html",
  "crate::parser::ParsePrattRHS": "parser/trait.ParsePrattRHS.html",
  "crate::parser::Parser::new": "parser/struct.Parser.html#method.new",
  "crate::parser::Pratt": "parser/struct.Pratt.html",
  "crate::parser::Pratt::with_cst_kinds": "parser/struct.Pratt.html#method.with_cst_kinds",
  "crate::parser::PrattInfix": "parser/enum.PrattInfix.html",
  "crate::parser::PrattLHS": "parser/enum.PrattLHS.html",
  "crate::parser::PrattPower": "parser/trait.PrattPower.html",
  "crate::parser::PrattPower::next": "parser/trait.PrattPower.html#tymethod.next",
  "crate::parser::PrattPower::prev": "parser/trait.PrattPower.html#tymethod.prev",
  "crate::parser::PrattRHS": "parser/enum.PrattRHS.html",
  "crate::parser::Precedenced": "parser/struct.Precedenced.html",
  "crate::parser::Recover": "parser/struct.Recover.html",
  "crate::parser::Repeated": "parser/struct.Repeated.html",
  "crate::parser::Separated": "parser/struct.Separated.html",
  "crate::parser::Separated::allow_leading": "parser/struct.Separated.html#method.allow_leading",
  "crate::parser::Separated::allow_trailing": "parser/struct.Separated.html#method.allow_trailing",
  "crate::parser::Separated::at_least": "parser/struct.Separated.html#method.at_least",
  "crate::parser::Separated::at_most": "parser/struct.Separated.html#method.at_most",
  "crate::parser::Separated::bounded": "parser/struct.Separated.html#method.bounded",
  "crate::parser::Separated::delimited": "parser/struct.Separated.html#method.delimited",
  "crate::parser::Separated::require_leading": "parser/struct.Separated.html#method.require_leading",
  "crate::parser::Separated::require_trailing": "parser/struct.Separated.html#method.require_trailing",
  "crate::parser::SeparatedWhile": "parser/struct.SeparatedWhile.html",
  "crate::parser::SeparatorHandler": "parser/trait.SeparatorHandler.html",
  "crate::parser::Todo": "parser/struct.Todo.html",
  "crate::parser::WithCstKinds": "parser/struct.WithCstKinds.html",
  "crate::parser::angles": "parser/fn.angles.html",
  "crate::parser::braces": "parser/fn.braces.html",
  "crate::parser::brackets": "parser/fn.brackets.html",
  "crate::parser::delimited": "parser/fn.delimited.html",
  "crate::parser::expect": "parser/fn.expect.html",
  "crate::parser::expect_of": "parser/fn.expect_of.html",
  "crate::parser::fail": "parser/fn.fail.html",
  "crate::parser::list_of": "parser/fn.list_of.html",
  "crate::parser::node": "parser/fn.node.html",
  "crate::parser::node(": "parser/fn.node.html",
  "crate::parser::node_at": "parser/fn.node_at.html",
  "crate::parser::node_at(": "parser/fn.node_at.html",
  "crate::parser::node_opt": "parser/fn.node_opt.html",
  "crate::parser::node_opt(": "parser/fn.node_opt.html",
  "crate::parser::opt": "parser/fn.opt.html",
  "crate::parser::parens": "parser/fn.parens.html",
  "crate::parser::pratt": "parser/fn.pratt.html",
  "crate::parser::pratt_of": "parser/fn.pratt_of.html",
  "crate::parser::separated1": "parser/fn.separated1.html",
  "crate::parser::try_angles": "parser/fn.try_angles.html",
  "crate::parser::try_braces": "parser/fn.try_braces.html",
  "crate::parser::try_brackets": "parser/fn.try_brackets.html",
  "crate::parser::try_delimited": "parser/fn.try_delimited.html",
  "crate::parser::try_expect": "parser/fn.try_expect.html",
  "crate::parser::try_ident_list": "parser/fn.try_ident_list.html",
  "crate::parser::try_parens": "parser/fn.try_parens.html",
  "crate::punct": "punct/index.html",
  "crate::punct::Angle": "punct/struct.Angle.html",
  "crate::punct::Brace": "punct/struct.Brace.html",
  "crate::punct::Bracket": "punct/struct.Bracket.html",
  "crate::punct::Colon": "punct/struct.Colon.html",
  "crate::punct::Comma": "punct/struct.Comma.html",
  "crate::punct::Paren": "punct/struct.Paren.html",
  "crate::punct::Punctuator": "punct/trait.Punctuator.html",
  "crate::punctuator": "macro.punctuator.html",
  "crate::slice::Sliced": "slice/struct.Sliced.html",
  "crate::span::AsSpan": "span/trait.AsSpan.html",
  "crate::span::IntoSpan": "span/trait.IntoSpan.html",
  "crate::span::Spanned": "span/struct.Spanned.html",
  "crate::syntax": "syntax/index.html",
  "crate::syntax::AstNode": "syntax/trait.AstNode.html",
  "crate::syntax::Language": "syntax/trait.Language.html",
  "crate::token::IdentifierToken": "token/trait.IdentifierToken.html",
  "crate::token::KeywordToken": "token/trait.KeywordToken.html",
  "crate::token::KeywordToken::keyword": "token/trait.KeywordToken.html#tymethod.keyword",
  "crate::token::LitToken": "token/trait.LitToken.html",
  "crate::token::PrattToken": "token/trait.PrattToken.html",
  "crate::token::PunctuatorToken": "token/trait.PunctuatorToken.html",
  "crate::traced": "fn.traced.html",
  "crate::try_parse_input": "try_parse_input/index.html",
  "crate::try_parse_input::ParseAttempt": "try_parse_input/enum.ParseAttempt.html",
  "crate::try_parse_input::TryParseInput::accepted": "try_parse_input/trait.TryParseInput.html#method.accepted",
  "crate::try_parse_input::TryParseInput::fold": "try_parse_input/trait.TryParseInput.html#method.fold",
  "crate::try_parse_input::TryParseInput::repeated": "try_parse_input/trait.TryParseInput.html#method.repeated",
  "crate::try_parse_input::TryParseInput::rfold": "try_parse_input/trait.TryParseInput.html#method.rfold",
  "crate::try_parse_input::TryParseInput::separated": "try_parse_input/trait.TryParseInput.html#method.separated",
  "crate::try_parse_input::TryParseInput::separated_by_comma": "try_parse_input/trait.TryParseInput.html#method.separated_by_comma",
  "crate::try_parse_input::TryParseInput::try_fold": "try_parse_input/trait.TryParseInput.html#method.try_fold",
  "crate::types": "types/index.html",
  "crate::types::Ident": "types/struct.Ident.html",
  "crate::types::IdentList": "types/struct.IdentList.html",
  "crate::types::Keyword": "types/struct.Keyword.html",
  "crate::types::Lit": "types/struct.Lit.html",
  "crate::types::LitBinary": "types/struct.LitBinary.html",
  "crate::types::LitBool": "types/struct.LitBool.html",
  "crate::types::LitByte": "types/struct.LitByte.html",
  "crate::types::LitByteString": "types/struct.LitByteString.html",
  "crate::types::LitChar": "types/struct.LitChar.html",
  "crate::types::LitDecimal": "types/struct.LitDecimal.html",
  "crate::types::LitFalse": "types/struct.LitFalse.html",
  "crate::types::LitFloat": "types/struct.LitFloat.html",
  "crate::types::LitHex": "types/struct.LitHex.html",
  "crate::types::LitHexFloat": "types/struct.LitHexFloat.html",
  "crate::types::LitMultilineString": "types/struct.LitMultilineString.html",
  "crate::types::LitNull": "types/struct.LitNull.html",
  "crate::types::LitOctal": "types/struct.LitOctal.html",
  "crate::types::LitRawString": "types/struct.LitRawString.html",
  "crate::types::LitString": "types/struct.LitString.html",
  "crate::types::LitTrue": "types/struct.LitTrue.html",
  "crate::types::Recoverable": "types/enum.Recoverable.html",
  "crate::utils::CharLen": "utils/trait.CharLen.html",
  "crate::utils::CowStr": "utils/struct.CowStr.html",
  "crate::utils::Delimited": "utils/struct.Delimited.html",
  "crate::utils::EscapedLexeme": "utils/struct.EscapedLexeme.html",
  "crate::utils::Expected": "utils/enum.Expected.html",
  "crate::utils::Expected::one": "utils/enum.Expected.html#method.one",
  "crate::utils::Expected::one_of": "utils/enum.Expected.html#method.one_of",
  "crate::utils::GenericArrayDeque": "utils/struct.GenericArrayDeque.html",
  "crate::utils::IntoComponents": "utils/trait.IntoComponents.html",
  "crate::utils::IsAsciiChar": "utils/trait.IsAsciiChar.html",
  "crate::utils::Lexeme": "utils/enum.Lexeme.html",
  "crate::utils::MultiCharEscape": "utils/struct.MultiCharEscape.html",
  "crate::utils::OneOf": "utils/struct.OneOf.html",
  "crate::utils::PositionedChar": "utils/struct.PositionedChar.html",
  "crate::utils::SingleCharEscape": "utils/struct.SingleCharEscape.html",
  "crate::utils::human_display": "utils/human_display/index.html",
  "crate::utils::sdl_display": "utils/sdl_display/index.html",
  "crate::utils::syntax_tree_display": "utils/syntax_tree_display/index.html",
  # `pub use typenum` is an extern-crate re-export: rustdoc emits no page for it, so point at the
  # crate the prose actually means.
  "crate::utils::typenum": "https://docs.rs/typenum/latest/typenum/",
}

# `[text](crate::path)` / `[text](super::path)` -- the two intra-doc link spellings the guide uses.
LINK = re.compile(r"\[([^\]]*)\]\((crate::[^)\s]+|super::[^)\s]+)\)")
# A guide chapter, addressed either absolutely or from a sibling chapter's own module scope.
CHAPTER = re.compile(r"^(?:crate::guide::|super::)((?:ch\d{2}|arch|ref|recipe)_\w+)$")

errors = set()


def rewrite(target: str) -> str:
  chapter = CHAPTER.match(target)
  if chapter:
    return f"{chapter.group(1)}.md"
  page = DOCS_RS_MAP.get(target)
  if page is None:
    errors.add(target)
    return target
  return page if page.startswith("http") else DOCS_RS + page


def convert(content: str) -> str:
  out = LINK.sub(lambda m: f"[{m.group(1)}]({rewrite(m.group(2))})", content)
  # Belt and braces: nothing of the intra-doc form may survive into the rendered book.
  errors.update(re.findall(r"\]\(((?:crate|super)::[^)\s]+)\)", out))
  return out


def walk(items):
  for item in items:
    chapter = item.get("Chapter") if isinstance(item, dict) else None
    if chapter is None:
      continue  # Separator / PartTitle
    chapter["content"] = convert(chapter["content"])
    walk(chapter.get("sub_items") or chapter.get("items") or [])


def main() -> int:
  if len(sys.argv) > 2 and sys.argv[1] == "supports":
    return 0  # every renderer

  _context, book = json.load(sys.stdin)
  # mdbook >= 0.5 calls the top-level list `items`; 0.4 called it `sections`.
  items = book.get("items")
  if items is None:
    items = book.get("sections")
  if items is None:
    print(f"mdbook_docs_links: unrecognised book shape: {sorted(book)}", file=sys.stderr)
    return 1
  walk(items)

  if errors:
    print(
      "mdbook_docs_links: the guide links to items with no entry in DOCS_RS_MAP, which would "
      "render as dead links in the book. Add them to tokora/tools/mdbook_docs_links.py:",
      file=sys.stderr,
    )
    for target in sorted(set(errors)):
      print(f"  {target}", file=sys.stderr)
    return 1

  json.dump(book, sys.stdout)
  return 0


if __name__ == "__main__":
  sys.exit(main())
