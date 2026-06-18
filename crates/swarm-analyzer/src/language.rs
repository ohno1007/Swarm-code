//! Language backends, each backed by a tree-sitter grammar.
//!
//! A backend is just a tree-sitter [`Language`] plus a table of [`NodeRule`]s
//! describing which grammar nodes are declarations. Adding a language = adding
//! its grammar crate and one rule table here.

use tree_sitter::Language;

use crate::parser::{parse_with, NodeRule};
use crate::symbol::{Symbol, SymbolKind};

/// A language-specific source analyzer.
pub trait LanguageBackend: Send + Sync {
    fn id(&self) -> &str;
    /// File extensions handled by this backend (without the dot).
    fn extensions(&self) -> &[&str];
    /// Parse source into a scope/symbol tree.
    fn parse(&self, file_name: &str, source: &str) -> Symbol;
}

/// A backend implemented on top of a tree-sitter grammar + rule table.
pub struct TreeSitterBackend {
    id: &'static str,
    extensions: &'static [&'static str],
    language: Language,
    rules: &'static [NodeRule],
}

impl TreeSitterBackend {
    pub fn new(
        id: &'static str,
        extensions: &'static [&'static str],
        language: Language,
        rules: &'static [NodeRule],
    ) -> Self {
        Self {
            id,
            extensions,
            language,
            rules,
        }
    }
}

impl LanguageBackend for TreeSitterBackend {
    fn id(&self) -> &str {
        self.id
    }

    fn extensions(&self) -> &[&str] {
        self.extensions
    }

    fn parse(&self, file_name: &str, source: &str) -> Symbol {
        parse_with(file_name, source, &self.language, self.rules)
    }
}

// ---- Rule tables ---------------------------------------------------------

const RUST_RULES: &[NodeRule] = &[
    NodeRule::new("mod_item", SymbolKind::Module, "name"),
    NodeRule::new("function_item", SymbolKind::Function, "name"),
    NodeRule::new("struct_item", SymbolKind::Struct, "name"),
    NodeRule::new("enum_item", SymbolKind::Enum, "name"),
    NodeRule::new("union_item", SymbolKind::Union, "name"),
    NodeRule::new("trait_item", SymbolKind::Trait, "name"),
    NodeRule::new("impl_item", SymbolKind::Impl, "type"),
    NodeRule::new("const_item", SymbolKind::Const, "name"),
    NodeRule::new("static_item", SymbolKind::Static, "name"),
    NodeRule::new("type_item", SymbolKind::TypeAlias, "name"),
    NodeRule::new("macro_definition", SymbolKind::Function, "name"),
];

const PYTHON_RULES: &[NodeRule] = &[
    NodeRule::new("function_definition", SymbolKind::Function, "name"),
    NodeRule::new("class_definition", SymbolKind::Class, "name"),
];

const JAVASCRIPT_RULES: &[NodeRule] = &[
    NodeRule::new("function_declaration", SymbolKind::Function, "name"),
    NodeRule::new("generator_function_declaration", SymbolKind::Function, "name"),
    NodeRule::new("class_declaration", SymbolKind::Class, "name"),
    NodeRule::new("method_definition", SymbolKind::Method, "name"),
];

const GO_RULES: &[NodeRule] = &[
    NodeRule::new("function_declaration", SymbolKind::Function, "name"),
    NodeRule::new("method_declaration", SymbolKind::Method, "name"),
    NodeRule::new("type_spec", SymbolKind::Struct, "name"),
];

fn rust_backend() -> TreeSitterBackend {
    TreeSitterBackend::new("rust", &["rs"], tree_sitter_rust::LANGUAGE.into(), RUST_RULES)
}

fn python_backend() -> TreeSitterBackend {
    TreeSitterBackend::new(
        "python",
        &["py", "pyi"],
        tree_sitter_python::LANGUAGE.into(),
        PYTHON_RULES,
    )
}

fn javascript_backend() -> TreeSitterBackend {
    TreeSitterBackend::new(
        "javascript",
        &["js", "jsx", "mjs", "cjs"],
        tree_sitter_javascript::LANGUAGE.into(),
        JAVASCRIPT_RULES,
    )
}

fn go_backend() -> TreeSitterBackend {
    TreeSitterBackend::new("go", &["go"], tree_sitter_go::LANGUAGE.into(), GO_RULES)
}

/// Registry mapping file extensions to language backends.
pub struct LanguageRegistry {
    backends: Vec<Box<dyn LanguageBackend>>,
}

impl LanguageRegistry {
    /// Registry with all built-in tree-sitter backends.
    pub fn with_builtins() -> Self {
        Self {
            backends: vec![
                Box::new(rust_backend()),
                Box::new(python_backend()),
                Box::new(javascript_backend()),
                Box::new(go_backend()),
            ],
        }
    }

    pub fn register(&mut self, backend: Box<dyn LanguageBackend>) {
        self.backends.push(backend);
    }

    pub fn for_extension(&self, ext: &str) -> Option<&dyn LanguageBackend> {
        self.backends
            .iter()
            .find(|b| b.extensions().contains(&ext))
            .map(|b| b.as_ref())
    }

    /// Ids of all registered languages.
    pub fn languages(&self) -> Vec<&str> {
        self.backends.iter().map(|b| b.id()).collect()
    }
}

impl Default for LanguageRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
