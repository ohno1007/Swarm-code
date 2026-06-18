//! Syntax & scope analysis for Swarm-code.
//!
//! Gives agents a fast structural understanding of source files: an outline of
//! symbols and the lexical scope tree (what is bound where). The default Rust
//! backend is dependency-free; see [`language::LanguageBackend`] to add more
//! languages.

pub mod language;
pub mod parser;
pub mod symbol;

use std::path::Path;

pub use language::{LanguageBackend, LanguageRegistry, TreeSitterBackend};
pub use symbol::{Binding, Symbol, SymbolKind};

#[derive(Debug, thiserror::Error)]
pub enum AnalyzerError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("no backend for extension {0:?}")]
    UnsupportedExtension(String),
}

/// High-level entry point that owns the language registry.
pub struct Analyzer {
    registry: LanguageRegistry,
}

impl Analyzer {
    pub fn new() -> Self {
        Self {
            registry: LanguageRegistry::with_builtins(),
        }
    }

    pub fn registry_mut(&mut self) -> &mut LanguageRegistry {
        &mut self.registry
    }

    /// Analyze in-memory source for a given file name (used for extension lookup).
    pub fn analyze_source(&self, file_name: &str, source: &str) -> Result<Symbol, AnalyzerError> {
        let ext = Path::new(file_name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        let backend = self
            .registry
            .for_extension(ext)
            .ok_or_else(|| AnalyzerError::UnsupportedExtension(ext.to_string()))?;
        Ok(backend.parse(file_name, source))
    }

    /// Read and analyze a file from disk.
    pub fn analyze_path(&self, path: impl AsRef<Path>) -> Result<Symbol, AnalyzerError> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("source");
        self.analyze_source(name, &source)
    }
}

impl Default for Analyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a symbol tree as an indented outline (handy for prompts and the CLI).
pub fn render_outline(root: &Symbol) -> String {
    let mut out = String::new();
    render_node(root, 0, &mut out);
    out
}

fn render_node(sym: &Symbol, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);
    if depth == 0 {
        out.push_str(&format!("{} ({})\n", sym.name, sym.kind.as_str()));
    } else {
        let sig = if sym.signature.is_empty() {
            sym.name.clone()
        } else {
            sym.signature.clone()
        };
        out.push_str(&format!(
            "{}{} {} [{}-{}]\n",
            indent,
            sym.kind.as_str(),
            sig,
            sym.start_line,
            sym.end_line
        ));
    }
    for child in &sym.children {
        render_node(child, depth + 1, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_basic_items() {
        let src = r#"
mod outer {
    pub struct Foo { x: i32 }

    impl Foo {
        pub fn bar(&self, count: usize) -> i32 {
            let total = count as i32;
            if total > 0 {
                let inner = total * 2;
                inner
            } else {
                0
            }
        }
    }
}

fn main() {
    let foo = Foo { x: 1 };
}
"#;
        let analyzer = Analyzer::new();
        let root = analyzer.analyze_source("lib.rs", src).unwrap();

        // Top-level: module `outer` and `fn main`.
        let names: Vec<_> = root.children.iter().map(|c| c.name.as_str()).collect();
        assert!(names.contains(&"outer"), "got {names:?}");
        assert!(names.contains(&"main"), "got {names:?}");

        let outer = root.children.iter().find(|c| c.name == "outer").unwrap();
        let impl_block = outer
            .children
            .iter()
            .find(|c| c.kind == SymbolKind::Impl)
            .expect("impl block");
        let bar = impl_block
            .children
            .iter()
            .find(|c| c.name == "bar")
            .expect("method bar");
        assert_eq!(bar.kind, SymbolKind::Function);
        // `count` parameter is bound in bar's scope.
        assert!(
            bar.bindings.iter().any(|b| b.name == "count"),
            "bindings: {:?}",
            bar.bindings
        );
    }
}
