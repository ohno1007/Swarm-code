//! The memory system.
//!
//! Two layers:
//!
//! * [`WorkingMemory`] — an agent's short-term conversation buffer. When it
//!   grows past a token budget it is *compacted*: the oldest whole turns are
//!   replaced by an LLM-written summary, preserving decisions/facts/edits while
//!   freeing context. Recent turns are kept verbatim.
//!
//! * [`MemoryStore`] — long-term, workspace-scoped memory persisted to
//!   `.swarm/memory.json`. Agents `remember` durable facts and `recall` them in
//!   later sessions; a compact digest is injected into the lead agent's prompt.

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use swarm_llm::{ChatRequest, LlmProvider, Message, Role};
use tracing::info;

// ===========================================================================
// Working memory (short-term + compression)
// ===========================================================================

/// An agent's conversation buffer with automatic context compaction.
pub struct WorkingMemory {
    messages: Vec<Message>,
    /// Approximate token budget that triggers compaction.
    max_tokens: usize,
    /// Number of most-recent messages always kept verbatim.
    keep_recent: usize,
}

impl WorkingMemory {
    pub fn new(system_prompt: impl Into<String>) -> Self {
        Self {
            messages: vec![Message::system(system_prompt)],
            max_tokens: 24_000,
            keep_recent: 10,
        }
    }

    pub fn push(&mut self, message: Message) {
        self.messages.push(message);
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Rough token estimate (~4 chars/token).
    fn estimated_tokens(&self) -> usize {
        let chars: usize = self
            .messages
            .iter()
            .map(|m| {
                let body = m.content.as_deref().map(str::len).unwrap_or(0);
                let calls = m
                    .tool_calls
                    .as_ref()
                    .map(|tcs| {
                        tcs.iter()
                            .map(|c| c.function.name.len() + c.function.arguments.len())
                            .sum::<usize>()
                    })
                    .unwrap_or(0);
                body + calls
            })
            .sum();
        chars / 4
    }

    /// Largest safe cut index: summarize `[1..cut)`, keep `[cut..]`. The cut
    /// must land on a `User` message so whole turns (and tool-call/result pairs)
    /// stay intact.
    fn safe_cut(&self) -> Option<usize> {
        let len = self.messages.len();
        if len <= self.keep_recent + 2 {
            return None;
        }
        let target = len - self.keep_recent;
        (2..=target)
            .rev()
            .find(|&i| self.messages[i].role == Role::User)
    }

    /// Compact the buffer if it exceeds the budget. Returns the number of
    /// messages folded into a summary (0 if nothing was done).
    pub async fn compact_if_needed(
        &mut self,
        provider: &dyn LlmProvider,
        model: &str,
    ) -> usize {
        if self.estimated_tokens() <= self.max_tokens {
            return 0;
        }
        let Some(cut) = self.safe_cut() else {
            return 0;
        };

        let block = &self.messages[1..cut];
        let rendered = render_block(block);
        let summary = match summarize(provider, model, &rendered).await {
            Ok(s) => s,
            Err(e) => {
                info!("memory compaction skipped: {e}");
                return 0;
            }
        };

        let summarized = cut - 1;
        let system = self.messages[0].clone();
        let tail = self.messages.split_off(cut);
        self.messages = Vec::with_capacity(tail.len() + 2);
        self.messages.push(system);
        self.messages.push(Message::system(format!(
            "[Memory: summary of {summarized} earlier messages]\n{summary}"
        )));
        self.messages.extend(tail);

        info!(summarized, "compacted working memory");
        summarized
    }
}

fn render_block(messages: &[Message]) -> String {
    let mut out = String::new();
    for m in messages {
        let role = match m.role {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        out.push_str(role);
        out.push_str(": ");
        if let Some(c) = &m.content {
            out.push_str(c);
        }
        if let Some(tcs) = &m.tool_calls {
            for c in tcs {
                out.push_str(&format!(
                    " [calls {}({})]",
                    c.function.name,
                    truncate(&c.function.arguments, 200)
                ));
            }
        }
        out.push('\n');
    }
    truncate(&out, 16_000)
}

async fn summarize(
    provider: &dyn LlmProvider,
    model: &str,
    block: &str,
) -> swarm_llm::Result<String> {
    let request = ChatRequest::new(
        model,
        vec![
            Message::system(
                "You compress an AI coding assistant's conversation. Produce a \
                 dense brief that preserves: decisions made, facts learned about \
                 the codebase, files/symbols created or edited, important tool \
                 results, and any open/pending tasks. Use terse bullet points. \
                 Omit greetings and filler.",
            ),
            Message::user(block.to_string()),
        ],
    )
    .with_temperature(0.0);
    let reply = provider.chat(request).await?;
    Ok(reply.content.unwrap_or_default())
}

// ===========================================================================
// Long-term memory store (persistent, workspace-scoped)
// ===========================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub text: String,
    #[serde(default)]
    pub tags: Vec<String>,
    pub author: String,
    pub created_at: u64,
}

/// Durable, workspace-scoped memory backed by `.swarm/memory.json`.
pub struct MemoryStore {
    path: PathBuf,
    entries: Mutex<Vec<MemoryEntry>>,
}

impl MemoryStore {
    /// Load (or create) the store under `<workspace>/.swarm/memory.json`.
    pub fn load(workspace: &std::path::Path) -> Self {
        let path = workspace.join(".swarm").join("memory.json");
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<MemoryEntry>>(&t).ok())
            .unwrap_or_default();
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&*self.entries.lock().unwrap()) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    /// Store a durable fact, returning its id.
    pub fn remember(&self, text: &str, tags: Vec<String>, author: &str) -> String {
        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        self.entries.lock().unwrap().push(MemoryEntry {
            id: id.clone(),
            text: text.to_string(),
            tags,
            author: author.to_string(),
            created_at: now(),
        });
        self.persist();
        id
    }

