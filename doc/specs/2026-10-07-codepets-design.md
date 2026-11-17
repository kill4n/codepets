# CodePets — Desktop pet for Claude Code sessions

Date: 2026-10-07
Status: draft for review

## 1. Goal

CodePets is a desktop app with transparent windows that shows one animated sprite for each
active Claude Code session on the machine. The sprite changes animation according to the
agent's state so that, at a glance and without switching windows, you can tell which agent
is working, which is idle, which needs the user's attention and which is waiting for
another agent.

**Who it is for:** the user themself, with several Claude Code sessions open at once.

**Success:** look at the screen and know which agent needs attention without touching anything.

## 2. Decisions made

| Topic | Decision |
|---|---|
| Stack | Tauri 2 (Rust + WebView), cross-platform (macOS, Windows, Linux). |
| Multi-PC | Designed for it (`host` field, token, `bind_all`), not implemented in v1. |
| Sprite/session relationship | One sprite per session, passive: animates by state, click shows a tooltip. |
| Assets | Sprite packs described by a JSON manifest. Placeholder pack `default` included. |
| Windows | One transparent, draggable Tauri window per sprite, position persisted. |
| State source | Hybrid: Claude Code HTTP hooks (push) + poll of `claude agents --json --all`. |

## 3. Architecture

```
hook http ──► hooks_server ─┐
                            ├─► AgentEvent ─► state ─► change ─► windows ─► UI (sprite)
claude agents --json ─► poller ┘
```

### 3.1 Core (Rust, Tauri process)

- **collector**: normalizes both sources into `AgentEvent`.
  - `hooks_server`: HTTP on `127.0.0.1:7433` (configurable). `POST /hook` with
    `Authorization: Bearer <token>`. Responds with an empty `200` in < 50 ms; processing
    goes to an internal queue. `GET /health` lets the installer check the app.
  - `poller`: every 2.5 s runs `claude agents --json --all` (2 s timeout), parses it and emits
    a `Snapshot` event with the full list.
  - `event.rs`: pure translation from hook JSON and poll JSON to `AgentEvent`.
- **state**: per-agent state machine (section 4). Single source of truth.
- **windows**: observes `state`. Creates a window for each new agent, closes the windows of
  `gone` agents after a 10 s grace period, persists positions in the app's configuration
  directory (`positions.json`) keyed by `host + name`.
- **installer**: adds or removes the hooks block in `~/.claude/settings.json` (section 6).
- **packs**: parsing and validation of sprite manifests (section 5).
- **config**: port, token, active pack, `bind_all` (false by default), autostart.

### 3.2 UI (web, Vite + TypeScript, no heavy framework)

- **sprite**: one window per agent. Receives `AgentState` via a Tauri event and plays the
  pack's animation. Canvas 2D with `imageSmoothingEnabled = false`.
- **settings**: window opened from the tray icon.
- **shared**: types shared with the core.

### 3.3 Main types

```rust
struct AgentEvent { host: String, session_id: String, kind: EventKind, at: Instant }

enum EventKind {
    Hook { event: String, tool_name: Option<String>, notification_type: Option<String>,
           payload: serde_json::Value },
    Snapshot(Vec<PolledAgent>),
}

struct PolledAgent { session_id, name, cwd, kind: Interactive | Background,
                     status: Option<Busy | Waiting | Idle>, waiting_for: Option<String>,
                     state: Option<Working | Blocked | Done | Failed | Stopped> }

struct AgentState { id: AgentId /* host + session_id */, name: String, cwd: PathBuf,
                    visible: Visible, waiting_for: Option<String>, peer: Option<String>,
                    subagents: u32, since: Instant, last_hook_at: Option<Instant>,
                    last_seen: Instant }

enum Visible { Working, Idle, NeedsInput, Delegating, WaitingPeer, Compacting, Error, Gone }
```

## 4. State machine

Priority between sources: a hook received less than 5 s ago wins over the poll; after that
time the poll rules. `delegating` takes priority over `working` while `subagents > 0`.
`waiting_peer` is inferred; if the poll reports `waiting` with `waitingFor`, that data wins.

| Visible | Entered by | Left by |
|---|---|---|
| `working` | hook `UserPromptSubmit`, `PreToolUse`, `PostToolUse` (tool ≠ SendMessage); poll `status: busy` or `state: working` | any other event |
| `idle` | hook `Stop`; poll `status: idle` | `UserPromptSubmit` or `busy` |
| `needs_input` | hook `PermissionRequest`; `Notification` with `permission_prompt`, `elicitation_dialog` or `idle_prompt`; poll `status: waiting` (stores `waitingFor`); background `state: blocked` | `UserPromptSubmit`, `PostToolUse`, poll `busy` |
| `delegating` | `SubagentStart` (subagents += 1) | `SubagentStop` brings subagents to 0 |
| `waiting_peer` | `PostToolUse` with `tool_name == "SendMessage"` (stores the recipient) followed by `Stop` or poll `idle` | `UserPromptSubmit` or poll `busy` |
| `compacting` | hook `PreCompact` | hook `PostCompact` (back to `working`), any other turn event, or poll `idle`; poll `busy` keeps it |
| `error` | hook `StopFailure` (stores the error kind in `waiting_for`) | `UserPromptSubmit`, any tool event, or poll `busy`; poll `idle` keeps it |
| `gone` | `SessionEnd`; absent from the poll for 2 consecutive cycles; background `done`, `failed`, `stopped` | the same `session_id` reappears within the grace period (the close is cancelled) |

