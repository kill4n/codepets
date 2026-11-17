use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct AgentEvent {
    pub host: String,
    pub session_id: String,
    pub kind: EventKind,
    pub at: Instant,
}

#[derive(Debug, Clone)]
pub enum EventKind {
    Hook(HookEvent),
    Snapshot(Vec<PolledAgent>),
}

#[derive(Debug, Clone)]
pub struct HookEvent {
    pub event: String,
    pub cwd: Option<PathBuf>,
    pub tool_name: Option<String>,
    pub notification_type: Option<String>,
    pub recipient: Option<String>,
    /// Present when the hook fired inside a subagent (or on SubagentStart/Stop for that subagent).
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
    /// Short human summary of `tool_input` (the Bash command, a file path, ...) for permission prompts.
    pub tool_summary: Option<String>,
    /// `StopFailure` only: why the turn ended (`rate_limit`, `server_error`, ...).
    pub error: Option<String>,
}

/// Longest tool input the bubble will show. Anything longer is marked truncated and the bubble
/// refuses to approve it: the user must see exactly what they allow.
pub const TOOL_SUMMARY_MAX: usize = 2000;
pub const TRUNCATED_MARK: &str = "\n[… truncated: decide in the terminal]";

/// The text describing a tool call for the bubble: the Bash command verbatim (newlines kept), a
/// path, or the compact JSON input. Ends with `TRUNCATED_MARK` when cut.
pub fn summarize_tool_input(tool: Option<&str>, input: Option<&serde_json::Value>) -> Option<String> {
    let v = input?;
    let text = match tool {
        Some("Bash") => v.get("command").and_then(|c| c.as_str()).map(str::to_string),
        _ => v.get("file_path").or_else(|| v.get("path")).or_else(|| v.get("url")).or_else(|| v.get("pattern"))
            .and_then(|c| c.as_str()).map(str::to_string),
    }.unwrap_or_else(|| v.to_string());
    let text = text.trim().to_string();
    if text.chars().count() > TOOL_SUMMARY_MAX {
        let mut out: String = text.chars().take(TOOL_SUMMARY_MAX).collect();
        out.push_str(TRUNCATED_MARK);
        return Some(out);
    }
    Some(text)
}

pub fn is_truncated_summary(s: &str) -> bool { s.ends_with(TRUNCATED_MARK) }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PollStatus { Busy, Waiting, Idle, #[serde(other)] Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PollState { Working, Blocked, Done, Failed, Stopped, #[serde(other)] Unknown }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PollKind { Interactive, Background, #[default] #[serde(other)] Unknown }

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PolledAgent {
    pub session_id: String,
    pub name: Option<String>,
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub kind: PollKind,
    pub status: Option<PollStatus>,
    pub waiting_for: Option<String>,
    pub state: Option<PollState>,
    /// OS pid of the interactive session (absent for background jobs)
    pub pid: Option<u32>,
    /// Short job id of a daemon-hosted (background) session
    pub id: Option<String>,
}

#[derive(Debug)]
pub enum ParseError { MissingField(&'static str), Json(serde_json::Error) }

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::MissingField(n) => write!(f, "missing field {n}"),
            ParseError::Json(e) => write!(f, "json: {e}"),
        }
    }
}
impl std::error::Error for ParseError {}
impl From<serde_json::Error> for ParseError { fn from(e: serde_json::Error) -> Self { ParseError::Json(e) } }

pub fn parse_hook(host: &str, body: &serde_json::Value) -> Result<AgentEvent, ParseError> {
    let session_id = body.get("session_id").and_then(|v| v.as_str())
        .ok_or(ParseError::MissingField("session_id"))?.to_string();
    let event = body.get("hook_event_name").and_then(|v| v.as_str())
        .ok_or(ParseError::MissingField("hook_event_name"))?.to_string();
    let str_field = |k: &str| body.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let tool_name = str_field("tool_name");
    let tool_summary = if matches!(event.as_str(), "PermissionRequest" | "PreToolUse" | "PostToolUse") {
        summarize_tool_input(tool_name.as_deref(), body.get("tool_input"))
    } else { None };
    let error = if event == "StopFailure" { str_field("error") } else { None };
    let recipient = if tool_name.as_deref() == Some("SendMessage") {
        body.pointer("/tool_input/to").and_then(|v| v.as_str()).map(str::to_string)
    } else { None };
    Ok(AgentEvent {
        host: host.to_string(),
        session_id,
        kind: EventKind::Hook(HookEvent {
            event,
            cwd: str_field("cwd").map(PathBuf::from),
            tool_name,
            notification_type: str_field("notification_type"),
            recipient,
            agent_id: str_field("agent_id"),
            agent_type: str_field("agent_type"),
            tool_summary,
            error,
        }),
        at: Instant::now(),
    })
}

