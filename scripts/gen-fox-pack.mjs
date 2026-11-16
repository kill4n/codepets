// Generates assets/packs/fox/{sheet.png,pack.json}: a pixel-art fox mascot.
// 64x64 frames, 8 columns x 8 rows (one row per state). State is shown by a coloured scarf + props/pose.
import { PNG } from "pngjs";
import { writeFileSync, mkdirSync } from "node:fs";

const F = 64, COLS = 8, ROWS = 8;
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
const inEllipse = (dx, dy, rx, ry) => (dx * dx) / (rx * rx) + (dy * dy) / (ry * ry) <= 1;
const ellipse = (cx, cy, rx, ry, c) => {
  for (let y = -ry; y <= ry; y++) for (let x = -rx; x <= rx; x++) if (inEllipse(x, y, rx, ry)) px(cx + x, cy + y, c);
};
const ellipseOutline = (cx, cy, rx, ry, c) => {
  for (let y = -ry - 1; y <= ry + 1; y++) for (let x = -rx - 1; x <= rx + 1; x++) {
    if (inEllipse(x, y, rx, ry)) continue;
    if (inEllipse(x + 1, y, rx, ry) || inEllipse(x - 1, y, rx, ry) || inEllipse(x, y + 1, rx, ry) || inEllipse(x, y - 1, rx, ry)) px(cx + x, cy + y, c);
  }
};
const circle = (cx, cy, r, c) => ellipse(cx, cy, r, r, c);
// filled triangle (scanline), points are [x,y]
const tri = (p1, p2, p3, c) => {
  const minY = Math.min(p1[1], p2[1], p3[1]), maxY = Math.max(p1[1], p2[1], p3[1]);
  const edge = (a, b, y) => (y < Math.min(a[1], b[1]) || y > Math.max(a[1], b[1]) || a[1] === b[1]) ? null : a[0] + ((y - a[1]) * (b[0] - a[0])) / (b[1] - a[1]);
  for (let y = Math.ceil(minY); y <= Math.floor(maxY); y++) {
    const xs = [edge(p1, p2, y), edge(p2, p3, y), edge(p3, p1, y)].filter((v) => v !== null);
    if (xs.length < 2) continue;
    const a = Math.min(...xs), b = Math.max(...xs);
    for (let x = Math.ceil(a); x <= Math.floor(b); x++) px(x, y, c);
  }
};
const triOutline = (p1, p2, p3, c) => { line(p1, p2, c); line(p2, p3, c); line(p3, p1, c); };
const line = (a, b, c) => {
  const n = Math.max(Math.abs(b[0] - a[0]), Math.abs(b[1] - a[1]), 1);
  for (let t = 0; t <= n; t++) px(a[0] + ((b[0] - a[0]) * t) / n, a[1] + ((b[1] - a[1]) * t) / n, c);
};
const withAlpha = (c, a) => [c[0], c[1], c[2], a];
const shade = (c, k) => [Math.max(0, Math.min(255, c[0] * k)), Math.max(0, Math.min(255, c[1] * k)), Math.max(0, Math.min(255, c[2] * k)), c[3] ?? 255];

// ---------- palette ----------
const INK = [40, 28, 24], WHITE = [252, 248, 240], CREAM = [255, 236, 210], FUR = [236, 128, 48], FUR_DARK = [196, 92, 32], PINK = [255, 170, 160];
const RED = [232, 64, 64], YELLOW = [255, 220, 90], SCREEN = [120, 200, 255];
const SCARF = {
  working: [70, 140, 255], idle: [150, 152, 170], needs_input: [255, 120, 20],
  delegating: [70, 190, 110], waiting_peer: [170, 110, 255], gone: [100, 100, 110],
  error: [220, 50, 50], compacting: [255, 120, 200],
};
const order = ["working", "idle", "needs_input", "delegating", "waiting_peer", "gone", "error", "compacting"];

