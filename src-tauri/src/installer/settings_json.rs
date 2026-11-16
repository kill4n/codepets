use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const HOOK_EVENTS: [&str; 13] = ["SessionStart", "SessionEnd", "UserPromptSubmit", "PreToolUse",
    "PostToolUse", "Stop", "PermissionRequest", "Notification", "SubagentStart", "SubagentStop",
    "PreCompact", "PostCompact", "StopFailure"];

#[derive(Debug)]
pub enum InstallError { Io(std::io::Error), Json(serde_json::Error), Shape(String) }
impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { write!(f, "{self:?}") }
}
impl std::error::Error for InstallError {}
impl From<std::io::Error> for InstallError { fn from(e: std::io::Error) -> Self { InstallError::Io(e) } }
impl From<serde_json::Error> for InstallError { fn from(e: serde_json::Error) -> Self { InstallError::Json(e) } }

/// Seconds Claude Code waits for our server per event. PermissionRequest is held open so the user
/// can answer from the bubble; everything else must never slow Claude Code down.
pub const PERMISSION_HOOK_TIMEOUT: u32 = 620;
pub fn hook_timeout(event: &str) -> u32 { if event == "PermissionRequest" { PERMISSION_HOOK_TIMEOUT } else { 2 } }

pub fn hook_entry(url: &str, token: &str) -> Value { hook_entry_for("Stop", url, token) }

pub fn hook_entry_for(event: &str, url: &str, token: &str) -> Value {
    json!({ "hooks": [{ "type": "http", "url": url,
        "headers": { "Authorization": format!("Bearer {token}") }, "timeout": hook_timeout(event) }] })
}

fn group_has_url(group: &Value, url: &str) -> bool {
    group.get("hooks").and_then(|h| h.as_array())
        .is_some_and(|arr| arr.iter().any(|h| h.get("url").and_then(|u| u.as_str()) == Some(url)))
}

pub fn remove_hooks(settings: &mut Value, url: &str) {
    let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else { return };
    let mut emptied = vec![];
    for (ev, groups) in hooks.iter_mut() {
        if let Some(arr) = groups.as_array_mut() {
            let before = arr.len();
            arr.retain(|g| !group_has_url(g, url));
            if arr.len() != before && arr.is_empty() { emptied.push(ev.clone()); }
        }
    }
    for ev in emptied { hooks.remove(&ev); }
}

fn check_shape(settings: &Value) -> Result<(), InstallError> {
    if !settings.is_object() {
        return Err(InstallError::Shape("settings root is not an object".into()));
    }
    let Some(hooks) = settings.get("hooks") else { return Ok(()) };
    let Some(hooks) = hooks.as_object() else {
        return Err(InstallError::Shape("hooks is not an object".into()));
    };
    for ev in HOOK_EVENTS {
        if hooks.get(ev).is_some_and(|g| !g.is_array()) {
            return Err(InstallError::Shape(format!("hooks.{ev} is not an array")));
        }
    }
    Ok(())
}

pub fn merge_hooks(settings: &mut Value, url: &str, token: &str) -> Result<(), InstallError> {
    check_shape(settings)?;
    remove_hooks(settings, url);
    let obj = settings.as_object_mut().expect("checked by check_shape");
    let hooks = obj.entry("hooks").or_insert_with(|| json!({}));
    let hooks = hooks.as_object_mut().expect("checked by check_shape");
    for ev in HOOK_EVENTS {
        let groups = hooks.entry(ev).or_insert_with(|| json!([]));
        groups.as_array_mut().expect("checked by check_shape").push(hook_entry_for(ev, url, token));
    }
    Ok(())
}

pub fn is_installed(settings: &Value, url: &str) -> bool {
    settings.get("hooks").and_then(|h| h.as_object())
        .is_some_and(|m| m.values().any(|groups|
            groups.as_array().is_some_and(|arr| arr.iter().any(|g| group_has_url(g, url)))))
}

