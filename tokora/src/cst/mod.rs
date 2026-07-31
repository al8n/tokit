//! Concrete Syntax Tree (CST) utilities: a rowan-free event vocabulary, and — under the
//! `rowan` feature — the typed-tree infrastructure built on top of
//! [rowan](https://docs.rs/rowan).
//!
//! This module provides infrastructure for building and working with typed concrete syntax trees.
//! Unlike Abstract Syntax Trees (ASTs), CSTs preserve all source information including whitespace,
//! comments, and exact token positions, making them ideal for:
//!
//! - **Code formatters**: Preserve exact formatting and whitespace
//! - **Linters**: Access complete source information for analysis
//! - **Language servers**: Provide accurate position information for IDE features
//! - **Refactoring tools**: Transform code while preserving formatting
//! - **Documentation generators**: Extract and preserve comments
//!
//! # Architecture
//!
//! Tree support is a **flat event stream that rides the emitter**: the parser records events
//! (through the [`CstEmitter`](crate::emitter::CstEmitter) capability subtrait), and the tree
//! is *derived* from the surviving events exactly once — never mutated mid-parse. Because the
//! events live in the emitter's rewindable channel, backtracking rewinds the tree for free:
//! the same mark that unwinds diagnostics unwinds tree events.
//!
//! The components, front half (rowan-free, available in every build) first:
//!
//! 1. **[`event`](crate::cst::event)**: The event vocabulary, the era-branded
//!    [`EventMark`](crate::cst::event::EventMark), and the
//!    [`Marker`](crate::cst::event::Marker) retro-wrap typestate
//! 2. **`Sink`** (`rowan`): The recording emitter — buffers events under the one
//!    checkpoint/rewind mark and materializes once into a green tree, configured by a
//!    `CstProfile` (the dialect's kind space: mapper, `KindValidator`, error and gap kinds)
//!    and reading its text through `CstText`
//! 3. **`SyntaxTreeBuilder`** (`rowan`): The low-level append-only builder over rowan's
//!    green tree builder (no rollback of its own — that is what the event buffer is for)
//! 4. **`Element`** / **`Node`** / **`Token`** (`rowan`): Typed views over the finished
//!    tree
//! 5. **`cast`** (`rowan`): Utility functions for the typed layer
//! 6. **`error`** (`rowan`): Error types for CST operations
//!
//! # Design Philosophy
//!
//! - **Zero-cost abstractions**: Typed CST nodes are just pointers, no runtime overhead
//! - **Lossless**: All source information is preserved in the tree
//! - **Immutable**: Trees are immutable by default (use `clone_for_update()` for mutations)
//! - **Type-safe**: Compile-time guarantees about node types and relationships
//!
//! # See Also
//!
//! - [rowan documentation](https://docs.rs/rowan) - The underlying CST library

#[cfg(feature = "rowan")]
use core::{cell::RefCell, marker::PhantomData};

#[cfg(feature = "rowan")]
use derive_more::{From, Into};
#[cfg(feature = "rowan")]
use rowan::{GreenNodeBuilder, Language, SyntaxNode, SyntaxToken};

#[cfg(feature = "rowan")]
use crate::syntax::Syntax;

pub mod event;
pub mod kinds;

// UN-GATED from `rowan` deliberately (T12). `CstProfile` and `KindValidator` are fn-pointer and
// data only — `profile.rs`'s single import is `event::TOMBSTONE`, which is itself unconditional
// — so the gate was placement, not design. A `macro_rules!` expansion cannot cfg on tokora's
// features, so a `syntax_kinds!`-style macro that names `KindValidator` in one arm was forcing
// `rowan` onto every consumer of the macro. Strictly more configurations compile; the sink half
// keeps its gate.
mod profile;

#[cfg(feature = "rowan")]
mod sink;

#[cfg(feature = "rowan")]
mod text;

pub use profile::{CstProfile, KindValidator};

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use sink::{FinishError, Sink, TriviaPolicy};

#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub use text::CstText;