// ---------- fox ----------
// Frame-space anchors: head centre (32,27), body centre (32,46). All drawing takes `a` (alpha) and `s` (scale, for gone).
function fox(o, state, opt = {}) {
  const { dy = 0, earDroop = 0, eyesOpen = true, look = 0, wide = false, mouth = "smile", armL = 20, armR = 20,
          tailWag = 0, alpha = 255, scale = 1, curled = false, blush = false } = opt;
  const A = (c) => withAlpha(c, alpha);
  const S = (v) => v * scale;
  const cx = o.x + 32, hy = o.y + 26 + dy + (1 - scale) * 14, by = o.y + 46 + dy + (1 - scale) * 6;
  const ink = A(INK), fur = A(FUR), furD = A(FUR_DARK), white = A(WHITE), cream = A(CREAM);

  // ground shadow
  ellipse(cx, o.y + 58, Math.round(S(14)), 3, [0, 0, 0, Math.round(alpha * 0.3)]);

  // tail (behind body, right side), bushy with white tip
  const tx = cx + Math.round(S(12)), ty = by + Math.round(S(5)) - tailWag;
  ellipseOutline(tx, ty, Math.round(S(8)), Math.round(S(4)), ink);
  ellipse(tx, ty, Math.round(S(8)), Math.round(S(4)), fur);
  ellipse(tx + Math.round(S(5)), ty - Math.round(S(1)) - tailWag, Math.round(S(3)), Math.round(S(3)), white);
  ellipse(tx - Math.round(S(3)), ty + Math.round(S(2)), Math.round(S(4)), Math.round(S(2)), furD);

  // body
  const brx = Math.round(S(9)), bry = Math.round(S(curled ? 10 : 12));
  ellipseOutline(cx, by, brx, bry, ink);
  ellipse(cx, by, brx, bry, fur);
  for (let y = Math.round(bry * 0.5); y <= bry; y++) for (let x = -brx; x <= brx; x++) if (inEllipse(x, y, brx, bry)) px(cx + x, by + y, furD);
  ellipse(cx, by + Math.round(S(3)), Math.round(S(4)), Math.round(S(6)), cream); // belly

  // feet
  ellipse(cx - Math.round(S(5)), by + bry - 1, Math.round(S(3)), Math.round(S(2)), ink);
  ellipse(cx + Math.round(S(5)), by + bry - 1, Math.round(S(3)), Math.round(S(2)), ink);
  ellipse(cx - Math.round(S(5)), by + bry - 2, Math.round(S(2)), Math.round(S(1.5)), furD);
  ellipse(cx + Math.round(S(5)), by + bry - 2, Math.round(S(2)), Math.round(S(1.5)), furD);

  // arms (front paws): angle 0 = down, 180 = up
  const arm = (side, angle) => {
    const sx = cx + side * (brx - 1), sy = by - Math.round(S(2));
    const rad = (angle * Math.PI) / 180, dx = side * Math.sin(rad), dyy = Math.cos(rad), len = S(7);
    for (let t = 0; t <= len; t++) { px(sx + dx * t, sy + dyy * t, ink); px(sx + dx * t + side, sy + dyy * t, ink); px(sx + dx * t + side * 0.5, sy + dyy * t - 1, fur); }
    circle(sx + dx * len, sy + dyy * len, Math.max(1, Math.round(S(2))), ink);
    circle(sx + dx * len, sy + dyy * len, Math.max(0, Math.round(S(1))), furD);
  };
  arm(-1, armL); arm(1, armR);

  // scarf (state colour) around the neck + a dangling end
  const sc = A(SCARF[state]);
  const ny = by - bry + Math.round(S(1));
  rect(cx - Math.round(S(7)), ny - 1, Math.round(S(14)), Math.round(S(4)) + 2, ink);
  rect(cx - Math.round(S(6)), ny, Math.round(S(12)), Math.round(S(4)), sc);
  hline(cx - Math.round(S(6)), ny + Math.round(S(4)) - 1, Math.round(S(12)), A(shade(SCARF[state], 0.7)));
  rect(cx - Math.round(S(8)), ny + 2, Math.round(S(4)) + 1, Math.round(S(8)), ink);
  rect(cx - Math.round(S(7)), ny + 3, Math.round(S(3)), Math.round(S(6)), sc);

  // ears (triangles) with pink inner, droop lowers the tip
  const hrx = Math.round(S(11)), hry = Math.round(S(9));
  const earBaseY = hy - Math.round(S(4));
  for (const side of [-1, 1]) {
    const bx = cx + side * Math.round(S(7));
    const tip = [cx + side * Math.round(S(11)), earBaseY - Math.round(S(12)) + earDroop];
    const b1 = [bx - side * Math.round(S(3)), earBaseY], b2 = [bx + side * Math.round(S(5)), earBaseY + 1];
    tri(b1, b2, tip, ink);
    const inner = [tip[0] - side * 1, tip[1] + 2];
    tri([b1[0] + side, b1[1]], [b2[0] - side, b2[1]], inner, fur);
    tri([b1[0] + side * 2, b1[1]], [b2[0] - side * 2, b2[1]], [inner[0], inner[1] + 3], A(PINK));
  }

  // head
  ellipseOutline(cx, hy, hrx, hry, ink);
  ellipse(cx, hy, hrx, hry, fur);
  // cheeks + muzzle (white), pointing down like a fox mask
  ellipse(cx - Math.round(S(6)), hy + Math.round(S(3)), Math.round(S(6)), Math.round(S(5)), white);
  ellipse(cx + Math.round(S(6)), hy + Math.round(S(3)), Math.round(S(6)), Math.round(S(5)), white);
  tri([cx - Math.round(S(5)), hy + Math.round(S(2))], [cx + Math.round(S(5)), hy + Math.round(S(2))], [cx, hy + hry], white);
  if (blush) { px(cx - Math.round(S(9)), hy + 2, A(PINK)); px(cx + Math.round(S(9)), hy + 2, A(PINK)); }

  // eyes
  const ey = hy - Math.round(S(1)), lx = cx - Math.round(S(5)), rxp = cx + Math.round(S(5));
  if (!eyesOpen) { hline(lx - 2, ey, 5, ink); hline(rxp - 2, ey, 5, ink); px(lx - 2, ey - 1, ink); px(rxp + 2, ey - 1, ink); }
  else {
    const r = wide ? 3 : 2;
    circle(lx, ey, r, ink); circle(rxp, ey, r, ink);
    if (wide) { circle(lx, ey, 2, white); circle(rxp, ey, 2, white); }
    px(lx + look, ey + (wide ? 0 : -1), wide ? ink : white); px(rxp + look, ey + (wide ? 0 : -1), wide ? ink : white);
    if (wide) { px(lx + look + 1, ey, ink); px(rxp + look + 1, ey, ink); }
  }
  // nose + mouth
  rect(cx - 1, hy + Math.round(S(4)), 3, 2, ink);
  const my = hy + Math.round(S(7));
  if (mouth === "smile") { px(cx - 2, my - 1, ink); px(cx - 1, my, ink); px(cx + 1, my, ink); px(cx + 2, my - 1, ink); }
  else if (mouth === "o") { circle(cx, my, 2, ink); circle(cx, my, 1, A(shade(RED, 0.8))); }
  else if (mouth === "flat") hline(cx - 2, my, 5, ink);

  return { cx, hy, by, brx, bry, hrx, hry };
}

