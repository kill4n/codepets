use crate::config::SpriteSize;
use crate::packs::Manifest;
use crate::state::agent_state::provisional_name;
use crate::state::store::{AgentView, Change};
use crate::state::Visible;
use crate::windows::positions::{Pos, Positions};
use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, EventTarget, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindowBuilder};

pub fn label_for(key: &str) -> String { format!("sprite-{}", key.replace([':', '/', '.'], "_")) }

/// Minimum window width so the speech bubble (name, folder, state + reason/peer + time) fits.
pub const MIN_WINDOW_W: f64 = 220.0;
/// Room above the sprite for the speech bubble and its tail.
pub const BUBBLE_H: f64 = 112.0;

/// Height of the session-name label under the sprite.
pub const LABEL_H: u32 = 18;

/// The pack manifest with the user's settings applied: size → `scale`, show_name → `label_h`.
pub fn effective_manifest(m: &Manifest, size: SpriteSize, show_name: bool) -> Manifest {
    let mut e = m.clone();
    e.scale = size.apply(m.scale);
    e.label_h = if show_name { LABEL_H } else { 0 };
    e
}

/// Label height for subagent sprites.
pub const CHILD_LABEL_H: u32 = 14;

/// Whether a window label belongs to a subagent sprite (its key contains the `/agent_id` suffix,
/// which `label_for` turns into `_`, so children are marked explicitly).
pub fn is_child_label(label: &str) -> bool { label.starts_with("sprite-sub-") }

pub fn label_for_view(v: &AgentView) -> String {
    if v.parent.is_some() { format!("sprite-sub-{}", v.key.replace([':', '/', '.'], "_")) } else { label_for(&v.key) }
}

/// The manifest a subagent sprite uses: half the parent's scale (never below 1) and a smaller label.
pub fn child_manifest(m: &Manifest) -> Manifest {
    let mut c = m.clone();
    if m.scale >= 2 { c.scale = m.scale / 2; } else { c.zoom = m.zoom * 0.5; }
    c.label_h = if m.label_h > 0 { CHILD_LABEL_H } else { 0 };
    c
}

/// The manifest for a given window label.
pub fn manifest_for_label(m: &Manifest, label: &str) -> Manifest {
    if is_child_label(label) { child_manifest(m) } else { m.clone() }
}

/// Where the `index`-th child of a parent at `parent` (collapsed size `pw`×`ph`) goes: hugging the
/// parent's side, alternating right/left, bottoms aligned, stacking outwards; kept on screen.
pub fn child_position(parent: Pos, pw: i32, ph: i32, cw: i32, ch: i32, index: usize, screen_w: i32) -> Pos {
    const GAP: i32 = 6;
    let slot = (index / 2) as i32;
    let right = index % 2 == 0;
    let y = (parent.y + ph - ch).max(0);
    let x = if right { parent.x + pw + GAP + slot * (cw + GAP) } else { parent.x - GAP - cw - slot * (cw + GAP) };
    Pos { x: x.clamp(0, (screen_w - cw).max(0)), y }
}

/// Window size while the bubble is closed: the sprite plus its name label (same width), so clicks
/// around it fall through.
pub fn collapsed_size(m: &Manifest) -> (f64, f64) {
    let [w, h] = m.frame_size;
    (((w * m.scale) as f64 * m.zoom).round(), ((h * m.scale) as f64 * m.zoom).round() + m.label_h as f64 - label_pull(m))
}

/// How far (screen px) the name label is pulled up over the empty space under the sprite's feet.
pub fn label_pull(m: &Manifest) -> f64 {
    if m.label_h == 0 { return 0.0; }
    (m.label_overlap * m.scale) as f64 * m.zoom
}

/// Window size while the bubble is open: wide enough for the text, taller by the bubble area.
pub fn window_size(m: &Manifest) -> (f64, f64) {
    let (w, h) = collapsed_size(m);
    (w.max(MIN_WINDOW_W), h + BUBBLE_H)
}

