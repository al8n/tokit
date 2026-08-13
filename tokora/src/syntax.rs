//! Syntax definition and incomplete syntax error types.
//!
//! This module provides types for representing syntax elements with a known number
//! of components, and errors for tracking missing components during parsing.
//!
//! # Design Philosophy
//!
//! When parsing syntax elements that require multiple components (like variable declarations,
//! function definitions, etc.), it's valuable to track *all* missing components rather than
//! failing on the first missing one. This enables:
//!
//! - Better error messages showing all missing parts
//! - Faster development iteration (see all errors at once)
//! - More helpful IDE diagnostics
//!
//! # Examples
//!
//! ```rust
//! # {
//! use tokora::{SimpleSpan, utils::{typenum::{self, U3}, GenericArrayDeque}, syntax::{Syntax, Language}, error::IncompleteSyntax};
//! use core::fmt;
//!
//! #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
//! struct MyLanguage;
//!
//! impl Language for MyLanguage {
//!   type SyntaxKind = (); // () is a placeholder
//! }
//!
//! #[derive(Debug, Clone, PartialEq, Eq, Hash)]
//! enum WhileComponent {
//!     WhileKeyword,
//!     Condition,
//!     Body,
//! }
//!
//! impl fmt::Display for WhileComponent {
//!     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//!         match self {
//!             Self::WhileKeyword => write!(f, "'while' keyword"),
//!             Self::Condition => write!(f, "condition"),
//!             Self::Body => write!(f, "body"),
//!         }
//!     }
//! }
//!
//! struct WhileLoop;
//!
//! impl Syntax for WhileLoop {
//!     type Lang = MyLanguage;
//!     const KIND: () = (); // () is a placeholder
//!     type Component = WhileComponent;
//!     type COMPONENTS = U3;
//!     type REQUIRED = U3;
//!
//!     fn possible_components() -> &'static GenericArrayDeque<Self::Component, U3> {
//!         static COMPONENTS: GenericArrayDeque<WhileComponent, U3> = {
//!             let mut deque = GenericArrayDeque::new();
//!             deque.push_back(WhileComponent::WhileKeyword);
//!             deque.push_back(WhileComponent::Condition);
//!             deque.push_back(WhileComponent::Body);
//!             deque
//!         };
//!         &COMPONENTS
//!     }
//!
//!     fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED> {
//!         static REQUIRED: GenericArrayDeque<WhileComponent, U3> = {
//!             let mut deque = GenericArrayDeque::new();
//!             deque.push_back(WhileComponent::WhileKeyword);
//!             deque.push_back(WhileComponent::Condition);
//!             deque.push_back(WhileComponent::Body);
//!             deque
//!         };
//!         &REQUIRED
//!     }
//! }
//!
//! let mut error = IncompleteSyntax::<WhileLoop>::new(SimpleSpan::new(10, 15), WhileComponent::Condition);
//! assert_eq!(error.len(), 1);
//! # }
//! ```
use generic_arraydeque::ArrayLength;

use core::{
  fmt::{Debug, Display},
  hash::Hash,
};

