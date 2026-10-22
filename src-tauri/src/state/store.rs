use super::machine::{apply_hook, apply_poll, mark_missed};
use super::agent_state::{child_session_id, provisional_name};
use super::{AgentId, AgentState, Visible};
use crate::collector::event::{AgentEvent, EventKind, PolledAgent, PollKind, PollState};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct AgentView {
    pub key: String,
    pub host: String,
    pub session_id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub pid: Option<u32>,
    pub job_id: Option<String>,
    /// Key of the parent agent when this sprite is a subagent.
    pub parent: Option<String>,
    /// Permission prompt held by CodePets: tool name and a one-line summary of its input.
    pub permission: Option<(String, String)>,
    pub visible: Visible,
    pub waiting_for: Option<String>,
    pub peer: Option<String>,
    pub since_secs: u64,
}

#[derive(Debug, Clone)]
pub enum Change { Added(AgentView), Updated(AgentView), Removed(String) }

#[derive(Debug, Clone, PartialEq)]
struct Fingerprint(Visible, String, Option<String>, Option<String>, Option<(String, String)>);
fn fp(s: &AgentState) -> Fingerprint { Fingerprint(s.visible, s.name.clone(), s.waiting_for.clone(), s.peer.clone(), s.permission.clone()) }

fn make_view(s: &AgentState, now: Instant) -> AgentView {
    AgentView {
        key: s.id.key(), host: s.id.host.clone(), session_id: s.id.session_id.clone(),
        name: s.name.clone(), cwd: s.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()), pid: s.pid, job_id: s.job_id.clone(), parent: s.parent.clone(), permission: s.permission.clone(),
        visible: s.visible, waiting_for: s.waiting_for.clone(), peer: s.peer.clone(),
        since_secs: now.saturating_duration_since(s.since).as_secs(),
    }
}

pub struct Store {
    agents: HashMap<String, AgentState>,
    pub gone_grace: Duration,
}

impl Default for Store { fn default() -> Self { Self::new() } }

impl Store {
    pub fn new() -> Self { Self { agents: HashMap::new(), gone_grace: Duration::from_secs(10) } }

    pub fn view(&self, s: &AgentState, now: Instant) -> AgentView { make_view(s, now) }

