import { describe, expect, it } from "vitest";
import type { ChordEvent } from "./types";
import { chordsInWindow, replaceChordWindow, TICKS_PER_BEAT } from "./types";

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
