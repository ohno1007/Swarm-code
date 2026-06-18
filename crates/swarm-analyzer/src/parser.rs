//! Generic tree-sitter walker.
//!
//! Given a tree-sitter [`Language`] and a table of [`NodeRule`]s mapping grammar
//! node kinds to [`SymbolKind`]s, this produces a [`Symbol`] scope tree. All
//! language-specific knowledge lives in the rule tables in [`crate::language`];
//! the traversal itself is language-agnostic.

use tree_sitter::{Language, Node, Parser};

use crate::symbol::{Binding, Symbol, SymbolKind};

/// Maps a grammar node kind to a [`SymbolKind`].
#[derive(Clone, Copy)]
pub struct NodeRule {
    /// tree-sitter node kind, e.g. `"function_item"`.
    pub node_kind: &'static str,
    pub symbol: SymbolKind,
    /// Field name holding the declaration's name node (usually `"name"`;
    /// `"type"` for Rust `impl` blocks).
    pub name_field: &'static str,
}

impl NodeRule {
    pub const fn new(node_kind: &'static str, symbol: SymbolKind, name_field: &'static str) -> Self {
        Self {
            node_kind,
            symbol,
            name_field,
        }
    }
}

/// Parse `source` with the given tree-sitter language and rule table.
pub fn parse_with(
    file_name: &str,
    source: &str,
    language: &Language,
    rules: &[NodeRule],
) -> Symbol {
    let mut root = Symbol::new(file_name, SymbolKind::Module, 1);

    let mut parser = Parser::new();
    if parser.set_language(language).is_err() {
        return root;
    }
    let Some(tree) = parser.parse(source, None) else {
        return root;
    };

    let src = source.as_bytes();
    root.end_line = tree.root_node().end_position().row + 1;
    walk(tree.root_node(), src, rules, &mut root);
    root
}

fn walk(node: Node, src: &[u8], rules: &[NodeRule], parent: &mut Symbol) {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(rule) = rules.iter().find(|r| r.node_kind == child.kind()) {
            let mut sym = make_symbol(child, src, rule);
            if matches!(sym.kind, SymbolKind::Function | SymbolKind::Method) {
                collect_params(child, src, &mut sym);
            }
            // Recurse to discover nested declarations.
            walk(child, src, rules, &mut sym);

            parent.bindings.push(Binding {
                name: sym.name.clone(),
                kind: sym.kind,
                line: sym.start_line,
            });
            parent.children.push(sym);
        } else {
            // Not a declaration node — keep descending under the same parent.
            walk(child, src, rules, parent);
        }
    }
}

fn make_symbol(node: Node, src: &[u8], rule: &NodeRule) -> Symbol {
    let name = node
        .child_by_field_name(rule.name_field)
        .or_else(|| node.child_by_field_name("name"))
        .and_then(|n| n.utf8_text(src).ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| format!("<{}>", rule.node_kind));

    let mut sym = Symbol::new(name, rule.symbol, node.start_position().row + 1);
    sym.end_line = node.end_position().row + 1;
    sym.signature = first_line(node.utf8_text(src).unwrap_or_default());
    sym
}

/// Collect parameter identifiers as `Parameter` bindings in the symbol's scope.
fn collect_params(func: Node, src: &[u8], sym: &mut Symbol) {
    let mut cursor = func.walk();
    let params = func.children(&mut cursor).find(|c| c.kind().contains("parameter"));
    let Some(params) = params else { return };

    let mut pcursor = params.walk();
    for param in params.children(&mut pcursor) {
        if param.kind() == "," || param.kind() == "(" || param.kind() == ")" {
            continue;
        }
        if let Some(name) = first_identifier(param, src) {
            if name != "self" {
                sym.bindings.push(Binding {
                    name,
                    kind: SymbolKind::Parameter,
                    line: param.start_position().row + 1,
                });
            }
        }
    }
}

/// Depth-first search for the first `identifier` leaf under `node`.
fn first_identifier(node: Node, src: &[u8]) -> Option<String> {
    if node.kind() == "identifier" {
        return node.utf8_text(src).ok().map(|s| s.to_string());
    }
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if let Some(found) = first_identifier(child, src) {
            return Some(found);
        }
    }
    None
}

fn first_line(text: &str) -> String {
    let line = text.lines().next().unwrap_or("").trim();
    if line.len() > 120 {
        format!("{}…", &line[..120])
    } else {
        line.to_string()
    }
}
