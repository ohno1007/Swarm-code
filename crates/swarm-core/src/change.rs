//! Change Buffer: a git-staging-area-like overlay over the workspace.
//!
//! Agents never write to disk directly. They stage writes/deletes/symbol-edits
//! here; reads see the staged overlay on top of disk. Commit/discard are
//! author-scoped.
//!
//! Granularity is **symbol-level**: a file edited via [`ChangeBuffer::stage_symbol_edit`]
//! keeps each agent's symbol edits separate, so two agents can edit two
//! functions in one file and commit them independently. Symbol edits re-locate
//! their target via tree-sitter at apply time, so they survive line shifts
//! caused by other commits. Whole-file writes (new files, non-symbol edits) are
//! still supported and committed as a unit.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::events::{Event, EventBus};

#[derive(Clone, Debug)]
enum Staged {
    Write(String),
    Delete,
}

/// One agent's replacement of a named symbol's full source.
#[derive(Clone, Debug)]
struct SymbolEdit {
    symbol: String,
    new_source: String,
    author: String,
}

/// What is staged for a single file.
enum FileEntry {
    /// A whole-file write or delete (commit unit = the file).
    Whole {
        staged: Staged,
        contributors: Vec<String>,
    },
    /// A set of independent symbol edits over the on-disk file.
    Symbolic { edits: Vec<SymbolEdit> },
}

impl FileEntry {
    fn has(&self, author: &str) -> bool {
        match self {
            FileEntry::Whole { contributors, .. } => contributors.iter().any(|a| a == author),
            FileEntry::Symbolic { edits } => edits.iter().any(|e| e.author == author),
        }
    }

    fn authors(&self) -> Vec<String> {
        match self {
            FileEntry::Whole { contributors, .. } => contributors.clone(),
            FileEntry::Symbolic { edits } => {
                let mut a: Vec<String> = Vec::new();
                for e in edits {
                    if !a.contains(&e.author) {
                        a.push(e.author.clone());
                    }
                }
                a
            }
        }
    }

    fn kind(&self) -> &'static str {
        match self {
            FileEntry::Whole {
                staged: Staged::Write(_),
                ..
            } => "write",
            FileEntry::Whole {
                staged: Staged::Delete,
                ..
            } => "delete",
            FileEntry::Symbolic { .. } => "edit",
        }
    }
}

/// Shared staging area for the workspace.
pub struct ChangeBuffer {
    workspace: PathBuf,
    entries: Mutex<HashMap<PathBuf, FileEntry>>,
    events: EventBus,
}

/// Summary of one pending change.
#[derive(Clone, Debug)]
pub struct PendingChange {
    pub path: PathBuf,
    pub authors: Vec<String>,
    pub kind: &'static str,
}

impl ChangeBuffer {
    pub fn new(workspace: PathBuf, events: EventBus) -> Self {
        Self {
            workspace,
            entries: Mutex::new(HashMap::new()),
            events,
        }
    }

    fn abs(&self, rel: &Path) -> PathBuf {
        self.workspace.join(rel)
    }

    fn base_content(&self, rel: &Path) -> String {
        std::fs::read_to_string(self.abs(rel)).unwrap_or_default()
    }

    // ---- staging --------------------------------------------------------

    /// Stage a whole-file write (create/overwrite).
    pub fn stage_write(&self, rel: impl AsRef<Path>, content: String, author: &str) {
        self.stage_whole(rel.as_ref().to_path_buf(), Staged::Write(content), author);
    }

    /// Stage a whole-file deletion.
    pub fn stage_delete(&self, rel: impl AsRef<Path>, author: &str) {
        self.stage_whole(rel.as_ref().to_path_buf(), Staged::Delete, author);
    }

    fn stage_whole(&self, rel: PathBuf, staged: Staged, author: &str) {
        {
            let mut entries = self.entries.lock().unwrap();
            match entries.get_mut(&rel) {
                Some(FileEntry::Whole {
                    staged: s,
                    contributors,
                }) => {
                    *s = staged;
                    if !contributors.iter().any(|a| a == author) {
                        contributors.push(author.to_string());
                    }
                }
                _ => {
                    entries.insert(
                        rel.clone(),
                        FileEntry::Whole {
                            staged,
                            contributors: vec![author.to_string()],
                        },
                    );
                }
            }
        }
        self.publish_staged(rel, author);
    }

