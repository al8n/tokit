/// One step of a path into a produced *result*, rather than into any source text.
///
/// # This is a coordinate in the output, not in the input
///
/// A [`Location`](super::Location) says where in a *document* something is; a path says which
/// field of which element of the *result* an error belongs to, so that a client holding the
/// result can line the error up with the hole in it. The two never substitute for one another,
/// and a producer that has only one of them answers zero of the other rather than converting.
///
/// The motivating case is a GraphQL execution error, whose specification says the path's entries
/// "should be strings for Object fields, and 0-indexed integers for List entries" — hence exactly
/// these two variants, and hence [`Index`](Self::Index) counting from zero. The shape generalises
/// to any protocol that answers with a tree and has to say where in that tree something went
/// wrong; it is a JSON Pointer without the escaping rules.
///
/// A field name is borrowed from the request that asked for it, which is why the segment carries
/// a lifetime rather than owning its text.
///
/// # Most diagnostics answer zero of these
///
/// A path is about a *response*, so anything reporting on a document — every lexical and
/// syntactic error a parser produces — carries none. That is not a missing feature or a
/// tri-state to branch on: zero segments is the positive statement that this diagnostic cannot be
/// associated with a particular field of a result, which is what a writer emitting the error
/// object needs to know.
///
/// A structured coordinate that is *not* a result path — a dotted `User.pet.first` naming a type
/// in a schema, say — must not be published through the path, because a writer would then emit a
/// result coordinate for an error that never had one on the wire.
///
/// ```
/// use tokora::diagnostic::PathSegment;
///
/// let path = [PathSegment::Field("heroes"), PathSegment::Index(2), PathSegment::Field("name")];
/// let rendered: Vec<String> = path.iter().map(ToString::to_string).collect();
/// assert_eq!(rendered, ["heroes", "2", "name"]);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PathSegment<'a> {
  /// A response key — a field's alias where it has one, otherwise its name.
  Field(&'a str),
  /// A zero-based index into a list.
  Index(u32),
}

impl core::fmt::Display for PathSegment<'_> {
  #[inline]
  fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
    match self {
      Self::Field(name) => f.write_str(name),
      Self::Index(index) => write!(f, "{index}"),
    }
  }
}
