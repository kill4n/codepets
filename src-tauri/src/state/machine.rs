use super::{AgentState, Visible};
#[cfg(test)]
use super::AgentId;
use crate::collector::event::{HookEvent, PollKind, PollState, PollStatus, PolledAgent};
use std::time::{Duration, Instant};

pub const HOOK_WINS_FOR: Duration = Duration::from_secs(5);
pub const GONE_AFTER_MISSED: u8 = 2;

const NEEDS_INPUT_NOTIFICATIONS: &[&str] = &["permission_prompt", "elicitation_dialog", "idle_prompt"];

fn effective(s: &AgentState, base: Visible) -> Visible {
    if base == Visible::Gone { return Visible::Gone; }
    if s.subagents > 0 && matches!(base, Visible::Working | Visible::Idle) { return Visible::Delegating; }
    base
}

pub fn apply_hook(s: &mut AgentState, h: &HookEvent, now: Instant) {
    // any hook other than the request itself means the prompt was answered (or abandoned)
    if h.event != "PermissionRequest" { s.permission = None; }
    s.last_hook_at = Some(now);
    s.last_seen = now;
    s.missed_polls = 0;
    if s.cwd.is_none() { s.cwd = h.cwd.clone(); }

    let base = match h.event.as_str() {
        "UserPromptSubmit" => {
            // a new turn: any subagent whose SubagentStop was lost (Esc, dropped hook) is over
            s.sent_message = false; s.peer = None; s.waiting_for = None; s.subagents = 0;
            Visible::Working
        }
        "PreToolUse" => Visible::Working,
        "PostToolUse" => {
            if h.tool_name.as_deref() == Some("SendMessage") {
                s.sent_message = true;
                s.peer = h.recipient.clone();
            }
            s.waiting_for = None;
            Visible::Working
        }
        "Stop" => if s.sent_message { Visible::WaitingPeer } else { Visible::Idle },
        "PermissionRequest" => {
            s.permission = Some((h.tool_name.clone().unwrap_or_else(|| "tool".into()), h.tool_summary.clone().unwrap_or_default()));
            Visible::NeedsInput
        }
        "Notification" => {
            if h.notification_type.as_deref().is_some_and(|t| NEEDS_INPUT_NOTIFICATIONS.contains(&t)) {
                Visible::NeedsInput
            } else { s.visible }
        }
        "SubagentStart" => { s.subagents = s.subagents.saturating_add(1); s.visible }
        "SubagentStop" => { s.subagents = s.subagents.saturating_sub(1);
            if s.visible == Visible::Delegating { Visible::Working } else { s.visible } }
        // compaction runs mid-turn: it ends back in the turn (a manual /compact is corrected by the poll)
        "PreCompact" => Visible::Compacting,
        "PostCompact" => Visible::Working,
        "StopFailure" => { s.waiting_for = h.error.clone(); Visible::Error }
        "SessionEnd" => Visible::Gone,
        _ => s.visible,
    };
    let next = effective(s, base);
    // the error kind only means something while the error is shown
    if s.visible == Visible::Error && next != Visible::Error { s.waiting_for = None; }
    s.set_visible(next, now);
}

pub fn apply_poll(s: &mut AgentState, p: &PolledAgent, now: Instant) {
    s.last_seen = now;
    s.missed_polls = 0;
    if let Some(n) = &p.name { s.name = n.clone(); }
    if p.cwd.is_some() { s.cwd = p.cwd.clone(); }
    if p.pid.is_some() { s.pid = p.pid; }
    if p.id.is_some() { s.job_id = p.id.clone(); }

    let recent_hook = s.last_hook_at.is_some_and(|t| now.duration_since(t) < HOOK_WINS_FOR);

    // Real "waiting" from the poll always wins over inferred states.
    if p.status == Some(PollStatus::Waiting) {
        s.waiting_for = p.waiting_for.clone();
        s.set_visible(Visible::NeedsInput, now);
        return;
    }
    if recent_hook { return; }

    let base = match (p.kind, p.status, p.state) {
        (PollKind::Background, _, Some(st)) => match st {
            PollState::Working => Visible::Working,
            PollState::Blocked => Visible::NeedsInput,
            PollState::Done | PollState::Failed | PollState::Stopped => Visible::Gone,
            PollState::Unknown => s.visible,
        },
        // a compaction can outlast the hook window: the poll reports busy throughout
        (_, Some(PollStatus::Busy), _) if s.visible == Visible::Compacting => Visible::Compacting,
        (_, Some(PollStatus::Busy), _) => {
            if s.visible == Visible::WaitingPeer { s.sent_message = false; s.peer = None; }
            s.waiting_for = None;
            Visible::Working
        }
        // a failed turn leaves the session idle: keep the error flagged until a new turn starts
        (_, Some(PollStatus::Idle), _) if s.visible == Visible::Error => Visible::Error,
        (_, Some(PollStatus::Idle), _) => {
            // idle with no recent hook: a lost SubagentStop must not pin the sprite in Delegating
            s.subagents = 0;
            if s.sent_message { Visible::WaitingPeer } else { Visible::Idle }
        }
        _ => s.visible,
    };
    s.set_visible(effective(s, base), now);
}

