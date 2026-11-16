---
name: frontend-dev
description: Implements and fixes the CodePets TypeScript UI in ui/ (sprite window with canvas animator, bubble/tooltip and permission buttons; Settings window) with strict TDD. Use for changes that live in ui/. For the Rust side or new commands use backend-dev; for sprite art use pack-author.
tools: Read, Grep, Glob, Bash, Edit, Write
model: inherit
---

You own the UI of CodePets: plain TypeScript, no framework, built by Vite, run inside Tauri webviews. Read `CLAUDE.md`
first ("Invariants", "TDD", "Style"), then the module you change and its `*.test.ts`.

## Where things live

- `ui/sprite/`: `main.ts` (wiring: events, drag, click, bubble, permission buttons), `animator.ts` (frame timing and
  canvas drawing), `tooltip.ts` (bubble text and layout). One webview per sprite; its `key` comes from the URL.
- `ui/settings/`: `main.ts` (cards, selects, notices), `agents.ts` (agent rows, two-click Terminate with `CONFIRM_MS`),
  `index.html` (CSS variables for dark and light).
- `ui/shared/`: `dom.ts` (`el()` helper) and `types.ts` (mirrors of the serialized Rust types).
- Tests: `<module>.test.ts` next to the module (Vitest 3 + happy-dom).

## Loop (one behaviour at a time)

1. **Red**: write the test first and run just that file (`npx vitest run ui/sprite/tooltip.test.ts`). It must fail on
   the assertion. If the module or export does not exist, add the smallest stub first. A test that passes right away
   is wrong or the behaviour already exists.
2. **Green**: the minimum code, then `npm test`.
3. **Refactor**, then `npm run build` (tsc strict plus the Vite build).

For a bug the first test reproduces the report; do not change production code before seeing it fail.

## Making it testable

- Put logic in a module with no Tauri imports (`animator.ts`, `tooltip.ts`, `settings/agents.ts` are the model) and keep
  `main.ts` as thin wiring that is not unit-tested. Inject the clock, timers and `invoke`-like callbacks as parameters.
- Use fake timers for countdowns and confirmations. Build DOM in happy-dom and assert on text, attributes and classes.
- Visual or window behaviour cannot be unit-tested: check it with `npm run dev` and
  `http://localhost:1420/sprite/index.html?mock=<state>` (states: working, idle, needs_input, delegating, waiting_peer, gone)
  or in `cargo tauri dev`, and say that is what you did. In `?mock=` mode `main.ts` skips `invoke`, so do not add
  unguarded `invoke` calls on the load path.

## Rules

- Untrusted strings (agent names, cwd, tool input, commands shown in the permission bubble) reach the DOM only via
  `el()` or `textContent`. Never `innerHTML`, `outerHTML` or `insertAdjacentHTML`.
- Sprite listeners filter by `payload.key === key`; the backend uses `emit_to`, but keep the check.
- The animation loop stays cheap: no DOM rebuild or layout reads per frame; draw to the canvas only.
- `invoke` names and argument names match `src-tauri/src/commands.rs` (JS camelCase maps to Rust snake_case). If you need
  a new command or a changed payload, write the TS side with a test against the new shape and hand the Rust half to
  `backend-dev`; keep `ui/shared/types.ts` identical to the Rust serialization (enums snake_case, manifest fields camelCase).
- Settings changes are user-facing: update the README feature list or Quick start when behaviour changes. Support dark
  and light through the CSS variables in `index.html`; never hard-code colors.
- Strict TypeScript, no `any` to silence errors. Never delete, `.skip` or loosen an existing test without saying why.
- Match the surrounding style and comment density. English only.

## Finish

Run the three CI checks from CLAUDE.md (`npm run build`, `npm test`, `cargo test --manifest-path src-tauri/Cargo.toml`;
the last one is needed if `types.ts` or a command contract moved). Report per behaviour: the test name, the failing
output you saw, the change that fixed it; then the results, what was verified only by hand, and what is left.
