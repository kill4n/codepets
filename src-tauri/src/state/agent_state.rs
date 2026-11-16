use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize)]
pub struct AgentId { pub host: String, pub session_id: String }

impl AgentId {
    pub fn key(&self) -> String { format!("{}:{}", self.host, self.session_id) }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visible { Working, Idle, NeedsInput, Delegating, WaitingPeer, Compacting, Error, Gone }

#[derive(Debug, Clone)]
pub struct AgentState {
    pub id: AgentId,
    pub name: String,
    pub cwd: Option<PathBuf>,
    pub pid: Option<u32>,
    pub job_id: Option<String>,
    /// Key of the parent agent when this is a subagent sprite.
    pub parent: Option<String>,
    /// A permission prompt CodePets is holding for this session: (tool, summary).
    pub permission: Option<(String, String)>,
    pub visible: Visible,
    pub waiting_for: Option<String>,
    pub peer: Option<String>,
    pub subagents: u32,
    pub sent_message: bool,
    pub since: Instant,
    pub last_hook_at: Option<Instant>,
    pub last_seen: Instant,
    pub missed_polls: u8,
}

/// Session id used for a subagent's own AgentId: `<parent session>/<agent id>`.
pub fn child_session_id(parent_session: &str, agent_id: &str) -> String { format!("{parent_session}/{agent_id}") }

/// Name shown until the poller reports the real one: the first 8 chars of the session id.
pub fn provisional_name(session_id: &str) -> String { session_id.chars().take(8).collect() }

impl AgentState {
    pub fn new(id: AgentId, now: Instant) -> Self {
        let name = provisional_name(&id.session_id);
        Self { id, name, cwd: None, pid: None, job_id: None, parent: None, permission: None, visible: Visible::Idle, waiting_for: None, peer: None,
            subagents: 0, sent_message: false, since: now, last_hook_at: None, last_seen: now, missed_polls: 0 }
    }

    pub fn set_visible(&mut self, v: Visible, now: Instant) {
        if self.visible != v { self.visible = v; self.since = now; }
    }
}
