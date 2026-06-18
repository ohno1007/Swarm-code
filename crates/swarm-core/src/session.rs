//! Concurrent session management.
//!
//! Each [`Session`] owns a lead agent and its conversation. The
//! [`SessionManager`] holds many sessions and lets them run concurrently:
//! every session is behind its own async mutex, so different sessions make
//! progress in parallel while turns within one session stay ordered.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::swarm::Swarm;
use crate::tool::ToolContext;

/// A single interactive session: one lead agent + its tool context.
pub struct Session {
    pub id: Uuid,
    pub title: String,
    agent: Agent,
    ctx: ToolContext,
}

impl Session {
    fn new(swarm: &Arc<Swarm>, title: impl Into<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            title: title.into(),
            agent: swarm.lead_agent(),
            ctx: swarm.lead_context(),
        }
    }

    /// Send a user message and run the agent to a final answer.
    pub async fn send(&mut self, input: impl Into<String>) -> anyhow::Result<String> {
        self.agent.push_user(input);
        self.agent.run(&self.ctx).await
    }

    /// Like [`Session::send`] but streams text deltas to `sink`.
    pub async fn send_streaming(
        &mut self,
        input: impl Into<String>,
        sink: swarm_llm::DeltaSink,
    ) -> anyhow::Result<String> {
        self.agent.push_user(input);
        self.agent.run_with_sink(&self.ctx, Some(sink)).await
    }

    pub fn turns(&self) -> usize {
        self.agent.history().len()
    }
}

/// Owns and coordinates concurrent sessions.
#[derive(Clone)]
pub struct SessionManager {
    swarm: Arc<Swarm>,
    sessions: Arc<Mutex<HashMap<Uuid, Arc<Mutex<Session>>>>>,
}

impl SessionManager {
    pub fn new(swarm: Arc<Swarm>) -> Self {
        Self {
            swarm,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Create a new session and return its id.
    pub async fn create(&self, title: impl Into<String>) -> Uuid {
        let session = Session::new(&self.swarm, title);
        let id = session.id;
        self.sessions
            .lock()
            .await
            .insert(id, Arc::new(Mutex::new(session)));
        id
    }

    pub async fn get(&self, id: Uuid) -> Option<Arc<Mutex<Session>>> {
        self.sessions.lock().await.get(&id).cloned()
    }

    /// Send input to a session. Acquires only that session's lock, so other
    /// sessions can run concurrently.
    pub async fn send(&self, id: Uuid, input: impl Into<String>) -> anyhow::Result<String> {
        let session = self
            .get(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("no such session: {id}"))?;
        let mut guard = session.lock().await;
        guard.send(input).await
    }

    /// Streaming variant of [`SessionManager::send`].
    pub async fn send_streaming(
        &self,
        id: Uuid,
        input: impl Into<String>,
        sink: swarm_llm::DeltaSink,
    ) -> anyhow::Result<String> {
        let session = self
            .get(id)
            .await
            .ok_or_else(|| anyhow::anyhow!("no such session: {id}"))?;
        let mut guard = session.lock().await;
        guard.send_streaming(input, sink).await
    }

    /// List `(id, title, turns)` for all live sessions.
    pub async fn list(&self) -> Vec<(Uuid, String, usize)> {
        let map = self.sessions.lock().await;
        let mut out = Vec::with_capacity(map.len());
        for (id, s) in map.iter() {
            let s = s.lock().await;
            out.push((*id, s.title.clone(), s.turns()));
        }
        out
    }

    pub async fn close(&self, id: Uuid) -> bool {
        self.sessions.lock().await.remove(&id).is_some()
    }

    pub fn swarm(&self) -> &Arc<Swarm> {
        &self.swarm
    }
}
