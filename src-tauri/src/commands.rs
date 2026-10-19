use crate::config::{config_path, Config, SpriteSize};
use crate::installer;
use crate::packs::{list_pack_infos, PackInfo, list_packs, load_pack, Manifest};
use crate::state::store::AgentView;
use crate::state::agent_state::provisional_name;
use crate::windows::manager::{child_manifest, collapsed_origin, effective_manifest, label_for, label_for_view, manifest_for_label, reposition_children, resize_all, save_position};
use crate::windows::positions::Pos;
use crate::App;
use base64::Engine;
use std::path::{Component, Path};
use std::sync::Arc;
use std::time::Instant;
use tauri::{AppHandle, Emitter, EventTarget, Manager, State};

#[derive(serde::Serialize)]
pub struct Status {
    pub hooks_installed: bool, pub poll_error: Option<String>, pub server_error: Option<String>,
    pub pack_warning: Option<String>, pub hook_url: String, pub token: String, pub pack: String,
    pub size: SpriteSize,
    pub wander: bool,
    pub wander_after_secs: u32,
    pub show_name: bool,
    pub permission_wait_secs: u32,
    pub version: &'static str,
}

#[tauri::command]
pub fn get_manifest(app: State<Arc<App>>, key: Option<String>) -> Option<Manifest> {
    let m = app.manifest.lock().unwrap().clone()?;
    let is_child = key.map(|k| app.store.lock().unwrap().views(Instant::now()).iter().any(|v| v.key == k && v.parent.is_some())).unwrap_or(false);
    Some(if is_child { child_manifest(&m) } else { m })
}

/// True if `s` is exactly one plain path component (no separators, `..`, root or prefix).
fn is_single_normal(s: &str) -> bool {
    let mut c = Path::new(s).components();
    matches!((c.next(), c.next()), (Some(Component::Normal(n)), None) if n == s)
}

