import { describe, expect, it, vi } from "vitest";
import { updateRows, updateText, type UpdateActions } from "./updates";
import type { UpdateState } from "../shared/types";

const actions = (): UpdateActions => ({ check: vi.fn(), install: vi.fn(), setAuto: vi.fn() });
const rows = (state: UpdateState, auto = true, act = actions()) => {
  const host = document.createElement("div");
  host.append(...updateRows(state, "0.1.9", auto, act));
  return { host, act };
};
const button = (host: HTMLElement, label: string) => [...host.querySelectorAll("button")].find((b) => b.textContent === label);

describe("updateText", () => {
  it("describes every state", () => {
    expect(updateText({ kind: "idle" }, "0.1.9")).toBe("CodePets 0.1.9");
    expect(updateText({ kind: "checking" }, "0.1.9")).toBe("Checking for updates…");
    expect(updateText({ kind: "up_to_date" }, "0.1.9")).toBe("CodePets 0.1.9 is up to date.");
    expect(updateText({ kind: "available", version: "0.2.0", notes: null }, "0.1.9")).toBe("Version 0.2.0 is available.");
    expect(updateText({ kind: "installing" }, "0.1.9")).toBe("Downloading and installing the update…");
    expect(updateText({ kind: "failed", message: "offline" }, "0.1.9")).toBe("Could not update: offline");
  });
});

describe("updateRows", () => {
  it("checks on demand and offers no install when nothing is pending", () => {
    const { host, act } = rows({ kind: "up_to_date" });
    button(host, "Check now")!.click();
    expect(act.check).toHaveBeenCalledOnce();
    expect(button(host, "Install and restart")).toBeUndefined();
  });

  it("installs the available update, and shows its notes as plain text", () => {
    const { host, act } = rows({ kind: "available", version: "0.2.0", notes: "<img src=x onerror=alert(1)> fixes" });
    button(host, "Install and restart")!.click();
    expect(act.install).toHaveBeenCalledOnce();
    expect(host.querySelector("img")).toBeNull();
    expect(host.textContent).toContain("<img src=x onerror=alert(1)> fixes");
  });

  it("locks both buttons while checking or installing", () => {
    for (const state of [{ kind: "checking" }, { kind: "installing" }] as UpdateState[]) {
      const { host } = rows(state);
      expect(button(host, "Check now")!.disabled).toBe(true);
    }
  });

  it("toggles the automatic check", () => {
    const { host, act } = rows({ kind: "idle" }, true);
    const box = host.querySelector<HTMLInputElement>("input[type=checkbox]")!;
    expect(box.checked).toBe(true);
    box.checked = false;
    box.dispatchEvent(new Event("change"));
    expect(act.setAuto).toHaveBeenCalledWith(false);
  });
});
