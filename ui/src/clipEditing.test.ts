import { describe, expect, it } from "vitest";
import { clipsInBox, dropBar, groupMoveDelta, moveClips, snapTo, toggleId, trimClip } from "./clipEditing";
import type { Clip } from "./types";
import { TICKS_PER_BEAT as B } from "./types";

// Positions read as beats here; the model counts ticks, so everything is scaled by B.
const c = (id: string, startBeats: number, lenBeats: number, patternId = "p"): Clip => ({
  id,
  patternId,
  startTick: startBeats * B,
  lengthTicks: lenBeats * B,
  offsetTicks: 0,
  muted: false,
});

describe("group move", () => {
  it("snaps the anchor to the grid and shifts the group together", () => {
    const a = c("a", 4, 4);
    const b = c("b", 12, 4);
    const d = groupMoveDelta(a, [a, b], 5.2 * B, 4 * B, 64 * B);
    expect(d).toBe(4 * B);
    expect(moveClips([a, b], d).map((x) => x.startTick)).toEqual([8 * B, 16 * B]);
  });

  it("stops at bar 1 and at the song end", () => {
    const a = c("a", 4, 4);
    const b = c("b", 12, 4);
    expect(groupMoveDelta(a, [a, b], -40 * B, 4 * B, 64 * B)).toBe(-4 * B);
    expect(groupMoveDelta(a, [a, b], 100 * B, 4 * B, 64 * B)).toBe(48 * B); // b ends exactly at 64
  });

  it("is zero for an empty group", () => {
    expect(groupMoveDelta(c("a", 0, 4), [], 8 * B, 4 * B, 64 * B)).toBe(0);
  });
});

describe("trim", () => {
  it("snaps, keeps at least one beat and never crosses the end", () => {
    expect(trimClip(c("a", 0, 4), 6.4 * B, 4 * B, 64 * B).lengthTicks).toBe(8 * B);
    expect(trimClip(c("a", 0, 4), -3 * B, 4 * B, 64 * B).lengthTicks).toBe(B);
    expect(trimClip(c("a", 56, 4), 30 * B, 4 * B, 64 * B).lengthTicks).toBe(8 * B);
    expect(trimClip(c("a", 63.5, 0.5), 4 * B, B, 64 * B).lengthTicks).toBe(0.5 * B);
  });
});

describe("drop", () => {
  it("lands on the bar under the pointer, inside the song only", () => {
    expect(dropBar(9.9 * B, 4 * B, 64 * B)).toBe(8 * B);
    expect(dropBar(0, 4 * B, 64 * B)).toBe(0);
    expect(dropBar(63.9 * B, 4 * B, 64 * B)).toBe(60 * B);
    expect(dropBar(64 * B, 4 * B, 64 * B)).toBeNull();
    expect(dropBar(-B, 4 * B, 64 * B)).toBeNull();
  });
});

describe("marquee", () => {
  it("hits clips on the spanned lanes that overlap the tick range", () => {
    const clips = [c("a", 0, 4, "x"), c("b", 8, 4, "y"), c("c", 20, 4, "x"), c("d", 4, 4, "z")];
    const lane = (cl: Clip) => ({ x: 0, y: 1, z: 2 })[cl.patternId as "x" | "y" | "z"] ?? -1;
    expect(clipsInBox(clips, lane, 1, 0, 10 * B, 2 * B)).toEqual(["a", "b"]);
    expect(clipsInBox(clips, lane, 0, 2, 4 * B, 4.5 * B)).toEqual(["d"]);
  });

  it("toggles ids", () => {
    expect(toggleId(["a", "b"], "a")).toEqual(["b"]);
    expect(toggleId(["a"], "b")).toEqual(["a", "b"]);
  });
});

describe("snap", () => {
  it("rounds to the grid", () => {
    expect(snapTo(5.9 * B, 4 * B)).toBe(4 * B);
    expect(snapTo(6.1 * B, 4 * B)).toBe(8 * B);
  });
});