pub fn parse_snapshot(host: &str, body: &str) -> Result<AgentEvent, ParseError> {
    let raw: Vec<serde_json::Value> = serde_json::from_str(body)?;
    let agents = raw.into_iter()
        .filter_map(|v| serde_json::from_value::<PolledAgent>(v).ok())
        .collect();
    Ok(AgentEvent { host: host.to_string(), session_id: String::new(),
        kind: EventKind::Snapshot(agents), at: Instant::now() })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hook_minimal_fields() {
        let body = json!({"session_id":"abc","cwd":"/tmp/p","hook_event_name":"Stop"});
        let ev = parse_hook("mac", &body).unwrap();
        assert_eq!(ev.session_id, "abc");
        assert_eq!(ev.host, "mac");
        match ev.kind {
            EventKind::Hook(h) => {
                assert_eq!(h.event, "Stop");
                assert_eq!(h.cwd.unwrap().to_str().unwrap(), "/tmp/p");
                assert!(h.tool_name.is_none());
            }
            _ => panic!(),
        }
    }

    #[test]
    fn hook_sendmessage_extracts_recipient() {
        let body = json!({"session_id":"abc","hook_event_name":"PostToolUse",
            "tool_name":"SendMessage","tool_input":{"to":"buddy-12","message":"hi"}});
        let ev = parse_hook("mac", &body).unwrap();
        let EventKind::Hook(h) = ev.kind else { panic!() };
        assert_eq!(h.tool_name.as_deref(), Some("SendMessage"));
        assert_eq!(h.recipient.as_deref(), Some("buddy-12"));
    }

    #[test]
    fn stop_failure_carries_the_error_kind() {
        let body = json!({"session_id":"abc","hook_event_name":"StopFailure","error":"rate_limit"});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.error.as_deref(), Some("rate_limit"));
        let body = json!({"session_id":"abc","hook_event_name":"Stop","error":"ignored"});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.error, None, "only StopFailure has an error kind");
    }

    #[test]
    fn hook_notification_type() {
        let body = json!({"session_id":"abc","hook_event_name":"Notification",
            "notification_type":"permission_prompt","message":"x"});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.notification_type.as_deref(), Some("permission_prompt"));
    }

    #[test]
    fn permission_request_summarizes_the_tool_input() {
        let body = json!({"session_id":"abc","hook_event_name":"PermissionRequest","tool_name":"Bash",
            "tool_input":{"command":"npm   test\n--watch","description":"x"}});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.tool_summary.as_deref(), Some("npm   test\n--watch"), "verbatim: newlines and spacing kept");
        let body = json!({"session_id":"abc","hook_event_name":"PermissionRequest","tool_name":"Write","tool_input":{"file_path":"/tmp/a.rs","content":"..."}});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.tool_summary.as_deref(), Some("/tmp/a.rs"));
        let long = "x".repeat(TOOL_SUMMARY_MAX + 5);
        let cut = summarize_tool_input(Some("Bash"), Some(&json!({"command": long}))).unwrap();
        assert!(is_truncated_summary(&cut));
        assert!(!is_truncated_summary(&summarize_tool_input(Some("Bash"), Some(&json!({"command": "ls"}))).unwrap()));
        assert_eq!(summarize_tool_input(Some("Bash"), None), None);
    }

    #[test]
    fn hook_carries_subagent_identity() {
        let body = json!({"session_id":"abc","hook_event_name":"SubagentStart","agent_id":"ag-1","agent_type":"Explore"});
        let EventKind::Hook(h) = parse_hook("mac", &body).unwrap().kind else { panic!() };
        assert_eq!(h.agent_id.as_deref(), Some("ag-1"));
        assert_eq!(h.agent_type.as_deref(), Some("Explore"));
        let EventKind::Hook(h) = parse_hook("mac", &json!({"session_id":"abc","hook_event_name":"Stop"})).unwrap().kind else { panic!() };
        assert_eq!(h.agent_id, None);
    }

    #[test]
    fn hook_without_session_id_is_error() {
        assert!(parse_hook("mac", &json!({"hook_event_name":"Stop"})).is_err());
    }

    #[test]
    fn hook_without_event_name_is_error() {
        assert!(parse_hook("mac", &json!({"session_id":"a"})).is_err());
    }

    #[test]
    fn snapshot_parses_interactive_and_background() {
        let body = r#"[
          {"pid":1,"cwd":"/a","kind":"interactive","startedAt":1,"sessionId":"s1","name":"n1","status":"busy"},
          {"id":"x","cwd":"/b","kind":"background","startedAt":2,"sessionId":"s2","name":"n2","state":"blocked"},
          {"cwd":"/c","kind":"interactive","sessionId":"s3","name":"n3","status":"waiting","waitingFor":"permission prompt"}
        ]"#;
        let ev = parse_snapshot("mac", body).unwrap();
        let EventKind::Snapshot(list) = ev.kind else { panic!() };
        assert_eq!(list.len(), 3);
        assert_eq!(list[0].status, Some(PollStatus::Busy));
        assert_eq!(list[0].kind, PollKind::Interactive);
        assert_eq!(list[0].pid, Some(1));
        assert_eq!(list[1].pid, None, "background jobs have no pid");
        assert_eq!(list[1].id.as_deref(), Some("x"));
        assert_eq!(list[0].id, None);
        assert_eq!(list[1].state, Some(PollState::Blocked));
        assert_eq!(list[1].kind, PollKind::Background);
        assert_eq!(list[2].waiting_for.as_deref(), Some("permission prompt"));
    }

    #[test]
    fn snapshot_unknown_values_are_kept_not_dropped() {
        let body = r#"[{"sessionId":"s1","kind":"weird","status":"frobnicating"},{"sessionId":"s2"}]"#;
        let EventKind::Snapshot(list) = parse_snapshot("mac", body).unwrap().kind else { panic!() };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].status, Some(PollStatus::Unknown));
        assert_eq!(list[0].kind, PollKind::Unknown);
        assert_eq!(list[1].status, None);
    }

    #[test]
    fn snapshot_entry_without_session_id_is_skipped() {
        let body = r#"[{"name":"orphan"},{"sessionId":"ok"}]"#;
        let EventKind::Snapshot(list) = parse_snapshot("mac", body).unwrap().kind else { panic!() };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].session_id, "ok");
    }

    #[test]
    fn snapshot_invalid_json_is_error() {
        assert!(parse_snapshot("mac", "not json").is_err());
    }
}