/// How far the open window's top-left sits from the collapsed one's, so the sprite stays put on screen.
pub fn bubble_offset(m: &Manifest) -> (f64, f64) {
    let (cw, _) = collapsed_size(m);
    let (ow, _) = window_size(m);
    (((ow - cw) / 2.0).round(), BUBBLE_H)
}

/// Outer position + size for a window whose collapsed (sprite) rect starts at `collapsed`.
pub fn rect_for(open: bool, collapsed: Pos, m: &Manifest) -> (Pos, (f64, f64)) {
    if !open { return (collapsed, collapsed_size(m)); }
    let (dx, dy) = bubble_offset(m);
    (Pos { x: collapsed.x - dx as i32, y: collapsed.y - dy as i32 }, window_size(m))
}

/// Given the window's current outer position and whether its bubble is open, the collapsed rect origin.
pub fn collapsed_origin(open: bool, current: Pos, m: &Manifest) -> Pos {
    if !open { return current; }
    let (dx, dy) = bubble_offset(m);
    Pos { x: current.x + dx as i32, y: current.y + dy as i32 }
}

/// Per-window bookkeeping for applying the saved `host+name` position once the real name is known.
#[derive(Default)]
pub struct Placement {
    /// labels whose window the user has moved (after the real name was known)
    dragged: HashSet<String>,
    /// last name each window was mirrored with
    names: HashMap<String, String>,
    /// labels whose speech bubble is open (window expanded)
    open: HashSet<String>,
}

impl Placement {
    pub fn created(&mut self, label: &str, name: &str) { self.names.insert(label.into(), name.into()); }
    pub fn mark_dragged(&mut self, label: &str) { self.dragged.insert(label.into()); }
    pub fn removed(&mut self, label: &str) { self.dragged.remove(label); self.names.remove(label); self.open.remove(label); }
    pub fn is_open(&self, label: &str) -> bool { self.open.contains(label) }
    /// Returns the previous open state.
    pub fn set_open(&mut self, label: &str, open: bool) -> bool {
        if open { !self.open.insert(label.into()) } else { self.open.remove(label) }
    }
    /// Record `v.name` for `label`; true when the window should now move to the saved position of
    /// the real name: the name just changed from the provisional one and the user has not dragged it.
    pub fn renamed(&mut self, label: &str, v: &AgentView) -> bool {
        let provisional = provisional_name(&v.session_id);
        let Some(prev) = self.names.insert(label.into(), v.name.clone()) else { return false };
        prev != v.name && prev == provisional && v.name != provisional && !self.dragged.contains(label)
    }
}

pub fn apply_changes(app: &AppHandle, changes: &[Change], manifest: Option<&Manifest>,
                     positions: &Mutex<Positions>, placement: &Mutex<Placement>) {
    for ch in changes {
        match ch {
            Change::Added(v) | Change::Updated(v) => {
                let label = label_for_view(v);
                if app.get_webview_window(&label).is_some() {
                    let should_move = placement.lock().unwrap().renamed(&label, v);
                    if should_move { move_to_saved(app, &label, v, positions); }
                } else if let Some(m) = manifest {
                    ensure_window(app, v, m, positions, placement);
                }
                push_state(app, v);
            }
            Change::Removed(key) => {
                for label in [label_for(key), format!("sprite-sub-{}", key.replace([':', '/', '.'], "_"))] {
                    placement.lock().unwrap().removed(&label);
                    if let Some(w) = app.get_webview_window(&label) { let _ = w.close(); }
                }
            }
        }
    }
}

/// Emit only to this agent's window: `emit` on a window broadcasts to every webview.
fn push_state(app: &AppHandle, v: &AgentView) {
    let label = label_for_view(v);
    if app.get_webview_window(&label).is_some() {
        let _ = app.emit_to(EventTarget::webview_window(label), "agent-state", v);
    }
}