    pub fn views(&self, now: Instant) -> Vec<AgentView> {
        let mut v: Vec<_> = self.agents.values().map(|s| self.view(s, now)).collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    fn upsert<F: FnOnce(&mut AgentState)>(&mut self, id: AgentId, now: Instant, f: F) -> Option<Change> {
        let key = id.key();
        let is_new = !self.agents.contains_key(&key);
        let s = self.agents.entry(key).or_insert_with(|| AgentState::new(id, now));
        let before = fp(s);
        f(s);
        let view = make_view(s, now);
        if is_new { Some(Change::Added(view)) }
        else if fp(s) != before { Some(Change::Updated(view)) }
        else { None }
    }

    pub fn apply(&mut self, ev: &AgentEvent) -> Vec<Change> {
        let now = ev.at;
        match &ev.kind {
            EventKind::Hook(h) => {
                if ev.session_id.is_empty() { return vec![]; }
                let id = AgentId { host: ev.host.clone(), session_id: ev.session_id.clone() };
                let parent_key = id.key();
                let mut changes: Vec<Change> = self.upsert(id, now, |s| apply_hook(s, h, now)).into_iter().collect();
                // a hook that names a subagent also drives that subagent's own (smaller) sprite
                if let Some(agent_id) = &h.agent_id {
                    let cid = AgentId { host: ev.host.clone(), session_id: child_session_id(&ev.session_id, agent_id) };
                    let is_stop = h.event == "SubagentStop";
                    if is_stop || self.agents.contains_key(&cid.key()) || h.event == "SubagentStart" || h.event.ends_with("ToolUse") {
                        let name = h.agent_type.clone().unwrap_or_else(|| provisional_name(agent_id));
                        let pk = parent_key.clone();
                        changes.extend(self.upsert(cid, now, |c| {
                            c.parent = Some(pk);
                            if c.name == provisional_name(&c.id.session_id) || h.agent_type.is_some() { c.name = name; }
                            c.last_hook_at = Some(now); c.last_seen = now;
                            let next = if is_stop { Visible::Gone } else if c.subagents > 0 { Visible::Delegating } else { Visible::Working };
                            c.set_visible(next, now);
                        }));
                    }
                }
                // the whole session ended: its subagents go too
                if h.event == "SessionEnd" {
                    let kids: Vec<String> = self.agents.values().filter(|c| c.parent.as_deref() == Some(&parent_key)).map(|c| c.id.key()).collect();
                    for k in kids {
                        if let Some(c) = self.agents.get_mut(&k) {
                            let before = fp(c); c.set_visible(Visible::Gone, now);
                            if fp(c) != before { changes.push(Change::Updated(make_view(c, now))); }
                        }
                    }
                }
                changes
            }
            EventKind::Snapshot(list) => {
                let mut changes = vec![];
                let mut seen = HashSet::new();
                let is_terminal = |p: &PolledAgent| p.kind == PollKind::Background
                    && matches!(p.state, Some(PollState::Done | PollState::Failed | PollState::Stopped));
                // `--all` also lists the finished background record of a session that is running again
                // interactively (same session id): the live record wins, or the agent would flip between
                // gone and working on every poll.
                let live: HashSet<&str> = list.iter().filter(|p| !is_terminal(p)).map(|p| p.session_id.as_str()).collect();
                for p in list {
                    if p.session_id.is_empty() { continue; }
                    let id = AgentId { host: ev.host.clone(), session_id: p.session_id.clone() };
                    let terminal = is_terminal(p);
                    if terminal && live.contains(p.session_id.as_str()) { continue; }
                    // `--all` keeps listing finished background agents: never (re)create one that is
                    // already terminal, or it would come back as a phantom gone window after each sweep.
                    if terminal && !self.agents.contains_key(&id.key()) { continue; }
                    seen.insert(id.key());
                    changes.extend(self.upsert(id, now, |s| apply_poll(s, p, now)));
                }
                for (key, s) in self.agents.iter_mut() {
                    if s.id.host != ev.host || seen.contains(key) || s.visible == Visible::Gone || s.parent.is_some() { continue; }
                    let before = fp(s);
                    mark_missed(s, now);
                    if fp(s) != before {
                        let view = AgentView { key: key.clone(), host: s.id.host.clone(), session_id: s.id.session_id.clone(),
                            name: s.name.clone(), cwd: s.cwd.as_ref().map(|p| p.to_string_lossy().into_owned()), pid: s.pid, job_id: s.job_id.clone(), parent: s.parent.clone(), permission: s.permission.clone(),
                            visible: s.visible, waiting_for: s.waiting_for.clone(), peer: s.peer.clone(), since_secs: 0 };
                        changes.push(Change::Updated(view));
                    }
                }
                changes
            }
        }
    }

    pub fn sweep(&mut self, now: Instant) -> Vec<Change> {
        let grace = self.gone_grace;
        let dead: Vec<String> = self.agents.iter()
            .filter(|(_, s)| s.visible == Visible::Gone && now.saturating_duration_since(s.since) >= grace)
            .map(|(k, _)| k.clone()).collect();
        for k in &dead { self.agents.remove(k); }
        dead.into_iter().map(Change::Removed).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::event::*;
    use std::time::{Duration, Instant};

    fn hook_ev(sid: &str, event: &str, cwd: Option<&str>) -> AgentEvent {
        AgentEvent { host: "h".into(), session_id: sid.into(), at: Instant::now(),
            kind: EventKind::Hook(HookEvent { event: event.into(), cwd: cwd.map(Into::into),
                tool_name: None, notification_type: None, recipient: None, agent_id: None, agent_type: None, tool_summary: None }) }
    }
    fn sub_ev(sid: &str, event: &str, agent: &str, kind: Option<&str>) -> AgentEvent {
        AgentEvent { host: "h".into(), session_id: sid.into(), at: Instant::now(),
            kind: EventKind::Hook(HookEvent { event: event.into(), cwd: None, tool_name: None, notification_type: None,
                recipient: None, agent_id: Some(agent.into()), agent_type: kind.map(Into::into), tool_summary: None }) }
    }

    #[test] fn subagent_start_creates_a_child_sprite_next_to_its_parent() {
        let mut st = Store::new();
        st.apply(&hook_ev("s1", "UserPromptSubmit", None));
        let ch = st.apply(&sub_ev("s1", "SubagentStart", "ag-1", Some("Explore")));
        let added: Vec<_> = ch.iter().filter_map(|c| if let Change::Added(v) = c { Some(v) } else { None }).collect();
        assert_eq!(added.len(), 1);
        assert_eq!(added[0].key, "h:s1/ag-1");
        assert_eq!(added[0].name, "Explore");
        assert_eq!(added[0].parent.as_deref(), Some("h:s1"));
        assert_eq!(added[0].visible, Visible::Working);
        let parent = st.views(Instant::now()).into_iter().find(|v| v.key == "h:s1").unwrap();
        assert_eq!(parent.visible, Visible::Delegating);
    }

    #[test] fn subagent_stop_makes_the_child_gone_and_tool_use_keeps_it_alive() {
        let mut st = Store::new();
        st.apply(&sub_ev("s1", "SubagentStart", "ag-1", Some("Explore")));
        assert!(st.apply(&sub_ev("s1", "PreToolUse", "ag-1", Some("Explore"))).is_empty(), "already working");
        let ch = st.apply(&sub_ev("s1", "SubagentStop", "ag-1", Some("Explore")));
        assert!(ch.iter().any(|c| matches!(c, Change::Updated(v) if v.key == "h:s1/ag-1" && v.visible == Visible::Gone)));
        // tool use of an unknown subagent (app started mid-task) creates it
        let ch = st.apply(&sub_ev("s2", "PostToolUse", "ag-9", None));
        assert!(ch.iter().any(|c| matches!(c, Change::Added(v) if v.key == "h:s2/ag-9" && v.name == "ag-9")));
    }

