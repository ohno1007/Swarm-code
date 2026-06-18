//! Change Buffer: a git-staging-area-like overlay over the workspace.
//!
//! Agents never write to disk directly. They stage writes/deletes here; reads
//! see the staged overlay on top of disk. A `commit` flushes everything to disk
//! atomically(ish) and clears the buffer; a `discard` throws staged work away.
//! This gives the coordinator a single place to review, validate and apply the
//! swarm's edits.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::events::{Event, EventBus};

#[derive(Clone, Debug)]
enum Staged {
    Write(String),
    Delete,
}

struct Entry {
    staged: Staged,
    /// Every agent that contributed to this file's staged content. A file is
    /// the commit unit, so committing/discarding by any contributor applies to
    /// the whole entry.
    contributors: Vec<String>,
}

impl Entry {
    fn add_contributor(&mut self, author: &str) {
        if !self.contributors.iter().any(|a| a == author) {
            self.contributors.push(author.to_string());
        }
    }

    fn has(&self, author: &str) -> bool {
        self.contributors.iter().any(|a| a == author)
    }
}

/// Shared staging area for the workspace.
pub struct ChangeBuffer {
    workspace: PathBuf,
    entries: Mutex<HashMap<PathBuf, Entry>>,
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

    /// Stage a write (create/overwrite).
    pub fn stage_write(&self, rel: impl AsRef<Path>, content: String, author: &str) {
        self.stage(rel.as_ref().to_path_buf(), Staged::Write(content), author);
    }

    /// Stage a deletion.
    pub fn stage_delete(&self, rel: impl AsRef<Path>, author: &str) {
        self.stage(rel.as_ref().to_path_buf(), Staged::Delete, author);
    }

    fn stage(&self, rel: PathBuf, staged: Staged, author: &str) {
        {
            let mut entries = self.entries.lock().unwrap();
            let entry = entries.entry(rel.clone()).or_insert_with(|| Entry {
                staged: staged.clone(),
                contributors: Vec::new(),
            });
            entry.staged = staged;
            entry.add_contributor(author);
        }
        self.events.publish(Event::Staged {
            path: rel,
            author: author.to_string(),
        });
    }

    /// Read a file through the overlay: staged content wins; otherwise disk.
    /// Returns `Ok(None)` if the file is (staged) deleted or absent.
    pub fn read(&self, rel: impl AsRef<Path>) -> std::io::Result<Option<String>> {
        let rel = rel.as_ref();
        if let Some(entry) = self.entries.lock().unwrap().get(rel) {
            return Ok(match &entry.staged {
                Staged::Write(c) => Some(c.clone()),
                Staged::Delete => None,
            });
        }
        match std::fs::read_to_string(self.abs(rel)) {
            Ok(c) => Ok(Some(c)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// List pending changes.
    pub fn pending(&self) -> Vec<PendingChange> {
        let entries = self.entries.lock().unwrap();
        let mut out: Vec<_> = entries
            .iter()
            .map(|(path, e)| PendingChange {
                path: path.clone(),
                authors: e.contributors.clone(),
                kind: match e.staged {
                    Staged::Write(_) => "write",
                    Staged::Delete => "delete",
                },
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
        for path in paths {
            let entry = &entries[&path];
            let old = std::fs::read_to_string(self.abs(&path)).unwrap_or_default();
            let new = match &entry.staged {
                Staged::Write(c) => c.clone(),
                Staged::Delete => String::new(),
            };
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

    /// Apply `author`'s staged changes to disk, leaving other authors' staged
    /// work untouched. Returns the list of affected paths.
    pub fn commit(&self, author: &str) -> std::io::Result<Vec<PathBuf>> {
        let drained: Vec<(PathBuf, Staged)> = {
            let mut entries = self.entries.lock().unwrap();
            let keys: Vec<PathBuf> = entries
                .iter()
                .filter(|(_, e)| e.has(author))
                .map(|(p, _)| p.clone())
                .collect();
            keys.into_iter()
                .map(|k| {
                    let e = entries.remove(&k).unwrap();
                    (k, e.staged)
                })
                .collect()
        };
        let mut applied = Vec::new();
        for (rel, staged) in drained {
            let abs = self.abs(&rel);
            match staged {
                Staged::Write(content) => {
                    if let Some(parent) = abs.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&abs, content)?;
                }
                Staged::Delete => {
                    let _ = std::fs::remove_file(&abs);
                }
            }
            applied.push(rel);
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
            let keys: Vec<PathBuf> = entries
                .iter()
                .filter(|(_, e)| e.has(author))
                .map(|(p, _)| p.clone())
                .collect();
            for k in keys {
                entries.remove(&k);
            }
        }
        self.events.publish(Event::Discarded {
            author: author.to_string(),
        });
    }
}
