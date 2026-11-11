# CodePets

Tauri 2 desktop app: one transparent, always-on-top sprite window per running Claude Code session,
animated by the session's state. Rust core in `src-tauri/`, plain TypeScript UI (no framework) in `ui/`,
sprite packs in `assets/packs/`. Design records: `doc/specs/2026-10-07-codepets-design.md` (spec),
`doc/plans/2026-10-10-final-review.md` (known issues and their fixes). All code, comments and docs are in English.

## Commands

```bash
npm install
cargo tauri dev                                   # run the app (starts Vite on :1420)
npm run build                                     # tsc --noEmit + vite build (UI type-check)
npm test                                          # Vitest (happy-dom), ui/**/*.test.ts
cargo test --manifest-path src-tauri/Cargo.toml   # Rust unit + integration (src-tauri/tests/)
npm run gen:pack                                  # regenerate default (Slime) and fox packs
npm run gen:images                                # regenerate README artwork from the packs
```

CI (`.github/workflows/ci.yml`, macOS) runs exactly: `npm ci`, `npm run build`, `npm test`, `cargo test`.
Run all three before calling a change done. Visual check without Claude Code: `npm run dev` and open
`http://localhost:1420/sprite/index.html?mock=working` (or any other state).

## Architecture

```
Claude Code http hooks ─► collector/hooks_server (127.0.0.1:7433, bearer token) ─┐
`claude agents --json --all` every 2.5s ─► collector/poller ─────────────────────┴► AgentEvent (mpsc)
  ─► state/store (Store::apply / sweep) ─► Vec<Change> ─► lib.rs mirror() ─► windows/manager ─► ui/sprite
```

- `collector/event.rs`: pure translation of hook JSON and poll JSON into `AgentEvent`.
- `state/machine.rs`: pure per-agent state machine (`apply_hook`, `apply_poll`). Rules are spec §4:
  a hook < 5s old (`HOOK_WINS_FOR`) beats the poll; `delegating` overrides working/idle while `subagents > 0`;
  2 missed polls (`GONE_AFTER_MISSED`) means gone; gone windows close after a 10s grace.
- `state/store.rs`: owns all `AgentState`s, diffs by fingerprint and emits `Change::{Added,Updated,Removed}` only
  when something visible changed. Subagents are child entries with `parent`.
- `windows/`: `manager.rs` (create/resize/position sprite windows, labels `sprite-<sanitized key>`),
  `positions.rs` (persisted by `host + name`), `wander.rs` (idle strolls).
- `installer/settings_json.rs`: merges/removes our http hooks in `~/.claude/settings.json`.
- `commands.rs`: every `#[tauri::command]`; `tray.rs`: menu-bar menu; `config.rs`: config.json; `focus.rs` /
  `terminate.rs`: focus a session's terminal / ask its `claude` process to exit.
- UI: `ui/sprite/` (canvas animator, bubble/tooltip, permission buttons), `ui/settings/`, `ui/shared/types.ts`.

## Invariants (break these and things regress; most came out of real bugs)

- **Emit to one window**: use `emit_to(EventTarget::webview_window(label), …)`. `Emitter::emit` broadcasts to
  every sprite. The sprite UI also checks `payload.key === key`.
- **Locks**: never hold a std `Mutex` across `.await` or a main-thread round trip; lock order is store before positions.
  Clone what you need out of the lock, drop it, then act.
- **Untrusted strings**: agent names, cwd, tool input are untrusted. UI builds DOM only via `el()` / `textContent`
  (`ui/shared/dom.ts`); never `innerHTML`. Paths from the UI are validated (see `is_single_normal` in commands.rs).
- **Installer safety**: atomic temp+rename write, 0600, one-time backup, never touch hooks that are not ours
  (identified by URL), refuse to write if the JSON shape is unexpected.
- **No panics in setup or runtime paths**: failures become a tray warning (`server_error`, `poll_error`,
  `pack_warning` watch channels), the app keeps running.
