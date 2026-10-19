import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import type { AgentView } from "../shared/types";
import { el } from "../shared/dom";
import { agentRow, type AgentActions } from "./agents";

interface Status {
  hooks_installed: boolean; poll_error: string | null; server_error: string | null; pack_warning: string | null;
  hook_url: string; token: string; pack: string; size: "small" | "normal" | "large"; wander: boolean; wander_after_secs: number;
  show_name: boolean; permission_wait_secs: number; version: string;
}

const PERMISSION_WAITS: [number, string][] = [
  [0, "Never (always in the terminal)"], [15, "15 seconds"], [30, "30 seconds"], [60, "1 minute"], [120, "2 minutes"],
];

const app = document.getElementById("app")!;
let status: Status; let agents: AgentView[] = []; let hidden = new Set<string>();
let packs: { dir: string; name: string }[] = []; let autostart = false;
let notice: { text: string; bad: boolean } | null = null; let noticeTimer: number | undefined;

function say(text: string, bad = false) {
  notice = { text, bad };
  window.clearTimeout(noticeTimer);
  noticeTimer = window.setTimeout(() => { notice = null; render(); }, 6000);
  render();
}
const fail = (e: unknown) => say(String(e), true);

/** Run a backend command, report a failure inline, then reload everything. */
async function run(cmd: string, args?: Record<string, unknown>, ok?: string) {
  try { await invoke(cmd, args); if (ok) say(ok); } catch (e) { fail(e); }
  await refresh();
}

const actions: AgentActions = {
  focus: (key) => void run("focus_terminal", { key }),
  hide: (key) => void run("hide_agent", { key }),
  show: (key) => void run("show_agent", { key }),
  terminate: (key) => void run("terminate_agent", { key }, "Asked the session to exit."),
};

function section(title: string, ...children: (Node | string)[]) {
  return el("section", undefined, { class: "card" }, el("h2", title), ...children);
}

function select(id: string, options: [string, string][], current: string, onchange: (v: string) => void) {
  const s = el("select", undefined, { id });
  for (const [value, label] of options) {
    const o = el("option", label, { value });
    o.selected = value === current;
    s.append(o);
  }
  s.onchange = () => onchange(s.value);
  return s;
}

function checkbox(id: string, checked: boolean, onchange: (v: boolean) => void) {
  const c = el("input", undefined, { type: "checkbox", id });
  c.checked = checked;
  c.onchange = () => onchange(c.checked);
  return c;
}

function render() {
  const hooksBtn = el("button", status.hooks_installed ? "Disconnect" : "Connect to Claude Code", { id: "hooks", type: "button" });
  hooksBtn.onclick = () => void run(status.hooks_installed ? "uninstall_hooks" : "install_hooks");

  const packSel = select("pack", packs.map((p) => [p.dir, p.name]), status.pack, (name) => void run("set_pack", { name }));
  const sizeSel = select("size", [["small", "Small"], ["normal", "Normal"], ["large", "Large"]], status.size, (size) => void run("set_size", { size }));
  const waitChoices = PERMISSION_WAITS.map(([s, l]) => [String(s), l] as [string, string]);
  if (!PERMISSION_WAITS.some(([s]) => s === status.permission_wait_secs)) waitChoices.push([String(status.permission_wait_secs), `${status.permission_wait_secs} seconds`]);
  const waitSel = select("permission-wait", waitChoices, String(status.permission_wait_secs), (v) => void run("set_permission_wait", { secs: Number(v) }));

  const showName = checkbox("show-name", status.show_name, (enabled) => void run("set_show_name", { enabled }));
  const wander = checkbox("wander", status.wander, () => saveWander());
  const wanderAfter = el("input", undefined, { type: "number", id: "wander-after", min: "5", max: "3600", step: "5", style: "width:5em" });
  wanderAfter.value = String(status.wander_after_secs);
  const saveWander = () => void run("set_wander", { enabled: wander.checked, afterSecs: Math.round(Number(wanderAfter.value)) });
  wanderAfter.onchange = saveWander;

  const auto = checkbox("auto", autostart, async (on) => {
    try { on ? await enable() : await disable(); } catch (e) { fail(e); }
    autostart = await isEnabled().catch(() => false);
    render();
  });

  const list = el("ul", undefined, { class: "agents" });
  if (agents.length === 0) list.append(el("li", "No Claude Code sessions are running.", { class: "muted" }));
  // subagents right under their parent
  const top = agents.filter((a) => !a.parent);
  const kids = (k: string) => agents.filter((a) => a.parent === k);
  for (const a of top) {
    list.append(agentRow(a, hidden.has(a.key), actions));
    for (const c of kids(a.key)) list.append(agentRow(c, hidden.has(c.key) || hidden.has(a.key), actions));
  }
  for (const a of agents) if (a.parent && !top.some((t) => t.key === a.parent)) list.append(agentRow(a, hidden.has(a.key), actions));

  const hiddenCount = agents.filter((a) => hidden.has(a.key)).length;
  const showAll = el("button", `Show all hidden (${hiddenCount})`, { type: "button", id: "show-all" });
  showAll.onclick = () => void Promise.all(agents.filter((a) => hidden.has(a.key)).map((a) => invoke("show_agent", { key: a.key }))).then(refresh).catch(fail);

  app.replaceChildren(
    ...(notice ? [el("p", notice.text, { class: notice.bad ? "banner bad" : "banner ok", role: "status" })] : []),
    section("Claude Code",
      el("p", "Hooks: ", {}, el("span", status.hooks_installed ? "installed" : "not installed", { class: status.hooks_installed ? "ok" : "bad" }), " ", hooksBtn),
      el("p", "Poll: ", {}, status.poll_error ? el("span", `⚠ ${status.poll_error}`, { class: "warn" }) : el("span", "ok", { class: "ok" })),
      el("p", "Server: ", {}, el("code", status.hook_url), " ",
        status.server_error ? el("span", `⚠ ${status.server_error}`, { class: "warn" }) : el("span", "ok", { class: "ok" }),
        el("br"), "Token: ", el("code", status.token))),
    section("Sprites",
      el("label", "Pack ", {}, packSel),
      el("label", "Size ", {}, sizeSel),
      el("label", undefined, {}, showName, " Show session name"),
      el("label", undefined, {}, wander, " Wander when idle"),
      el("label", "Start wandering after ", {}, wanderAfter, " seconds idle"),
      ...(status.pack_warning ? [el("p", `⚠ ${status.pack_warning}`, { class: "warn" })] : [])),
    section("Permissions",
      el("p", "When Claude Code asks for a permission, the sprite's bubble can answer it. If you don't answer in time, the terminal asks as usual.", { class: "muted" }),
      el("label", "Hold a request for the bubble ", {}, waitSel)),
    section("System", el("label", undefined, {}, auto, " Launch at login")),
    section(`Agents (${agents.length})`, list, ...(hiddenCount > 0 ? [showAll] : [])),
    el("p", `CodePets ${status.version}`, { class: "muted footer" }),
  );
}

async function refresh() {
  [status, agents, hidden, packs, autostart] = await Promise.all([
    invoke<Status>("get_status"), invoke<AgentView[]>("list_agents"),
    invoke<string[]>("list_hidden").then((k) => new Set(k)),
    invoke<{ dir: string; name: string }[]>("list_pack_names"), isEnabled().catch(() => false),
  ]);
  render();
}

listen<AgentView[]>("agents", () => void refresh());
refresh().catch(fail);
setInterval(() => invoke<Status>("get_status").then((s) => { if (JSON.stringify(s) !== JSON.stringify(status)) { status = s; render(); } }), 3000);
