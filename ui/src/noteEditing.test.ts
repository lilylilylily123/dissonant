import { describe, expect, it } from "vitest";
import {
  adjustVelocity,
  duplicateNotes,
  moveNotes,
  noteAt,
  notesInRect,
  pasteNotes,
  placeNote,
  resizeNotes,
  snapFloor,
  snapRound,
} from "./noteEditing";
import { TICKS_PER_BEAT, type NoteEvent } from "./types";

const n = (id: string, start: number, len: number, pitch: number, velocity = 100): NoteEvent => ({
  id,
  startTick: start,
  lengthTicks: len,
  pitch,
  velocity,
});

describe("placement", () => {
  it("snaps to the grid and refuses overlaps on the same pitch", () => {
    const notes = [n("a", 0, 960, 60)];
    expect(placeNote(notes, 672, 60, 960, 480)).toBeNull();
    const placed = placeNote(notes, 1248, 60, 480, 480);
    expect(placed?.startTick).toBe(960);
    expect(placed?.lengthTicks).toBe(480);
    expect(placeNote(notes, 192, 62, 960, 960)?.pitch).toBe(62);
  });

  it("snapFloor is stable at grid boundaries", () => {
    expect(snapFloor(959.9999999999, 240)).toBe(960);
    expect(snapFloor(1190, 240)).toBe(960);
  });

  it("snaps exactly on a triplet grid, where beat-fraction floats drifted", () => {
    const g = TICKS_PER_BEAT / 3; // 320: eighth-note triplets, the grid 1/3 of a beat never divided cleanly
    for (let i = 0; i < 24; i++) {
      const onGrid = i * g;
      expect(snapFloor(onGrid, g)).toBe(onGrid); // never falls back into the previous cell
      expect(snapRound(onGrid, g)).toBe(onGrid);
      expect(snapFloor(onGrid, g) % g).toBe(0); // exact multiples, no half-tick residue
      expect(snapFloor(onGrid + 1, g)).toBe(onGrid); // just inside the cell stays in it
      expect(snapFloor(onGrid + g - 1, g)).toBe(onGrid);
      expect(snapRound(onGrid + 159, g)).toBe(onGrid); // under half a cell rounds back
      expect(snapRound(onGrid + 161, g)).toBe(onGrid + g); // over half a cell rounds on
    }
    expect(snapFloor(4 * TICKS_PER_BEAT, g)).toBe(3840); // 12 triplets to the 4/4 bar, no drift
    expect(snapRound(4 * TICKS_PER_BEAT - 1, g)).toBe(3840);
  });

  it("finds the note under a point, preferring the latest", () => {
    const notes = [n("a", 0, 1920, 60), n("b", 960, 960, 60)];
    expect(noteAt(notes, 1440, 60)?.id).toBe("b");
    expect(noteAt(notes, 480, 60)?.id).toBe("a");
    expect(noteAt(notes, 480, 61)).toBeNull();
  });
});

describe("move", () => {
  const notes = [n("a", 960, 960, 60), n("b", 2880, 960, 64), n("c", 7680, 960, 50)];

  it("moves the whole selection together", () => {
    const out = moveNotes(notes, new Set(["a", "b"]), 1920, -1, { min: 0, max: 127 });
    expect(out.find((x) => x.id === "a")).toMatchObject({ startTick: 2880, pitch: 59 });
    expect(out.find((x) => x.id === "b")).toMatchObject({ startTick: 4800, pitch: 63 });
    expect(out.find((x) => x.id === "c")).toMatchObject({ startTick: 7680, pitch: 50 });
  });

  it("clamps so nothing goes before tick 0 or outside the pitch range", () => {
    const out = moveNotes(notes, new Set(["a", "b"]), -4800, 70, { min: 24, max: 84 });
    expect(out.find((x) => x.id === "a")!.startTick).toBe(0);
    expect(out.find((x) => x.id === "b")!.startTick).toBe(1920);
    expect(out.find((x) => x.id === "b")!.pitch).toBe(84);
    expect(out.find((x) => x.id === "a")!.pitch).toBe(80);
  });

  it("respects a maximum end", () => {
    const out = moveNotes(notes, new Set(["c"]), 9600, 0, { min: 0, max: 127 }, 15360);
    expect(out.find((x) => x.id === "c")!.startTick).toBe(14400);
  });

  it("returns the same array when nothing changes", () => {
    expect(moveNotes(notes, new Set(["zzz"]), 960, 1, { min: 0, max: 127 })).toBe(notes);
  });
});

describe("resize / select / velocity", () => {
  const notes = [n("a", 0, 960, 60), n("b", 1920, 1920, 62)];

  it("resizes with a minimum length", () => {
    const out = resizeNotes(notes, new Set(["a", "b"]), -4800, 240);
    expect(out.every((x) => x.lengthTicks === 240)).toBe(true);
  });

  it("marquee selects overlapping notes in any drag direction", () => {
    expect([...notesInRect(notes, 2880, 480, 63, 60)].sort()).toEqual(["a", "b"]);
    expect([...notesInRect(notes, 960, 1824, 60, 62)]).toEqual([]);
  });

  it("clamps velocity", () => {
    const out = adjustVelocity(notes, new Set(["a"]), 200);
    expect(out[0].velocity).toBe(127);
    expect(adjustVelocity(out, new Set(["a"]), -500)[0].velocity).toBe(1);
  });
});

describe("duplicate / paste", () => {
  it("duplicates right after the selection on the grid with fresh ids", () => {
    const notes = [n("a", 0, 960, 60), n("b", 960, 480, 62)];
    const { notes: out, newIds } = duplicateNotes(notes, new Set(["a", "b"]), 480);
    expect(out).toHaveLength(4);
    expect(newIds.size).toBe(2);
    const copies = out.filter((x) => newIds.has(x.id)).sort((p, q) => p.startTick - q.startTick);
    expect(copies[0]).toMatchObject({ startTick: 1440, pitch: 60 });
    expect(copies[1]).toMatchObject({ startTick: 2400, pitch: 62 });
  });

  it("pastes relative to the earliest clipboard note", () => {
    const clip = [n("x", 3840, 960, 60), n("y", 4800, 960, 64)];
    const { notes: out } = pasteNotes([], clip, 7680);
    expect(out.map((x) => x.startTick).sort()).toEqual([7680, 8640]);
    expect(out.every((x) => x.id !== "x" && x.id !== "y")).toBe(true);
  });
});