fn read_settings(path: &Path) -> Result<Value, InstallError> {
    if !path.exists() { return Ok(json!({})); }
    let text = std::fs::read_to_string(path)?;
    if text.trim().is_empty() { return Ok(json!({})); }
    Ok(serde_json::from_str(&text)?)
}

fn write_with_backup(path: &Path, value: &Value) -> Result<(), InstallError> {
    let target = if path.is_symlink() { std::fs::canonicalize(path)? } else { path.to_path_buf() };
    let name = target.file_name().unwrap().to_string_lossy().into_owned();
    if target.exists() {
        let backup = target.with_file_name(format!("{name}.codepets-backup"));
        if !backup.exists() { std::fs::copy(&target, &backup)?; }
    } else if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut text = serde_json::to_string_pretty(value)?;
    text.push('\n');
    let tmp = target.with_file_name(format!("{name}.codepets-tmp"));
    std::fs::write(&tmp, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::rename(&tmp, &target)?;
    Ok(())
}

pub fn install(path: &Path, url: &str, token: &str) -> Result<(), InstallError> {
    let mut v = read_settings(path)?;
    merge_hooks(&mut v, url, token)?;
    write_with_backup(path, &v)
}

pub fn uninstall(path: &Path, url: &str) -> Result<(), InstallError> {
    if !path.exists() { return Ok(()); }
    let mut v = read_settings(path)?;
    remove_hooks(&mut v, url);
    write_with_backup(path, &v)
}

pub fn default_settings_path() -> PathBuf {
    let base = std::env::var_os("CLAUDE_CONFIG_DIR").map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|b| b.home_dir().join(".claude")))
        .unwrap_or_else(|| PathBuf::from(".claude"));
    base.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const URL: &str = "http://127.0.0.1:7433/hook";

    #[test] fn merge_registers_the_compaction_and_failure_hooks() {
        let mut s = json!({});
        merge_hooks(&mut s, URL, "tok").unwrap();
        for ev in ["PreCompact", "PostCompact", "StopFailure"] {
            assert_eq!(s["hooks"][ev].as_array().map(Vec::len), Some(1), "{ev}");
        }
    }

    #[test] fn merge_into_empty_adds_all_events() {
        let mut s = json!({"model":"x"});
        merge_hooks(&mut s, URL, "tok").unwrap();
        for ev in HOOK_EVENTS { assert_eq!(s["hooks"][ev].as_array().unwrap().len(), 1, "{ev}"); }
        assert_eq!(s["model"], "x");
        assert_eq!(s["hooks"]["Stop"][0]["hooks"][0]["type"], "http");
        assert_eq!(s["hooks"]["Stop"][0]["hooks"][0]["headers"]["Authorization"], "Bearer tok");
        assert_eq!(s["hooks"]["Stop"][0]["hooks"][0]["timeout"], 2);
        assert_eq!(s["hooks"]["PermissionRequest"][0]["hooks"][0]["timeout"], PERMISSION_HOOK_TIMEOUT, "held for the bubble");
        assert_eq!(s["hooks"]["PreToolUse"][0]["hooks"][0]["timeout"], 2);
    }

    #[test] fn merge_is_idempotent() {
        let mut s = json!({});
        merge_hooks(&mut s, URL, "tok").unwrap();
        merge_hooks(&mut s, URL, "tok").unwrap();
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }

    #[test] fn merge_replaces_stale_token_for_same_url() {
        let mut s = json!({});
        merge_hooks(&mut s, URL, "old").unwrap();
        merge_hooks(&mut s, URL, "new").unwrap();
        assert_eq!(s["hooks"]["Stop"][0]["hooks"][0]["headers"]["Authorization"], "Bearer new");
    }

    #[test] fn merge_keeps_foreign_entries_and_remove_only_touches_ours() {
        let mut s = json!({"hooks":{"Stop":[{"matcher":"","hooks":[{"type":"command","command":"say done"}]}],
                                    "PreToolUse":[{"matcher":"Bash","hooks":[{"type":"command","command":"lint"}]}]}});
        merge_hooks(&mut s, URL, "tok").unwrap();
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert!(is_installed(&s, URL));
        remove_hooks(&mut s, URL);
        assert!(!is_installed(&s, URL));
        assert_eq!(s["hooks"]["Stop"].as_array().unwrap().len(), 1);
        assert_eq!(s["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(s["hooks"]["PreToolUse"][0]["matcher"], "Bash");
        assert!(s["hooks"].get("SessionStart").is_none(), "events we created are removed when empty");
    }

    #[test] fn remove_tolerates_mixed_legacy_shapes() {
        // a foreign entry whose "hooks" is not an array, and one group where hooks is a string
        let mut s = json!({"hooks":{"Stop":[{"hooks":"weird"}, 42]}});
        merge_hooks(&mut s, URL, "tok").unwrap();
        remove_hooks(&mut s, URL);
        assert_eq!(s["hooks"]["Stop"], json!([{"hooks":"weird"}, 42]));
    }

    #[test] fn install_writes_backup_and_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"model":"fable"}"#).unwrap();
        install(&p, URL, "tok").unwrap();
        let backup = std::fs::read_to_string(d.path().join("settings.json.codepets-backup")).unwrap();
        assert_eq!(backup, r#"{"model":"fable"}"#);
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(is_installed(&v, URL));
        assert_eq!(v["model"], "fable");
        uninstall(&p, URL).unwrap();
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(!is_installed(&v, URL));
    }

    #[test] fn install_on_missing_file_creates_it() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        install(&p, URL, "tok").unwrap();
        assert!(p.is_file());
    }

    #[test] fn install_on_invalid_json_fails_without_writing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, "{ not json").unwrap();
        assert!(matches!(install(&p, URL, "tok"), Err(InstallError::Json(_))));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "{ not json");
    }

    #[test] fn install_writes_atomically_and_leaves_no_temp_file() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"model":"fable"}"#).unwrap();
        install(&p, URL, "tok").unwrap();
        assert!(!d.path().join("settings.json.codepets-tmp").exists());
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        assert!(is_installed(&v, URL));
    }

    #[cfg(unix)]
    #[test] fn install_writes_owner_only_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        install(&p, URL, "tok").unwrap();
        let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[cfg(unix)]
    #[test] fn install_through_symlink_preserves_link() {
        let d = tempfile::tempdir().unwrap();
        let real = d.path().join("real.json");
        let link = d.path().join("settings.json");
        std::fs::write(&real, r#"{"model":"fable"}"#).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        install(&link, URL, "tok").unwrap();
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&real).unwrap()).unwrap();
        assert!(is_installed(&v, URL));
    }

    #[test] fn backup_is_written_once() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"model":"orig"}"#).unwrap();
        install(&p, URL, "a").unwrap();
        install(&p, URL, "b").unwrap();
        let backup = std::fs::read_to_string(d.path().join("settings.json.codepets-backup")).unwrap();
        assert_eq!(backup, r#"{"model":"orig"}"#);
    }

    #[test] fn install_on_unexpected_shape_fails_without_writing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        std::fs::write(&p, r#"{"hooks":{"Stop":"nope"}}"#).unwrap();
        assert!(matches!(install(&p, URL, "tok"), Err(InstallError::Shape(_))));
        assert_eq!(std::fs::read_to_string(&p).unwrap(), r#"{"hooks":{"Stop":"nope"}}"#);
    }

    #[test] fn merge_rejects_non_object_hooks() {
        let mut s = json!({"hooks": []});
        assert!(merge_hooks(&mut s, URL, "tok").is_err());
    }

    #[test] fn remove_keeps_foreign_empty_arrays() {
        let mut s = json!({"hooks":{"Foo":[]}});
        merge_hooks(&mut s, URL, "tok").unwrap();
        remove_hooks(&mut s, URL);
        assert_eq!(s["hooks"]["Foo"], json!([]));
    }

    #[test] fn uninstall_missing_file_is_noop() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("settings.json");
        uninstall(&p, URL).unwrap();
        assert!(!p.exists());
    }
}