pub fn mark_missed(s: &mut AgentState, now: Instant) {
    s.missed_polls = s.missed_polls.saturating_add(1);
    if s.missed_polls >= GONE_AFTER_MISSED { s.set_visible(Visible::Gone, now); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collector::event::*;
    use std::time::Duration;

    fn hook(event: &str) -> HookEvent {
        HookEvent { event: event.into(), cwd: None, tool_name: None, notification_type: None, recipient: None, agent_id: None, agent_type: None, tool_summary: None, error: None }
    }
    fn fresh() -> (AgentState, Instant) {
        let now = Instant::now();
        (AgentState::new(AgentId { host: "h".into(), session_id: "abcdef123456".into() }, now), now)
    }
    fn polled(status: Option<PollStatus>) -> PolledAgent {
        PolledAgent { session_id: "abcdef123456".into(), name: Some("buddy-1".into()), cwd: None,
            kind: PollKind::Interactive, status, waiting_for: None, state: None, pid: Some(4242), id: None }
    }

    fn stop_failure(kind: &str) -> HookEvent { let mut h = hook("StopFailure"); h.error = Some(kind.into()); h }

    #[test] fn pre_compact_shows_compacting() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        apply_hook(&mut s, &hook("PreCompact"), now);
        assert_eq!(s.visible, Visible::Compacting);
    }

    #[test] fn post_compact_goes_back_to_working() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("PreCompact"), now);
        apply_hook(&mut s, &hook("PostCompact"), now);
        assert_eq!(s.visible, Visible::Working);
    }

    #[test] fn compacting_is_not_turned_into_delegating_by_running_subagents() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("SubagentStart"), now);
        apply_hook(&mut s, &hook("PreCompact"), now);
        assert_eq!(s.visible, Visible::Compacting);
    }

    #[test] fn compacting_outlives_the_hook_window_while_the_poll_says_busy() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("PreCompact"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + HOOK_WINS_FOR + Duration::from_secs(1));
        assert_eq!(s.visible, Visible::Compacting, "a long compaction must not flip back to working");
    }

    #[test] fn compacting_ends_when_the_poll_says_idle() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("PreCompact"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now + HOOK_WINS_FOR + Duration::from_secs(1));
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn stop_failure_is_error_and_remembers_why() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        apply_hook(&mut s, &stop_failure("rate_limit"), now);
        assert_eq!(s.visible, Visible::Error);
        assert_eq!(s.waiting_for.as_deref(), Some("rate_limit"));
    }

    #[test] fn error_survives_an_idle_poll_after_the_hook_window() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &stop_failure("server_error"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now + HOOK_WINS_FOR + Duration::from_secs(1));
        assert_eq!(s.visible, Visible::Error, "the failed turn stays flagged until the user acts");
    }

    #[test] fn a_new_prompt_clears_the_error() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &stop_failure("server_error"), now);
        apply_hook(&mut s, &hook("UserPromptSubmit"), now + Duration::from_secs(30));
        assert_eq!(s.visible, Visible::Working);
        assert_eq!(s.waiting_for, None, "the error kind must not leak into the next turn");
    }

    #[test] fn a_tool_call_after_an_error_drops_the_error_kind() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &stop_failure("rate_limit"), now);
        apply_hook(&mut s, &hook("PreToolUse"), now + Duration::from_secs(30));
        assert_eq!((s.visible, s.waiting_for), (Visible::Working, None));
    }

    #[test] fn a_busy_poll_clears_the_error() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &stop_failure("server_error"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + HOOK_WINS_FOR + Duration::from_secs(1));
        assert_eq!(s.visible, Visible::Working, "busy means a new turn started, e.g. from the terminal");
        assert_eq!(s.waiting_for, None);
    }

    #[test] fn new_agent_has_provisional_name_and_idle() {
        let (s, _) = fresh();
        assert_eq!(s.name, "abcdef12");
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn prompt_submit_is_working() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        assert_eq!(s.visible, Visible::Working);
    }

    #[test] fn stop_is_idle_and_resets_since_only_on_change() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        let t1 = now + Duration::from_secs(1);
        apply_hook(&mut s, &hook("Stop"), t1);
        assert_eq!(s.visible, Visible::Idle);
        assert_eq!(s.since, t1);
        apply_hook(&mut s, &hook("Stop"), t1 + Duration::from_secs(1));
        assert_eq!(s.since, t1);
    }

    #[test] fn permission_request_needs_input() {
        let (mut s, now) = fresh();
        let mut h = hook("PermissionRequest"); h.tool_name = Some("Bash".into()); h.tool_summary = Some("rm -rf build".into());
        apply_hook(&mut s, &h, now);
        assert_eq!(s.visible, Visible::NeedsInput);
        assert_eq!(s.permission, Some(("Bash".into(), "rm -rf build".into())));
        apply_hook(&mut s, &hook("PostToolUse"), now);
        assert_eq!(s.visible, Visible::Working);
        assert_eq!(s.permission, None, "answered");
    }

    #[test] fn notification_types_need_input() {
        for t in ["permission_prompt", "elicitation_dialog", "idle_prompt"] {
            let (mut s, now) = fresh();
            let mut h = hook("Notification"); h.notification_type = Some(t.into());
            apply_hook(&mut s, &h, now);
            assert_eq!(s.visible, Visible::NeedsInput, "{t}");
        }
        let (mut s, now) = fresh();
        let mut h = hook("Notification"); h.notification_type = Some("auth_success".into());
        apply_hook(&mut s, &h, now);
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn subagents_delegating_and_back() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        apply_hook(&mut s, &hook("SubagentStart"), now);
        apply_hook(&mut s, &hook("SubagentStart"), now);
        assert_eq!(s.visible, Visible::Delegating);
        apply_hook(&mut s, &hook("PreToolUse"), now);
        assert_eq!(s.visible, Visible::Delegating, "delegating beats working");
        apply_hook(&mut s, &hook("SubagentStop"), now);
        assert_eq!(s.visible, Visible::Delegating);
        apply_hook(&mut s, &hook("SubagentStop"), now);
        assert_eq!(s.visible, Visible::Working);
    }

    #[test] fn prompt_submit_resets_lost_subagents() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        apply_hook(&mut s, &hook("SubagentStart"), now);
        assert_eq!(s.visible, Visible::Delegating);
        // user hits Esc: no SubagentStop/Stop arrive; the next prompt starts clean
        apply_hook(&mut s, &hook("UserPromptSubmit"), now + Duration::from_secs(30));
        assert_eq!(s.subagents, 0);
        assert_eq!(s.visible, Visible::Working);
        apply_hook(&mut s, &hook("Stop"), now + Duration::from_secs(31));
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn poll_idle_outside_hook_window_resets_subagents() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        apply_hook(&mut s, &hook("SubagentStart"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now + Duration::from_secs(2));
        assert_eq!(s.visible, Visible::Delegating, "within the hook window the hook state wins");
        assert_eq!(s.subagents, 1);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now + Duration::from_secs(6));
        assert_eq!(s.subagents, 0);
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn subagent_stop_without_start_saturates_at_zero() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("SubagentStop"), now);
        assert_eq!(s.subagents, 0);
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn send_message_then_stop_is_waiting_peer() {
        let (mut s, now) = fresh();
        let mut h = hook("PostToolUse");
        h.tool_name = Some("SendMessage".into()); h.recipient = Some("buddy-2".into());
        apply_hook(&mut s, &h, now);
        assert_eq!(s.visible, Visible::Working);
        apply_hook(&mut s, &hook("Stop"), now);
        assert_eq!(s.visible, Visible::WaitingPeer);
        assert_eq!(s.peer.as_deref(), Some("buddy-2"));
        apply_hook(&mut s, &hook("UserPromptSubmit"), now);
        assert_eq!(s.visible, Visible::Working);
        assert!(s.peer.is_none());
    }

    #[test] fn session_end_is_gone() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("SessionEnd"), now);
        assert_eq!(s.visible, Visible::Gone);
    }

    #[test] fn poll_updates_name_and_status() {
        let (mut s, now) = fresh();
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now);
        assert_eq!(s.name, "buddy-1");
        assert_eq!(s.visible, Visible::Working);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now);
        assert_eq!(s.visible, Visible::Idle);
    }

    #[test] fn poll_waiting_sets_needs_input_with_reason() {
        let (mut s, now) = fresh();
        let mut p = polled(Some(PollStatus::Waiting)); p.waiting_for = Some("permission prompt".into());
        apply_poll(&mut s, &p, now);
        assert_eq!(s.visible, Visible::NeedsInput);
        assert_eq!(s.waiting_for.as_deref(), Some("permission prompt"));
    }

    #[test] fn recent_hook_beats_poll() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("PermissionRequest"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(2));
        assert_eq!(s.visible, Visible::NeedsInput);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(6));
        assert_eq!(s.visible, Visible::Working);
    }

    #[test] fn poll_waiting_beats_inferred_waiting_peer_even_if_recent() {
        let (mut s, now) = fresh();
        let mut h = hook("PostToolUse"); h.tool_name = Some("SendMessage".into());
        apply_hook(&mut s, &h, now);
        apply_hook(&mut s, &hook("Stop"), now);
        assert_eq!(s.visible, Visible::WaitingPeer);
        let mut p = polled(Some(PollStatus::Waiting)); p.waiting_for = Some("input needed".into());
        apply_poll(&mut s, &p, now + Duration::from_secs(1));
        assert_eq!(s.visible, Visible::NeedsInput);
    }

    #[test] fn poll_idle_after_send_message_is_waiting_peer() {
        let (mut s, now) = fresh();
        let mut h = hook("PostToolUse"); h.tool_name = Some("SendMessage".into());
        apply_hook(&mut s, &h, now);
        apply_poll(&mut s, &polled(Some(PollStatus::Idle)), now + Duration::from_secs(6));
        assert_eq!(s.visible, Visible::WaitingPeer);
    }

    #[test] fn background_states_map() {
        let cases = [(PollState::Working, Visible::Working), (PollState::Blocked, Visible::NeedsInput),
                     (PollState::Done, Visible::Gone), (PollState::Failed, Visible::Gone), (PollState::Stopped, Visible::Gone)];
        for (st, vis) in cases {
            let (mut s, now) = fresh();
            let mut p = polled(None); p.kind = PollKind::Background; p.state = Some(st);
            apply_poll(&mut s, &p, now);
            assert_eq!(s.visible, vis, "{st:?}");
        }
    }

    #[test] fn missed_polls_twice_is_gone() {
        let (mut s, now) = fresh();
        mark_missed(&mut s, now);
        assert_eq!(s.visible, Visible::Idle);
        mark_missed(&mut s, now);
        assert_eq!(s.visible, Visible::Gone);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now);
        assert_eq!(s.missed_polls, 0);
        assert_eq!(s.visible, Visible::Working, "reappearing cancels gone");
    }

    #[test] fn session_end_beats_poll_within_hook_window() {
        let (mut s, now) = fresh();
        apply_hook(&mut s, &hook("SessionEnd"), now);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(2));
        assert_eq!(s.visible, Visible::Gone);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(6));
        assert_eq!(s.visible, Visible::Working);
    }

    #[test] fn poll_busy_while_working_keeps_pending_send() {
        let (mut s, now) = fresh();
        let mut h = hook("PostToolUse");
        h.tool_name = Some("SendMessage".into()); h.recipient = Some("buddy-2".into());
        apply_hook(&mut s, &h, now);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(6));
        assert_eq!(s.visible, Visible::Working);
        apply_hook(&mut s, &hook("Stop"), now + Duration::from_secs(7));
        assert_eq!(s.visible, Visible::WaitingPeer);
        assert_eq!(s.peer.as_deref(), Some("buddy-2"));
    }

    #[test] fn poll_busy_leaves_waiting_peer() {
        let (mut s, now) = fresh();
        let mut h = hook("PostToolUse");
        h.tool_name = Some("SendMessage".into()); h.recipient = Some("buddy-2".into());
        apply_hook(&mut s, &h, now);
        apply_hook(&mut s, &hook("Stop"), now);
        assert_eq!(s.visible, Visible::WaitingPeer);
        apply_poll(&mut s, &polled(Some(PollStatus::Busy)), now + Duration::from_secs(6));
        assert_eq!(s.visible, Visible::Working);
        assert!(s.peer.is_none());
    }
}
