//! The coordination layer shared by every agent in a workspace.
//!
//! Bundles the [`ChangeBuffer`], [`LockManager`], [`EventBus`] and the compile
//! [`Validator`] so the whole swarm — lead agent, workers, concurrent sessions
//! — coordinates through one git-like surface: stage → lock → commit → validate,
//! with events propagating throughout.

use std::path::PathBuf;
use std::sync::Arc;

use crate::change::ChangeBuffer;
use crate::events::{Event, EventBus};
use crate::lock::LockManager;
use crate::mcp::McpManager;
use crate::memory::MemoryStore;
use crate::skills::SkillStore;
use crate::validate::{self, ValidationResult};

pub struct Coordinator {
    pub workspace: PathBuf,
    pub events: EventBus,
    pub buffer: Arc<ChangeBuffer>,
    pub locks: Arc<LockManager>,
    /// Durable, workspace-scoped long-term memory.
    pub memory: Arc<MemoryStore>,
    /// Agent/user-authored skills.
    pub skills: Arc<SkillStore>,
    /// External MCP servers and their tools.
    pub mcp: Arc<McpManager>,
}

impl Coordinator {
    pub fn new(workspace: PathBuf) -> Arc<Self> {
        let events = EventBus::new();
        let buffer = Arc::new(ChangeBuffer::new(workspace.clone(), events.clone()));
        let locks = Arc::new(LockManager::new(events.clone()));
        let memory = Arc::new(MemoryStore::load(&workspace));
        let skills = Arc::new(SkillStore::new(&workspace));
        let mcp = Arc::new(McpManager::new(&workspace));
        Arc::new(Self {
            workspace,
            events,
            buffer,
            locks,
            memory,
            skills,
            mcp,
        })
    }

    /// Apply staged changes to disk, release the author's locks, then run a
    /// compile check if one applies. Returns affected paths + validation.
    pub async fn commit(
        &self,
        author: &str,
    ) -> std::io::Result<(Vec<PathBuf>, Option<ValidationResult>)> {
        let paths = self.buffer.commit(author)?;
        self.locks.release_all(author);

        let validation = match validate::for_workspace(&self.workspace) {
            Some(validator) => {
                let result = validator.validate(&self.workspace).await;
                self.events.publish(Event::Validated {
                    ok: result.ok,
                    summary: result.summary(),
                });
                Some(result)
            }
            None => None,
        };
        Ok((paths, validation))
    }

    /// Discard the author's staged changes and locks.
    pub fn discard(&self, author: &str) {
        self.buffer.discard(author);
        self.locks.release_all(author);
    }
}