/// A builder for constructing concrete syntax trees.
///
/// `SyntaxTreeBuilder` wraps rowan's [`GreenNodeBuilder`] and provides a convenient
/// interface for building syntax trees from tokens during parsing. The builder uses
/// interior mutability ([`RefCell`]) to allow sharing across parser combinators.
///
/// # Type Parameters
///
/// - `Lang`: The [`Language`] type that defines the syntax kinds for your language
///
/// # Usage Pattern
///
/// The typical usage pattern is:
///
/// 1. Create a builder with [`new()`](Self::new)
/// 2. Pass it to your parser implementation
/// 3. The parser calls [`start_node()`](Self::start_node), [`token()`](Self::token),
///    and [`finish_node()`](Self::finish_node) to build the tree
/// 4. Call [`finish()`](Self::finish) to get the final [`rowan::GreenNode`]
///
/// # Examples
///
/// ```rust,ignore
/// use tokora::cst::SyntaxTreeBuilder;
///
/// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
///
/// // Build a simple tree: Root(Identifier("hello"))
/// builder.start_node(SyntaxKind::Root);
/// builder.token(SyntaxKind::Identifier, "hello");
/// builder.finish_node();
///
/// let green_node = builder.finish();
/// ```
///
/// ## Using Checkpoints for Lookahead
///
/// Checkpoints allow you to start nodes retroactively, which is useful for
/// handling left-recursive or ambiguous grammars:
///
/// ```rust,ignore
/// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
/// let checkpoint = builder.checkpoint();
///
/// builder.token(SyntaxKind::Number, "42");
///
/// // Decide to wrap the number in a UnaryExpression
/// builder.start_node_at(checkpoint, SyntaxKind::UnaryExpression);
/// builder.token(SyntaxKind::Plus, "+");
/// builder.finish_node();
/// ```
///
/// # Interior Mutability
///
/// The builder uses [`RefCell`] internally, which means:
/// - It can be shared immutably across parser combinators
/// - Mutations are checked at runtime (will panic if you violate borrow rules)
/// - Typically safe in single-threaded parsing contexts
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
#[derive(Debug)]
pub struct SyntaxTreeBuilder<Lang> {
  builder: RefCell<GreenNodeBuilder<'static>>,
  _marker: PhantomData<Lang>,
}

#[cfg(feature = "rowan")]
impl<Lang> Default for SyntaxTreeBuilder<Lang>
where
  Lang: Language,
{
  #[inline]
  fn default() -> Self {
    Self::new()
  }
}

