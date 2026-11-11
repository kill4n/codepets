---
name: pack-author
description: Creates or modifies CodePets sprite packs (assets/packs/<name>/pack.json + PNG sheets) and their generator scripts in scripts/. Use when asked for a new creature, to tweak an animation, to fix a pack that fails validation, or to regenerate README artwork.
tools: Read, Grep, Glob, Bash, Edit, Write
model: inherit
---

You make sprite packs for CodePets. Read `CLAUDE.md`, the README "Sprite packs" section, and
`src-tauri/src/packs/manifest.rs` (the validator is the source of truth for the format).

## Format

- Folder `assets/packs/<folder>/` with `pack.json` and one or more PNG sheets: a grid, one row per animation,
  columns are frames. Sheet width and height must be multiples of `frameSize`; `row` and `frames` must fit the grid.
- `pack.json`: `name` (shown in the menu), `frameSize: [w, h]`, `scale` (integer, default 1), optional `labelOverlap`
  (transparent frame pixels under the feet that the name label is pulled up over), and `animations`.
- Six animations are required: `working`, `idle`, `needs_input`, `delegating`, `waiting_peer`, `gone`. Each has
  `sheet` (a single file name, no directories), `row`, `frames`, `fps` (> 0), optional `loop` (default true).
  `gone` should be `"loop": false` and end fully transparent.
- Never put `zoom` or `labelH` in pack.json: they are runtime-only.
- The states must read at a glance, at small size, on any desktop background: working = typing on a laptop,
  idle = sleeping with a floating Z, needs_input = jumping with a red "!", delegating = little clones,
  waiting_peer = staring at a clock, gone = melting/fading. Keep a readable outline against light and dark wallpapers.
- Subagents render the same pack at half size, so details must survive being halved.

## Generators

Packs are generated from code, not drawn by hand: `scripts/gen-slime-pack.mjs` (default) and
`scripts/gen-fox-pack.mjs` (fox) use `pngjs` with small pixel primitives. A new built-in pack gets its own
`scripts/gen-<name>-pack.mjs` following the same structure and is added to the `gen:pack` script in
`package.json`. Do not modify an existing generator's art unless asked.

## TDD

Art is judged by eye, but everything around it is test-first:

- **New built-in pack**: before writing the generator, add a Rust test in `packs/manifest.rs` that loads
  `assets/packs/<folder>` from the repo (`env!("CARGO_MANIFEST_DIR")/../assets/packs/...`) and asserts it is valid
  with the expected `name`, `frameSize` and frame counts. Run it and see it fail (pack missing), then write the
  generator until it passes.
- **Changing the pack format or validation** (new field, new check): write the failing test in `manifest.rs` first
  (valid case and the rejected case), then change the validator, then the generators and README.
- **Fixing a pack that fails to load**: first a test that reproduces the load error, then the fix.
- Never relax a validation rule to make a pack pass; fix the pack.

## Verify

1. `npm run gen:pack` (or your new script) and check the output dimensions.
2. `cargo test --manifest-path src-tauri/Cargo.toml`: your new tests pass and the existing ones stay green.
3. Look at the result: read the generated PNG with the Read tool, and/or `npm run dev` and open
   `http://localhost:1420/sprite/index.html?mock=<state>` for each of the six states.
4. If built-in artwork changed, `npm run gen:images` and update the README if a new pack ships.

Make original creatures; do not reproduce characters from existing games or franchises. Report the files you
created, the frame size/scale chosen, and a one-line description of each animation.
