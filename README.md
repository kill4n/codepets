# CodePets

**A desktop pet for Claude Code.** Every Claude Code session you have open becomes a small animated sprite floating on your desktop, so you can tell at a glance which agent is working, which one is waiting for you, and which one is done, without switching windows.

![Several CodePets sprites on a desktop: working, needs input with a speech bubble, delegating to a subagent, a fox waiting for a peer, and an idle slime](docs/images/hero.png)

[![Release](https://img.shields.io/github/v/release/kill4n/codepets?label=release)](https://github.com/kill4n/codepets/releases/latest)
[![CI](https://github.com/kill4n/codepets/actions/workflows/ci.yml/badge.svg)](https://github.com/kill4n/codepets/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Why

Running several Claude Code sessions at once means constantly tabbing between terminals to see who needs you. CodePets turns each session into a creature with a mood:

| Sprite says | Meaning |
| --- | --- |
| typing on a laptop | **working**: the agent is running tools |
| sleeping, "Z" floating up | **idle**: waiting for your next prompt |
| jumping with a red "!" | **needs your input**: a permission prompt or a question is waiting |
| little clones around it | **delegating**: it spawned subagents (each one gets its own tiny sprite) |
| staring at a clock | **waiting for a peer**: it sent a message to another agent and is waiting for the reply |
| melting away | **done**: the session ended; the sprite fades out |

![The slime's six animations: working, idle, needs input, delegating, waiting for a peer, done](docs/images/slime-states.gif)

Prefer a fox? Packs are swappable from the menu, and you can draw your own.

![The fox pack's six animations](docs/images/fox-states.gif)

## Features

- **One sprite per session**, discovered automatically; subagents appear as half-size sprites hugging their parent.
- **Instant updates** through Claude Code hooks, with a background poll as a safety net.
- **Answer permissions from the sprite.** When Claude Code asks for a permission, the bubble shows the exact command and offers Allow / Deny / decide in the terminal.
- **Double-click to focus** the terminal of that session (iTerm, Terminal, VS Code, Warp, Cursor, Ghostty).
- **Drag anywhere.** Positions are remembered per session name; click for a speech bubble with the folder, state and elapsed time.
- **Idle life.** Sprites that have been idle for a while take short strolls and come back home.
- **Menu-bar settings** and a **Settings window**: pack, size, session name labels, wandering, permission handling, launch at login.
- **Hide or end a session** from the Settings window: hide a sprite without touching its session, or ask the session's `claude` process to exit (two clicks to confirm).
- Cross-platform (macOS, Windows, Linux) with a transparent, always-on-top, click-through-when-closed window per sprite.

## Install

Download the installer for your platform from the [latest release](https://github.com/kill4n/codepets/releases/latest):

| Platform | File |
| --- | --- |
| macOS (Apple Silicon) | `CodePets_<version>_aarch64.dmg` |
| macOS (Intel) | `CodePets_<version>_x64.dmg` |
| Windows | `CodePets_<version>_x64-setup.exe` or `.msi` |
| Linux | `.AppImage`, `.deb` or `.rpm` |

The macOS builds are not notarized yet: on first launch use Control-click → Open.

Requirements: Claude Code 2.1.224 or newer (`claude agents --json` must exist).

## Quick start

1. Launch CodePets. It lives in the menu bar (tray); sprites appear for every running Claude Code session.
2. Open the menu and choose **Connect to Claude Code**. This adds `http` hooks to `~/.claude/settings.json` (a backup is written first) so state changes reach CodePets instantly. Sessions started after that will report every event.
3. Send a prompt in any session and watch its sprite get to work.

Everything else is in the same menu: pack, size, labels, wandering, how long a permission prompt is held for the bubble, launch at login, and the list of agents (click one to focus its terminal).

If CodePets reports that it cannot find `claude` (common when launched from the Dock, which has a minimal `PATH`), set `claude_path` in the config file (`~/Library/Application Support/dev.kill4n.codepets/config.json` on macOS, `~/.config/codepets/config.json` on Linux) or the `CODEPETS_CLAUDE_BIN` environment variable.

## How it works

```
Claude Code ──http hooks──▶ CodePets server (127.0.0.1:7433, bearer token)
Claude Code ◀─poll every 2.5s── `claude agents --json --all`
                                    │
                            state machine per agent
                                    │
                        one transparent window per sprite
```

- Hooks give immediate signals (prompt submitted, tool calls, permission requests, subagent start/stop, session end). The poll reconciles: it discovers sessions that started before CodePets, notices dead ones, and reads the official `waiting` status.
- A hook received in the last 5 seconds wins over the poll. An agent missing from two polls is marked done and its window closes after 10 seconds.
- Permission prompts: the `PermissionRequest` hook is held open until you answer in the bubble or the configured wait expires, in which case Claude Code shows its own prompt as usual. Inputs too long to display cannot be approved from the bubble.
- Nothing leaves your machine. The server binds to localhost only unless you enable `bind_all`.

Design and engineering records live in [`doc/`](doc/): the [spec](doc/specs/2026-10-07-codepets-design.md), the implementation plan and the final review.

## Sprite packs

A pack is a folder under `assets/packs/<name>/` with a `pack.json` and one or more PNG sprite sheets (one row per animation). Six animations are required: `working`, `idle`, `needs_input`, `delegating`, `waiting_peer`, `gone`.

```json
{
  "name": "Slime",
  "frameSize": [96, 96],
  "scale": 2,
  "animations": {
    "working": { "sheet": "sheet.png", "row": 0, "frames": 8, "fps": 10 },
    "gone":    { "sheet": "sheet.png", "row": 5, "frames": 8, "fps": 10, "loop": false }
  }
}
```

CodePets ships with `default` (Slime) and `fox` (Fox), both generated by scripts in `scripts/`. Drop your own folder next to them and pick it from the menu; the `name` field is what the menu shows.

## Development

```bash
npm install
npm run gen:pack        # regenerate assets/packs/default (Slime) and assets/packs/fox (Fox)
npm run gen:images      # regenerate the README artwork from the packs
cargo tauri dev
```

- Tests: `cargo test --manifest-path src-tauri/Cargo.toml` (unit + integration), `npm test` (Vitest, UI logic) and `npm run build` (type-checks the UI).
- Visual check without Claude Code: `npm run dev` and open `http://localhost:1420/sprite/index.html?mock=working` (or `idle`, `needs_input`, `delegating`, `waiting_peer`, `gone`).
- The dev build loads its UI from the Vite dev server, so "Launch at login" is only offered by the installed app.

## Releases

Pushing a tag `vX.Y.Z` runs the release workflow, which builds macOS (Apple Silicon and Intel), Windows and Linux installers and publishes a GitHub Release. To sign and notarize the macOS builds, add the `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_ID`, `APPLE_PASSWORD` and `APPLE_TEAM_ID` repository secrets; without them the builds are unsigned.

```bash
git tag v0.2.0 && git push origin v0.2.0
```

## Contributing

Issues and pull requests are welcome. `main` is protected: changes land through pull requests with CI green. See [SECURITY.md](SECURITY.md) for reporting vulnerabilities.

## License

MIT. See [LICENSE](LICENSE).
