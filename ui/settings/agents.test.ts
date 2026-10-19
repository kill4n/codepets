import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { agentRow, CONFIRM_MS, stateText, type AgentActions } from "./agents";
import type { AgentView } from "../shared/types";

const view = (over: Partial<AgentView> = {}): AgentView => ({
  key: "local:abc", host: "local", session_id: "abc", name: "api-server", cwd: "/work/api", pid: 4242,
  job_id: null, parent: null, permission: null, visible: "working", waiting_for: null, peer: null, since_secs: 0, ...over,
});

let act: { [K in keyof AgentActions]: ReturnType<typeof vi.fn> };
const btn = (li: HTMLElement, name: string) => li.querySelector<HTMLButtonElement>(`button[data-act=${name}]`);

beforeEach(() => { vi.useFakeTimers(); act = { focus: vi.fn(), hide: vi.fn(), show: vi.fn(), terminate: vi.fn() }; });
afterEach(() => vi.useRealTimers());

describe("stateText", () => {
  it("describes each state, with what it waits for", () => {
    expect(stateText(view({ visible: "needs_input", waiting_for: "permission prompt" }))).toBe("needs input (permission prompt)");
    expect(stateText(view({ visible: "waiting_peer", peer: "docs" }))).toBe("waiting for a peer → docs");
  });
});

describe("agentRow", () => {
  it("shows the session as text, never as markup", () => {
    const li = agentRow(view({ name: "<b onclick=x>evil</b>", cwd: "<i>x</i>" }), false, act);
    expect(li.querySelector("b")?.textContent).toBe("<b onclick=x>evil</b>");
    expect(li.querySelector("i")).toBeNull();
  });

  it("offers focus, hide and terminate for a normal session", () => {
    const li = agentRow(view(), false, act);
    btn(li, "focus")!.click(); btn(li, "hide")!.click();
    expect(act.focus).toHaveBeenCalledWith("local:abc");
    expect(act.hide).toHaveBeenCalledWith("local:abc");
    expect(btn(li, "terminate")).not.toBeNull();
  });

  it("offers Show instead of Hide for a hidden sprite", () => {
    const li = agentRow(view(), true, act);
    expect(btn(li, "hide")).toBeNull();
    btn(li, "show")!.click();
    expect(act.show).toHaveBeenCalledWith("local:abc");
    expect(li.textContent).toContain("hidden");
  });

  it("has nothing to focus or terminate without a process", () => {
    const li = agentRow(view({ pid: null, job_id: null }), false, act);
    expect(btn(li, "focus")).toBeNull();
    expect(btn(li, "terminate")).toBeNull();
  });

  it("can focus a daemon-hosted session that has a job id but no pid", () => {
    expect(btn(agentRow(view({ pid: null, job_id: "job1" }), false, act), "focus")).not.toBeNull();
  });

  it("subagents can only be hidden", () => {
    const li = agentRow(view({ parent: "local:p" }), false, act);
    expect(btn(li, "hide")).not.toBeNull();
    expect(btn(li, "focus")).toBeNull();
    expect(btn(li, "terminate")).toBeNull();
    expect(li.classList.contains("child")).toBe(true);
  });

  it("cannot terminate a session that already ended", () => {
    expect(btn(agentRow(view({ visible: "gone" }), false, act), "terminate")).toBeNull();
  });

  describe("terminate asks for a second click", () => {
    it("terminates only on the second click", () => {
      const li = agentRow(view(), false, act);
      const kill = btn(li, "terminate")!;
      kill.click();
      expect(act.terminate).not.toHaveBeenCalled();
      expect(kill.textContent).toBe("Click again to confirm");
      kill.click();
      expect(act.terminate).toHaveBeenCalledOnce();
      expect(act.terminate).toHaveBeenCalledWith("local:abc");
      expect(kill.textContent).toBe("Terminate");
    });

    it("goes back to the first state when the confirmation times out", () => {
      const kill = btn(agentRow(view(), false, act), "terminate")!;
      kill.click();
      vi.advanceTimersByTime(CONFIRM_MS + 1);
      expect(kill.textContent).toBe("Terminate");
      kill.click();
      expect(act.terminate).not.toHaveBeenCalled();
    });
  });
});
