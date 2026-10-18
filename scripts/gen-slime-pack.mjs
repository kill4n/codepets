// Generates assets/packs/default/{sheet.png,sheet.json}: an anime cel-shaded slime (Rimuru-style).
// 96x96 frames, 8 columns x 6 rows (one row per state). Flat colour zones with hard edges,
// a big hard-edged white highlight, thin dark outline, no face. Body tint per state.
import { PNG } from "pngjs";
import { writeFileSync, mkdirSync } from "node:fs";

const F = 96, COLS = 8, ROWS = 6;
const png = new PNG({ width: F * COLS, height: F * ROWS });

// ---------- primitives ----------
const px = (x, y, c) => {
  x = Math.round(x); y = Math.round(y);
  if (x < 0 || y < 0 || x >= png.width || y >= png.height) return;
  const a = c[3] ?? 255;
  if (a <= 0) return;
  const i = (png.width * y + x) << 2;
  if (a >= 255) { png.data[i] = c[0]; png.data[i + 1] = c[1]; png.data[i + 2] = c[2]; png.data[i + 3] = 255; return; }
  const da = png.data[i + 3] / 255, sa = a / 255, oa = sa + da * (1 - sa);
  for (let k = 0; k < 3; k++) png.data[i + k] = Math.round((c[k] * sa + png.data[i + k] * da * (1 - sa)) / (oa || 1));
  png.data[i + 3] = Math.round(oa * 255);
};
const rect = (x, y, w, h, c) => { for (let j = 0; j < h; j++) for (let i = 0; i < w; i++) px(x + i, y + j, c); };
const hline = (x, y, w, c) => rect(x, y, w, 1, c);
const circle = (cx, cy, r, c) => { for (let y = -r; y <= r; y++) for (let x = -r; x <= r; x++) if (x * x + y * y <= r * r) px(cx + x, cy + y, c); };
const ellipse = (cx, cy, rx, ry, c) => { for (let y = -ry; y <= ry; y++) for (let x = -rx; x <= rx; x++) if ((x * x) / (rx * rx) + (y * y) / (ry * ry) <= 1) px(cx + x, cy + y, c); };
const withAlpha = (c, a) => [c[0], c[1], c[2], a];
const mix = (a, b, t) => [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t, 255];
const hex = (h) => [parseInt(h.slice(1, 3), 16), parseInt(h.slice(3, 5), 16), parseInt(h.slice(5, 7), 16), 255];

// Slime silhouette: near-sphere with a gently flattened base. (dx,dy) relative to centre.
function inBlob(dx, dy, rx, ry, lean = 0) {
  const x = dx - lean * (dy / ry);
  const nx = x / rx, ny = dy / ry;
  if (ny <= 0) return nx * nx + ny * ny <= 1;
  const n = 2.4; // squarer bottom = sits on the floor
  return Math.pow(Math.abs(nx), n) + Math.pow(Math.abs(ny) * 1.08, n) <= 1;
}

// ---------- palette (anime cel zones) ----------
// base / shadow band / deep / inner halo, for the pure Rimuru look (idle)
const RIMURU = { base: hex("#A6E3FA"), shadow: hex("#6FC2EA"), deep: hex("#4FA3D6"), halo: hex("#D6F4FF"), line: hex("#2B4C77") };
const WHITE = [255, 255, 255, 255], RED = [232, 64, 64, 255], YELLOW = [255, 236, 140, 255], SCREEN = [140, 215, 255, 255], INK = hex("#2B4C77");
// per-state tints applied to every zone (0 = pure Rimuru)
const TINT = {
  working: { c: hex("#6A9BFF"), k: 0.5 }, idle: { c: RIMURU.base, k: 0 }, needs_input: { c: hex("#FFC95C"), k: 0.85 },
  delegating: { c: hex("#5FE0A8"), k: 0.45 }, waiting_peer: { c: hex("#B48CFF"), k: 0.5 }, gone: { c: hex("#9AA3AE"), k: 0.7 },
};
const zones = (state) => {
  const { c, k } = TINT[state];
  const t = (z) => mix(z, c, k);
  return { base: t(RIMURU.base), shadow: t(RIMURU.shadow), deep: t(RIMURU.deep), halo: mix(t(RIMURU.halo), WHITE, 0.3), line: mix(RIMURU.line, c, k * 0.4) };
};
const order = ["working", "idle", "needs_input", "delegating", "waiting_peer", "gone"];

