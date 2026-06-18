//! Event bus for change propagation.
//!
//! Coordination services (change buffer, lock manager, validator) publish
//! [`Event`]s here; agents, sessions and the REPL subscribe to react to each
//! other's work — the "changes propagate automatically" piece.

use std::path::PathBuf;

use tokio::sync::broadcast;

/// Something noteworthy happened in the shared workspace.
#[derive(Clone, Debug)]
pub enum Event {
    /// A change was staged into the buffer (not yet on disk).
    Staged { path: PathBuf, author: String },
    /// Staged changes were applied to disk.
    Committed { paths: Vec<PathBuf>, author: String },
    /// Staged changes were discarded.
    Discarded { author: String },
    /// A symbol was locked for editing.
    Locked { key: String, owner: String },
    /// A symbol lock was released.
    Released { key: String, owner: String },
    /// A validation run finished.
    Validated { ok: bool, summary: String },
}

impl Event {
    /// One-line human description (used by the REPL notifier).
    pub fn describe(&self) -> String {
        match self {
            Event::Staged { path, author } => {
                format!("[{author}] staged {}", path.display())
            }
            Event::Committed { paths, author } => {
                format!("[{author}] committed {} file(s)", paths.len())
            }
            Event::Discarded { author } => format!("[{author}] discarded changes"),
            Event::Locked { key, owner } => format!("[{owner}] locked {key}"),
            Event::Released { key, owner } => format!("[{owner}] released {key}"),
            Event::Validated { ok, summary } => {
                let status = if *ok { "ok" } else { "FAILED" };
                format!("validation {status}: {summary}")
            }
        }
    }
}

/// Clonable handle to a broadcast event channel.
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<Event>,
}

impl EventBus {
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(256);
        Self { tx }
    }

    /// Publish an event. Ignores the "no subscribers" case.
    pub fn publish(&self, event: Event) {
        let _ = self.tx.send(event);
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.tx.subscribe()
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}
