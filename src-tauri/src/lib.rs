pub mod collector;
pub mod commands;
pub mod config;
pub mod focus;
pub mod installer;
pub mod packs;
pub mod state;
pub mod terminate;
pub mod tray;
pub mod windows;

use collector::event::AgentEvent;
use config::{config_path, positions_path, Config};
use packs::{load_pack, Manifest};
use state::store::{Change, Store};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{Emitter, EventTarget, Manager};
use tokio::sync::{mpsc, watch};
use windows::manager::Placement;
use windows::positions::Positions;

/// Host id for sessions observed on this machine.
pub const LOCAL_HOST: &str = "local";

pub struct App {
    pub config: Mutex<Config>,
    pub store: Mutex<Store>,
    pub positions: Mutex<Positions>,
    pub placement: Mutex<Placement>,
    /// agents whose sprite the user hid (keys); cleared when the agent goes away
    pub hidden: Mutex<std::collections::HashSet<String>>,
    pub wanderers: Mutex<windows::wander::Wanderers>,
    /// permission prompts held for the bubble, and how long to hold them
    pub parked: Arc<collector::hooks_server::Parked>,
    pub permission_wait: Arc<Mutex<Duration>>,
    /// None when no valid pack could be loaded: the app keeps running, sprites just don't render.
    pub manifest: Mutex<Option<Manifest>>,
    pub packs_root: PathBuf,
    pub host: String,
    pub poll_error: watch::Receiver<Option<String>>,
    pub server_error: watch::Sender<Option<String>>,
    pub pack_warning: watch::Sender<Option<String>>,
}

impl App {
    /// First active warning (hooks server, poller, pack), for the tray tooltip.
    pub fn first_warning(&self) -> Option<String> {
        let server = self.server_error.borrow().clone();
        let poll = self.poll_error.borrow().clone();
        let pack = self.pack_warning.borrow().clone();
        server.or(poll).or(pack)
    }
}

/// Where packs live inside the bundle's resource dir; must match the target of the
/// `bundle.resources` map in tauri.conf.json (checked by a test).
pub const PACKS_RESOURCE_DIR: &str = "assets/packs";

fn packs_root(handle: &tauri::AppHandle) -> PathBuf {
    // debug builds read the repo's packs directly; release builds only use the bundled resources
    if cfg!(debug_assertions) {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/packs");
        if dev.is_dir() { return dev; }
    }
    match handle.path().resource_dir() {
        Ok(r) => r.join(PACKS_RESOURCE_DIR),
        Err(e) => { log::error!("no resource dir: {e}"); PathBuf::from(PACKS_RESOURCE_DIR) }
    }
}

/// Load the configured pack, falling back to `default`. Never panics: returns the manifest (if any),
/// the pack name actually in use, and a warning for the tray/settings when something went wrong.
pub fn load_initial_pack(root: &Path, pack: &str) -> (Option<Manifest>, String, Option<String>) {
    match load_pack(&root.join(pack)) {
        Ok(m) => (Some(m), pack.to_string(), None),
        Err(e) if pack != "default" => {
            log::warn!("pack {pack} invalid ({e}), falling back to default");
            match load_pack(&root.join("default")) {
                Ok(m) => (Some(m), "default".into(), Some(format!("pack {pack} is invalid, using default"))),
                Err(e2) => {
                    log::error!("default pack invalid too ({e2}); sprites disabled");
                    (None, pack.to_string(), Some(format!("no valid pack in {}", root.display())))
                }
            }
        }
        Err(e) => {
            log::error!("default pack invalid ({e}); sprites disabled");
            (None, pack.to_string(), Some(format!("default pack is invalid in {}", root.display())))
        }
    }
}