fn move_to_saved(app: &AppHandle, label: &str, v: &AgentView, positions: &Mutex<Positions>) {
    let saved = positions.lock().unwrap().get(&v.host, &v.name);
    let (Some(p), Some(w)) = (saved, app.get_webview_window(label)) else { return };
    if let Err(e) = w.set_position(LogicalPosition::new(p.x as f64, p.y as f64)) {
        log::warn!("cannot move {label} to saved position: {e}");
    }
}

// Never hold the positions/placement locks across calls that wait on the main thread
// (primary_monitor, build): main-thread commands may be waiting on those locks.
fn ensure_window(app: &AppHandle, v: &AgentView, manifest: &Manifest, positions: &Mutex<Positions>,
                 placement: &Mutex<Placement>) {
    // a gone agent never gets a new window (e.g. a SessionEnd for an unknown session)
    if v.visible == Visible::Gone { return; }
    let label = label_for_view(v);
    if app.get_webview_window(&label).is_some() { return; }
    let m_main = manifest;
    let child = child_manifest(m_main);
    let manifest = if v.parent.is_some() { &child } else { m_main };
    let (w, h) = collapsed_size(manifest);
    let (sw, sh) = app.primary_monitor().ok().flatten()
        .map(|m| { let s = m.size(); let f = m.scale_factor(); ((s.width as f64 / f) as i32, (s.height as f64 / f) as i32) })
        .unwrap_or((1440, 900));
    let pos = if let Some(parent_key) = &v.parent {
        // subagents hug their parent: no saved position, no cascade
        let plabel = label_for(parent_key);
        let parent_win = app.get_webview_window(&plabel);
        let parent_pos = parent_win.as_ref().and_then(current_pos).map(|cur| {
            let open = placement.lock().unwrap().is_open(&plabel);
            collapsed_origin(open, cur, m_main)
        });
        let siblings = child_labels(app, parent_key).len();
        match parent_pos {
            Some(pp) => { let (pw, ph) = collapsed_size(m_main); child_position(pp, pw as i32, ph as i32, w as i32, h as i32, siblings, sw) }
            None => positions.lock().unwrap().cascade(siblings, sw, sh, w as i32, h as i32),
        }
    } else {
        let saved = positions.lock().unwrap().get(&v.host, &v.name);
        saved.unwrap_or_else(|| {
            let index = app.webview_windows().keys().filter(|k| k.starts_with("sprite-") && !is_child_label(k)).count();
            positions.lock().unwrap().cascade(index, sw, sh, w as i32, h as i32)
        })
    };
    let url = WebviewUrl::App(format!("sprite/index.html?key={}", urlencoding(&v.key)).into());
    let built = WebviewWindowBuilder::new(app, &label, url)
        .title(&v.name).inner_size(w, h).position(pos.x as f64, pos.y as f64)
        .transparent(true).decorations(false).always_on_top(true).skip_taskbar(true)
        .resizable(false).shadow(false).visible_on_all_workspaces(true)
        // macOS: act on the first click even when the window is not key (buttons in the bubble)
        .accept_first_mouse(true)
        .build();
    match built {
        Ok(_) => placement.lock().unwrap().created(&label, &v.name),
        Err(e) => log::error!("cannot create sprite window: {e}"),
    }
}

/// Labels of the child windows of `parent_key`, in a stable order.
fn child_labels(app: &AppHandle, parent_key: &str) -> Vec<String> {
    let prefix = format!("sprite-sub-{}_", parent_key.replace([':', '/', '.'], "_"));
    let mut v: Vec<String> = app.webview_windows().into_keys().filter(|k| k.starts_with(&prefix)).collect();
    v.sort();
    v
}

