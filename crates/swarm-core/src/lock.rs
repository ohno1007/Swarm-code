//! Symbol-level locks.
//!
//! Finer-grained than a file lock: two agents can edit two different functions
//! in the same file at once, but not the *same* symbol. Locks are advisory and
//! keyed by `path::symbol`; they are released explicitly or in bulk when an
//! agent commits/discards its work.

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

use crate::events::{Event, EventBus};

/// Tracks who holds which symbol locks.
pub struct LockManager {
    locks: Mutex<HashMap<String, String>>, // key -> owner
    events: EventBus,
}

/// Outcome of a lock attempt.
#[derive(Debug)]
pub enum LockResult {
    Acquired,
    /// Already held by `owner` (possibly the same agent — re-entrant ok).
    Held { owner: String },
}

impl LockManager {
    pub fn new(events: EventBus) -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
            events,
        }
    }

    pub fn key(path: &Path, symbol: &str) -> String {
        format!("{}::{}", path.display(), symbol)
    }

    /// Try to acquire `path::symbol` for `owner`. Re-entrant for the same owner.
    pub fn try_acquire(&self, path: &Path, symbol: &str, owner: &str) -> LockResult {
        let key = Self::key(path, symbol);
        let mut locks = self.locks.lock().unwrap();
        match locks.get(&key) {
            Some(existing) if existing != owner => LockResult::Held {
                owner: existing.clone(),
            },
            Some(_) => LockResult::Acquired, // already ours
            None => {
                locks.insert(key.clone(), owner.to_string());
                drop(locks);
                self.events.publish(Event::Locked {
                    key,
                    owner: owner.to_string(),
                });
                LockResult::Acquired
            }
        }
    }

    /// Release a single lock if owned by `owner`.
    pub fn release(&self, path: &Path, symbol: &str, owner: &str) -> bool {
        let key = Self::key(path, symbol);
        let mut locks = self.locks.lock().unwrap();
        if locks.get(&key).map(|o| o == owner).unwrap_or(false) {
            locks.remove(&key);
            drop(locks);
            self.events.publish(Event::Released {
                key,
                owner: owner.to_string(),
            });
            true
        } else {
            false
        }
    }

    /// Release every lock held by `owner` (called on commit/discard).
    pub fn release_all(&self, owner: &str) {
        let mut locks = self.locks.lock().unwrap();
        let owned: Vec<String> = locks
            .iter()
            .filter(|(_, o)| *o == owner)
            .map(|(k, _)| k.clone())
            .collect();
        for key in &owned {
            locks.remove(key);
        }
        drop(locks);
        for key in owned {
            self.events.publish(Event::Released {
                key,
                owner: owner.to_string(),
            });
        }
    }

    /// Snapshot of all held locks as `(key, owner)`.
    pub fn list(&self) -> Vec<(String, String)> {
        let mut out: Vec<_> = self
            .locks
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        out.sort();
        out
    }
}