pub fn run() {
    env_logger::init();
    let rt = tokio::runtime::Runtime::new().expect("tokio");
    let _guard = rt.enter();

    tauri::Builder::default()
        // must be the first plugin: a second launch just opens the settings of the running one
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::open_settings(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .setup(move |app| {
            // tray pet: no Dock icon, no Cmd-Tab entry
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let handle = app.handle().clone();
            let mut cfg = Config::load(&config_path());
            let root = packs_root(&handle);
            let (manifest, pack, pack_warn) = load_initial_pack(&root, &cfg.pack);
            let manifest = manifest.map(|m| windows::manager::effective_manifest(&m, cfg.size, cfg.show_name));
            cfg.pack = pack;
            let _ = cfg.save(&config_path());
            // Sessions collected on this machine use a fixed host id: the OS hostname changes with the
            // network (Mac.lan vs name.local) and positions are keyed by host, so sprites would get lost.
            // Remote hosts (future multi-PC) carry their own id in the events they send.
            let host = LOCAL_HOST.to_string();

            let (tx, mut rx) = mpsc::channel::<AgentEvent>(256);
            let (err_tx, err_rx) = watch::channel(None);

            let state = Arc::new(App {
                config: Mutex::new(cfg.clone()), store: Mutex::new(Store::new()),
                positions: Mutex::new(Positions::load(positions_path())),
                placement: Mutex::new(Placement::default()),
                hidden: Mutex::new(Default::default()),
                wanderers: Mutex::new(windows::wander::Wanderers::default()),
                parked: Arc::new(collector::hooks_server::Parked::default()),
                permission_wait: Arc::new(Mutex::new(Duration::from_secs(cfg.permission_wait_secs as u64))),
                manifest: Mutex::new(manifest), packs_root: root, host: host.clone(), poll_error: err_rx,
                server_error: watch::channel(None).0, pack_warning: watch::channel(pack_warn).0,
            });
            app.manage(state.clone());

            tray::build(&handle)?;
            tray::refresh_warning(&handle, &state);

            let addr = cfg.bind_addr();
            let (h2, t2, tx2, st2) = (host.clone(), cfg.token.clone(), tx.clone(), state.clone());
            let (parked, wait) = (state.parked.clone(), state.permission_wait.clone());
            tauri::async_runtime::spawn(async move {
                match collector::hooks_server::serve(addr, h2, t2, tx2, parked, wait).await {
                    Ok(a) => log::info!("hooks server on {a}"),
                    Err(e) => {
                        log::error!("hooks server failed to bind {addr}: {e}");
                        st2.server_error.send_replace(Some(format!("hooks server could not bind {addr}: {e}")));
                    }
                }
            });
            collector::poller::spawn_poller(host.clone(), cfg.claude_path.clone(), tx.clone(), err_tx);

            // event loop: apply events, mirror windows, sweep gone agents, reflect warnings in the tray
            let st = state.clone();
            let (mut poll_rx, mut server_rx, mut pack_rx) =
                (st.poll_error.clone(), st.server_error.subscribe(), st.pack_warning.subscribe());
            tauri::async_runtime::spawn(async move {
                let mut sweep = tokio::time::interval(Duration::from_secs(1));
                loop {
                    tokio::select! {
                        Some(ev) = rx.recv() => {
                            let changes = st.store.lock().unwrap().apply(&ev);
                            mirror(&handle, &st, changes);
                        }
                        _ = sweep.tick() => {
                            let changes = st.store.lock().unwrap().sweep(Instant::now());
                            mirror(&handle, &st, changes);
                            wander_tick(&handle, &st);
                        }
                        Ok(()) = poll_rx.changed() => tray::refresh_warning(&handle, &st),
                        Ok(()) = server_rx.changed() => tray::refresh_warning(&handle, &st),
                        Ok(()) = pack_rx.changed() => tray::refresh_warning(&handle, &st),
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_manifest, commands::get_pack_url, commands::report_position, commands::list_agents,
            commands::get_status, commands::install_hooks, commands::uninstall_hooks, commands::list_pack_names,
            commands::set_pack, commands::get_config, commands::set_bubble, commands::set_size, commands::focus_terminal, commands::set_wander, commands::set_show_name, commands::answer_permission, commands::set_permission_wait, commands::hide_agent, commands::show_agent, commands::list_hidden, commands::terminate_agent, commands::ui_log
        ])
        .build(tauri::generate_context!())
        .expect("error while building codepets")
        .run(|_app, event| {
            // keep running with no windows (tray app); an explicit app.exit(code) still quits
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() { api.prevent_exit(); }
            }
        });
}

/// Mirror store changes into sprite windows and refresh the settings window.
fn mirror(handle: &tauri::AppHandle, st: &Arc<App>, changes: Vec<Change>) {
    if changes.is_empty() { return; }
    tray::refresh_menu(handle, st);
    for ch in &changes {
        if let Change::Removed(key) = ch { st.wanderers.lock().unwrap().remove(&windows::manager::label_for(key)); }
    }
    let m = st.manifest.lock().unwrap().clone();
    windows::manager::apply_changes(handle, &changes, m.as_ref(), &st.positions, &st.placement, &st.hidden);
    notify_settings(handle, st);
}

/// Once a second: let idle sprites stroll, and bring strolling ones home when they get busy.
fn wander_tick(handle: &tauri::AppHandle, st: &Arc<App>) {
    use windows::wander::{step_position, Decision, STROLL_STEP, STROLL_STEPS};
    let (enabled, min_idle) = { let c = st.config.lock().unwrap(); (c.wander, Duration::from_secs(c.wander_after_secs as u64)) };
    if !enabled { return; }
    let Some(m) = st.manifest.lock().unwrap().clone() else { return };
    let now = Instant::now();
    let views = st.store.lock().unwrap().views(now);
    let screen_w = handle.primary_monitor().ok().flatten()
        .map(|mon| (mon.size().width as f64 / mon.scale_factor()) as i32).unwrap_or(1440);
    let (sprite_w, _) = windows::manager::collapsed_size(&m);
    for v in views {
        if v.parent.is_some() { continue; } // subagent sprites stay by their parent
        let label = windows::manager::label_for(&v.key);
        let Some(w) = handle.get_webview_window(&label) else { continue };
        let open = st.placement.lock().unwrap().is_open(&label);
        let strollable = !open && matches!(v.visible, state::Visible::Idle | state::Visible::WaitingPeer);
        let idle_for = strollable.then(|| Duration::from_secs(v.since_secs));
        let Some(cur) = windows::manager::current_pos(&w) else { continue };
        let pos = windows::manager::collapsed_origin(open, cur, &m);
        let decision = st.wanderers.lock().unwrap().tick(&label, idle_for, min_idle, pos, screen_w, sprite_w as i32, now);
        let (from, to, away) = match decision {
            Decision::Nothing => continue,
            Decision::Stroll { from, to } => (from, to, true),
            Decision::GoHome { from, to } => (from, to, false),
        };
        st.wanderers.lock().unwrap().stroll_started(&label);
        let (st2, label2, m2, handle2) = (st.clone(), label.clone(), m.clone(), handle.clone());
        tauri::async_runtime::spawn(async move {
            let open_now = st2.placement.lock().unwrap().is_open(&label2);
            let (dx, dy) = if open_now { windows::manager::bubble_offset(&m2) } else { (0.0, 0.0) };
            for i in 1..=STROLL_STEPS {
                let p = step_position(from, to, i, STROLL_STEPS);
                let _ = w.set_position(tauri::LogicalPosition::new(p.x as f64 - dx, p.y as f64 - dy));
                tokio::time::sleep(STROLL_STEP).await;
            }
            st2.wanderers.lock().unwrap().stroll_finished(&label2, away, Instant::now());
            // labels are sanitized keys: find the real key through the store, then bring the children along
            let views = st2.store.lock().unwrap().views(Instant::now());
            if let Some(v) = views.iter().find(|v| windows::manager::label_for(&v.key) == label2) {
                windows::manager::reposition_children(&handle2, &v.key, &m2, &st2.placement);
            }
        });
    }
}

fn notify_settings(handle: &tauri::AppHandle, st: &Arc<App>) {
    if handle.get_webview_window("settings").is_some() {
        let views = st.store.lock().unwrap().views(Instant::now());
        let _ = handle.emit_to(EventTarget::webview_window("settings"), "agents", views);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn bundle_resources_map_packs_to_packs_resource_dir() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let res = &conf["bundle"]["resources"];
        let target = res.get("../assets/packs/").and_then(|v| v.as_str())
            .expect("bundle.resources must map ../assets/packs/ (map form avoids the _up_ rewrite)");
        assert_eq!(target.trim_end_matches('/'), PACKS_RESOURCE_DIR);
        assert!(Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/packs/default/pack.json").is_file());
    }

    #[test] fn initial_pack_loads_default_from_repo() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../assets/packs");
        let (m, pack, warn) = load_initial_pack(&root, "default");
        assert!(m.is_some()); assert_eq!(pack, "default"); assert!(warn.is_none());
        let (m, pack, warn) = load_initial_pack(&root, "does-not-exist");
        assert!(m.is_some()); assert_eq!(pack, "default"); assert!(warn.is_some());
    }

    #[test] fn initial_pack_missing_everything_does_not_panic() {
        let d = tempfile::tempdir().unwrap();
        let (m, _, warn) = load_initial_pack(d.path(), "default");
        assert!(m.is_none()); assert!(warn.is_some());
        let (m, pack, warn) = load_initial_pack(d.path(), "cat");
        assert!(m.is_none()); assert_eq!(pack, "cat"); assert!(warn.is_some());
    }
}
