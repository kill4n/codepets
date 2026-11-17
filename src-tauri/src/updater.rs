use crate::App;
use serde::Serialize;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

/// How often a running app looks for a new release.
pub const CHECK_EVERY: Duration = Duration::from_secs(6 * 3600);
/// Wait this long after launch before the first check, so startup stays quiet.
pub const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available { version: String, notes: Option<String> },
    Installing,
    Failed { message: String },
}

/// True when a periodic check should run now.
pub fn check_due(enabled: bool, last_check: Option<Instant>, now: Instant, every: Duration) -> bool {
    enabled && last_check.map_or(true, |t| now.duration_since(t) >= every)
}

/// Tray menu entry for the state, if it deserves one.
pub fn tray_label(state: &UpdateState) -> Option<String> {
    match state {
        UpdateState::Available { version, .. } => Some(format!("Update to v{version}…")),
        UpdateState::Installing => Some("Installing update…".into()),
        _ => None,
    }
}

/// Look for a release newer than this build; remembers the update so `install` can apply it.
pub async fn check(handle: &AppHandle, st: &App) -> UpdateState {
    if *st.update.borrow() == UpdateState::Installing { return UpdateState::Installing; }
    st.update.send_replace(UpdateState::Checking);
    let next = match handle.updater() {
        Err(e) => UpdateState::Failed { message: e.to_string() },
        Ok(updater) => match updater.check().await {
            Ok(Some(up)) => {
                let s = UpdateState::Available { version: up.version.clone(), notes: up.body.clone() };
                *st.pending_update.lock().unwrap() = Some(up);
                s
            }
            Ok(None) => UpdateState::UpToDate,
            Err(e) => UpdateState::Failed { message: e.to_string() },
        },
    };
    if let UpdateState::Failed { message } = &next { log::warn!("update check failed: {message}"); }
    st.update.send_replace(next.clone());
    next
}

/// Download, verify (minisign) and install the pending update, then restart into it.
pub async fn install(handle: &AppHandle, st: &App) -> Result<(), String> {
    let Some(up) = st.pending_update.lock().unwrap().take() else { return Err("no update to install: check for updates first".into()) };
    st.update.send_replace(UpdateState::Installing);
    match up.download_and_install(|_, _| {}, || {}).await {
        Ok(()) => handle.restart(),
        Err(e) => {
            log::warn!("update install failed: {e}");
            st.update.send_replace(UpdateState::Failed { message: e.to_string() });
            Err(e.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn checks_right_away_when_never_checked() {
        let now = Instant::now();
        assert!(check_due(true, None, now, CHECK_EVERY));
    }
    #[test] fn waits_for_the_interval_between_checks() {
        let now = Instant::now();
        assert!(!check_due(true, Some(now), now + CHECK_EVERY - Duration::from_secs(1), CHECK_EVERY));
        assert!(check_due(true, Some(now), now + CHECK_EVERY, CHECK_EVERY));
    }
    #[test] fn never_checks_on_its_own_when_disabled() {
        assert!(!check_due(false, None, Instant::now(), CHECK_EVERY));
    }
    #[test] fn only_an_available_or_installing_update_gets_a_tray_entry() {
        let av = UpdateState::Available { version: "0.2.0".into(), notes: None };
        assert_eq!(tray_label(&av).as_deref(), Some("Update to v0.2.0…"));
        assert_eq!(tray_label(&UpdateState::Installing).as_deref(), Some("Installing update…"));
        for s in [UpdateState::Idle, UpdateState::Checking, UpdateState::UpToDate, UpdateState::Failed { message: "x".into() }] {
            assert_eq!(tray_label(&s), None, "{s:?}");
        }
    }
    #[test] fn state_serializes_with_a_kind_tag_for_the_ui() {
        let v = serde_json::to_value(UpdateState::Available { version: "0.2.0".into(), notes: Some("n".into()) }).unwrap();
        assert_eq!(v, serde_json::json!({"kind": "available", "version": "0.2.0", "notes": "n"}));
        assert_eq!(serde_json::to_value(UpdateState::UpToDate).unwrap(), serde_json::json!({"kind": "up_to_date"}));
    }
}