/// Move every child sprite back next to its (possibly moved) parent. Locks are taken only briefly.
pub fn reposition_children(app: &AppHandle, parent_key: &str, m: &Manifest, placement: &Mutex<Placement>) {
    let plabel = label_for(parent_key);
    let Some(pw_win) = app.get_webview_window(&plabel) else { return };
    let Some(cur) = current_pos(&pw_win) else { return };
    let parent_open = placement.lock().unwrap().is_open(&plabel);
    let parent = collapsed_origin(parent_open, cur, m);
    let (pw, ph) = collapsed_size(m);
    let child = child_manifest(m);
    let (cw, chh) = collapsed_size(&child);
    let screen_w = app.primary_monitor().ok().flatten().map(|mon| (mon.size().width as f64 / mon.scale_factor()) as i32).unwrap_or(1440);
    for (i, label) in child_labels(app, parent_key).into_iter().enumerate() {
        let Some(w) = app.get_webview_window(&label) else { continue };
        let target = child_position(parent, pw as i32, ph as i32, cw as i32, chh as i32, i, screen_w);
        let open = placement.lock().unwrap().is_open(&label);
        let (pos, _) = rect_for(open, target, &child);
        if let Err(e) = w.set_position(LogicalPosition::new(pos.x as f64, pos.y as f64)) { log::warn!("move child {label}: {e}"); }
    }
}

/// Current outer position of a window in logical pixels.
pub fn current_pos(w: &tauri::WebviewWindow) -> Option<Pos> {
    let p = w.outer_position().ok()?;
    let f = w.scale_factor().ok()?;
    Some(Pos { x: (p.x as f64 / f).round() as i32, y: (p.y as f64 / f).round() as i32 })
}

