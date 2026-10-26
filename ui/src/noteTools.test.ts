import { describe, expect, it } from "vitest";
import {
  arpeggiateNotes,
  chopNotes,
  clearRow,
  duplicateBySpan,
  euclideanRow,
  fillEverySteps,
  glueNotes,
  humanizeNotes,
  invertNotes,
  nudgeNoteProp,
  quantizeNoteProp,
  rampNoteProp,
  rampVelocity,
  randomizeRow,
  reverseNotes,
  rotateRow,
  setLength,
  setNoteProp,
  splitNotes,
  stepIndexOf,
  stepSelection,
  toggleMute,
  legatoNotes,
  magnetPitch,
  quantizeNotes,
  resolveTargets,
  scaleNotes,
  seededRng,
  stampChord,
  strumNotes,
  type StepGrid,
} from "./noteEditing";
import { explainNote, progression, tierMap } from "./theory";
import { swingWarp, type NoteEvent } from "./types";

const n = (id: string, start: number, len: number, pitch: number, velocity = 100): NoteEvent => ({ id, startTick: start, lengthTicks: len, pitch, velocity });
const all = new Set<string>();

describe("swing warp", () => {
  it("delays off-subdivisions, keeps downbeats, and is the identity at 50", () => {
    expect(swingWarp(480, 50, 480)).toBe(480);
    expect(swingWarp(480, 200 / 3, 480)).toBeCloseTo(640, 6);
    expect(swingWarp(960, 66, 480)).toBe(960);
    expect(swingWarp(240, 75, 240)).toBeCloseTo(360, 6);
    expect(swingWarp(480, 90, 480)).toBe(480);
  });
});