// ---------- slime ----------
// base of the blob sits at y = 72 in frame space, centred at x = 48.
function slime(o, state, { sx = 1, sy = 1, lean = 0, alpha = 255, dy = 0 } = {}) {
  const Z = zones(state);
  const rx = Math.round(28 * sx), ry = Math.round(26 * sy);
  const cx = o.x + 48, cy = o.y + 72 - ry + dy;
  const A = (c) => withAlpha(c, alpha);

  // ground shadow (flat, hard-edged like anime)
  ellipse(cx, o.y + 74, rx + 3, 4, [0, 0, 0, Math.round(alpha * 0.18)]);

  // body zones: painted per pixel with hard thresholds
  for (let y = -ry; y <= ry; y++) for (let x = -rx; x <= rx; x++) {
    if (!inBlob(x, y, rx, ry, lean)) continue;
    const nx = x / rx, ny = y / ry;
    // distance from an off-centre "light" point (top-left) decides the zone
    const lx = nx + 0.35, ly = ny + 0.45;
    const d = Math.sqrt(lx * lx + ly * ly);
    let c = Z.base;
    if (d > 1.32) c = Z.deep;
    else if (d > 1.02) c = Z.shadow;
    // inner halo: a light ring just inside the outline, suggests translucency (only on the lit side)
    const edge = !inBlob(x * 1.09, y * 1.09, rx, ry, lean);
    if (edge && ny < 0.25 && d < 1.02) c = Z.halo;
    px(cx + x, cy + y, A(c));
  }
  // shadow-band "bounce" light: thin lighter line along the bottom edge (anime rim)
  for (let x = -rx; x <= rx; x++) {
    const nx = x / rx;
    if (Math.abs(nx) > 0.85) continue;
    const yEdge = (() => { for (let y = ry; y >= 0; y--) if (inBlob(x, y, rx, ry, lean)) return y; return ry; })();
    px(cx + x, cy + yEdge - 1, A(mix(Z.shadow, WHITE, 0.35)));
  }
  // outline 2px (1px solid + 1px inner soft)
  for (let y = -ry - 2; y <= ry + 2; y++) for (let x = -rx - 2; x <= rx + 2; x++) {
    if (inBlob(x, y, rx, ry, lean)) continue;
    const n1 = inBlob(x + 1, y, rx, ry, lean) || inBlob(x - 1, y, rx, ry, lean) || inBlob(x, y + 1, rx, ry, lean) || inBlob(x, y - 1, rx, ry, lean);
    if (n1) px(cx + x, cy + y, A(Z.line));
  }
  // main highlight: big hard-edged white oval top-left with a "bite" (anime style)
  const hx = cx - Math.round(rx * 0.40) + Math.round(lean * 0.6), hy = cy - Math.round(ry * 0.50);
  const hrx = Math.round(rx * 0.34), hry = Math.round(ry * 0.22);
  for (let y = -hry; y <= hry; y++) for (let x = -hrx; x <= hrx; x++) {
    if ((x * x) / (hrx * hrx) + (y * y) / (hry * hry) > 1) continue;
    // bite: subtract a circle at the lower-right of the highlight
    const bx = x - hrx * 0.55, by = y - hry * 0.9;
    if ((bx * bx) / (hrx * hrx * 0.35) + (by * by) / (hry * hry * 1.2) <= 1) continue;
    px(hx + x, hy + y, A(WHITE));
  }
  // secondary small highlight (right) and a tiny one below the main
  ellipse(cx + Math.round(rx * 0.48), cy - Math.round(ry * 0.18), 3, 2, A(WHITE));
  circle(hx + Math.round(hrx * 0.9), hy + Math.round(hry * 1.9), 1, A(WHITE));
  return { cx, cy, rx, ry };
}