    /// Stage a symbol-level edit: replace `symbol`'s full source with
    /// `new_source`. Kept separate per agent for independent commits.
    pub fn stage_symbol_edit(
        &self,
        rel: impl AsRef<Path>,
        symbol: &str,
        new_source: String,
        author: &str,
    ) {
        let rel = rel.as_ref().to_path_buf();
        {
            let mut entries = self.entries.lock().unwrap();
            match entries.get_mut(&rel) {
                // A whole-file write is already staged: fold the symbol edit
                // into that content (degrades to whole-file granularity).
                Some(FileEntry::Whole {
                    staged: Staged::Write(content),
                    contributors,
                }) => {
                    let fname = file_name(&rel);
                    if let Some(updated) =
                        swarm_analyzer::replace_symbol(&fname, content, symbol, &new_source)
                    {
                        *content = updated;
                    }
                    if !contributors.iter().any(|a| a == author) {
                        contributors.push(author.to_string());
                    }
                }
                Some(FileEntry::Symbolic { edits }) => {
                    edits.retain(|e| !(e.symbol == symbol && e.author == author));
                    edits.push(SymbolEdit {
                        symbol: symbol.to_string(),
                        new_source,
                        author: author.to_string(),
                    });
                }
                _ => {
                    entries.insert(
                        rel.clone(),
                        FileEntry::Symbolic {
                            edits: vec![SymbolEdit {
                                symbol: symbol.to_string(),
                                new_source,
                                author: author.to_string(),
                            }],
                        },
                    );
                }
            }
        }
        self.publish_staged(rel, author);
    }

    fn publish_staged(&self, path: PathBuf, author: &str) {
        self.events.publish(Event::Staged {
            path,
            author: author.to_string(),
        });
    }

    // ---- reading --------------------------------------------------------