#[cfg(feature = "rowan")]
impl<Lang> SyntaxTreeBuilder<Lang>
where
  Lang: Language,
{
  /// Creates a new empty syntax tree builder.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// ```
  #[inline]
  pub fn new() -> Self {
    Self {
      builder: RefCell::new(GreenNodeBuilder::new()),
      _marker: PhantomData,
    }
  }

  /// Creates a checkpoint representing the current position in the tree.
  ///
  /// Checkpoints can be used with [`start_node_at()`](Self::start_node_at) to
  /// retroactively wrap already-added children in a new parent node. This is
  /// useful for handling left-recursive or ambiguous grammars.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// let checkpoint = builder.checkpoint();
  ///
  /// builder.token(SyntaxKind::Number, "42");
  ///
  /// // Wrap the number in an expression node
  /// builder.start_node_at(checkpoint, SyntaxKind::Expression);
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::checkpoint`]
  #[inline]
  #[must_use]
  pub fn checkpoint(&self) -> rowan::Checkpoint {
    self.builder.borrow().checkpoint()
  }

  /// Starts a new node with the given syntax kind.
  ///
  /// Must be paired with a corresponding [`finish_node()`](Self::finish_node) call.
  /// All tokens and child nodes added between `start_node()` and `finish_node()`
  /// will be children of this node.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::BinaryExpression);
  /// builder.token(SyntaxKind::Number, "1");
  /// builder.token(SyntaxKind::Plus, "+");
  /// builder.token(SyntaxKind::Number, "2");
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::start_node`]
  #[inline]
  pub fn start_node(&self, kind: Lang::Kind) {
    self
      .builder
      .borrow_mut()
      .start_node(Lang::kind_to_raw(kind));
  }

  /// Starts a new node at a previously created checkpoint.
  ///
  /// This allows you to retroactively wrap children that were added after the
  /// checkpoint was created. Useful for handling operator precedence and
  /// left-recursive grammars.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  /// let checkpoint = builder.checkpoint();
  ///
  /// // Add a number
  /// builder.token(SyntaxKind::Number, "42");
  ///
  /// // Later, decide to wrap it in a unary expression
  /// builder.start_node_at(checkpoint, SyntaxKind::UnaryExpression);
  /// builder.token(SyntaxKind::Minus, "-");
  /// builder.finish_node();
  /// // Result: UnaryExpression(Number("42"), Minus("-"))
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::start_node_at`]
  #[inline]
  pub fn start_node_at(&self, checkpoint: rowan::Checkpoint, kind: Lang::Kind) {
    self
      .builder
      .borrow_mut()
      .start_node_at(checkpoint, Lang::kind_to_raw(kind));
  }

  /// Adds a token with the given kind and text to the current node.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Identifier);
  /// builder.token(SyntaxKind::IdentifierToken, "my_variable");
  /// builder.finish_node();
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::token`]
  #[inline]
  pub fn token(&self, kind: Lang::Kind, text: &str) {
    self
      .builder
      .borrow_mut()
      .token(Lang::kind_to_raw(kind), text);
  }

  /// Finishes the current node started with [`start_node()`](Self::start_node)
  /// or [`start_node_at()`](Self::start_node_at).
  ///
  /// # Panics
  ///
  /// Panics if there is no node to finish (i.e., `finish_node()` was called
  /// more times than `start_node()`).
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Root);
  /// builder.token(SyntaxKind::Identifier, "foo");
  /// builder.finish_node(); // Finishes the Root node
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::finish_node`]
  #[inline]
  pub fn finish_node(&self) {
    self.builder.borrow_mut().finish_node();
  }

  /// Completes the tree building and returns the final green node.
  ///
  /// This consumes the builder and returns the root [`rowan::GreenNode`],
  /// which can be converted to a [`rowan::SyntaxNode`] for traversal.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use tokora::cst::SyntaxTreeBuilder;
  /// use rowan::SyntaxNode;
  ///
  /// let builder = SyntaxTreeBuilder::<MyLanguage>::new();
  ///
  /// builder.start_node(SyntaxKind::Root);
  /// builder.token(SyntaxKind::Identifier, "foo");
  /// builder.finish_node();
  ///
  /// let green = builder.finish();
  /// let root = SyntaxNode::new_root(green);
  /// ```
  ///
  /// See also: [`rowan::GreenNodeBuilder::finish`]
  #[inline]
  pub fn finish(self) -> rowan::GreenNode {
    self.builder.into_inner().finish()
  }
}

