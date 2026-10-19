import { beforeEach, describe, expect, it, vi } from "vitest";
import { fmtSince, render, shortCwd } from "./tooltip";
import type { AgentView } from "../shared/types";

const view = (over: Partial<AgentView> = {}): AgentView => ({
  key: "local:abc", host: "local", session_id: "abc", name: "api-server", cwd: "/Users/me/code/api", pid: 1,
  job_id: null, parent: null, permission: null, visible: "working", waiting_for: null, peer: null, since_secs: 5, ...over,
});

let el: HTMLElement;
beforeEach(() => { el = document.createElement("div"); });

describe("shortCwd / fmtSince", () => {
  it("keeps the last two path components", () => {
    expect(shortCwd("/Users/me/code/api")).toBe("…/code/api");
    expect(shortCwd("/tmp")).toBe("/tmp");
    expect(shortCwd(null)).toBe("");
  });
  it("formats elapsed time", () => {
    expect(fmtSince(5)).toBe("5s");
    expect(fmtSince(125)).toBe("2m");
    expect(fmtSince(3725)).toBe("1h 2m");
  });
});

describe("render", () => {
  it("shows name, folder, state and elapsed time", () => {
    render(el, view(), 0);
    expect(el.textContent).toContain("api-server");
    expect(el.textContent).toContain("…/code/api");
    expect(el.textContent).toContain("working · 5s");
  });

  it("shows what a needs-input agent waits for and who a waiting one waits on", () => {
    render(el, view({ visible: "needs_input", waiting_for: "permission prompt" }), 0);
    expect(el.textContent).toContain("needs your input (permission prompt)");
    render(el, view({ visible: "waiting_peer", peer: "docs-site" }), 0);
    expect(el.textContent).toContain("→ docs-site");
  });

  it("treats the session name as text, never as markup", () => {
    render(el, view({ name: "<img src=x onerror=alert(1)>" }), 0);
    expect(el.querySelector("img")).toBeNull();
    expect(el.textContent).toContain("<img src=x");
  });

  it("returns an updater that changes only the elapsed time", () => {
    const update = render(el, view({ since_secs: 5 }), 0);
    const first = el.querySelector("b");
    update(60);
    expect(el.textContent).toContain("1m");
    expect(el.querySelector("b")).toBe(first);
  });

  it("offers Allow, Deny and the terminal for a pending permission, and reports the choice", () => {
    const onDecide = vi.fn();
    render(el, view({ visible: "needs_input", permission: ["Bash", "rm -rf build"] }), 0, onDecide);
    expect(el.querySelector(".ask code")?.textContent).toBe("Bash: rm -rf build");
    const buttons = [...el.querySelectorAll("button")];
    expect(buttons.map((b) => b.textContent)).toEqual(["Allow", "Deny", "In the terminal"]);
    buttons[0].click(); buttons[1].click(); buttons[2].click();
    expect(onDecide.mock.calls.map((c) => c[0])).toEqual(["allow", "deny", "terminal"]);
  });

  it("cannot approve an input that was truncated for display", () => {
    render(el, view({ permission: ["Bash", "x… [… truncated: decide in the terminal]"] }), 0, vi.fn());
    expect([...el.querySelectorAll("button")].map((b) => b.textContent)).toEqual(["Deny", "In the terminal"]);
  });

  it("draws no buttons without a decision callback", () => {
    render(el, view({ permission: ["Bash", "ls"] }), 0);
    expect(el.querySelector("button")).toBeNull();
  });

  it("keeps button presses inside the bubble so they do not start a drag", () => {
    render(el, view({ permission: ["Bash", "ls"] }), 0, vi.fn());
    const outer = vi.fn();
    el.addEventListener("mousedown", outer);
    el.querySelector("button")!.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(outer).not.toHaveBeenCalled();
  });
});
