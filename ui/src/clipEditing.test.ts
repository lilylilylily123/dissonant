import { describe, expect, it } from "vitest";
import { clipsInBox, deletePieces, deletePoint, dropAt, duplicateClips, groupMoveDelta, insertPieces, insertPoint, moveClips, pasteClips, snapGrid, snapTo, toggleId, toggleMuted, trimClip, windowPiece } from "./clipEditing";
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
  it("lands on the grid slot under the pointer, inside the song only", () => {
    expect(dropAt(9.9 * B, 4 * B, 64 * B)).toBe(8 * B);
    expect(dropAt(0, 4 * B, 64 * B)).toBe(0);
    expect(dropAt(63.9 * B, 4 * B, 64 * B)).toBe(60 * B);
    expect(dropAt(64 * B, 4 * B, 64 * B)).toBeNull();
    expect(dropAt(-B, 4 * B, 64 * B)).toBeNull();
  });

  it("drops on the exact tick when snap is off", () => {
    expect(dropAt(9.9 * B, snapGrid("off", 4 * B), 64 * B)).toBe(Math.floor(9.9 * B));
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

  it("resolves the setting against the project's bar", () => {
    expect(snapGrid("bar", 3 * B)).toBe(3 * B);
    expect(snapGrid(B / 2, 4 * B)).toBe(B / 2);
    expect(snapGrid("off", 4 * B)).toBe(1);
    expect(snapGrid("bar", 4 * B, true)).toBe(1); // ⇧ bypasses whatever is set
  });

  it("snapping to one tick leaves a position alone", () => {
    const grid = snapGrid("off", 4 * B);
    expect(snapTo(1234.4, grid)).toBe(1234);
    const a = c("a", 4, 4);
    expect(groupMoveDelta(a, [a], 101.2, grid, 64 * B)).toBe(101);
  });
});

describe("mute", () => {
  it("silences a mixed selection, then brings it all back", () => {
    const group = [c("a", 0, 4), { ...c("b", 8, 4), muted: true }];
    const muted = toggleMuted(group);
    expect(muted.map((x) => x.muted)).toEqual([true, true]);
    expect(toggleMuted(muted).map((x) => x.muted)).toEqual([false, false]);
  });

  it("is empty for an empty group", () => {
    expect(toggleMuted([])).toEqual([]);
  });
});

describe("clipboard", () => {
  it("pastes the group with its shape, earliest clip at the playhead", () => {
    const group = [c("a", 8, 4, "x"), c("b", 12, 4, "y")];
    expect(pasteClips(group, 32 * B, 64 * B)).toEqual([
      { src: group[0], startTick: 32 * B },
      { src: group[1], startTick: 36 * B },
    ]);
  });

  it("keeps each clip on its own pattern, so copies stay on their tracks", () => {
    const group = [c("a", 0, 4, "x"), c("b", 0, 4, "y")];
    expect(pasteClips(group, 16 * B, 64 * B).map((x) => x.src.patternId)).toEqual(["x", "y"]);
  });

  it("drops copies that would land outside the song, and pastes nothing from nothing", () => {
    const group = [c("a", 0, 4), c("b", 40, 4)];
    expect(pasteClips(group, 32 * B, 64 * B).map((x) => x.startTick)).toEqual([32 * B]);
    expect(pasteClips(group, -99 * B, 64 * B).map((x) => x.startTick)).toEqual([0, 40 * B]);
    expect(pasteClips([], 0, 64 * B)).toEqual([]);
  });

  it("duplicates one group-span later so repeats march along", () => {
    const group = [c("a", 0, 4), c("b", 4, 4)];
    const once = duplicateClips(group, 64 * B);
    expect(once.map((x) => x.startTick)).toEqual([8 * B, 12 * B]);
    const twice = duplicateClips(once.map((x, i) => ({ ...x.src, id: `n${i}`, startTick: x.startTick })), 64 * B);
    expect(twice.map((x) => x.startTick)).toEqual([16 * B, 20 * B]);
  });

  it("duplicates nothing past the song end", () => {
    expect(duplicateClips([c("a", 60, 4)], 64 * B)).toEqual([]);
    expect(duplicateClips([], 64 * B)).toEqual([]);
  });
});

describe("ripple", () => {
  const piece = (start: number, skip: number, len: number) => ({ start: start * B, skip: skip * B, len: len * B });

  it("moves a point only when the time opens at or before it", () => {
    expect(insertPoint(4 * B, 8 * B, 4 * B)).toBe(4 * B);
    expect(insertPoint(8 * B, 8 * B, 4 * B)).toBe(12 * B);
    expect(insertPoint(20 * B, 8 * B, 4 * B)).toBe(24 * B);
  });

  it("collapses a point inside a cut onto the cut's start", () => {
    expect(deletePoint(4 * B, 8 * B, 4 * B)).toBe(4 * B);
    expect(deletePoint(8 * B, 8 * B, 4 * B)).toBe(8 * B);
    expect(deletePoint(10 * B, 8 * B, 4 * B)).toBe(8 * B);
    expect(deletePoint(12 * B, 8 * B, 4 * B)).toBe(8 * B);
    expect(deletePoint(20 * B, 8 * B, 4 * B)).toBe(16 * B);
  });

  it("leaves material before the cut alone and moves material after it whole", () => {
    expect(insertPieces(0, 8 * B, 8 * B, 4 * B)).toEqual([piece(0, 0, 8)]);
    expect(insertPieces(8 * B, 8 * B, 8 * B, 4 * B)).toEqual([piece(12, 0, 8)]);
    expect(deletePieces(0, 8 * B, 8 * B, 4 * B)).toEqual([piece(0, 0, 8)]);
    expect(deletePieces(16 * B, 8 * B, 8 * B, 4 * B)).toEqual([piece(12, 0, 8)]);
  });

  it("splits what the cut falls inside, so both halves play what they played", () => {
    expect(insertPieces(0, 16 * B, 4 * B, 4 * B)).toEqual([piece(0, 0, 4), piece(8, 4, 12)]);
    expect(deletePieces(0, 16 * B, 4 * B, 4 * B)).toEqual([piece(0, 0, 4), piece(4, 8, 8)]);
  });

  it("drops material a delete swallows whole", () => {
    expect(deletePieces(4 * B, 4 * B, 4 * B, 8 * B)).toEqual([]);
    expect(deletePieces(4 * B, 0, 0, 8 * B)).toEqual([]);
    expect(insertPieces(4 * B, 0, 0, 8 * B)).toEqual([]);
  });

  it("takes only the part of a span that overlaps the window it copies", () => {
    expect(windowPiece(0, 16 * B, 4 * B, 8 * B, 32 * B)).toEqual(piece(32, 4, 4));
    expect(windowPiece(4 * B, 4 * B, 4 * B, 8 * B, 32 * B)).toEqual(piece(32, 0, 4));
    expect(windowPiece(0, 4 * B, 4 * B, 8 * B, 32 * B)).toBeNull();
  });
});