// ---------- props ----------
function laptop(o, keyGlow) {
  const x = o.x + 30, y = o.y + 78;
  rect(x, y, 36, 6, INK); rect(x + 1, y + 1, 34, 4, hex("#5B6B85"));
  rect(x + 4, y - 15, 28, 16, INK); rect(x + 5, y - 14, 26, 14, SCREEN);
  for (let i = 0; i < 3; i++) hline(x + 8, y - 11 + i * 4, 8 + ((i + keyGlow) % 3) * 5, mix(SCREEN, INK, 0.45));
}
function exclaim(o, dy, alpha = 255) {
  const x = o.x + 45, y = o.y + 6 + dy;
  rect(x - 1, y - 1, 8, 18, withAlpha(INK, alpha)); rect(x, y, 6, 15, withAlpha(RED, alpha));
  rect(x - 1, y + 18, 8, 6, withAlpha(INK, alpha)); rect(x, y + 19, 6, 4, withAlpha(RED, alpha));
}
function sweat(x, y, alpha = 255) {
  const c = withAlpha(hex("#7FD4FF"), alpha), l = withAlpha(INK, alpha);
  circle(x, y + 3, 3, l); circle(x, y + 3, 2, c); px(x, y, l); px(x, y + 1, c); px(x - 1, y + 2, c); px(x + 1, y + 2, c); px(x - 1, y + 3, WHITE);
}
function zee(x, y, alpha, w) {
  const a = withAlpha(INK, alpha);
  rect(x, y, w, 2, a); for (let i = 1; i < w - 1; i++) { px(x + w - 1 - i, y + i, a); px(x + w - 2 - i, y + i, a); } rect(x, y + w - 1, w, 2, a);
}
function clock(x, y, frame) {
  circle(x, y, 11, INK); circle(x, y, 10, WHITE);
  for (const [dx, dy] of [[0, -7], [0, 7], [-7, 0], [7, 0]]) circle(x + dx, y + dy, 1, INK);
  const a = (frame / 8) * Math.PI * 2;
  for (let t = 1; t <= 7; t++) rect(x + Math.round(Math.sin(a) * t), y - Math.round(Math.cos(a) * t), 1, 1, INK);
  hline(x - 4, y, 4, INK);
}
function dots(x, y, n) { for (let i = 0; i < n; i++) circle(x + i * 6, y, 2, INK); }
function sparkle(x, y, c = YELLOW) { rect(x - 1, y - 4, 3, 9, c); rect(x - 4, y - 1, 9, 3, c); px(x - 3, y - 3, c); px(x + 3, y - 3, c); px(x - 3, y + 3, c); px(x + 3, y + 3, c); }
function miniSlime(o, x, y, state, bob) {
  const Z = zones(state), rx = 10, ry = 9, cx = x, cy = y - bob;
  for (let yy = -ry; yy <= ry; yy++) for (let xx = -rx; xx <= rx; xx++) {
    if (!inBlob(xx, yy, rx, ry)) continue;
    const nx = xx / rx, ny = yy / ry, lx = nx + 0.35, ly = ny + 0.45, d = Math.sqrt(lx * lx + ly * ly);
    px(cx + xx, cy + yy, d > 1.1 ? Z.shadow : Z.base);
  }
  for (let yy = -ry - 1; yy <= ry + 1; yy++) for (let xx = -rx - 1; xx <= rx + 1; xx++) {
    if (inBlob(xx, yy, rx, ry)) continue;
    if (inBlob(xx + 1, yy, rx, ry) || inBlob(xx - 1, yy, rx, ry) || inBlob(xx, yy + 1, rx, ry) || inBlob(xx, yy - 1, rx, ry)) px(cx + xx, cy + yy, Z.line);
  }
  ellipse(cx - 4, cy - 4, 3, 2, WHITE);
}