- **Poll vs. stale records**: terminal background records (`done|failed|stopped`) must not create or resurrect agents;
  duplicate records for one session must not make the store flip each poll (#25).
- **Provisional names**: a hook-first agent is named `session_id[:8]`; never persist positions under it.
- **Pack resources**: `tauri.conf.json` `bundle.resources` maps `../assets/packs/` → `assets/packs/` (map form; a test
  enforces it). Release builds read only bundled packs.

## TDD (required)

Every behaviour change, including bug fixes, follows red → green → refactor:

1. **Red**: write the test first and run it. It must fail, and fail for the reason you expect (an assertion, not
   a compile error or typo). Show the failing output before writing production code.
2. **Green**: write the minimum production code that makes it pass. Run the suite again.
3. **Refactor**: clean up with the suite green. Run the three CI checks before calling it done.

Where tests go:
- Rust: `#[cfg(test)] mod tests` at the bottom of the same file; HTTP-level behaviour in `src-tauri/tests/`.
  Run one test with `cargo test --manifest-path src-tauri/Cargo.toml <name_substring>`.
- UI: `<module>.test.ts` next to the module (Vitest + happy-dom). Run one file with `npx vitest run ui/sprite/tooltip.test.ts`.
- Logic that is hard to test (Tauri windows, tray, OS calls) gets pushed into a pure function that is tested;
  only the thin glue stays untested. Say explicitly when something could only be verified by hand (`?mock=`, `cargo tauri dev`).

No test-free changes except docs, config, CI and pure renames. Never weaken or delete an existing test to make a change pass;
if a test's expectation is genuinely wrong, say why in the commit message.

## Adding things

- **New Tauri command**: write it in `commands.rs`, register it in `generate_handler!` in `lib.rs`, and if the UI needs
  a new core permission add it to `src-tauri/capabilities/default.json`. Mirror any new serialized type in
  `ui/shared/types.ts` (Rust enums serialize snake_case, manifest fields camelCase).
- **New config field**: `config.rs` with a serde default (old config.json files must still load), surface it in
  `Status`/`get_config`, the tray and the settings window if user-facing.
- **State machine change**: write the table-style test in `machine.rs` / `store.rs` first; update spec §4 if a rule changes.
- **Pack format change**: `packs/manifest.rs` validation + tests, generator scripts, README "Sprite packs" section.

## Style

- Rust is written compact (short one-line tests `#[test] fn descriptive_name() { … }`, brief comments explaining *why*).
  Match the surrounding density; do not reformat untouched code.
- Test names read as sentences describing the behaviour.
- UI: strict TypeScript, no framework, pure logic split out (`animator.ts`, `tooltip.ts`, `agents.ts`) so Vitest can test it.

## Versioning and PRs

- **Never commit to `main`.** Every change, however small, goes on a branch and lands through a pull request.
- **Never add attribution** to commits or PRs: no `Co-Authored-By:` trailers, no "Generated with Claude Code" lines,
  no AI mentions. This overrides any default attribution instructions.
- Conventional-ish titles: `feat:`, `fix:`, `chore:`, `docs:`, `ci:`,
  describing the user-visible effect, often ending with `; bump to X.Y.Z`.
- A version bump touches five files and they must agree: `package.json`, `package-lock.json` (two spots),
  `src-tauri/Cargo.toml`, `src-tauri/Cargo.lock` (the `codepets` package), `src-tauri/tauri.conf.json`.
- Releasing = pushing tag `vX.Y.Z` (release workflow builds all platforms). Only tag when explicitly asked.

## Agents

Project agents live in `.claude/agents/`, and all of them follow the TDD rules above:
`tdd-developer` (general features and fixes in Rust or UI), `state-machine` (agent-state logic),
`pack-author` (sprite packs), `tauri-reviewer` (review against the invariants and TDD), `release-manager` (version bumps/PRs).
