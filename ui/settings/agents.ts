import type { AgentView } from "../shared/types";
import { el } from "../shared/dom";

export interface AgentActions {
  focus(key: string): void;
  hide(key: string): void;
  show(key: string): void;
  terminate(key: string): void;
}

/** How long the "Confirm?" state of the terminate button lasts before it reverts. */
export const CONFIRM_MS = 4000;

const STATE: Record<string, string> = {
  working: "working", idle: "idle", needs_input: "needs input", delegating: "delegating",
  waiting_peer: "waiting for a peer", gone: "done",
};

export function stateText(a: AgentView): string {
  let t = STATE[a.visible] ?? a.visible;
  if (a.waiting_for) t += ` (${a.waiting_for})`;
  if (a.peer) t += ` → ${a.peer}`;
  return t;
}

/** One row of the agents list: name, state, folder and the actions that make sense for it. */
export function agentRow(a: AgentView, hidden: boolean, act: AgentActions): HTMLLIElement {
  const li = el("li", undefined, { class: a.parent ? "agent child" : "agent" });
  li.append(el("b", a.name), el("span", ` · ${stateText(a)}`, { class: "muted" }));
  if (hidden) li.append(el("span", "hidden", { class: "tag" }));
  if (a.cwd) li.append(el("br"), el("code", a.cwd));

  const actions = el("div", undefined, { class: "row-actions" });
  if (!a.parent && (a.pid !== null || a.job_id !== null)) {
    const b = el("button", "Focus", { type: "button", "data-act": "focus" });
    b.onclick = () => act.focus(a.key);
    actions.append(b);
  }
  const vis = el("button", hidden ? "Show" : "Hide", { type: "button", "data-act": hidden ? "show" : "hide" });
  vis.onclick = () => (hidden ? act.show(a.key) : act.hide(a.key));
  actions.append(vis);

  // terminating ends a real process: it takes two clicks
  if (!a.parent && a.pid !== null && a.visible !== "gone") {
    const kill = el("button", "Terminate", { type: "button", class: "danger", "data-act": "terminate" });
    let timer: number | undefined;
    kill.onclick = () => {
      if (timer === undefined) {
        kill.textContent = "Click again to confirm";
        timer = window.setTimeout(() => { timer = undefined; kill.textContent = "Terminate"; }, CONFIRM_MS);
        return;
      }
      window.clearTimeout(timer); timer = undefined;
      kill.textContent = "Terminate";
      act.terminate(a.key);
    };
    actions.append(kill);
  }
  li.append(actions);
  return li;
}