// ---------- animations ----------
const anims = {
  working(o, f) {
    const wob = [0, 0.02, 0.04, 0.02, 0, -0.02, -0.04, -0.02][f];
    slime(o, "working", { sx: 1 + wob, sy: 1 - wob });
    laptop(o, f);
    if (f % 4 === 0) sparkle(o.x + 78, o.y + 50); if (f % 4 === 2) sparkle(o.x + 18, o.y + 44);
  },
  idle(o, f) {
    const t = Math.sin((f / 6) * Math.PI * 2);
    slime(o, "idle", { sx: 1 + t * 0.03, sy: 1 - t * 0.04 });
    const z = f / 6;
    zee(o.x + 70, o.y + 20 - Math.round(z * 12), 255 - Math.round(z * 160), 9);
    if (f >= 2) zee(o.x + 78, o.y + 32 - (f - 2) * 3, 210, 6);
  },
  needs_input(o, f) {
    const stretch = [0, 0.16, 0.28, 0.28, 0.16, 0][f % 6];
    slime(o, "needs_input", { sx: 1 - stretch * 0.4, sy: 1 + stretch });
    exclaim(o, -Math.round(stretch * 24) + (f % 2));
    sweat(o.x + 76, o.y + 30 + (f % 3), 255);
  },
  delegating(o, f) {
    const spread = [0, 2, 5, 8, 10, 10, 8, 4][f];
    slime(o, "delegating", { sx: 1 - spread * 0.012, sy: 1 + spread * 0.008, lean: f < 4 ? -3 : 3 });
    const bob = [0, 1, 3, 2, 0, 1, 3, 2][f];
    miniSlime(o, o.x + 14 - Math.round(spread * 0.4), o.y + 80, "delegating", bob);
    miniSlime(o, o.x + 82 + Math.round(spread * 0.4), o.y + 80, "delegating", 3 - bob);
  },
  waiting_peer(o, f) {
    const lean = [-5, -5, -2, 0, 5, 5, 2, 0][f];
    slime(o, "waiting_peer", { lean });
    clock(o.x + 80, o.y + 16, f);
    dots(o.x + 8, o.y + 16, (f % 3) + 1);
  },
  gone(o, f) {
    const t = f / 7, alpha = Math.round(255 * (1 - t * 0.9));
    slime(o, "gone", { sx: 1 + t * 0.5, sy: 1 - t * 0.85, alpha });
    for (let i = 0; i < 3; i++) {
      const pa = Math.round(220 * (1 - t));
      circle(o.x + 26 + i * 22, o.y + 48 - f * 5 - i * 3, 2, withAlpha(hex("#D6F4FF"), pa));
    }
  },
};
const frames = { working: 8, idle: 6, needs_input: 6, delegating: 8, waiting_peer: 8, gone: 8 };
const fps = { working: 10, idle: 3, needs_input: 8, delegating: 8, waiting_peer: 4, gone: 10 };

order.forEach((state, row) => {
  for (let col = 0; col < frames[state]; col++) anims[state]({ x: col * F, y: row * F }, col);
});

mkdirSync("assets/packs/default", { recursive: true });
writeFileSync("assets/packs/default/sheet.png", PNG.sync.write(png));
const manifest = { name: "Slime", frameSize: [F, F], scale: 2, labelOverlap: 8, animations: {} };
order.forEach((state, row) => {
  manifest.animations[state] = { sheet: "sheet.png", row, frames: frames[state], fps: fps[state], ...(state === "gone" ? { loop: false } : {}) };
});
writeFileSync("assets/packs/default/pack.json", JSON.stringify(manifest, null, 2) + "\n");
console.log(`slime (default) pack written (${png.width}x${png.height})`);