    /// Read a file through the overlay. `Ok(None)` if (staged) deleted/absent.
    pub fn read(&self, rel: impl AsRef<Path>) -> std::io::Result<Option<String>> {
        let rel = rel.as_ref();
        let entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get(rel) {
            return Ok(self.materialize(rel, entry));
        }
        drop(entries);
        match std::fs::read_to_string(self.abs(rel)) {
            Ok(c) => Ok(Some(c)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Compute a file's overlay content (disk + staged edits). `None` if deleted.
    fn materialize(&self, rel: &Path, entry: &FileEntry) -> Option<String> {
        match entry {
            FileEntry::Whole {
                staged: Staged::Write(c),
                ..
            } => Some(c.clone()),
            FileEntry::Whole {
                staged: Staged::Delete,
                ..
            } => None,
            FileEntry::Symbolic { edits } => {
                Some(apply_edits(&self.base_content(rel), &file_name(rel), edits))
            }
        }
    }

    // ---- inspection -----------------------------------------------------

    pub fn pending(&self) -> Vec<PendingChange> {
        let entries = self.entries.lock().unwrap();
        let mut out: Vec<_> = entries
            .iter()
            .map(|(path, e)| PendingChange {
                path: path.clone(),
                authors: e.authors(),
                kind: e.kind(),
            })
            .collect();
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }

    /// Whether `author` contributed to any staged change.
    pub fn has_pending(&self, author: &str) -> bool {
        self.entries.lock().unwrap().values().any(|e| e.has(author))
    }

    /// Render a unified diff of all staged changes against disk.
    pub fn diff(&self) -> String {
        let entries = self.entries.lock().unwrap();
        let mut paths: Vec<_> = entries.keys().cloned().collect();
        paths.sort();

        let mut out = String::new();
        for path in &paths {
            let old = self.base_content(path);
            let new = self.materialize(path, &entries[path]).unwrap_or_default();
            out.push_str(&format!("--- a/{}\n+++ b/{}\n", path.display(), path.display()));
            let diff = similar::TextDiff::from_lines(&old, &new);
            for change in diff.iter_all_changes() {
                let sign = match change.tag() {
                    similar::ChangeTag::Delete => "-",
                    similar::ChangeTag::Insert => "+",
                    similar::ChangeTag::Equal => " ",
                };
                out.push_str(sign);
                out.push_str(change.value());
                if !change.value().ends_with('\n') {
                    out.push('\n');
                }
            }
            out.push('\n');
        }
        if out.is_empty() {
            out.push_str("(no staged changes)\n");
        }
        out
    }

    // ---- commit / discard ----------------------------------------------

    /// Apply `author`'s staged changes to disk, leaving others' work staged.
    /// Returns the affected paths.
    pub fn commit(&self, author: &str) -> std::io::Result<Vec<PathBuf>> {
        let mut applied = Vec::new();
        {
            let mut entries = self.entries.lock().unwrap();
            let paths: Vec<PathBuf> = entries.keys().cloned().collect();
            for rel in paths {
                let abs = self.abs(&rel);
                let entry = entries.get_mut(&rel).unwrap();
                match entry {
                    FileEntry::Whole {
                        staged,
                        contributors,
                    } => {
                        if !contributors.iter().any(|a| a == author) {
                            continue;
                        }
                        let staged = staged.clone();
                        apply_whole(&abs, &staged)?;
                        entries.remove(&rel);
                        applied.push(rel);
                    }
                    FileEntry::Symbolic { edits } => {
                        let mine: Vec<SymbolEdit> =
                            edits.iter().filter(|e| e.author == author).cloned().collect();
                        if mine.is_empty() {
                            continue;
                        }
                        edits.retain(|e| e.author != author);
                        let empty = edits.is_empty();

                        // Apply only this author's symbol edits to current disk.
                        let mut content = std::fs::read_to_string(&abs).unwrap_or_default();
                        let fname = file_name(&rel);
                        for e in &mine {
                            if let Some(u) =
                                swarm_analyzer::replace_symbol(&fname, &content, &e.symbol, &e.new_source)
                            {
                                content = u;
                            }
                        }
                        if let Some(parent) = abs.parent() {
                            std::fs::create_dir_all(parent)?;
                        }
                        std::fs::write(&abs, &content)?;

                        if empty {
                            entries.remove(&rel);
                        }
                        applied.push(rel);
                    }
                }
            }
        }
        applied.sort();
        self.events.publish(Event::Committed {
            paths: applied.clone(),
            author: author.to_string(),
        });
        Ok(applied)
    }

    /// Discard `author`'s staged changes (others' remain).
    pub fn discard(&self, author: &str) {
        {
            let mut entries = self.entries.lock().unwrap();
            let paths: Vec<PathBuf> = entries.keys().cloned().collect();
            for rel in paths {
                let remove = match entries.get_mut(&rel).unwrap() {
                    FileEntry::Whole { contributors, .. } => contributors.iter().any(|a| a == author),
                    FileEntry::Symbolic { edits } => {
                        edits.retain(|e| e.author != author);
                        edits.is_empty()
                    }
                };
                if remove {
                    entries.remove(&rel);
                }
            }
        }
        self.events.publish(Event::Discarded {
            author: author.to_string(),
        });
    }
}

/// Apply a sequence of symbol edits to `base`, re-locating each symbol against
/// the running content (robust to line shifts).
fn apply_edits(base: &str, file_name: &str, edits: &[SymbolEdit]) -> String {
    let mut content = base.to_string();
    for e in edits {
        if let Some(u) = swarm_analyzer::replace_symbol(file_name, &content, &e.symbol, &e.new_source) {
            content = u;
        }
    }
    content
}

fn apply_whole(abs: &Path, staged: &Staged) -> std::io::Result<()> {
    match staged {
        Staged::Write(content) => {
            if let Some(parent) = abs.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(abs, content)?;
        }
        Staged::Delete => {
            let _ = std::fs::remove_file(abs);
        }
    }
    Ok(())
}

fn file_name(rel: &Path) -> String {
    rel.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("source")
        .to_string()
}