/// Base trait for all typed CST elements (nodes and tokens).
///
/// `Element` provides the common interface shared by both CST nodes
/// ([`Node`]) and CST tokens ([`Token`]). It enables:
/// - **Type checking**: Verify if an untyped element can be cast to a specific type
/// - **Type identity**: Associate elements with their syntax kind
/// - **Polymorphism**: Write generic code that works with both nodes and tokens
///
/// # Design
///
/// This trait serves as the foundation of the typed CST hierarchy:
/// ```text
/// Element (base)
///     ├── Node (for interior nodes)
///     └── Token (for leaf tokens)
/// ```
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Element<Lang: Language>: core::fmt::Debug {
  /// The syntax kind of this CST element.
  ///
  /// For enum elements representing multiple variants, this can be a marker value
  /// that is not directly used for casting but serves as documentation.
  const KIND: Lang::Kind;

  /// Returns `true` if the given kind can be cast to this CST element.
  ///
  /// This method determines whether an untyped rowan element with a specific
  /// syntax kind can be safely converted to this typed element.
  ///
  /// # Implementation Guidelines
  ///
  /// - **Single variant**: Return `kind == Self::KIND`
  /// - **Multiple variants**: Use pattern matching to check all valid kinds
  /// - **Performance**: This method is often called frequently, keep it fast
  ///
  /// # Examples
  ///
  /// ## Simple Element (Single Kind)
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// impl Element for Comma {
  ///     const KIND: SyntaxKind = SyntaxKind::Comma;
  ///
  ///     fn castable(kind: SyntaxKind) -> bool {
  ///         kind == SyntaxKind::Comma
  ///     }
  /// }
  /// ```
  ///
  /// ## Enum Element (Multiple Kinds)
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// impl Element for BinaryOperator {
  ///     const KIND: SyntaxKind = SyntaxKind::BinaryOp; // Marker
  ///
  ///     fn castable(kind: SyntaxKind) -> bool {
  ///         matches!(
  ///             kind,
  ///             SyntaxKind::Plus
  ///             | SyntaxKind::Minus
  ///             | SyntaxKind::Star
  ///             | SyntaxKind::Slash
  ///         )
  ///     }
  /// }
  /// ```
  ///
  /// ## Usage in Type Checking
  ///
  /// ```rust,ignore
  /// use tokora::cst::Element;
  ///
  /// // Check before casting
  /// if Comma::castable(token.kind()) {
  ///     let comma = Comma::try_cast_node(token).unwrap();
  /// }
  /// ```
  fn castable(kind: Lang::Kind) -> bool
  where
    Self: Sized;
}

/// Trait for typed CST tokens (leaf elements in the syntax tree).
///
/// `Token` provides a type-safe wrapper around rowan's untyped [`SyntaxToken`],
/// representing terminal elements in the concrete syntax tree. Tokens are the leaf nodes
/// that contain actual source text (keywords, identifiers, literals, punctuation, etc.).
///
/// # Design
///
/// Tokens differ from nodes ([`Node`]) in that:
/// - **Tokens are leaves**: They contain source text directly
/// - **Nodes are interior**: They have children and structure the tree
/// - **Zero-cost**: Token wrappers have the same memory layout as [`SyntaxToken`]
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Token<Lang: Language>: Element<Lang> {
  /// Attempts to cast the given syntax token to this typed token.
  ///
  /// Returns an error if the token's kind doesn't match this type.
  ///
  /// # Errors
  ///
  /// Returns [`TokenMismatch`](error::TokenMismatch) if:
  /// - The token's kind doesn't match the expected kind for this type
  /// - For enum tokens, the kind is not one of the valid variants
  fn try_cast_token(syntax: SyntaxToken<Lang>) -> Result<Self, error::TokenMismatch<Self, Lang>>
  where
    Self: Sized;

  /// Returns a reference to the underlying syntax token.
  ///
  /// This provides access to rowan's token APIs for inspecting position,
  /// text, and tree structure.
  fn syntax(&self) -> &SyntaxToken<Lang>;

  /// Returns the source text of this token.
  ///
  /// This is a convenience method that extracts the text from the underlying
  /// [`SyntaxToken`]. The text is always valid UTF-8.
  fn text(&self) -> &str
  where
    Lang: 'static,
  {
    self.syntax().text()
  }
}

