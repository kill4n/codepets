// ui/sprite/main.ts
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { AgentView, Manifest } from "../shared/types";
import { Animator } from "./animator";
import { render as renderTip, type Decision } from "./tooltip";

const params = new URLSearchParams(location.search);
const key = params.get("key") ?? "";
const mock = params.get("mock");

const canvas = document.getElementById("c") as HTMLCanvasElement;
const tip = document.getElementById("tip") as HTMLDivElement;
const nameEl = document.getElementById("name") as HTMLDivElement;
(window as any).__codepetsLog = (m: string) => uiLog(m);
// diagnostics: which element receives presses inside this window
document.addEventListener("mousedown", (e) => {
  const t = e.target as HTMLElement | null;
  uiLog(`doc mousedown on <${t?.tagName?.toLowerCase()} id=${t?.id ?? ""} class=${t?.className ?? ""}> at ${e.clientX},${e.clientY} tip=${tipVisible}`);
}, true);
document.addEventListener("click", (e) => {
  const t = e.target as HTMLElement | null;
  uiLog(`doc click on <${t?.tagName?.toLowerCase()} id=${t?.id ?? ""} class=${t?.className ?? ""}>`);
}, true);
window.addEventListener("load", () => uiLog(`loaded ${innerWidth}x${innerHeight}`));
const ctx2d = canvas.getContext("2d");
if (!ctx2d) throw new Error("2d context unavailable");
const ctx: CanvasRenderingContext2D = ctx2d;
ctx.imageSmoothingEnabled = false;

let manifest: Manifest;
let sheets = new Map<string, HTMLImageElement>();
let animator: Animator;
let view: AgentView | null = null;
let viewAt = performance.now();
let tipVisible = false;

let loadChain: Promise<void> = Promise.resolve();
function loadManifest(m: Manifest): Promise<void> {
  loadChain = loadChain.then(() => doLoadManifest(m)).catch((e) => console.warn("loadManifest failed", e));
  return loadChain;
}

async function doLoadManifest(m: Manifest) {
  manifest = m;
  const [fw, fh] = m.frameSize;
  canvas.width = fw; canvas.height = fh;
  const zoom = m.zoom ?? 1;
  canvas.style.width = `${Math.round(fw * m.scale * zoom)}px`; canvas.style.height = `${Math.round(fh * m.scale * zoom)}px`;
  // the speech bubble sits right above the sprite; tell CSS how tall the sprite is
  document.documentElement.style.setProperty("--sprite-h", `${Math.round(fh * m.scale * zoom)}px`);
  document.documentElement.style.setProperty("--label-h", `${m.labelH ?? 0}px`);
  // pack-declared empty space under the feet: pull the name label up over it
  const pull = m.labelH > 0 ? Math.round((m.labelOverlap ?? 0) * m.scale * zoom) : 0;
  document.documentElement.style.setProperty("--label-pull", `${pull}px`);
  nameEl.classList.toggle("hidden", !(m.labelH > 0));
  sheets = new Map();
  for (const a of Object.values(m.animations)) {
    if (sheets.has(a.sheet)) continue;
    const img = new Image();
    img.src = mock ? `/assets-dev/${a.sheet}` : await invoke<string>("get_pack_url", { sheet: a.sheet });
    await img.decode().catch((e) => console.warn("sheet decode failed", a.sheet, e));
    sheets.set(a.sheet, img);
  }
  animator = new Animator(m, view?.visible ?? "idle");
}

// permission prompts open the bubble by themselves and close it again once answered
let autoOpened = false;
function uiLog(msg: string) { if (!mock) invoke("ui_log", { msg: `${key}: ${msg}` }).catch(() => {}); }
function decide(d: Decision) {
  uiLog(`decide ${d}`);
  if (!view) return;
  invoke("answer_permission", { key: view.key, decision: d })
    .then(() => uiLog(`answer_permission ${d} ok`))
    .catch((err) => { console.warn("answer_permission failed", err); uiLog(`answer_permission failed: ${err}`); });
}
let tipUpdate: ((since: number) => void) | null = null;
function paintTip(v: AgentView, since: number) { tipUpdate = renderTip(tip, v, since, decide); }

function setView(v: AgentView) {
  const hadPermission = !!view?.permission;
  view = v; viewAt = performance.now();
  nameEl.textContent = v.name; nameEl.title = v.name;
  nameEl.classList.toggle("fade", v.visible === "gone");
  animator?.play(v.visible);
  if (tipVisible) paintTip(v, 0);
  if (v.permission && !tipVisible && !mock) { autoOpened = true; void toggleBubble(); }
  else if (hadPermission && !v.permission && autoOpened && tipVisible) { autoOpened = false; void toggleBubble(); }
}

