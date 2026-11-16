import type { UpdateState } from "../shared/types";
import { el } from "../shared/dom";

export interface UpdateActions {
  check(): void;
  install(): void;
  setAuto(enabled: boolean): void;
}

export function updateText(s: UpdateState, version: string): string {
  switch (s.kind) {
    case "idle": return `CodePets ${version}`;
    case "checking": return "Checking for updates…";
    case "up_to_date": return `CodePets ${version} is up to date.`;
    case "available": return `Version ${s.version} is available.`;
    case "installing": return "Downloading and installing the update…";
    case "failed": return `Could not update: ${s.message}`;
  }
}

/** The body of the "Updates" card. Release notes and error text are untrusted: textContent only. */
export function updateRows(s: UpdateState, version: string, auto: boolean, act: UpdateActions): Node[] {
  const busy = s.kind === "checking" || s.kind === "installing";
  const check = el("button", "Check now", { type: "button", id: "check-update" });
  check.disabled = busy;
  check.onclick = () => act.check();

  const status = el("p", updateText(s, version) + " ", { class: s.kind === "failed" ? "warn" : s.kind === "available" ? "ok" : "" }, check);
  const rows: Node[] = [status];

  if (s.kind === "available") {
    const install = el("button", "Install and restart", { type: "button", id: "install-update" });
    install.onclick = () => act.install();
    rows.push(el("p", undefined, {}, install));
    if (s.notes) rows.push(el("p", s.notes, { class: "muted notes" }));
  }

  const box = el("input", undefined, { type: "checkbox", id: "auto-update" });
  box.checked = auto;
  box.onchange = () => act.setAuto(box.checked);
  rows.push(el("label", undefined, {}, box, " Check for updates automatically"));
  return rows;
}
