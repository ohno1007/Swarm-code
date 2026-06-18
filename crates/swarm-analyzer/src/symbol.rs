use serde::Serialize;

/// The kind of a declared symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SymbolKind {
    Module,
    Function,
    Method,
    Struct,
    Class,
    Enum,
    Union,
    Trait,
    Impl,
    Const,
    Static,
    TypeAlias,
    Field,
    Variable,
    Parameter,
    /// An anonymous lexical block (`if`/`for`/`{ ... }`).
    Block,
}

impl SymbolKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SymbolKind::Module => "module",
            SymbolKind::Function => "function",
            SymbolKind::Method => "method",
            SymbolKind::Struct => "struct",
            SymbolKind::Class => "class",
            SymbolKind::Enum => "enum",
            SymbolKind::Union => "union",
            SymbolKind::Trait => "trait",
            SymbolKind::Impl => "impl",
            SymbolKind::Const => "const",
            SymbolKind::Static => "static",
            SymbolKind::TypeAlias => "type",
            SymbolKind::Field => "field",
            SymbolKind::Variable => "variable",
            SymbolKind::Parameter => "parameter",
            SymbolKind::Block => "block",
        }
    }

    /// Whether this kind opens its own lexical scope.
    pub fn opens_scope(self) -> bool {
        matches!(
            self,
            SymbolKind::Module
                | SymbolKind::Function
                | SymbolKind::Method
                | SymbolKind::Trait
                | SymbolKind::Impl
                | SymbolKind::Class
                | SymbolKind::Block
        )
    }
}

/// A symbol declared in source, possibly with nested children.
///
/// The tree of [`Symbol`]s doubles as the lexical scope tree: a symbol that
/// [`SymbolKind::opens_scope`] introduces a scope whose `bindings` are the
/// names visible to code nested under it.
#[derive(Debug, Clone, Serialize)]
pub struct Symbol {
    pub name: String,
    pub kind: SymbolKind,
    /// 1-based line of the declaration start.
    pub start_line: usize,
    /// 1-based line of the declaration end (closing brace or `;`).
    pub end_line: usize,
    /// The raw signature/header text (single line, normalized whitespace).
    pub signature: String,
    /// Names bound directly inside this symbol's scope (params, `let`, items).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<Binding>,
    /// Nested symbols.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Symbol>,
}

impl Symbol {
    pub fn new(name: impl Into<String>, kind: SymbolKind, start_line: usize) -> Self {
        Self {
            name: name.into(),
            kind,
            start_line,
            end_line: start_line,
            signature: String::new(),
            bindings: Vec::new(),
            children: Vec::new(),
        }
    }
}

/// A name binding visible within a scope (variable, parameter, or item).
#[derive(Debug, Clone, Serialize)]
pub struct Binding {
    pub name: String,
    pub kind: SymbolKind,
    pub line: usize,
}
