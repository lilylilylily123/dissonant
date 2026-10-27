import { describe, expect, it } from "vitest";
import type { AutoTarget, Breakpoint, ChordEvent, Curve } from "./types";
import { autoTargetRange, automationValueAt, chordsInWindow, replaceChordWindow, sameAutoTarget, TICKS_PER_BEAT } from "./types";

// The model is in ticks; the cases read better in beats, so build and report in beats.
const ch = (id: string, startBeat: number, lengthBeats: number): ChordEvent => ({
  id,
  startTick: startBeat * TICKS_PER_BEAT,
  lengthTicks: lengthBeats * TICKS_PER_BEAT,
  pitchClasses: [0, 4, 7],
  name: id,
});
const spans = (cs: ChordEvent[]) =>
  cs.map((c) => [c.name, c.startTick / TICKS_PER_BEAT, c.lengthTicks / TICKS_PER_BEAT]);

describe("chordsInWindow", () => {
  const song = [ch("C", 0, 4), ch("F", 4, 4), ch("G", 8, 4), ch("Am", 12, 4)];

  it("shifts the window to 0 and clips chords at its edges", () => {
    expect(spans(chordsInWindow(song, 6 * TICKS_PER_BEAT, 8 * TICKS_PER_BEAT))).toEqual([
      ["F", 0, 2],
      ["G", 2, 4],
      ["Am", 6, 2],
    ]);
  });

  it("is the whole progression from 0", () => {
    expect(spans(chordsInWindow(song, 0, 16 * TICKS_PER_BEAT))).toEqual(spans(song));
    expect(chordsInWindow(song, 16 * TICKS_PER_BEAT, 8 * TICKS_PER_BEAT)).toEqual([]);
  });
});

describe("replaceChordWindow", () => {
  it("keeps what lies outside, trims chords crossing the edges, and places the new ones", () => {
    const song = [ch("C", 0, 8), ch("F", 8, 8)];
    const next = replaceChordWindow(song, 4 * TICKS_PER_BEAT, 8 * TICKS_PER_BEAT, [ch("G", 0, 8)]);
    expect(spans(next)).toEqual([
      ["C", 0, 4],
      ["G", 4, 8],
      ["F", 12, 4],
    ]);
  });

  it("splits a chord spanning the whole window into two with distinct ids", () => {
    const next = replaceChordWindow([ch("C", 0, 16)], 4 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, []);
    expect(spans(next)).toEqual([
      ["C", 0, 4],
      ["C", 8, 8],
    ]);
    expect(next[0].id).not.toBe(next[1].id);
  });

  it("round-trips with chordsInWindow", () => {
    const song = [ch("C", 0, 4), ch("F", 4, 4), ch("G", 8, 4)];
    expect(
      spans(replaceChordWindow(song, 4 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, chordsInWindow(song, 4 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT))),
    ).toEqual(spans(song));
  });
});

describe("automationValueAt", () => {
  const bp = (beat: number, value: number, curve: Curve): Breakpoint => ({
    id: `${beat}`,
    tick: beat * TICKS_PER_BEAT,
    value,
    curve,
  });
  const fade = [bp(0, 1, "step"), bp(4, 0, "ramp"), bp(8, 1, "step")];
  const at = (beat: number) => automationValueAt(fade, beat * TICKS_PER_BEAT);

  it("interpolates into a ramp and holds the previous value up to a step", () => {
    expect(at(0)).toBe(1);
    expect(at(2)).toBeCloseTo(0.5, 12);
    expect(at(4)).toBe(0);
    expect(at(6)).toBe(0);
    expect(at(8)).toBe(1);
  });

  it("holds the first value before the first point and the last past the last", () => {
    expect(at(99)).toBe(1);
    const late = [bp(4, 0.25, "ramp")];
    expect(automationValueAt(late, 0)).toBe(0.25);
    expect(automationValueAt(late, 10 * TICKS_PER_BEAT)).toBe(0.25);
  });

  it("is null for a lane with nothing to say", () => {
    expect(automationValueAt([], 0)).toBeNull();
  });
});

describe("automation targets", () => {
  const TRACK = "track-a";
  const OTHER = "track-b";

  it("compares a target by what it names, not by identity", () => {
    const volume: AutoTarget = { track: [TRACK, "volume"] };
    expect(sameAutoTarget(volume, { track: [TRACK, "volume"] })).toBe(true);
    expect(sameAutoTarget(volume, { track: [TRACK, "pan"] })).toBe(false);
    expect(sameAutoTarget(volume, { track: [OTHER, "volume"] })).toBe(false);
    expect(sameAutoTarget(volume, { master: "gain" })).toBe(false);
    expect(sameAutoTarget({ master: "gain" }, { master: "gain" })).toBe(true);
    expect(sameAutoTarget({ trackPlugin: [TRACK, 7] }, { trackPlugin: [TRACK, 7] })).toBe(true);
    expect(sameAutoTarget({ trackPlugin: [TRACK, 7] }, { trackPlugin: [TRACK, 8] })).toBe(false);
    const insert: AutoTarget = { insert: [{ track: TRACK }, "fx", 3] };
    expect(sameAutoTarget(insert, { insert: [{ track: TRACK }, "fx", 3] })).toBe(true);
    expect(sameAutoTarget(insert, { insert: [{ track: OTHER }, "fx", 3] })).toBe(false);
    expect(sameAutoTarget(insert, { insert: ["master", "fx", 3] })).toBe(false);
    expect(sameAutoTarget({ insert: ["master", "fx", 3] }, { insert: ["master", "fx", 3] })).toBe(true);
  });

  it("knows the range of every control it writes to, and that a plugin keeps its own", () => {
    expect(autoTargetRange({ track: [TRACK, "volume"] })).toEqual([0, 1.5]);
    expect(autoTargetRange({ track: [TRACK, "reverbSend"] })).toEqual([0, 1]);
    expect(autoTargetRange({ track: [TRACK, "tone"] })).toEqual([200, 20000]);
    expect(autoTargetRange({ track: [TRACK, "pan"] })).toEqual([-1, 1]);
    expect(autoTargetRange({ master: "gain" })).toEqual([0, 1.5]);
    expect(autoTargetRange({ trackPlugin: [TRACK, 7] })).toBeNull();
    expect(autoTargetRange({ insert: [{ track: TRACK }, "fx", 3] })).toBeNull();
  });
});
