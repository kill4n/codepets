import type { AgentView } from "../shared/types";

const LABEL: Record<string, string> = {
  working: "working", idle: "idle", needs_input: "needs your input",
  delegating: "delegating to subagents", waiting_peer: "waiting for another agent",
  compacting: "compacting its context", error: "error", gone: "done",
};

export function shortCwd(cwd: string | null): string {
  if (!cwd) return "";
  const parts = cwd.split("/").filter(Boolean);
  return parts.length <= 2 ? cwd : "…/" + parts.slice(-2).join("/");
}

export function fmtSince(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

export type Decision = "allow" | "deny" | "terminal";

/// Builds the bubble once and returns an updater for the elapsed time, so the DOM (and its buttons)
/// is not rebuilt every frame: a button replaced between mousedown and mouseup never gets its click.
export function render(el: HTMLElement, v: AgentView, sinceOffset: number, onDecide?: (d: Decision) => void): (sinceOffset: number) => void {
  const detail = (v.visible === "needs_input" || v.visible === "error") && v.waiting_for ? ` (${v.waiting_for})`
               : v.visible === "waiting_peer" && v.peer ? ` → ${v.peer}` : "";
  const b = document.createElement("b");
  b.textContent = v.name;
  const stateLine = `${LABEL[v.visible] ?? v.visible}${detail} · `;
  const since = document.createTextNode(fmtSince(v.since_secs + sinceOffset));
  el.replaceChildren(
    b,
    document.createElement("br"),
    document.createTextNode(shortCwd(v.cwd)),
    document.createElement("br"),
    document.createTextNode(stateLine),
    since,
  );
  const update = (off: number) => { since.textContent = fmtSince(v.since_secs + off); };
  if (v.permission && onDecide) {
    const [tool, summary] = v.permission;
    const ask = document.createElement("div");
    ask.className = "ask";
    const what = document.createElement("code");
    what.textContent = summary ? `${tool}: ${summary}` : tool;
    const actions = document.createElement("div");
    actions.className = "actions";
    const truncated = summary.endsWith("[… truncated: decide in the terminal]");
    const choices = truncated
      ? ([["Deny", "deny", "bad"], ["In the terminal", "terminal", ""]] as const)
      : ([["Allow", "allow", "ok"], ["Deny", "deny", "bad"], ["In the terminal", "terminal", ""]] as const);
    for (const [label, d, cls] of choices) {
      const b = document.createElement("button");
      b.textContent = label; b.className = cls; b.type = "button";
      b.addEventListener("mousedown", (e) => { e.stopPropagation(); (window as any).__codepetsLog?.("mousedown " + d); });
      b.addEventListener("mouseup", (e) => e.stopPropagation());
      b.addEventListener("click", (e) => { e.stopPropagation(); onDecide(d); });
      actions.append(b);
    }
    ask.append(what, actions);
    el.append(ask);
  }
  return update;
}