describe("transform tools", () => {
  it("scales a selection around its first note", () => {
    const notes = [n("a", 960, 480, 60), n("b", 1920, 480, 62), n("c", 3840, 960, 64)];
    const out = scaleNotes(notes, new Set(["a", "b"]), 2);
    expect(out[0]).toMatchObject({ startTick: 960, lengthTicks: 960 });
    expect(out[1]).toMatchObject({ startTick: 2880, lengthTicks: 960 });
    expect(out[2]).toBe(notes[2]);
    const back = scaleNotes(out, new Set(["a", "b"]), 0.5);
    expect(back[1]).toMatchObject({ startTick: 1920, lengthTicks: 480 });
    expect(scaleNotes(notes, all, 1)).toBe(notes);
  });

  it("quantizes starts and lengths to the grid, with strength", () => {
    const out = quantizeNotes([n("a", 288, 576, 60)], all, 480);
    expect(out[0]).toMatchObject({ startTick: 480, lengthTicks: 480 });
    const half = quantizeNotes([n("a", 288, 480, 60)], all, 480, 0.5);
    expect(half[0].startTick).toBe(384); // halfway to the grid, still a whole tick
  });

  it("humanizes chord tones less than other notes", () => {
    const notes = [n("ct", 0, 960, 60), n("tn", 960, 960, 62)];
    const out = humanizeNotes(notes, all, { timing: 96, velocity: 20 }, (x) => (x.id === "ct" ? "chordTone" : "tension"), seededRng(1));
    expect(Math.abs(out[0].startTick - 0)).toBeLessThanOrEqual(32);
    expect(Math.abs(out[1].startTick - 960)).toBeLessThanOrEqual(96);
    expect(out.every((x) => Number.isInteger(x.startTick))).toBe(true);
    expect(out.every((x) => x.velocity >= 1 && x.velocity <= 127)).toBe(true);
  });

  it("arpeggiates a stack into grid steps across its span", () => {
    const chord = [n("c", 0, 1920, 60), n("e", 0, 1920, 64), n("g", 0, 1920, 67)];
    const out = arpeggiateNotes(chord, all, 480, "up");
    expect(out).toHaveLength(4);
    expect(out.map((x) => x.pitch)).toEqual([60, 64, 67, 60]);
    expect(out.map((x) => x.startTick)).toEqual([0, 480, 960, 1440]);
    expect(arpeggiateNotes(chord, all, 480, "down")[0].pitch).toBe(67);
  });

  it("strums a stack and keeps the end", () => {
    const out = strumNotes([n("c", 0, 960, 60), n("e", 0, 960, 64), n("g", 0, 960, 67)], all, 96);
    const byPitch = Object.fromEntries(out.map((x) => [x.pitch, x]));
    expect(byPitch[60].startTick).toBe(0);
    expect(byPitch[64].startTick).toBe(96);
    expect(byPitch[67].startTick).toBe(192);
    expect(byPitch[67].startTick + byPitch[67].lengthTicks).toBe(960);
  });

  it("chops long notes and leaves short ones", () => {
    const out = chopNotes([n("a", 0, 960, 60), n("b", 1920, 240, 62)], all, 240);
    expect(out).toHaveLength(5);
    expect(out[0].id).toBe("a");
  });

  it("legato extends to the next note's start", () => {
    const out = legatoNotes([n("a", 0, 240, 60), n("b", 960, 240, 62), n("c", 2880, 240, 64)], all);
    expect(out[0].lengthTicks).toBe(960);
    expect(out[1].lengthTicks).toBe(1920);
    expect(out[2].lengthTicks).toBe(240);
  });

  it("stamps a chord upward from the base pitch, skipping clashes", () => {
    const added = stampChord([n("x", 0, 960, 64)], 288, [0, 4, 7], 60, 960, 240);
    expect(added.map((a) => a.pitch).sort()).toEqual([60, 67]);
    expect(added[0].startTick).toBe(240);
    expect(stampChord([], 0, [9, 0, 4], 62, 960, 960).map((a) => a.pitch).sort()).toEqual([64, 69, 72]);
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

describe("roll editing commands", () => {
  it("splits straddling notes at a tick and glues touching same-pitch notes", () => {
    const notes = [n("a", 0, 1920, 60), n("b", 0, 960, 62)];
    const split = splitNotes(notes, all, 960);
    expect(split.length).toBe(3);
    expect(split[0]).toMatchObject({ id: "a", startTick: 0, lengthTicks: 960 });
    expect(split[1]).toMatchObject({ startTick: 960, lengthTicks: 960, pitch: 60 });
    expect(split[2]).toBe(notes[1]); // not straddling: untouched
    const glued = glueNotes(split, all);
    expect(glued.length).toBe(2);
    expect(glued.find((x) => x.id === "a")).toMatchObject({ startTick: 0, lengthTicks: 1920 });
    // A gap keeps notes apart.
    expect(glueNotes([n("a", 0, 480, 60), n("b", 960, 480, 60)], all).length).toBe(2);
  });

  it("duplicates by span, reverses in time and inverts pitch", () => {
    const notes = [n("a", 0, 960, 60), n("b", 960, 480, 64), n("c", 2880, 960, 67)];
    const dup = duplicateBySpan(notes, all, 3840);
    expect(dup.notes.length).toBe(6);
    expect(dup.notes[3]).toMatchObject({ startTick: 3840, pitch: 60 });
    const rev = reverseNotes(notes, all);
    expect(rev.map((x) => x.startTick)).toEqual([2880, 2400, 0]);
    const inv = invertNotes(notes, all, { min: 0, max: 127 });
    expect(inv.map((x) => x.pitch)).toEqual([67, 63, 60]);
  });

  it("sets length, toggles mute and ramps velocity", () => {
    const notes = [n("a", 0, 960, 60), n("b", 960, 960, 62), n("c", 1920, 960, 64), n("d", 4800, 960, 65)];
    expect(setLength(notes, new Set(["a"]), 240)[0].lengthTicks).toBe(240);
    const muted = toggleMute(notes, new Set(["a", "b"]));
    expect(muted[0].muted && muted[1].muted && !muted[2].muted).toBe(true);
    expect(toggleMute(muted, new Set(["a", "b"]))[0].muted).toBe(false);
    const ramp = rampVelocity(notes, all, 0, 20, 1920, 120);
    expect(ramp.map((x) => x.velocity)).toEqual([20, 70, 120, 100]);
    expect(rampVelocity(notes, all, 1920, 120, 0, 20)[0].velocity).toBe(20); // direction-agnostic
  });

  it("steps the selection through notes in time order", () => {
    const notes = [n("b", 960, 960, 62), n("a", 0, 960, 60), n("c", 1920, 960, 64)];
    expect(stepSelection(notes, new Set(), 1)).toBe("a");
    expect(stepSelection(notes, new Set(["a"]), 1)).toBe("b");
    expect(stepSelection(notes, new Set(["c"]), 1)).toBe("a");
    expect(stepSelection(notes, new Set(["a"]), -1)).toBe("c");
  });
});

describe("note properties", () => {
  const notes = [n("a", 0, 960, 60), n("b", 960, 960, 62), n("c", 1920, 960, 64)];
  const sel = (...ids: string[]) => new Set(ids);

  it("clamps every property to its own range at both ends", () => {
    expect(setNoteProp(notes, sel("a"), "pan", 5)[0].pan).toBe(1);
    expect(setNoteProp(notes, sel("a"), "pan", -5)[0].pan).toBe(0);
    expect(setNoteProp(notes, sel("a"), "fine", 9999)[0].fine).toBe(1200);
    expect(setNoteProp(notes, sel("a"), "fine", -9999)[0].fine).toBe(-1200);
    expect(setNoteProp(notes, sel("a"), "velocity", 500)[0].velocity).toBe(127);
    expect(setNoteProp(notes, sel("a"), "velocity", -500)[0].velocity).toBe(1);
    expect(setNoteProp(notes, sel("a"), "shiftTicks", 9000)[0].shiftTicks).toBe(480);
    expect(setNoteProp(notes, sel("a"), "shiftTicks", -9000)[0].shiftTicks).toBe(-480);
    expect(setNoteProp(notes, sel("a"), "release", 3)[0].release).toBe(1);
    expect(setNoteProp(notes, sel("a"), "pan", 1)[1]).toBe(notes[1]); // unselected notes untouched
  });

  it("quantises a raw lane value to the property's grid, for gesture endpoints", () => {
    expect(quantizeNoteProp("velocity", 140)).toBe(127);
    expect(quantizeNoteProp("velocity", 83.6)).toBe(84);
    expect(quantizeNoteProp("fine", -1500.2)).toBe(-1200);
    expect(quantizeNoteProp("repeats", 2.5)).toBe(3);
    expect(quantizeNoteProp("pan", 0.3125)).toBe(0.3125); // continuous props keep their precision
    expect(quantizeNoteProp("shiftTicks", 2000)).toBe(480);
    expect(quantizeNoteProp("shiftTicks", 12.4)).toBe(12); // ticks are whole numbers
    // The ramp's interior is the line between its quantised ends.
    const ends = [quantizeNoteProp("velocity", 140), quantizeNoteProp("velocity", 20.4)];
    expect(rampNoteProp(notes, sel("a", "c"), "velocity", ends[0], ends[1]).map((x) => x.velocity)).toEqual([127, 100, 20]);
  });

  it("rounds the integer-backed props and leaves the continuous ones exact", () => {
    expect(setNoteProp(notes, sel("a"), "repeats", 3.6)[0].repeats).toBe(4);
    const five = setNoteProp(notes, sel("a"), "repeats", 5);
    expect(setNoteProp(five, sel("a"), "repeats", 0.4)[0].repeats).toBe(0); // rounds down, off again
    expect(setNoteProp(notes, sel("a"), "repeats", 20)[0].repeats).toBe(14);
    expect(setNoteProp(notes, sel("a"), "velocity", 101.6)[0].velocity).toBe(102);
    expect(setNoteProp(notes, sel("a"), "fine", 12.4)[0].fine).toBe(12);
    expect(setNoteProp(notes, sel("a"), "modX", 0.123)[0].modX).toBe(0.123);
    expect(setNoteProp(notes, sel("a"), "shiftTicks", -60.4)[0].shiftTicks).toBe(-60);
  });

  it("nudges from each note's own value and keeps the array when nothing moves", () => {
    const start = setNoteProp(notes, sel("a", "b"), "pan", 0.4);
    const up = nudgeNoteProp(start, sel("a", "b"), "pan", 0.25);
    expect(up[0].pan).toBeCloseTo(0.65, 9);
    expect(up[1].pan).toBeCloseTo(0.65, 9);
    expect(nudgeNoteProp(notes, sel("c"), "pan", 0.2)[2].pan).toBeCloseTo(0.7, 9); // unset: from neutral
    expect(nudgeNoteProp(notes, sel("a"), "pan", 0)).toBe(notes);
    expect(nudgeNoteProp(notes, new Set<string>(), "pan", 0.2)).toBe(notes);
    expect(setNoteProp(notes, sel("a"), "velocity", 100)).toBe(notes); // already there
    expect(setNoteProp(notes, sel("a"), "pan", 0.5)[0].pan).toBeUndefined(); // neutral stays unset
    const maxed = setNoteProp(notes, sel("a"), "repeats", 14);
    expect(nudgeNoteProp(maxed, sel("a"), "repeats", 3)).toBe(maxed); // clamped at the ceiling
  });

  it("ramps from the earliest selected note to the latest, in time not array order", () => {
    const shuffled = [n("c", 1920, 960, 64), n("a", 0, 960, 60), n("b", 960, 960, 62)];
    const out = rampNoteProp(shuffled, sel("a", "b", "c"), "velocity", 20, 120);
    const by = Object.fromEntries(out.map((x) => [x.id, x.velocity]));
    expect([by.a, by.b, by.c]).toEqual([20, 70, 120]); // same line rampVelocity draws
  });

  it("spaces the ramp by tick distance, not by note count", () => {
    const uneven = [n("a", 0, 960, 60), n("b", 2880, 960, 62), n("c", 3840, 960, 64)];
    expect(rampNoteProp(uneven, sel("a", "b", "c"), "modX", 0, 1).map((x) => x.modX)).toEqual([0, 0.75, 1]);
    expect(rampNoteProp(uneven, sel("b"), "modX", 0, 0.25)[1].modX).toBe(0.25); // a lone note takes the far end
    expect(rampNoteProp(uneven, new Set<string>(), "modX", 0, 1)).toBe(uneven);
    expect(rampNoteProp(uneven, sel("zzz"), "modX", 0, 1)).toBe(uneven);
  });
});

describe("step rows", () => {
  const grid: StepGrid = { pitch: 36, stepTicks: 240, steps: 16 };
  const hit = (step: number, extra: Partial<NoteEvent> = {}): NoteEvent => ({
    id: `s${step}`,
    startTick: step * grid.stepTicks,
    lengthTicks: grid.stepTicks,
    pitch: grid.pitch,
    velocity: 100,
    ...extra,
  });
  const other = n("snare", 480, 240, 38);
  const offGrid = n("ghost", 288, 240, 36);
  const past = n("late", 3840, 240, 36); // 16 × 240 = the next turn
  const steps = (out: NoteEvent[]) => out.map((x) => stepIndexOf(x, grid)).filter((i) => i !== null);

  it("locates a note's step and rejects anything that is not one", () => {
    expect(stepIndexOf(hit(5), grid)).toBe(5);
    expect(stepIndexOf(other, grid)).toBeNull(); // another pitch
    expect(stepIndexOf(offGrid, grid)).toBeNull(); // between steps
    expect(stepIndexOf(past, grid)).toBeNull(); // past the turn
  });

  it("fills each 4th step and nothing else, keeping the rest of the pattern", () => {
    const out = fillEverySteps([other, offGrid, past], grid, 4, 0, 100);
    expect(steps(out)).toEqual([0, 4, 8, 12]);
    expect(out.filter((x) => x.pitch === 36 && x.lengthTicks === 240 && stepIndexOf(x, grid) !== null).map((x) => x.startTick)).toEqual([0, 960, 1920, 2880]);
    expect(out[0]).toBe(other); // other pitch, off-grid and next-turn notes survive, in order
    expect(out[1]).toBe(offGrid);
    expect(out[2]).toBe(past);
    expect(steps(fillEverySteps([], grid, 4, 2, 100))).toEqual([2, 6, 10, 14]); // offset shifts the phase
    expect(steps(fillEverySteps([hit(1)], grid, 4, 0, 100))).toEqual([0, 4, 8, 12]); // replaces, never adds
  });

  it("gives new hits fresh ids and the asked velocity, but keeps a surviving hit's own", () => {
    const out = fillEverySteps([], grid, 8, 0, 72);
    expect(out.map((x) => x.velocity)).toEqual([72, 72]);
    expect(new Set(out.map((x) => x.id)).size).toBe(2);
    expect(out.every((x) => x.pitch === 36 && x.id !== "")).toBe(true);
    const kept = fillEverySteps([hit(8, { velocity: 42, pan: 0.2 })], grid, 8, 0, 72);
    expect(kept.map((x) => x.velocity)).toEqual([72, 42]);
    expect(kept[1].pan).toBe(0.2);
  });

  it("clears only its own row inside the turn", () => {
    const out = clearRow([hit(0), other, hit(3), offGrid, past], grid);
    expect(out.map((x) => x.id)).toEqual(["snare", "ghost", "late"]);
    const empty = [other, offGrid, past];
    expect(clearRow(empty, grid)).toBe(empty);
  });

  it("rotates with wrap, carrying each hit's whole property set", () => {
    const notes = [hit(15, { velocity: 42, shiftTicks: -96, repeats: 3 }), hit(0, { velocity: 7 }), other];
    const out = rotateRow(notes, grid, 1);
    const moved = out.filter((x) => stepIndexOf(x, grid) !== null);
    expect(steps(out)).toEqual([0, 1]);
    expect(moved[0]).toMatchObject({ id: "s15", startTick: 0, velocity: 42, shiftTicks: -96, repeats: 3 }); // wrapped round
    expect(moved[1]).toMatchObject({ id: "s0", startTick: 240, velocity: 7 });
    expect(out.find((x) => x.id === "snare")).toBe(other);
    const back = rotateRow(out, grid, -1);
    expect(steps(back)).toEqual([0, 15]);
    expect(back.filter((x) => stepIndexOf(x, grid) !== null).map((x) => x.velocity)).toEqual([7, 42]);
    expect(rotateRow(notes, grid, 0)).toBe(notes);
    expect(rotateRow(notes, grid, 16)).toBe(notes); // a full turn is the identity
  });

  it("randomizes repeatably under a seeded rng, one draw per step", () => {
    const a = randomizeRow([other], grid, 0.5, 90, seededRng(7));
    const b = randomizeRow([other], grid, 0.5, 90, seededRng(7));
    expect(steps(a)).toEqual(steps(b));
    expect(steps(a).length).toBeGreaterThan(0);
    expect(steps(a).length).toBeLessThan(16);
    expect(a[0]).toBe(other);
    expect(steps(randomizeRow([], grid, 1, 90, seededRng(1)))).toEqual([...Array(16).keys()]);
    expect(steps(randomizeRow([hit(0)], grid, 0, 90, seededRng(1)))).toEqual([]);
    expect(randomizeRow([], grid, 1, 90, seededRng(1)).every((x) => x.velocity === 90)).toBe(true);
  });

  it("matches FL's own Bresenham euclid and rotates the pattern earlier", () => {
    // slope 5/16: int(i*slope) steps up at i = 0, 4, 7, 10, 13 — hand-walked, not snapshotted.
    expect(steps(euclideanRow([], grid, 5, 0, 100))).toEqual([0, 4, 7, 10, 13]);
    expect(steps(euclideanRow([], grid, 5, 1, 100))).toEqual([3, 6, 9, 12, 15]); // left by one
    expect(steps(euclideanRow([], grid, 4, 0, 100))).toEqual([0, 4, 8, 12]);
    expect(steps(euclideanRow([], grid, 3, 0, 100))).toEqual([0, 6, 11]);
    expect(steps(euclideanRow([], grid, 16, 0, 100))).toEqual([...Array(16).keys()]);
    expect(steps(euclideanRow([hit(2)], grid, 0, 0, 100))).toEqual([]); // no onsets, no hits
    expect(euclideanRow([other], grid, 5, 0, 100)[0]).toBe(other);
    expect(euclideanRow([], grid, 5, 0, 64).every((x) => x.velocity === 64 && x.lengthTicks === 240)).toBe(true);
  });
});