/// The main trait for typed CST nodes with zero-cost conversions.
///
/// `Node` provides a type-safe wrapper around rowan's untyped [`SyntaxNode`], allowing
/// you to work with strongly-typed CST nodes. The conversion between typed and untyped
/// nodes has **zero runtime cost** - both representations have exactly the same memory
/// layout (a pointer to the tree root and a pointer to the node itself).
///
/// # Design
///
/// The `Node` trait enables:
/// - **Type safety**: Compile-time guarantees about node types
/// - **Zero-cost**: No runtime overhead for typed wrappers
/// - **Pattern matching**: Cast nodes to specific types
/// - **Tree traversal**: Navigate the CST with type information
///
/// # Type Parameters
///
/// - `Lang`: The rowan [`Language`] type defining syntax kinds
///
/// # One language, named once
///
/// The [`Syntax`] supertrait is bound at `Lang = Lang` rather than left free, and that
/// equality is load-bearing. [`Element<Lang>`](Element) says which tree this node lives in;
/// [`Syntax`] carries the `KIND` constant that says which node it *is* — and `Syntax::KIND` is
/// typed `<Self::Lang as Language>::SyntaxKind`. With `Syntax::Lang` unconstrained, a type
/// could be `Element<A> + Syntax<Lang = B>` and still satisfy `Node<A>`: a node claiming
/// membership in one language while its kind authority answers for another. Nothing would
/// reject it, and the mismatch would surface as a cast that never matches.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait Node<Lang: Language>: Element<Lang> + Syntax<Lang = Lang> {
  /// Attempts to cast the given syntax node to this CST node.
  ///
  /// Returns an error if the node's kind doesn't match this type.
  fn try_cast_node(syntax: SyntaxNode<Lang>) -> Result<Self, error::SyntaxError<Self, Lang>>
  where
    Self: Sized;

  /// Returns a reference to the underlying syntax node.
  ///
  /// This provides access to rowan's tree traversal APIs.
  fn syntax(&self) -> &SyntaxNode<Lang>;

  /// Returns the source string of this CST node.
  ///
  /// This includes all text spanned by this node, including whitespace and trivia.
  fn source_string(&self) -> String {
    self.syntax().to_string()
  }

  /// Clones this CST node for update operations.
  ///
  /// This creates a mutable copy of the node that can be modified using rowan's
  /// mutation APIs. The original tree remains unchanged.
  fn clone_for_update(&self) -> Self
  where
    Self: Sized,
  {
    Self::try_cast_node(self.syntax().clone_for_update()).unwrap()
  }

  /// Clones the subtree rooted at this CST node.
  ///
  /// This creates a deep copy of this node and all its descendants, detached
  /// from the original tree.
  fn clone_subtree(&self) -> Self
  where
    Self: Sized,
  {
    Self::try_cast_node(self.syntax().clone_subtree()).unwrap()
  }
}

/// The one capability tree *navigation* needs: turn an untyped [`SyntaxNode`] into a typed
/// node, or decline.
///
/// [`cast::child`], [`cast::children`] and [`NodeChildren`] are bound on this trait rather
/// than on [`Node`], because this is all they ever call. `child` is `find_map(N::cast_node)`
/// and nothing more — it never reads a `KIND`, a component list, or a component count.
///
/// # Why this is not [`Node`]
///
/// [`Node`] requires [`Syntax`], and [`Syntax`] is the *parser's* model of a production: a
/// `Component` enum, and `COMPONENTS`/`REQUIRED` as type-level counts, all of it in service of
/// reporting which parts of a production were missing. That is the right shape for a parser
/// and the wrong toll for a reader. A typed CST layer whose job is `field.name()` would
/// otherwise have to invent a component enum and a typenum count per node kind — once per node
/// kind, for a model it never consults.
///
/// So the navigation helpers ask for navigation. A parser-facing node still satisfies them,
/// through the blanket impl below.
///
/// # One impl per type, by construction
///
/// Every [`Node`] receives `CastNode` from a blanket impl, which is what keeps every existing
/// call site compiling and asks nothing new of existing [`Node`] implementors.
///
/// The price is coherence: because that blanket impl covers all of [`Node`], a type cannot
/// implement `CastNode` **directly** and also implement [`Node`] — the impls would overlap and
/// rustc rejects the direct one (`E0119`).
///
/// That is a commitment, not an accident. The two intended positions are disjoint, and a type
/// belongs to exactly one:
///
/// - a **parser-facing** node implements [`Node`] — hence [`Syntax`], hence the component model
///   — and gets `CastNode` for free;
/// - a **navigation-only** typed CST node implements `CastNode` directly and never names
///   [`Syntax`] at all.
///
/// A type that wants both is asking for the component model, and should implement [`Node`].
///
/// # Examples
///
/// A navigation-only node — no [`Syntax`], no components, no counts:
///
/// ```rust,ignore
/// use rowan::SyntaxNode;
/// use tokora::cst::{CastNode, cast};
///
/// struct Field(SyntaxNode<MyLang>);
/// struct Name(SyntaxNode<MyLang>);
///
/// impl CastNode<MyLang> for Name {
///     fn cast_node(syntax: SyntaxNode<MyLang>) -> Option<Self> {
///         (syntax.kind() == MyKind::Name).then_some(Self(syntax))
///     }
/// }
///
/// impl Field {
///     fn name(&self) -> Option<Name> {
///         cast::child(&self.0)
///     }
/// }
/// ```
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub trait CastNode<Lang: Language>: Sized {
  /// Casts an untyped syntax node into this typed node, or returns `None` if it is not one.
  ///
  /// This is the whole trait. Implementations should be a kind check and a wrap; they must not
  /// panic, because the navigation helpers call this once per child and read `None` as "not
  /// this type, keep looking".
  fn cast_node(syntax: SyntaxNode<Lang>) -> Option<Self>;
}