/// Expand (open) or shrink (closed) a sprite window around its sprite, which stays put on screen.
/// Locks are only held to flip the flag, never across the main-thread calls.
pub fn set_bubble(app: &AppHandle, label: &str, m: &Manifest, open: bool, placement: &Mutex<Placement>) -> Result<(), String> {
    let w = app.get_webview_window(label).ok_or_else(|| format!("no window {label}"))?;
    let was_open = placement.lock().unwrap().set_open(label, open);
    if was_open == open { return Ok(()); }
    let cur = current_pos(&w).ok_or("cannot read window position")?;
    let collapsed = collapsed_origin(was_open, cur, m);
    let (pos, (sw, sh)) = rect_for(open, collapsed, m);
    w.set_size(LogicalSize::new(sw, sh)).map_err(|e| e.to_string())?;
    w.set_position(LogicalPosition::new(pos.x as f64, pos.y as f64)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Resize every sprite window from the `old` to the `new` effective manifest, keeping each sprite's
/// top-left corner where it is. Locks are only touched to read the open flag.
pub fn resize_all(app: &AppHandle, old: &Manifest, new: &Manifest, placement: &Mutex<Placement>) {
    for (label, w) in app.webview_windows() {
        if !label.starts_with("sprite-") { continue; }
        let open = placement.lock().unwrap().is_open(&label);
        let Some(cur) = current_pos(&w) else { continue };
        let (old_m, new_m) = (manifest_for_label(old, &label), manifest_for_label(new, &label));
        let collapsed = collapsed_origin(open, cur, &old_m);
        let (pos, (sw, sh)) = rect_for(open, collapsed, &new_m);
        if let Err(e) = w.set_size(LogicalSize::new(sw, sh)) { log::warn!("resize {label}: {e}"); }
        if let Err(e) = w.set_position(LogicalPosition::new(pos.x as f64, pos.y as f64)) { log::warn!("move {label}: {e}"); }
    }
}

pub fn save_position(positions: &mut Positions, host: &str, name: &str, x: i32, y: i32) {
    positions.set(host, name, Pos { x, y });
}

fn urlencoding(s: &str) -> String {
    s.bytes().map(|b| match b {
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
        _ => format!("%{b:02X}"),
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(name: &str) -> AgentView {
        AgentView { key: "h:abcdef123456".into(), host: "h".into(), session_id: "abcdef123456".into(),
            name: name.into(), cwd: None, pid: None, job_id: None, parent: None, permission: None, visible: Visible::Working, waiting_for: None, peer: None, since_secs: 0 }
    }

    #[test] fn rename_from_provisional_triggers_move_once() {
        let mut p = Placement::default();
        p.created("w", "abcdef12");
        assert!(!p.renamed("w", &view("abcdef12")), "no rename yet");
        assert!(p.renamed("w", &view("buddy-7")), "provisional -> real name");
        assert!(!p.renamed("w", &view("buddy-7")), "only once");
        assert!(!p.renamed("w", &view("buddy-8")), "real -> real rename does not move");
    }

    #[test] fn dragged_window_is_not_moved_on_rename() {
        let mut p = Placement::default();
        p.created("w", "abcdef12");
        p.mark_dragged("w");
        assert!(!p.renamed("w", &view("buddy-7")));
    }

    #[test] fn unknown_or_removed_window_is_not_moved() {
        let mut p = Placement::default();
        assert!(!p.renamed("w", &view("buddy-7")));
        p.created("x", "abcdef12");
        p.mark_dragged("x");
        p.removed("x");
        p.created("x", "abcdef12");
        assert!(p.renamed("x", &view("buddy-7")), "removal clears the dragged flag");
    }

    #[test] fn renamed_agent_finds_position_saved_under_real_name() {
        let d = tempfile::tempdir().unwrap();
        let mut ps = Positions::load(d.path().join("p.json"));
        save_position(&mut ps, "h", "buddy-7", 300, 200);
        let mut p = Placement::default();
        p.created("w", "abcdef12");
        assert_eq!(ps.get("h", "abcdef12"), None, "provisional name has no saved position");
        let v = view("buddy-7");
        assert!(p.renamed("w", &v));
        assert_eq!(ps.get(&v.host, &v.name), Some(Pos { x: 300, y: 200 }));
    }

    fn manifest(frame: u32, scale: u32) -> Manifest {
        serde_json::from_value(serde_json::json!({
            "name": "t", "frameSize": [frame, frame], "scale": scale, "animations": {}
        })).unwrap()
    }

    #[test] fn effective_manifest_applies_size_to_scale() {
        let m = manifest(96, 2);
        assert_eq!(effective_manifest(&m, SpriteSize::Small, false).scale, 1);
        assert_eq!(effective_manifest(&m, SpriteSize::Large, false).scale, 3);
        assert_eq!(collapsed_size(&effective_manifest(&m, SpriteSize::Small, false)), (96.0, 96.0));
    }

    #[test] fn name_label_adds_its_height_below_the_sprite_only() {
        let m = manifest(96, 2);
        let e = effective_manifest(&m, SpriteSize::Normal, true);
        assert_eq!(e.label_h, LABEL_H);
        assert_eq!(collapsed_size(&e), (192.0, 192.0 + LABEL_H as f64), "same width, taller");
        assert_eq!(window_size(&e).1, 192.0 + LABEL_H as f64 + BUBBLE_H);
        assert_eq!(bubble_offset(&e), (14.0, BUBBLE_H), "label never shifts the sprite");
        let hidden = effective_manifest(&m, SpriteSize::Normal, false);
        assert_eq!(collapsed_size(&hidden), (192.0, 192.0));
    }

    #[test] fn child_manifest_is_half_scale_with_small_label() {
        let m = effective_manifest(&manifest(96, 2), SpriteSize::Normal, true);
        let c = child_manifest(&m);
        assert_eq!(c.scale, 1); assert_eq!(c.label_h, CHILD_LABEL_H);
        assert_eq!(collapsed_size(&c), (96.0, 96.0 + CHILD_LABEL_H as f64));
        // a parent already at scale 1 gets a half-size child through a fractional zoom
        let tiny = child_manifest(&effective_manifest(&manifest(96, 1), SpriteSize::Normal, false));
        assert_eq!((tiny.scale, tiny.zoom), (1, 0.5));
        assert_eq!(collapsed_size(&tiny), (48.0, 48.0));
        assert_eq!(tiny.label_h, 0);
    }

    #[test] fn children_hug_their_parent_alternating_sides() {
        let parent = Pos { x: 500, y: 300 };
        let (pw, ph, cw, ch) = (192, 210, 96, 110);
        assert_eq!(child_position(parent, pw, ph, cw, ch, 0, 1440), Pos { x: 500 + 192 + 6, y: 300 + 210 - 110 });
        assert_eq!(child_position(parent, pw, ph, cw, ch, 1, 1440), Pos { x: 500 - 6 - 96, y: 400 });
        assert_eq!(child_position(parent, pw, ph, cw, ch, 2, 1440), Pos { x: 500 + 192 + 6 + 102, y: 400 });
        assert_eq!(child_position(Pos { x: 10, y: 300 }, pw, ph, cw, ch, 1, 1440).x, 0, "clamped to the left edge");
        assert_eq!(child_position(Pos { x: 1400, y: 300 }, pw, ph, cw, ch, 0, 1440).x, 1440 - 96, "clamped to the right edge");
    }

    #[test] fn child_labels_are_distinct_and_recognisable() {
        let mut v = view("buddy-7");
        assert_eq!(label_for_view(&v), label_for(&v.key));
        v.parent = Some("h:abcdef123456".into()); v.key = "h:abcdef123456/ag-1".into();
        assert!(is_child_label(&label_for_view(&v)));
        assert!(!is_child_label(&label_for("h:abcdef123456")));
        assert_eq!(manifest_for_label(&manifest(96, 2), &label_for_view(&v)).scale, 1);
    }

    #[test] fn collapsed_window_is_exactly_the_sprite() {
        assert_eq!(collapsed_size(&manifest(96, 2)), (192.0, 192.0));
        assert_eq!(collapsed_size(&manifest(32, 4)), (128.0, 128.0));
    }

    #[test] fn name_label_is_pulled_up_over_the_empty_space_under_the_feet() {
        let mut m = manifest(96, 2);
        m.label_overlap = 8;
        assert_eq!(collapsed_size(&m), (192.0, 192.0), "no label, nothing to pull");
        m.label_h = LABEL_H;
        assert_eq!(label_pull(&m), 16.0);
        assert_eq!(collapsed_size(&m), (192.0, 192.0 + LABEL_H as f64 - 16.0));
        let c = child_manifest(&m);
        assert_eq!(label_pull(&c), 8.0, "half-size child pulls half as far");
    }

    #[test] fn open_rect_keeps_the_sprite_in_place() {
        let m = manifest(96, 2); // collapsed 192x192, open 220x(192+BUBBLE_H) -> offset (14, BUBBLE_H)
        let c = Pos { x: 500, y: 400 };
        let (p, size) = rect_for(true, c, &m);
        assert_eq!(size, (220.0, 192.0 + BUBBLE_H));
        assert_eq!(p, Pos { x: 486, y: 400 - BUBBLE_H as i32 });
        assert_eq!(collapsed_origin(true, p, &m), c, "round trip");
        assert_eq!(rect_for(false, c, &m), (c, (192.0, 192.0)));
        assert_eq!(collapsed_origin(false, c, &m), c);
        let wide = manifest(64, 4); // 256 wide: no horizontal offset
        assert_eq!(bubble_offset(&wide), (0.0, BUBBLE_H));
    }

    #[test] fn placement_tracks_open_state() {
        let mut p = Placement::default();
        assert!(!p.is_open("w"));
        assert!(!p.set_open("w", true), "was closed");
        assert!(p.is_open("w"));
        assert!(p.set_open("w", true), "already open");
        assert!(p.set_open("w", false), "was open");
        p.set_open("w", true); p.removed("w");
        assert!(!p.is_open("w"), "removal clears open");
    }

    #[test] fn window_is_at_least_bubble_wide_and_reserves_bubble_height() {
        assert_eq!(window_size(&manifest(32, 4)).0, MIN_WINDOW_W);
        assert_eq!(window_size(&manifest(64, 4)).0, 256.0);
        assert_eq!(window_size(&manifest(32, 4)).1, 128.0 + BUBBLE_H);
        assert_eq!(window_size(&manifest(64, 2)).1, 128.0 + BUBBLE_H);
    }
}