#[tauri::command]
pub fn get_pack_url(app: State<Arc<App>>, sheet: String) -> Result<String, String> {
    if !is_single_normal(&sheet) { return Err(format!("invalid sheet name: {sheet}")); }
    let in_pack = app.manifest.lock().unwrap().as_ref().is_some_and(|m| m.animations.values().any(|a| a.sheet == sheet));
    if !in_pack {
        return Err(format!("sheet not in current pack: {sheet}"));
    }
    let pack = app.config.lock().unwrap().pack.clone();
    let path = app.packs_root.join(&pack).join(&sheet);
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

#[tauri::command(async)]
pub fn report_position(handle: AppHandle, app: State<Arc<App>>, key: String, x: i32, y: i32) {
    let view = app.store.lock().unwrap().views(Instant::now()).into_iter().find(|v| v.key == key);
    let Some(view) = view else { return };
    // subagent sprites hug their parent and never persist a position
    if view.parent.is_some() { return; }
    // the provisional session_id[:8] name is throwaway: saving under it would never be found again
    if view.name == provisional_name(&view.session_id) { return; }
    // saved positions always describe the collapsed (sprite-only) rect
    let label = label_for(&key);
    let open = app.placement.lock().unwrap().is_open(&label);
    let manifest = app.manifest.lock().unwrap().clone();
    let origin = match manifest {
        Some(m) => collapsed_origin(open, Pos { x, y }, &m),
        None => Pos { x, y },
    };
    // window moves caused by a stroll are not the user's: never save them
    let now = Instant::now();
    if app.wanderers.lock().unwrap().ignores_reports(&label, now) { return; }
    save_position(&mut app.positions.lock().unwrap(), &view.host, &view.name, origin.x, origin.y);
    app.placement.lock().unwrap().mark_dragged(&label);
    app.wanderers.lock().unwrap().rehome(&label, origin, now);
    if let Some(m) = app.manifest.lock().unwrap().clone() { reposition_children(&handle, &view.key, &m, &app.placement); }
}

/// Push a new effective manifest to every sprite window and resize them around their sprites.
fn apply_manifest(handle: &AppHandle, app: &App, old: Option<&Manifest>, new: &Manifest) {
    if let Some(old) = old { resize_all(handle, old, new, &app.placement); }
    for label in handle.webview_windows().into_keys() {
        if label.starts_with("sprite-") {
            let m = manifest_for_label(new, &label);
            let _ = handle.emit_to(EventTarget::webview_window(label), "manifest", &m);
        }
    }
}

/// Change the on-screen sprite size (small / normal / large) and resize every window in place.
#[tauri::command(async)]
pub fn set_size(handle: AppHandle, app: State<Arc<App>>, size: String) -> Result<(), String> {
    let size = SpriteSize::parse(&size).ok_or_else(|| format!("unknown size: {size}"))?;
    do_set_size(&handle, &app, size)
}

pub fn do_set_size(handle: &AppHandle, app: &App, size: SpriteSize) -> Result<(), String> {
    { let mut cfg = app.config.lock().unwrap(); cfg.size = size; if let Err(e) = cfg.save(&config_path()) { log::warn!("cannot save config: {e}"); } }
    reapply_pack(handle, app)
}

/// Show or hide the session-name label under every sprite.
#[tauri::command(async)]
pub fn set_show_name(handle: AppHandle, app: State<Arc<App>>, enabled: bool) -> Result<(), String> { do_set_show_name(&handle, &app, enabled) }

pub fn do_set_show_name(handle: &AppHandle, app: &App, enabled: bool) -> Result<(), String> {
    { let mut cfg = app.config.lock().unwrap(); cfg.show_name = enabled; if let Err(e) = cfg.save(&config_path()) { log::warn!("cannot save config: {e}"); } }
    reapply_pack(handle, app)
}

/// Reload the configured pack with the current settings and resize/refresh every sprite window.
fn reapply_pack(handle: &AppHandle, app: &App) -> Result<(), String> {
    let (pack, size, show_name) = { let c = app.config.lock().unwrap(); (c.pack.clone(), c.size, c.show_name) };
    let loaded = load_pack(&app.packs_root.join(&pack)).map_err(|e| e.to_string())?;
    let m = effective_manifest(&loaded, size, show_name);
    let old = app.manifest.lock().unwrap().replace(m.clone());
    apply_manifest(handle, app, old.as_ref(), &m);
    crate::tray::refresh_menu(handle, app);
    Ok(())
}

/// Turn idle strolling on or off.
#[tauri::command]
pub fn set_wander(handle: AppHandle, app: State<Arc<App>>, enabled: bool, after_secs: u32) -> Result<(), String> {
    do_set_wander(&app, enabled, after_secs)?;
    crate::tray::refresh_menu(&handle, &app);
    Ok(())
}

pub fn do_set_wander(app: &App, enabled: bool, after_secs: u32) -> Result<(), String> {
    if !(5..=3600).contains(&after_secs) { return Err("the delay must be between 5 and 3600 seconds".into()); }
    let mut cfg = app.config.lock().unwrap();
    cfg.wander = enabled;
    cfg.wander_after_secs = after_secs;
    if let Err(e) = cfg.save(&config_path()) { log::warn!("cannot save config: {e}"); }
    Ok(())
}

/// Diagnostics from the webviews (they have no visible console in release builds).
#[tauri::command]
pub fn ui_log(msg: String) { log::debug!("ui: {msg}"); }

/// Answer a permission prompt held for this agent: "allow" | "deny" | "terminal".
#[tauri::command]
pub fn answer_permission(app: State<Arc<App>>, key: String, decision: String) -> Result<(), String> {
    let d = crate::collector::hooks_server::Decision::parse(&decision).ok_or_else(|| format!("unknown decision: {decision}"))?;
    let delivered = app.parked.answer(&key, d);
    log::info!("answer_permission {key}: {d:?} delivered={delivered}");
    if !delivered { return Err("that permission is no longer pending".into()); }
    Ok(())
}

/// How long permission prompts are held for the bubble (0 disables holding).
#[tauri::command]
pub fn set_permission_wait(handle: AppHandle, app: State<Arc<App>>, secs: u32) -> Result<(), String> {
    do_set_permission_wait(&app, secs)?; crate::tray::refresh_menu(&handle, &app); Ok(())
}

pub fn do_set_permission_wait(app: &App, secs: u32) -> Result<(), String> {
    if secs > 600 { return Err("600 seconds maximum".into()); }
    { let mut cfg = app.config.lock().unwrap(); cfg.permission_wait_secs = secs; if let Err(e) = cfg.save(&config_path()) { log::warn!("cannot save config: {e}"); } }
    *app.permission_wait.lock().unwrap() = std::time::Duration::from_secs(secs as u64);
    Ok(())
}

/// Bring the terminal hosting this agent's session to the front (double-click on the sprite).
#[tauri::command(async)]
pub fn focus_terminal(app: State<Arc<App>>, key: String) -> Result<(), String> { do_focus_terminal(&app, &key) }

pub fn do_focus_terminal(app: &App, key: &str) -> Result<(), String> {
    let views = app.store.lock().unwrap().views(Instant::now());
    let mut view = views.iter().find(|v| v.key == key).cloned().ok_or("unknown session")?;
    // a subagent sprite focuses its parent's terminal
    if let Some(parent) = &view.parent { view = views.iter().find(|v| &v.key == parent).cloned().ok_or("unknown parent session")?; }
    if view.pid.is_none() && view.job_id.is_none() { return Err("this session has no terminal attached".into()); }
    let r = crate::focus::focus_session(view.pid, view.job_id.as_deref());
    if let Err(e) = &r { log::warn!("focus_terminal {}: {e}", view.name); }
    r
}

/// Open or close the speech bubble: the window grows/shrinks around the sprite.
#[tauri::command(async)]
pub fn set_bubble(handle: AppHandle, app: State<Arc<App>>, key: String, open: bool) -> Result<(), String> {
    let manifest = app.manifest.lock().unwrap().clone().ok_or("no pack loaded")?;
    let view = app.store.lock().unwrap().views(Instant::now()).into_iter().find(|v| v.key == key).ok_or("unknown session")?;
    let label = label_for_view(&view);
    let m = manifest_for_label(&manifest, &label);
    crate::windows::manager::set_bubble(&handle, &label, &m, open, &app.placement)
}

#[tauri::command]
pub fn list_agents(app: State<Arc<App>>) -> Vec<AgentView> { app.store.lock().unwrap().views(Instant::now()) }

#[tauri::command]
pub fn get_status(app: State<Arc<App>>) -> Status { status_of(&app) }

pub fn status_of(app: &App) -> Status {
    let cfg = app.config.lock().unwrap().clone();
    let settings = std::fs::read_to_string(installer::default_settings_path()).ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok()).unwrap_or(serde_json::json!({}));
    Status { hooks_installed: installer::is_installed(&settings, &cfg.hook_url()),
             poll_error: app.poll_error.borrow().clone(), server_error: app.server_error.borrow().clone(),
             pack_warning: app.pack_warning.borrow().clone(), hook_url: cfg.hook_url(), token: cfg.token.clone(), pack: cfg.pack, size: cfg.size, wander: cfg.wander, wander_after_secs: cfg.wander_after_secs, show_name: cfg.show_name, permission_wait_secs: cfg.permission_wait_secs, version: env!("CARGO_PKG_VERSION") }
}