#[cfg(feature = "rowan")]
impl<N, Lang> CastNode<Lang> for N
where
  N: Node<Lang>,
  Lang: Language,
{
  #[inline]
  fn cast_node(syntax: SyntaxNode<Lang>) -> Option<Self> {
    Self::try_cast_node(syntax).ok()
  }
}

/// An iterator over typed CST children of a particular node type.
///
/// `NodeChildren` filters and casts child nodes to a specific typed node type,
/// skipping any children that cannot be cast to the target type.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
#[derive(Debug, From, Into)]
#[repr(transparent)]
pub struct NodeChildren<N, Lang: Language> {
  inner: rowan::SyntaxNodeChildren<Lang>,
  _m: PhantomData<N>,
}

#[cfg(feature = "rowan")]
impl<N, Lang: Language> Clone for NodeChildren<N, Lang> {
  #[inline]
  fn clone(&self) -> Self {
    Self {
      inner: self.inner.clone(),
      _m: PhantomData,
    }
  }
}

#[cfg(feature = "rowan")]
impl<N, Lang: Language> NodeChildren<N, Lang> {
  #[inline]
  fn new(parent: &SyntaxNode<Lang>) -> Self {
    Self {
      inner: parent.children(),
      _m: PhantomData,
    }
  }

  /// Returns an iterator over syntax node children matching a kind predicate.
  ///
  /// This allows further filtering of children based on their syntax kind,
  /// returning the underlying [`SyntaxNode`] instead of typed nodes.
  pub fn by_kind<F>(self, f: F) -> impl Iterator<Item = SyntaxNode<Lang>>
  where
    F: Fn(Lang::Kind) -> bool,
  {
    self.inner.by_kind(f)
  }
}

#[cfg(feature = "rowan")]
impl<N, Lang> Iterator for NodeChildren<N, Lang>
where
  N: CastNode<Lang>,
  Lang: Language,
{
  type Item = N;

  #[inline]
  fn next(&mut self) -> Option<N> {
    self.inner.find_map(N::cast_node)
  }
}

/// Utility functions for casting and accessing CST nodes.
///
/// This module provides convenient functions for working with typed CST nodes,
/// including finding children, accessing tokens, and casting between types.
///
/// The node-typed helpers — [`child`](cast::child) and [`children`](cast::children) — are bound
/// on [`CastNode`], not on [`Node`]. Casting a child is the only thing they do with `N`, so it
/// is the only thing they ask of it, and a navigation-only typed CST layer can use them without
/// ever naming the parser's [`Syntax`] model. Every [`Node`] still qualifies, through the
/// blanket impl. [`token`](cast::token) is not node-typed at all — it matches on a
/// [`Lang::Kind`](Language::Kind) value and needs no bound beyond the language.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub mod cast;

/// Error types for CST operations.
#[cfg(feature = "rowan")]
#[cfg_attr(docsrs, doc(cfg(feature = "rowan")))]
pub mod error;

#[cfg(all(test, feature = "rowan"))]
mod tests;