    #[test] fn children_are_not_marked_missing_by_polls_but_die_with_their_session() {
        let mut st = Store::new();
        st.apply(&sub_ev("s1", "SubagentStart", "ag-1", Some("Explore")));
        st.apply(&snap(&[("s1", "n", PollStatus::Busy)]));
        assert!(st.apply(&snap(&[("s1", "n", PollStatus::Busy)])).is_empty());
        let child = st.views(Instant::now()).into_iter().find(|v| v.key == "h:s1/ag-1").unwrap();
        assert_eq!(child.visible, Visible::Working, "polls never list subagents; they must not go missing");
        let ch = st.apply(&hook_ev("s1", "SessionEnd", None));
        assert!(ch.iter().any(|c| matches!(c, Change::Updated(v) if v.key == "h:s1/ag-1" && v.visible == Visible::Gone)));
    }
    fn snap(agents: &[(&str, &str, PollStatus)]) -> AgentEvent {
        AgentEvent { host: "h".into(), session_id: String::new(), at: Instant::now(),
            kind: EventKind::Snapshot(agents.iter().map(|(sid, name, st)| PolledAgent {
                session_id: sid.to_string(), name: Some(name.to_string()), cwd: None,
                kind: PollKind::Interactive, status: Some(*st), waiting_for: None, state: None, pid: Some(1), id: None }).collect()) }
    }

    #[test] fn hook_before_poll_creates_provisional_then_poll_renames() {
        let mut st = Store::new();
        let ch = st.apply(&hook_ev("abcdef123456", "UserPromptSubmit", Some("/p")));
        assert!(matches!(&ch[0], Change::Added(v) if v.name == "abcdef12" && v.cwd.as_deref() == Some("/p") && v.visible == Visible::Working));
        let ch = st.apply(&snap(&[("abcdef123456", "buddy-7", PollStatus::Busy)]));
        assert!(matches!(&ch[0], Change::Updated(v) if v.name == "buddy-7" && v.key == "h:abcdef123456"));
        assert_eq!(st.views(Instant::now()).len(), 1);
    }

    #[test] fn no_change_emits_nothing() {
        let mut st = Store::new();
        st.apply(&snap(&[("s1", "n", PollStatus::Busy)]));
        assert!(st.apply(&snap(&[("s1", "n", PollStatus::Busy)])).is_empty());
    }

    #[test] fn a_finished_background_record_does_not_fight_the_live_session_with_the_same_id() {
        let mut st = Store::new();
        let rec = |kind, status, state| PolledAgent {
            session_id: "s1".into(), name: Some("n".into()), cwd: None, kind, status, waiting_for: None, state, pid: Some(1), id: None };
        let both = AgentEvent { host: "h".into(), session_id: String::new(), at: Instant::now(), kind: EventKind::Snapshot(vec![
            rec(PollKind::Background, None, Some(PollState::Done)),
            rec(PollKind::Interactive, Some(PollStatus::Busy), None)]) };
        let ch = st.apply(&both);
        assert_eq!(ch.len(), 1, "one Added, no flip-flop");
        assert!(matches!(&ch[0], Change::Added(v) if v.visible == Visible::Working));
        assert!(st.apply(&both).is_empty(), "stable on the next poll: nothing to redraw");
    }

    #[test] fn snapshot_adds_and_marks_missing() {
        let mut st = Store::new();
        let ch = st.apply(&snap(&[("s1", "a", PollStatus::Idle), ("s2", "b", PollStatus::Busy)]));
        assert_eq!(ch.len(), 2);
        assert!(st.apply(&snap(&[("s1", "a", PollStatus::Idle)])).is_empty(), "first miss: no change yet");
        let ch = st.apply(&snap(&[("s1", "a", PollStatus::Idle)]));
        assert!(matches!(&ch[0], Change::Updated(v) if v.key == "h:s2" && v.visible == Visible::Gone));
    }

    #[test] fn session_end_then_sweep_removes_after_grace() {
        let mut st = Store::new();
        st.gone_grace = Duration::from_secs(10);
        st.apply(&hook_ev("s1", "UserPromptSubmit", None));
        let ch = st.apply(&hook_ev("s1", "SessionEnd", None));
        assert!(matches!(&ch[0], Change::Updated(v) if v.visible == Visible::Gone));
        let now = Instant::now();
        assert!(st.sweep(now + Duration::from_secs(5)).is_empty());
        let ch = st.sweep(now + Duration::from_secs(11));
        assert!(matches!(&ch[0], Change::Removed(k) if k == "h:s1"));
        assert!(st.views(now).is_empty());
    }

    #[test] fn hook_for_gone_agent_revives_it() {
        let mut st = Store::new();
        st.apply(&hook_ev("s1", "SessionEnd", None));
        let ch = st.apply(&hook_ev("s1", "UserPromptSubmit", None));
        assert!(matches!(&ch[0], Change::Updated(v) if v.visible == Visible::Working));
    }

    fn bg_snap(sid: &str, state: PollState) -> AgentEvent {
        AgentEvent { host: "h".into(), session_id: String::new(), at: Instant::now(),
            kind: EventKind::Snapshot(vec![PolledAgent { session_id: sid.into(), name: Some("job".into()), cwd: None,
                kind: PollKind::Background, status: None, waiting_for: None, state: Some(state), pid: None, id: Some("job1".into()) }]) }
    }

    #[test] fn finished_background_agent_is_not_recreated_after_sweep() {
        let mut st = Store::new();
        let ch = st.apply(&bg_snap("bg1", PollState::Working));
        assert!(matches!(&ch[0], Change::Added(v) if v.visible == Visible::Working));
        let ch = st.apply(&bg_snap("bg1", PollState::Done));
        assert!(matches!(&ch[0], Change::Updated(v) if v.visible == Visible::Gone));
        let ch = st.sweep(Instant::now() + Duration::from_secs(11));
        assert!(matches!(&ch[0], Change::Removed(k) if k == "h:bg1"));
        let ch = st.apply(&bg_snap("bg1", PollState::Done));
        assert!(!ch.iter().any(|c| matches!(c, Change::Added(_))), "no phantom re-add: {ch:?}");
        assert!(st.views(Instant::now()).is_empty());
    }

    #[test] fn unknown_terminal_background_agent_is_ignored_at_startup() {
        let mut st = Store::new();
        for state in [PollState::Done, PollState::Failed, PollState::Stopped] {
            assert!(st.apply(&bg_snap("old", state)).is_empty());
        }
        assert!(st.views(Instant::now()).is_empty());
    }

    #[test] fn snapshot_events_do_not_create_agent_from_empty_session_id() {
        let mut st = Store::new();
        st.apply(&snap(&[]));
        assert!(st.views(Instant::now()).is_empty());
    }
}