    /// Recall entries. With a query, rank by keyword overlap; otherwise return
    /// the most recent. Caps at `limit`.
    pub fn recall(&self, query: Option<&str>, limit: usize) -> Vec<MemoryEntry> {
        let entries = self.entries.lock().unwrap();
        match query {
            None => entries.iter().rev().take(limit).cloned().collect(),
            Some(q) => {
                let terms: Vec<String> = q.to_lowercase().split_whitespace().map(String::from).collect();
                let mut scored: Vec<(usize, &MemoryEntry)> = entries
                    .iter()
                    .filter_map(|e| {
                        let hay = format!("{} {}", e.text, e.tags.join(" ")).to_lowercase();
                        let score = terms.iter().filter(|t| hay.contains(t.as_str())).count();
                        (score > 0).then_some((score, e))
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.created_at.cmp(&a.1.created_at)));
                scored.into_iter().take(limit).map(|(_, e)| e.clone()).collect()
            }
        }
    }

    pub fn forget(&self, id: &str) -> bool {
        let mut entries = self.entries.lock().unwrap();
        let before = entries.len();
        entries.retain(|e| e.id != id);
        let removed = entries.len() != before;
        drop(entries);
        if removed {
            self.persist();
        }
        removed
    }

    pub fn is_empty(&self) -> bool {
        self.entries.lock().unwrap().is_empty()
    }

    /// Compact digest for prompt injection (most recent first).
    pub fn digest(&self, max_entries: usize) -> String {
        let entries = self.entries.lock().unwrap();
        if entries.is_empty() {
            return String::new();
        }
        let mut out = String::new();
        for e in entries.iter().rev().take(max_entries) {
            let tags = if e.tags.is_empty() {
                String::new()
            } else {
                format!(" [{}]", e.tags.join(","))
            };
            out.push_str(&format!("- ({}){tags} {}\n", e.id, truncate(&e.text, 200)));
        }
        out
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}