Additional rules:
- An agent that arrives via hook before appearing in the poll is created with the provisional
  name `session_id[:8]` and the hook's `cwd`; the poll completes it with `name`.
- `since` is reset only when `visible` changes.
- A malformed event is discarded and logged; it never changes the state.

## 5. Sprite packs

Folder `assets/packs/<name>/` with `pack.json` and one or more PNGs (spritesheets in a
grid, one row per animation).

```json
{
  "name": "default",
  "frameSize": [64, 64],
  "scale": 2,
  "animations": {
    "working":      { "sheet": "sheet.png", "row": 0, "frames": 6, "fps": 10 },
    "idle":         { "sheet": "sheet.png", "row": 1, "frames": 4, "fps": 4 },
    "needs_input":  { "sheet": "sheet.png", "row": 2, "frames": 4, "fps": 6 },
    "delegating":   { "sheet": "sheet.png", "row": 3, "frames": 6, "fps": 8 },
    "waiting_peer": { "sheet": "sheet.png", "row": 4, "frames": 4, "fps": 4 },
    "gone":         { "sheet": "sheet.png", "row": 5, "frames": 6, "fps": 12, "loop": false }
  }
}
```

- All six animations are required. `loop` defaults to `true`.
- The core validates the pack on load (fields present, PNGs exist, dimensions are a
  multiple of `frameSize`). If validation fails, it falls back to `default` and warns in the tray.
- We generate the `default` pack ourselves: simple pixel art, distinguishable by state
  (working: typing; idle: sleeping; needs_input: exclamation mark; delegating:
  mini-clones; waiting_peer: looking at a clock; gone: fading away).

## 6. Windows and UI

### 6.1 Sprite window
- Size `frameSize × scale` plus a margin for the tooltip. Transparent, undecorated,
  always-on-top, hidden from the taskbar and alt-tab.
- The whole surface is a drag region. On release, the UI sends the position to the core.
- A click without dragging toggles the tooltip: name, abbreviated `cwd`, state, `waitingFor` or
  the peer's recipient, and time spent in that state.
- Initial position: the last known one for that `host + name`; if none, cascaded from
  the bottom-right corner of the primary monitor.
- `?mock=<state>` mode for visual review without Claude Code.

### 6.2 Settings window (tray)
- Connect/disconnect hooks, with an installed indicator.
- Pack selector.
- List of active agents with their state.
- Server port and token.
- Launch at login.
- Warning icon if the collector or poller report errors.

## 7. Claude Code integration

### 7.1 Hooks installed by the installer

Events: `SessionStart`, `SessionEnd`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`,
`Stop`, `PermissionRequest`, `Notification`, `SubagentStart`, `SubagentStop`.

Entry per event:

```json
{ "hooks": [{ "type": "http", "url": "http://127.0.0.1:7433/hook",
              "headers": { "Authorization": "Bearer <token>" }, "timeout": 2 }] }
```

### 7.2 Installer rules
- Backup `settings.json.buddy-backup` before writing.
- Merges: adds our entry to the event's array; never deletes entries that are not ours.
- Our entries are identified by the URL; uninstalling means filtering by that URL.
- Short `timeout` so Claude Code is not slowed down if the app is not open.

### 7.3 Poller
- If `claude` is not on the PATH or the output does not parse: log once, show a
  warning, retry on the next cycle. The app keeps going with hooks only.

### 7.4 Multi-PC groundwork (not implemented in v1)
- `host` is filled with the local hostname.
- `bind_all` is off by default; when turned on, the server listens on `0.0.0.0`.
- The token already travels in a header. A remote PC would point its hooks at this IP.
- Remote polling would require a reporter on the other machine (out of scope).

## 8. Error handling

- Collector or poller failures: log + warning icon in the tray. They never close the app.
- Malformed event: discarded.
- Invalid pack: `default` is used.
- Invalid `settings.json` on install: nothing is written, the error is shown.

## 9. Project structure

```
buddy/
  src-tauri/
    src/
      main.rs
      collector/   hooks_server.rs, poller.rs, event.rs
      state/       agent_state.rs, machine.rs
      windows/     manager.rs, positions.rs
      installer/   settings_json.rs
      packs/       manifest.rs
      config.rs
    tauri.conf.json
  ui/
    sprite/
    settings/
    shared/
  assets/packs/default/
  doc/specs/
  releases/
```

The existing `api` folder is unused and is removed.

## 10. Testing

- Rust unit tests: `state::machine` (`(state, event) → state` table), `packs::manifest`
  (valid and invalid), `installer` (merge and uninstall on sample JSON),
  `collector::event` (hook and poll JSON → `AgentEvent`).
- Integration: `hooks_server` on a free port, send a recorded hook, verify the change in
  `state`.
- UI: manual visual review with `?mock`.
- Final manual check: two Claude Code sessions, a `SendMessage` between them, verify all
  six states.

## 11. Scope

**v1:** everything above.

**Out of v1:** multi-PC, click to focus the terminal, sending messages from the sprite,
sounds, per-monitor positions in multi-monitor setups.