// ---------- props ----------
function laptop(o, keyGlow) {
  const x = o.x + 21, y = o.y + 56;
  rect(x, y, 22, 4, INK); rect(x + 1, y + 1, 20, 2, [70, 70, 80]);
  rect(x + 2, y - 9, 18, 10, INK); rect(x + 3, y - 8, 16, 8, SCREEN);
  for (let i = 0; i < 2; i++) hline(x + 5, y - 6 + i * 3, 6 + ((i + keyGlow) % 3) * 3, shade(SCREEN, 0.6));
}
function exclaim(o, dy, alpha = 255) {
  const x = o.x + 31, y = o.y + 2 + dy;
  rect(x - 1, y - 1, 5, 11, withAlpha(INK, alpha)); rect(x, y, 3, 8, withAlpha(RED, alpha));
  rect(x - 1, y + 11, 5, 4, withAlpha(INK, alpha)); rect(x, y + 12, 3, 2, withAlpha(RED, alpha));
}
function zee(x, y, alpha) {
  const a = withAlpha(INK, alpha);
  hline(x, y, 5, a); px(x + 3, y + 1, a); px(x + 2, y + 2, a); px(x + 1, y + 3, a); hline(x, y + 4, 5, a);
}
function clock(x, y, frame) {
  circle(x, y, 7, INK); circle(x, y, 6, WHITE);
  px(x, y - 4, INK); px(x, y + 4, INK); px(x - 4, y, INK); px(x + 4, y, INK);
  const a = (frame / 8) * Math.PI * 2;
  for (let t = 1; t <= 4; t++) px(x + Math.round(Math.sin(a) * t), y - Math.round(Math.cos(a) * t), INK);
  px(x - 2, y, INK); px(x - 1, y, INK);
}
function miniFox(x, y, state, bob) {
  const cy = y - bob;
  ellipseOutline(x, cy, 6, 5, INK); ellipse(x, cy, 6, 5, FUR);
  ellipse(x, cy + 2, 3, 2, CREAM);
  tri([x - 5, cy - 3], [x - 2, cy - 3], [x - 5, cy - 8], INK); tri([x + 2, cy - 3], [x + 5, cy - 3], [x + 5, cy - 8], INK);
  tri([x - 4, cy - 3], [x - 3, cy - 3], [x - 4, cy - 6], FUR); tri([x + 3, cy - 3], [x + 4, cy - 3], [x + 4, cy - 6], FUR);
  px(x - 2, cy - 1, INK); px(x + 2, cy - 1, INK);
  hline(x - 3, cy + 1, 7, SCARF[state]);
}
function cross(x, y, r) {
  for (let t = -r; t <= r; t++) {
    for (const [dx, dy] of [[-1, 0], [1, 0], [0, -1], [0, 1]]) { px(x + t + dx, y + t + dy, INK); px(x + t + dx, y - t + dy, INK); }
  }
  for (let t = -r; t <= r; t++) { px(x + t, y + t, RED); px(x + t, y - t, RED); px(x + t + 1, y + t, RED); px(x + t + 1, y - t, RED); }
}
function puff(x, y, r, alpha) { ellipse(x, y, r, r, [110, 114, 126, alpha]); }
function arrow(tx, y, dir) {
  // tip at tx, pointing at the fox (dir 1 = right, -1 = left)
  const sx = dir > 0 ? tx - 8 : tx + 2;
  hline(sx, y, 7, INK); hline(sx + 1, y, 5, [255, 120, 200]);
  for (let i = 0; i < 4; i++) { const h = 3 - i; for (let k = -h; k <= h; k++) px(tx - i * dir, y + k, k === -h || k === h ? INK : [255, 120, 200]); }
}
function dots(x, y, n) { for (let i = 0; i < n; i++) px(x + i * 3, y, INK); }
function sparkle(x, y) { px(x, y, YELLOW); px(x - 1, y, YELLOW); px(x + 1, y, YELLOW); px(x, y - 1, YELLOW); px(x, y + 1, YELLOW); }