let last = performance.now();
function frame(now: number) {
  const dt = Math.min(now - last, 100); last = now;
  if (animator && manifest) {
    const { sheet, sx, sy } = animator.tick(dt);
    const img = sheets.get(sheet);
    const [fw, fh] = manifest.frameSize;
    ctx.clearRect(0, 0, fw, fh);
    if (img?.complete) ctx.drawImage(img, sx, sy, fw, fh, 0, 0, fw, fh);
  }
  if (tipVisible && view && tipUpdate) tipUpdate(Math.floor((now - viewAt) / 1000));
  requestAnimationFrame(frame);
}
requestAnimationFrame(frame);

// drag vs click: a mousedown followed by movement starts an OS drag; a plain click toggles the tooltip
let downAt: { x: number; y: number } | null = null;
for (const target of [canvas, nameEl] as HTMLElement[]) {
  target.addEventListener("mousedown", (e) => { if (e.button === 0) downAt = { x: e.screenX, y: e.screenY }; });
  target.addEventListener("mousemove", (e) => {
    if (!downAt) return;
    if (Math.hypot(e.screenX - downAt.x, e.screenY - downAt.y) > 4) {
      downAt = null;
      if (!mock) getCurrentWindow().startDragging().catch((err) => console.warn("startDragging failed", err));
    }
  });
  // A plain click toggles the bubble, but only after the double-click window has passed, so a
  // double click (focus the terminal) does not open and close the bubble on the way.
  let clickTimer: number | undefined;
  target.addEventListener("mouseup", () => {
    if (downAt) {
      window.clearTimeout(clickTimer);
      clickTimer = window.setTimeout(() => void toggleBubble(), 250);
    }
    downAt = null;
  });
  target.addEventListener("dblclick", () => {
    window.clearTimeout(clickTimer);
    if (mock) return;
    invoke("focus_terminal", { key }).catch((err) => console.warn("focus_terminal failed", err));
  });
}

// The window is only as big as the sprite while the bubble is closed; ask Rust to grow it before
// showing the bubble, and shrink it after hiding, so the bubble is never clipped.
let toggling = false;
async function toggleBubble() {
  if (toggling) return;
  toggling = true;
  const next = !tipVisible;
  try {
    if (next) {
      if (!mock) await invoke("set_bubble", { key, open: true });
      tipVisible = true; tip.classList.add("show"); if (view) paintTip(view, 0);
    } else {
      tipVisible = false; tip.classList.remove("show");
      if (!mock) await invoke("set_bubble", { key, open: false });
    }
  } catch (err) {
    console.warn("set_bubble failed", err);
  } finally {
    toggling = false;
  }
}

async function main() {
  if (mock) {
    const m: Manifest = await (await fetch("/assets-dev/pack.json")).json();
    for (const a of Object.values(m.animations)) a.loop ??= true; // Rust applies this serde default; raw pack.json omits it
    await loadManifest(m);
    setView({ key: "mock", host: "local", session_id: "mock0000", name: "mock-agent", cwd: "/Users/me/proj", pid: null, job_id: null, parent: null, permission: mock === "needs_input" ? ["Bash", "npm test"] : null,
      visible: mock as AgentView["visible"], waiting_for: mock === "needs_input" ? "permission prompt" : null,
      peer: mock === "waiting_peer" ? "buddy-2" : null, since_secs: 0 });
    return;
  }
  let gotEvent = false;
  await listen<AgentView>("agent-state", (e) => {
    if (e.payload.key !== key) return; // defence in depth: only this window's agent
    gotEvent = true; setView(e.payload);
  });
  await listen<Manifest>("manifest", (e) => { loadManifest(e.payload); });
  const w = getCurrentWindow();
  let moveTimer: number | undefined;
  await w.onMoved(() => {
    clearTimeout(moveTimer);
    moveTimer = window.setTimeout(async () => {
      try {
        const p = await w.outerPosition();
        const f = await w.scaleFactor();
        await invoke("report_position", { key, x: Math.round(p.x / f), y: Math.round(p.y / f) });
      } catch (err) { console.warn("report_position failed", err); }
    }, 300);
  });
  // null when no valid pack is installed: stay blank until set_pack emits "manifest"
  const m = await invoke<Manifest | null>("get_manifest", { key });
  if (m) await loadManifest(m);
  const agents = await invoke<AgentView[]>("list_agents");
  const mine = agents.find((a) => a.key === key);
  if (mine && !gotEvent) setView(mine);
}
main();