#[tauri::command]
pub fn install_hooks(handle: AppHandle, app: State<Arc<App>>) -> Result<(), String> {
    do_set_hooks(&app, true)?; crate::tray::refresh_menu(&handle, &app); Ok(())
}

#[tauri::command]
pub fn uninstall_hooks(handle: AppHandle, app: State<Arc<App>>) -> Result<(), String> {
    do_set_hooks(&app, false)?; crate::tray::refresh_menu(&handle, &app); Ok(())
}

pub fn do_set_hooks(app: &App, install: bool) -> Result<(), String> {
    let cfg = app.config.lock().unwrap().clone();
    let path = installer::default_settings_path();
    let r = if install { installer::install(&path, &cfg.hook_url(), &cfg.token) } else { installer::uninstall(&path, &cfg.hook_url()) };
    r.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_pack_names(app: State<Arc<App>>) -> Vec<PackInfo> { list_pack_infos(&app.packs_root) }

#[tauri::command]
pub fn set_pack(handle: AppHandle, app: State<Arc<App>>, name: String) -> Result<(), String> { do_set_pack(&handle, &app, name) }

pub fn do_set_pack(handle: &AppHandle, app: &App, name: String) -> Result<(), String> {
    if !is_single_normal(&name) || !list_packs(&app.packs_root).contains(&name) {
        return Err(format!("unknown pack: {name}"));
    }
    let loaded = load_pack(&app.packs_root.join(&name)).map_err(|e| e.to_string())?;
    let (size, show_name) = {
        let mut cfg = app.config.lock().unwrap();
        cfg.pack = name;
        if let Err(e) = cfg.save(&config_path()) { log::warn!("cannot save config: {e}"); }
        (cfg.size, cfg.show_name)
    };
    let m = effective_manifest(&loaded, size, show_name);
    let old = app.manifest.lock().unwrap().replace(m.clone());
    app.pack_warning.send_replace(None);
    apply_manifest(handle, app, old.as_ref(), &m);
    crate::tray::refresh_menu(handle, app);
    Ok(())
}

#[tauri::command]
pub fn get_config(app: State<Arc<App>>) -> Config { app.config.lock().unwrap().clone() }

/// Hide an agent's sprite (and its subagents') without touching the session. It stays hidden until
/// shown again or until the agent ends.
#[tauri::command]
pub fn hide_agent(handle: AppHandle, app: State<Arc<App>>, key: String) -> Result<(), String> {
    if !app.store.lock().unwrap().views(Instant::now()).iter().any(|v| v.key == key) { return Err("unknown session".into()); }
    app.hidden.lock().unwrap().insert(key.clone());
    app.wanderers.lock().unwrap().remove(&label_for(&key));
    crate::windows::manager::close_agent_windows(&handle, &key, &app.placement);
    Ok(())
}

/// Bring a hidden agent's sprite (and its subagents') back.
#[tauri::command(async)]
pub fn show_agent(handle: AppHandle, app: State<Arc<App>>, key: String) -> Result<(), String> {
    app.hidden.lock().unwrap().remove(&key);
    let Some(m) = app.manifest.lock().unwrap().clone() else { return Ok(()) };
    let views = app.store.lock().unwrap().views(Instant::now());
    // the agent first, so its children can hug it
    for v in views.iter().filter(|v| v.key == key).chain(views.iter().filter(|v| v.parent.as_deref() == Some(key.as_str()))) {
        if !crate::windows::manager::is_hidden(&app.hidden, v) {
            crate::windows::manager::reveal(&handle, v, &m, &app.positions, &app.placement);
        }
    }
    Ok(())
}

#[tauri::command]
pub fn list_hidden(app: State<Arc<App>>) -> Vec<String> {
    let mut v: Vec<String> = app.hidden.lock().unwrap().iter().cloned().collect();
    v.sort();
    v
}

/// End a session by asking its `claude` process to exit. Only for top-level sessions whose pid is
/// known; the UI asks for confirmation first.
#[tauri::command(async)]
pub fn terminate_agent(app: State<Arc<App>>, key: String) -> Result<(), String> {
    let view = app.store.lock().unwrap().views(Instant::now()).into_iter().find(|v| v.key == key).ok_or("unknown session")?;
    if view.parent.is_some() { return Err("subagents end with their session".into()); }
    if view.visible == crate::state::Visible::Gone { return Err("that session already ended".into()); }
    let pid = view.pid.ok_or("this session has no known process to terminate")?;
    log::info!("terminate_agent {} pid {pid}", view.name);
    crate::terminate::terminate(pid)
}
