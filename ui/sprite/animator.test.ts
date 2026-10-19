import { describe, expect, it } from "vitest";
import { Animator } from "./animator";
import type { Manifest } from "../shared/types";

const manifest: Manifest = {
  name: "t", frameSize: [10, 20], scale: 1, zoom: 1, labelH: 0,
  animations: {
    idle: { sheet: "a.png", row: 0, frames: 3, fps: 10, loop: true },
    working: { sheet: "a.png", row: 1, frames: 4, fps: 10, loop: true },
    gone: { sheet: "b.png", row: 2, frames: 3, fps: 10, loop: false },
  },
};

describe("Animator", () => {
  it("starts on the first frame of the requested row", () => {
    expect(new Animator(manifest, "working").tick(0)).toEqual({ sheet: "a.png", sx: 0, sy: 20 });
  });

  it("advances one frame per 1/fps and wraps when looping", () => {
    const a = new Animator(manifest, "idle");
    expect(a.tick(100).sx).toBe(10);
    expect(a.tick(100).sx).toBe(20);
    expect(a.tick(100).sx).toBe(0);
  });

  it("keeps the remainder between ticks", () => {
    const a = new Animator(manifest, "idle");
    expect(a.tick(60).sx).toBe(0);
    expect(a.tick(60).sx).toBe(10); // 120 ms in
  });

  it("stops on the last frame of a non-looping animation", () => {
    const a = new Animator(manifest, "gone");
    expect(a.tick(10_000).sx).toBe(20);
    expect(a.tick(10_000).sx).toBe(20);
  });

  it("falls back to idle for an unknown state", () => {
    expect(new Animator(manifest, "mystery").tick(0).sy).toBe(0);
  });

  it("play() restarts on another animation but ignores the one already playing", () => {
    const a = new Animator(manifest, "idle");
    a.tick(100);
    a.play("idle");
    expect(a.tick(0).sx).toBe(10);
    a.play("working");
    expect(a.tick(0)).toEqual({ sheet: "a.png", sx: 0, sy: 20 });
  });

  it("can replay a finished animation", () => {
    const a = new Animator(manifest, "gone");
    a.tick(10_000);
    a.play("idle");
    a.play("gone");
    expect(a.tick(0).sx).toBe(0);
  });
});