/// A trait representing a syntax with a type-level number of components.
///
/// This trait defines the structure of a syntax element that has a known number
/// of required components. It uses `typenum` for type-level component count,
/// enabling compile-time arithmetic and better integration with generic-array-based code.
///
/// # Type Parameters
///
/// - `Component`: The type representing individual syntax components (usually an enum)
/// - `COMPONENTS`: A type-level unsigned integer (via `ArrayLength`) specifying component count
///
/// # Examples
///
/// ```rust
/// # {
/// use tokora::{utils::{typenum, GenericArrayDeque}, syntax::{Syntax, Language}};
/// use typenum::U5;
/// use core::fmt;
///
/// #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// struct MyLanguage;
///
/// impl Language for MyLanguage {
///   type SyntaxKind = (); // () is a placeholder
/// }
///
/// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// enum LetStatementComponent {
///     LetKeyword,
///     Identifier,
///     Equals,
///     Expression,
///     Semicolon,
/// }
///
/// impl fmt::Display for LetStatementComponent {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         match self {
///             Self::LetKeyword => write!(f, "'let' keyword"),
///             Self::Identifier => write!(f, "identifier"),
///             Self::Equals => write!(f, "'=' operator"),
///             Self::Expression => write!(f, "expression"),
///             Self::Semicolon => write!(f, "';' semicolon"),
///         }
///     }
/// }
///
/// struct LetStatement;
///
/// impl Syntax for LetStatement {
///     type Lang = MyLanguage;
///     const KIND: () = (); // () is a placeholder
///     type Component = LetStatementComponent;
///     type COMPONENTS = U5;
///     type REQUIRED = U5;
///
///     fn possible_components() -> &'static GenericArrayDeque<Self::Component, Self::COMPONENTS> {
///         static COMPONENTS: GenericArrayDeque<LetStatementComponent, typenum::U5> = {
///             let mut deque = GenericArrayDeque::new();
///             deque.push_back(LetStatementComponent::LetKeyword);
///             deque.push_back(LetStatementComponent::Identifier);
///             deque.push_back(LetStatementComponent::Equals);
///             deque.push_back(LetStatementComponent::Expression);
///             deque.push_back(LetStatementComponent::Semicolon);
///             deque
///         };
///         &COMPONENTS
///     }
///
///     fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED> {
///         static REQUIRED: GenericArrayDeque<LetStatementComponent, typenum::U5> = {
///             let mut deque = GenericArrayDeque::new();
///             deque.push_back(LetStatementComponent::LetKeyword);
///             deque.push_back(LetStatementComponent::Identifier);
///             deque.push_back(LetStatementComponent::Equals);
///             deque.push_back(LetStatementComponent::Expression);
///             deque.push_back(LetStatementComponent::Semicolon);
///             deque
///         };
///         &REQUIRED
///     }
/// }
/// # }
/// ```
pub trait Syntax {
  /// The language this syntax belongs to.
  type Lang: Language;

  /// The kind of the syntax.
  const KIND: <Self::Lang as Language>::SyntaxKind;

  /// The component type of this syntax.
  ///
  /// Usually this is an enum representing different variants of syntax components.
  /// This type is used for error reporting to specify which components are missing.
  ///
  /// **Do not implement this so that [`Eq`], [`Hash`], or [`Display`] can report something
  /// different for the same value over time** — most easily done by deriving one of them from
  /// a `Cell`, an atomic, or other interior-mutable or ambient state.
  /// [`IncompleteSyntax`](crate::error::IncompleteSyntax) stores `Component` values in a
  /// deduplicating set keyed on exactly those impls, and after insertion can only ever hand
  /// this type back out by shared reference — so `Component`'s own author is the only one able
  /// to keep that promise, or break it. See *Uniqueness is a logic error* on
  /// [`IncompleteSyntax`](crate::error::IncompleteSyntax) for what breaking it costs.
  type Component: Display + Debug + Clone + PartialEq + Eq + Hash;

  /// The number of components in this syntax, represented as a type-level unsigned integer.
  ///
  /// Uses `typenum` to represent the count at the type level, enabling compile-time
  /// arithmetic without requiring unstable `generic_const_exprs` feature.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use typenum::U3; // For a syntax with 3 components
  ///
  /// impl Syntax for MySyntax {
  ///     type COMPONENTS = U3;
  ///     // ...
  /// }
  /// ```
  type COMPONENTS: ArrayLength + Debug + Eq + Hash;

  /// The number of required components in this syntax, represented as a type-level unsigned integer.
  ///
  /// Uses `typenum` to represent the count at the type level, enabling compile-time
  /// arithmetic without requiring unstable `generic_const_exprs` feature.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// use typenum::U3; // For a syntax with 3 components
  ///
  /// impl Syntax for MySyntax {
  ///     type COMPONENTS = U3;
  ///     // ...
  /// }
  /// ```
  type REQUIRED: ArrayLength + Debug + Eq + Hash;

  /// Returns a static reference to all possible components for this syntax.
  ///
  /// The deque contains all components that can be part of this syntax element,
  /// in a canonical order. The returned reference points to a static, never-changing
  /// collection that is initialized once at program startup.
  ///
  /// # Implementation Pattern
  ///
  /// Implementations should use a `static` item initialized in a const context:
  ///
  /// ```rust,ignore
  /// fn possible_components() -> &'static GenericArrayDeque<Self::Component, Self::COMPONENTS> {
  ///     static COMPONENTS: GenericArrayDeque<MyComponent, U3> = {
  ///         let mut deque = GenericArrayDeque::new();
  ///         // Push components in const context
  ///         deque.push_back(MyComponent::Foo);
  ///         deque.push_back(MyComponent::Bar);
  ///         deque.push_back(MyComponent::Baz);
  ///         deque
  ///     };
  ///     &COMPONENTS
  /// }
  /// ```
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// let components = MySyntax::possible_components();
  /// for component in components.iter() {
  ///     println!("{}", component);
  /// }
  /// ```
  fn possible_components()
  -> &'static generic_arraydeque::GenericArrayDeque<Self::Component, Self::COMPONENTS>;

