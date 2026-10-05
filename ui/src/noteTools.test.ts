import { describe, expect, it } from "vitest";
import {
  arpeggiateNotes,
  chopNotes,
  humanizeNotes,
  legatoNotes,
  magnetPitch,
  quantizeNotes,
  resolveTargets,
  scaleNotes,
  seededRng,
  stampChord,
  strumNotes,
} from "./noteEditing";
import { explainNote, progression, tierMap } from "./theory";
import { swingWarp, type NoteEvent } from "./types";

const n = (id: string, start: number, len: number, pitch: number, velocity = 100): NoteEvent => ({ id, startBeat: start, lengthBeats: len, pitch, velocity });
const all = new Set<string>();

describe("swing warp", () => {
  it("delays off-subdivisions, keeps downbeats, and is the identity at 50", () => {
    expect(swingWarp(0.5, 50, 0.5)).toBe(0.5);
    expect(swingWarp(0.5, 200 / 3, 0.5)).toBeCloseTo(2 / 3, 9);
    expect(swingWarp(1, 66, 0.5)).toBe(1);
    expect(swingWarp(0.25, 75, 0.25)).toBeCloseTo(0.375, 9);
    expect(swingWarp(0.5, 90, 0.5)).toBe(0.5);
  });
});

describe("transform tools", () => {
  it("scales a selection around its first note", () => {
    const notes = [n("a", 1, 0.5, 60), n("b", 2, 0.5, 62), n("c", 4, 1, 64)];
    const out = scaleNotes(notes, new Set(["a", "b"]), 2);
    expect(out[0]).toMatchObject({ startBeat: 1, lengthBeats: 1 });
    expect(out[1]).toMatchObject({ startBeat: 3, lengthBeats: 1 });
    expect(out[2]).toBe(notes[2]);
    const back = scaleNotes(out, new Set(["a", "b"]), 0.5);
    expect(back[1]).toMatchObject({ startBeat: 2, lengthBeats: 0.5 });
    expect(scaleNotes(notes, all, 1)).toBe(notes);
  });

  it("quantizes starts and lengths to the grid, with strength", () => {
    const out = quantizeNotes([n("a", 0.3, 0.6, 60)], all, 0.5);
    expect(out[0]).toMatchObject({ startBeat: 0.5, lengthBeats: 0.5 });
    const half = quantizeNotes([n("a", 0.3, 0.5, 60)], all, 0.5, 0.5);
    expect(half[0].startBeat).toBeCloseTo(0.4);
  });

  it("humanizes chord tones less than other notes", () => {
    const notes = [n("ct", 0, 1, 60), n("tn", 1, 1, 62)];
    const out = humanizeNotes(notes, all, { timing: 0.1, velocity: 20 }, (x) => (x.id === "ct" ? "chordTone" : "tension"), seededRng(1));
    expect(Math.abs(out[0].startBeat - 0)).toBeLessThanOrEqual(0.034);
    expect(Math.abs(out[1].startBeat - 1)).toBeLessThanOrEqual(0.1);
    expect(out.every((x) => x.velocity >= 1 && x.velocity <= 127)).toBe(true);
  });

  it("arpeggiates a stack into grid steps across its span", () => {
    const chord = [n("c", 0, 2, 60), n("e", 0, 2, 64), n("g", 0, 2, 67)];
    const out = arpeggiateNotes(chord, all, 0.5, "up");
    expect(out).toHaveLength(4);
    expect(out.map((x) => x.pitch)).toEqual([60, 64, 67, 60]);
    expect(out.map((x) => x.startBeat)).toEqual([0, 0.5, 1, 1.5]);
    expect(arpeggiateNotes(chord, all, 0.5, "down")[0].pitch).toBe(67);
  });

  it("strums a stack and keeps the end", () => {
    const out = strumNotes([n("c", 0, 1, 60), n("e", 0, 1, 64), n("g", 0, 1, 67)], all, 0.1);
    const byPitch = Object.fromEntries(out.map((x) => [x.pitch, x]));
    expect(byPitch[60].startBeat).toBe(0);
    expect(byPitch[64].startBeat).toBeCloseTo(0.1);
    expect(byPitch[67].startBeat).toBeCloseTo(0.2);
    expect(byPitch[67].startBeat + byPitch[67].lengthBeats).toBeCloseTo(1);
  });

  it("chops long notes and leaves short ones", () => {
    const out = chopNotes([n("a", 0, 1, 60), n("b", 2, 0.25, 62)], all, 0.25);
    expect(out).toHaveLength(5);
    expect(out[0].id).toBe("a");
  });

  it("legato extends to the next note's start", () => {
    const out = legatoNotes([n("a", 0, 0.25, 60), n("b", 1, 0.25, 62), n("c", 3, 0.25, 64)], all);
    expect(out[0].lengthBeats).toBe(1);
    expect(out[1].lengthBeats).toBe(2);
    expect(out[2].lengthBeats).toBe(0.25);
  });

  it("stamps a chord upward from the base pitch, skipping clashes", () => {
    const added = stampChord([n("x", 0, 1, 64)], 0.3, [0, 4, 7], 60, 1, 0.25);
    expect(added.map((a) => a.pitch).sort()).toEqual([60, 67]);
    expect(added[0].startBeat).toBe(0.25);
    expect(stampChord([], 0, [9, 0, 4], 62, 1, 1).map((a) => a.pitch).sort()).toEqual([64, 69, 72]);
  });
});

describe("magnet, resolve, explain", () => {
  const chords = progression([0], 0, "major", 4, 4); // C major
  const tiers = tierMap(0, chords, { rootPitchClass: 0, scale: "major", isLocked: true });

  it("pulls to the nearest chord tone, then tension", () => {
    expect(magnetPitch(65, tiers)).toBe(64); // F → E
    expect(magnetPitch(60, tiers)).toBe(60);
    expect(magnetPitch(61, tiers)).toBe(60);
  });

  it("finds resolve targets", () => {
    expect(resolveTargets(65, tiers)).toEqual({ down: 64, up: 67 });
  });

  it("explains in plain words", () => {
    const key = { rootPitchClass: 0, scale: "major" as const, isLocked: true };
    expect(explainNote(60, chords, key, 0)).toBe("C over C: the root. Solid.");
    expect(explainNote(62, chords, key, 0)).toBe("D over C: the 9th. Spicy but good.");
    expect(explainNote(65, chords, key, 0)).toContain("a half step above E");
    expect(explainNote(66, chords, key, 0)).toContain("a half step below G");
    expect(explainNote(70, chords, key, 0)).toContain("outside the key");
    expect(explainNote(60, [], key, 0)).toContain("is in C maj");
    expect(explainNote(60, [], { rootPitchClass: null, scale: "major", isLocked: false }, 0)).toContain("anything goes");
  });
});