// ---------- animations ----------
const anims = {
  working(o, f) {
    fox(o, "working", { dy: f % 2, armL: 40 + (f % 2) * 25, armR: 40 + ((f + 1) % 2) * 25, tailWag: [0, 1, 2, 1, 0, -1, -2, -1][f], look: 0 });
    laptop(o, f);
    if (f % 4 === 0) sparkle(o.x + 48, o.y + 40); if (f % 4 === 2) sparkle(o.x + 16, o.y + 40);
  },
  idle(o, f) {
    const sq = f < 3 ? 0 : 1;
    fox(o, "idle", { dy: sq, eyesOpen: false, mouth: "flat", earDroop: 3, armL: 5, armR: 5, curled: true, tailWag: 0 });
    const t = f / 6;
    zee(o.x + 46, o.y + 14 - Math.round(t * 8), 255 - Math.round(t * 150));
    if (f >= 3) zee(o.x + 50, o.y + 20 - (f - 3) * 2, 200);
  },
  needs_input(o, f) {
    const jump = [0, 3, 5, 5, 3, 0][f % 6];
    fox(o, "needs_input", { dy: -jump, wide: true, mouth: "o", armL: 150 + jump * 4, armR: 150 + jump * 4, earDroop: -1, tailWag: jump });
    exclaim(o, -jump + (f % 2));
  },
  delegating(o, f) {
    const left = f < 4;
    fox(o, "delegating", { dy: f % 2, look: left ? -1 : 1, armL: left ? 95 : 20, armR: left ? 20 : 95, tailWag: f % 2, blush: true });
    const bob = [0, 1, 2, 1, 0, 1, 2, 1][f];
    miniFox(o.x + 9, o.y + 53, "delegating", bob); miniFox(o.x + 55, o.y + 53, "delegating", 2 - bob);
  },
  waiting_peer(o, f) {
    fox(o, "waiting_peer", { look: f < 4 ? -2 : 2, mouth: "flat", armL: 75, armR: 75, tailWag: [0, 0, 2, 2, 0, 0, 2, 2][f], earDroop: 1 });
    clock(o.x + 53, o.y + 12, f);
    dots(o.x + 8, o.y + 12, (f % 3) + 1);
  },
  error(o, f) {
    const shake = [-2, 2, -1, 1, -1, 1][f];
    fox(o, "error", { dy: f % 2, look: shake > 0 ? 1 : -1, wide: true, mouth: "o", earDroop: 4, armL: 130, armR: 130, tailWag: shake });
    cross(o.x + 32, o.y + 7 + (f % 2), 4);
    puff(o.x + 12 + (f % 3), o.y + 28 - f * 2, 2, 220 - f * 25); puff(o.x + 52 - (f % 3), o.y + 24 - f * 2, 2, 200 - f * 25);
  },
  compacting(o, f) {
    const sq = [0, 1, 2, 3, 3, 2, 1, 0][f];
    fox(o, "compacting", { dy: sq, eyesOpen: sq < 2, mouth: sq > 1 ? "flat" : "smile", earDroop: sq, armL: 100, armR: 100, tailWag: 0 });
    arrow(o.x + 11 + sq * 3, o.y + 42, 1); arrow(o.x + 53 - sq * 3, o.y + 42, -1);
  },
  gone(o, f) {
    const t = f / 7, alpha = Math.round(255 * (1 - t));
    fox(o, "gone", { scale: 1 - t * 0.5, alpha, dy: Math.round(t * 4), eyesOpen: f < 3, mouth: "flat", earDroop: Math.round(t * 4), armL: 10, armR: 10 });
    for (let i = 0; i < 2; i++) {
      const pa = Math.round(220 * (1 - t));
      px(o.x + 20 + i * 24, o.y + 34 - f * 3 - i * 2, withAlpha(FUR, pa));
      px(o.x + 21 + i * 24, o.y + 33 - f * 3 - i * 2, withAlpha(WHITE, pa));
    }
  },
};
const frames = { working: 8, idle: 6, needs_input: 6, delegating: 8, waiting_peer: 8, gone: 8, error: 6, compacting: 8 };
const fps = { working: 10, idle: 3, needs_input: 8, delegating: 8, waiting_peer: 4, gone: 10, error: 8, compacting: 6 };

order.forEach((state, row) => {
  for (let col = 0; col < frames[state]; col++) anims[state]({ x: col * F, y: row * F }, col);
});

mkdirSync("assets/packs/fox", { recursive: true });
writeFileSync("assets/packs/fox/sheet.png", PNG.sync.write(png));
const manifest = { name: "Fox", frameSize: [F, F], scale: 2, animations: {} };
order.forEach((state, row) => {
  manifest.animations[state] = { sheet: "sheet.png", row, frames: frames[state], fps: fps[state], ...(state === "gone" ? { loop: false } : {}) };
});
writeFileSync("assets/packs/fox/pack.json", JSON.stringify(manifest, null, 2) + "\n");
console.log(`fox pack written (${png.width}x${png.height})`);