  /// Returns a static reference to all required components for this syntax.
  ///
  /// The deque contains all components that are required for this syntax element,
  /// in a canonical order. The returned reference points to a static, never-changing
  /// collection that is initialized once at program startup.
  ///
  /// # Implementation Pattern
  ///
  /// Implementations should use a `static` item initialized in a const context:
  ///
  /// ```rust,ignore
  /// fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED> {
  ///     static REQUIRED: GenericArrayDeque<MyComponent, U2> = {
  ///         let mut deque = GenericArrayDeque::new();
  ///         deque.push_back(MyComponent::Foo);
  ///         deque.push_back(MyComponent::Bar);
  ///         deque
  ///     };
  ///     &REQUIRED
  /// }
  /// ```
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// let required = MySyntax::required_components();
  /// assert_eq!(required.len(), 2);
  /// ```
  fn required_components()
  -> &'static generic_arraydeque::GenericArrayDeque<Self::Component, Self::REQUIRED>;
}

/// A trait representing an AST node associated with a syntax definition.
///
/// This trait creates a type-level bridge between AST node types and their corresponding
/// `Syntax` types, enabling generic, type-safe error handling and parser implementation.
/// By associating an AST node with its syntax structure, we can automatically derive
/// error construction logic and write language-polymorphic parsers.
///
/// # Design Philosophy
///
/// When parsing AST nodes, incomplete syntax errors need to know which syntax element
/// failed to parse. The `AstNode` trait makes this relationship explicit at the type level:
///
/// - Each AST node type declares its corresponding `Syntax` type
/// - Generic code can use `T::Syntax` to construct appropriate `IncompleteSyntax<T::Syntax>` errors
/// - The `Lang` parameter enables the same structural node (e.g., `Name<S>`) to have
///   different syntax types in different language dialects
///
/// # Benefits
///
/// 1. **Type Safety**: Impossible to construct errors with the wrong syntax type
/// 2. **Generic Parsers**: Write parsers that work for any `T: AstNode<Lang>`
/// 3. **Discoverability**: Given an AST node, easily find its syntax definition
/// 4. **Language Polymorphism**: Same node structure, different syntax per dialect
/// 5. **Reduced Boilerplate**: Generic error handling without manual trait implementations
///
/// # Type Parameters
///
/// - `Lang`: The language or dialect this AST node belongs to. This enables:
///   - Distinguishing GraphQL from GraphQLx nodes
///   - Supporting multiple language dialects in one codebase
///   - Language-specific syntax customization
///
/// # Examples
///
/// ## Basic Implementation
///
/// ```rust
/// # {
/// use tokora::{SimpleSpan, utils::{GenericArrayDeque, typenum::U2}, syntax::{Syntax, AstNode, Language}, error::IncompleteSyntax};
/// use core::fmt;
///
/// // Define a language
/// #[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
/// struct MyLanguage;
///
/// impl Language for MyLanguage {
///     type SyntaxKind = (); // () is a placeholder
/// }
///
/// // Define syntax components
/// #[derive(Debug, Clone, PartialEq, Eq, Hash)]
/// enum VariableComponent {
///     Dollar,
///     Name,
/// }
///
/// impl fmt::Display for VariableComponent {
///     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
///         match self {
///             Self::Dollar => write!(f, "'$' prefix"),
///             Self::Name => write!(f, "variable name"),
///         }
///     }
/// }
///
/// // Define syntax type
/// struct VariableSyntax;
///
/// impl Syntax for VariableSyntax {
///     type Lang = MyLanguage;
///     const KIND: () = (); // () is a placeholder
///     type Component = VariableComponent;
///     type COMPONENTS = U2;
///     type REQUIRED = U2;
///
///     fn possible_components() -> &'static GenericArrayDeque<Self::Component, Self::COMPONENTS> {
///         const COMPONENTS: &GenericArrayDeque<VariableComponent, U2> = &GenericArrayDeque::from_array([VariableComponent::Dollar, VariableComponent::Name]);
///         COMPONENTS
///     }
///
///     fn required_components() -> &'static GenericArrayDeque<Self::Component, Self::REQUIRED> {
///         const REQUIRED: &GenericArrayDeque<VariableComponent, U2> = &GenericArrayDeque::from_array([VariableComponent::Dollar, VariableComponent::Name]);
///         REQUIRED
///     }
/// }
///
/// // Define AST node
/// struct Variable {
///     name: String,
/// }
///
/// // Implement AstNode to bridge AST and Syntax
/// impl AstNode<MyLanguage> for Variable {
///     type Syntax = VariableSyntax;
/// }
///
/// // Now generic code can use T::Syntax automatically
/// fn create_incomplete_error<T>(span: SimpleSpan, component: <T::Syntax as Syntax>::Component) -> IncompleteSyntax<T::Syntax>
/// where
///     T: AstNode<MyLanguage>,
/// {
///     IncompleteSyntax::new(span, component)
/// }
///
/// let error = create_incomplete_error::<Variable>(
///     SimpleSpan::new(0, 3),
///     VariableComponent::Dollar
/// );
/// # }
/// ```
///
/// ## Language Polymorphism
///
/// ```rust,ignore
/// // Same structure, different syntax per language
/// struct Name<S> {
///     source: S,
/// }
///
/// // GraphQL implementation
/// impl<S> AstNode<GraphQL> for Name<S> {
///     type Syntax = GraphQLNameSyntax;
/// }
///
/// // GraphQLx implementation (extended dialect)
/// impl<S> AstNode<GraphQLx> for Name<S> {
///     type Syntax = GraphQLxNameSyntax; // Different syntax rules
/// }
/// ```
///
/// # Common Patterns
///
/// ## With Generic AST Nodes
///
/// For AST nodes with generic parameters, implement `AstNode` for the generic type:
///
/// ```rust,ignore
/// struct TypeDefinition<Name, Directives> {
///     name: Name,
///     directives: Option<Directives>,
/// }
///
/// impl<Name, Directives> AstNode<GraphQL> for TypeDefinition<Name, Directives> {
///     type Syntax = TypeDefinitionSyntax;
/// }
/// ```
///
/// ## Multiple Language Support
///
/// The same node structure can implement `AstNode` for multiple languages:
///
/// ```rust,ignore
/// impl<S> AstNode<GraphQL> for Variable<S> {
///     type Syntax = GraphQLVariableSyntax;
/// }
///
/// impl<S> AstNode<GraphQLx> for Variable<S> {
///     type Syntax = GraphQLxVariableSyntax;
/// }
/// ```
///
/// # See Also
///
/// - [`Syntax`]: Defines the structure and components of syntax elements
/// - [`IncompleteSyntax`](crate::error::IncompleteSyntax): Error type for tracking missing syntax components
pub trait AstNode<Lang> {
  /// The syntax type associated with this AST node.
  ///
  /// This type defines the structural components that make up the AST node during parsing.
  /// It must implement `Syntax<Lang = Lang>`, ensuring language consistency.
  ///
  /// # Examples
  ///
  /// ```rust,ignore
  /// impl AstNode<MyLanguage> for Variable {
  ///     type Syntax = VariableSyntax;
  ///     //             ^^^^^^^^^^^^^^
  ///     // This syntax type defines the components needed to parse a Variable
  /// }
  /// ```
  type Syntax: Syntax<Lang = Lang>;
}

/// Marker trait tying a language to its syntax kinds.
///
/// The `Language` trait connects your parser/AST to the concrete set of syntax kinds
/// (tokens, node tags, etc.) that belong to a language. Implement it once per language or
/// dialect so generic parsing infrastructure can stay agnostic of the actual enum.
///
/// ## Example
///
/// ```rust,ignore
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// pub enum MySyntaxKind {
///     Identifier,
///     Number,
///     // ...
/// }
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
/// pub struct MyLanguage;
///
/// impl Language for MyLanguage {
///     type SyntaxKind = MySyntaxKind;
/// }
/// ```
pub trait Language: Sized + Copy + core::fmt::Debug + Eq + Ord + core::hash::Hash {
  /// The syntax kind enum associated with this language.
  type SyntaxKind: Sized + Copy + core::fmt::Debug + Eq + Ord + core::hash::Hash;
}

#[cfg(feature = "rowan")]
impl<T: rowan::Language> Language for T {
  type SyntaxKind = T::Kind;
}
