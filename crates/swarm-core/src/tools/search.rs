//! Search tools: content grep and filename find, over the workspace overlay.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};

use crate::tool::{Tool, ToolContext};

const IGNORED_DIRS: &[&str] = &["target", ".git", ".swarm", "node_modules", ".venv", "dist"];
const MAX_RESULTS: usize = 200;

/// Recursively walk the workspace, skipping ignored/hidden directories.
fn walk(root: &Path, ext_filter: Option<&str>, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            if name.starts_with('.') || IGNORED_DIRS.contains(&name.as_ref()) {
                continue;
            }
            walk(&path, ext_filter, out);
        } else {
            if let Some(ext) = ext_filter {
                if path.extension().and_then(|e| e.to_str()) != Some(ext) {
                    continue;
                }
            }
            out.push(path);
        }
    }
}

/// Grep file contents for a regex (searches the staged overlay).
pub struct Search;

#[async_trait]
impl Tool for Search {
    fn name(&self) -> &str {
        "search"
    }

    fn description(&self) -> &str {
        "Search file contents across the workspace with a regular expression \
         (like grep -rn). Returns `path:line: text` matches. Searches the staged \
         overlay, so uncommitted edits are included."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regular expression to search for." },
                "path": { "type": "string", "description": "Optional subdirectory to scope the search (relative to workspace)." },
                "ext": { "type": "string", "description": "Optional file extension filter (without dot), e.g. 'rs'." },
                "max_results": { "type": "integer", "description": "Cap on matches (default 200)." }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: pattern"))?;
        let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
        let ext = args["ext"].as_str();
        let limit = args["max_results"].as_u64().map(|n| n as usize).unwrap_or(MAX_RESULTS);

        let root = match args["path"].as_str() {
            Some(p) => ctx.workspace.join(p),
            None => ctx.workspace.clone(),
        };

        let mut files = Vec::new();
        walk(&root, ext, &mut files);
        files.sort();

        let mut hits = Vec::new();
        'outer: for file in files {
            let rel = file.strip_prefix(&ctx.workspace).unwrap_or(&file);
            // Read via overlay so staged edits are searched too.
            let content = match ctx.coordinator.buffer.read(rel) {
                Ok(Some(c)) => c,
                _ => continue,
            };
            for (i, line) in content.lines().enumerate() {
                if re.is_match(line) {
                    hits.push(format!("{}:{}: {}", rel.display(), i + 1, line.trim_end()));
                    if hits.len() >= limit {
                        hits.push(format!("… (truncated at {limit} matches)"));
                        break 'outer;
                    }
                }
            }
        }

        if hits.is_empty() {
            Ok(format!("no matches for /{pattern}/"))
        } else {
            Ok(hits.join("\n"))
        }
    }
}

/// Find files by a substring/regex on their path.
pub struct FindFiles;

#[async_trait]
impl Tool for FindFiles {
    fn name(&self) -> &str {
        "find_files"
    }

    fn description(&self) -> &str {
        "List workspace files whose path matches a regular expression. Useful \
         for locating files by name."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex matched against each relative path." },
                "ext": { "type": "string", "description": "Optional extension filter (without dot)." }
            },
            "required": ["pattern"]
        })
    }

    async fn execute(&self, args: Value, ctx: &ToolContext) -> anyhow::Result<String> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("missing required argument: pattern"))?;
        let re = Regex::new(pattern).map_err(|e| anyhow::anyhow!("invalid regex: {e}"))?;
        let ext = args["ext"].as_str();

        let mut files = Vec::new();
        walk(&ctx.workspace, ext, &mut files);

        let mut matches: Vec<String> = files
            .iter()
            .filter_map(|f| f.strip_prefix(&ctx.workspace).ok())
            .map(|p| p.display().to_string())
            .filter(|p| re.is_match(p))
            .collect();
        matches.sort();
        matches.truncate(MAX_RESULTS);

        if matches.is_empty() {
            Ok(format!("no files match /{pattern}/"))
        } else {
            Ok(matches.join("\n"))
        }
    }
}
