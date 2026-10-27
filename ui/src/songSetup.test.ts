import { describe, expect, it } from "vitest";
import {
  barsForSeconds,
  barsText,
  buildChord,
  degreeOf,
  formatMinSec,
  gapAt,
  identifyChord,
  keySpans,
  lengthText,
  matchesWindow,
  nameOf,
  nextChords,
  parseMinSec,
  placeChord,
  QUALITIES,
  rootOfName,
  sectionDuplicate,
  sectionSwap,
  tileStarter,
} from "./songSetup";
import { tempoMap, TICKS_PER_BEAT as B, type ChordEvent, type Section } from "./types";

// Positions read as beats here; the model counts ticks, so everything is scaled by B.
const ch = (id: string, startBeats: number, lenBeats: number, pcs = [0, 4, 7]): ChordEvent => ({
  id,
  startTick: startBeats * B,
  lengthTicks: lenBeats * B,
  pitchClasses: pcs,
  name: null,
});
const beats = (c: ChordEvent) => [c.startTick / B, c.lengthTicks / B];
const q = (id: string) => QUALITIES.find((x) => x.id === id)!;

describe("building chords", () => {
  it("builds and names root + quality", () => {
    expect(buildChord(9, q("min"))).toEqual({ pitchClasses: [0, 4, 9], name: "Am" });
    expect(buildChord(7, q("7"))).toEqual({ pitchClasses: [2, 5, 7, 11], name: "G7" });
  });
  it("identifies what a set spells, preferring the named root", () => {
    expect(identifyChord([0, 4, 9])).toMatchObject({ root: 9, quality: { id: "min" } });
    expect(identifyChord([0, 4, 7, 11])?.quality.id).toBe("maj7");
    // {C, D, G} is both Csus2 and Gsus4.
    expect(nameOf([0, 2, 7], 0)).toBe("Csus2");
    expect(nameOf([0, 2, 7], 7)).toBe("Gsus4");
    expect(identifyChord([0, 1])).toBeNull();
    expect(nameOf([0, 1, 2])).toBe("C·C#·D");
  });
  it("reads a root from a name", () => {
    expect(rootOfName("F#m7")).toBe(6);
    expect(rootOfName("Bdim")).toBe(11);
    expect(rootOfName("—")).toBeNull();
    expect(rootOfName(null)).toBeNull();
  });
});

describe("placing chords", () => {
  it("trims, removes and splits what the new chord covers", () => {
    const song = [ch("a", 0, 4), ch("b", 4, 4), ch("c", 8, 8)];
    const out = placeChord(song, ch("n", 2, 8));
    expect(out.map((c) => [c.id, ...beats(c)])).toEqual([
      ["a", 0, 2],
      ["n", 2, 8],
      ["c", 10, 6],
    ]);
    const split = placeChord([ch("long", 0, 16)], ch("n", 4, 4));
    expect(split.map(beats)).toEqual([
      [0, 4],
      [4, 4],
      [8, 8],
    ]);
    // The right half of a split chord is a new chord.
    expect(new Set(split.map((c) => c.id)).size).toBe(3);
  });
  it("moving a chord replaces its old self", () => {
    const out = placeChord([ch("a", 0, 4), ch("b", 4, 4)], ch("a", 8, 4));
    expect(out.map((c) => [c.id, c.startTick / B])).toEqual([
      ["b", 4],
      ["a", 8],
    ]);
  });
  it("finds the gap around a tick", () => {
    const song = [ch("a", 0, 4), ch("b", 12, 4)];
    expect(gapAt(song, 6 * B, 32 * B)).toEqual({ start: 4 * B, end: 12 * B });
    expect(gapAt(song, 20 * B, 32 * B)).toEqual({ start: 16 * B, end: 32 * B });
    expect(gapAt([], 3 * B, 16 * B)).toEqual({ start: 0, end: 16 * B });
  });
});

describe("starters across the song", () => {
  const C = { rootPitchClass: 0, scale: "major" as const, isLocked: true };
  const sec = (startBeats: number, key: Section["key"]): Section => ({
    id: String(startBeats),
    name: "s",
    startTick: startBeats * B,
    key,
    color: null,
  });

  it("tiles the whole song in one key", () => {
    const spans = keySpans(C, [], 0, 64 * B);
    expect(spans).toEqual([{ start: 0, end: 64 * B, root: 0, scale: "major" }]);
    const chords = tileStarter([0, 3, 4, 5], spans, 4 * B);
    expect(chords).toHaveLength(16);
    expect(chords.map((c) => c.name).slice(0, 5)).toEqual(["C", "F", "G", "Am", "C"]);
    expect(chords[15].startTick + chords[15].lengthTicks).toBe(64 * B);
  });
  it("restarts in each section's own key, and merges sections that share one", () => {
    const sections = [sec(0, null), sec(16, null), sec(32, { rootPitchClass: 9, scale: "minor", isLocked: true })];
    const spans = keySpans(C, sections, 0, 48 * B);
    expect(spans).toEqual([
      { start: 0, end: 32 * B, root: 0, scale: "major" },
      { start: 32 * B, end: 48 * B, root: 9, scale: "minor" },
    ]);
    const chords = tileStarter([0, 5], spans, 8 * B);
    expect(chords.map((c) => [c.startTick / B, c.name])).toEqual([
      [0, "C"],
      [8, "Am"],
      [16, "C"],
      [24, "Am"],
      [32, "Am"],
      [40, "F"],
    ]);
  });
  it("tiles a bar range only, and knows when a window already holds it", () => {
    const spans = keySpans(C, [], 16 * B, 32 * B);
    const tiled = tileStarter([0, 4], spans, 4 * B);
    expect(tiled[0].startTick).toBe(16 * B);
    expect(tiled[tiled.length - 1].startTick + tiled[tiled.length - 1].lengthTicks).toBe(32 * B);
    expect(matchesWindow([ch("x", 0, 16), ...tiled], tiled, 16 * B, 32 * B)).toBe(true);
    expect(matchesWindow([ch("x", 0, 20)], tiled, 16 * B, 32 * B)).toBe(false);
  });
  it("an unset key falls back to C", () => {
    expect(keySpans({ rootPitchClass: null, scale: "minor", isLocked: false }, [], 0, 8 * B)[0]).toMatchObject({ root: 0, scale: "minor" });
  });
});

describe("next chords", () => {
  it("puts the usual moves first, in plain words", () => {
    const fromV = nextChords(4, 0, "major");
    expect(fromV).toHaveLength(6);
    expect(fromV.slice(0, 3).map((n) => [n.name, n.common])).toEqual([
      ["C", true],
      ["Am", true],
      ["F", true],
    ]);
    expect(fromV[0].why).toBe("resolve home");
    expect(fromV[1].why).toContain("deceptive");
    expect(fromV.some((n) => n.degree === 4)).toBe(false);
    expect(nextChords(null, 0, "major").map((n) => n.roman)).toEqual(["I", "ii", "iii", "IV", "V", "vi", "vii°"]);
  });
  it("knows a chord's degree in the key", () => {
    expect(degreeOf([2, 7, 11], 0, "major")).toBe(4);
    expect(degreeOf([1, 5, 8], 0, "major")).toBeNull();
  });
});

describe("song length", () => {
  it("formats and parses min:sec", () => {
    expect(formatMinSec(65.4)).toBe("1:05");
    expect(formatMinSec(0)).toBe("0:00");
    expect(parseMinSec("1:05")).toBe(65);
    expect(parseMinSec(" 2:30.5 ")).toBe(150.5);
    expect(parseMinSec("95")).toBe(95);
    expect(parseMinSec("1:75")).toBeNull();
    expect(parseMinSec("abc")).toBeNull();
  });
  it("converts seconds to the nearest whole bar along the tempo map", () => {
    const flat = tempoMap(120, []);
    // 4/4 at 120: 2 s per bar.
    expect(barsForSeconds(flat.secondsAt, 64, 4 * B)).toBe(32);
    expect(barsForSeconds(flat.secondsAt, 64.9, 4 * B)).toBe(32);
    expect(barsForSeconds(flat.secondsAt, 65.1, 4 * B)).toBe(33);
    expect(barsForSeconds(flat.secondsAt, 0, 4 * B)).toBe(1);
    // Half time from bar 9: bars 1–8 take 16 s, then 4 s each.
    const slow = tempoMap(120, [{ id: "p", tick: 32 * B, bpm: 60, ramp: false }]);
    expect(barsForSeconds(slow.secondsAt, 24, 4 * B)).toBe(10);
  });
  it("names lengths", () => {
    expect(barsText(64 * B, 4 * B)).toBe("16");
    expect(barsText(64 * B, 3 * B)).toBe("21.3");
    expect(lengthText(4 * B, 4 * B)).toBe("1 bar");
    expect(lengthText(8 * B, 4 * B)).toBe("2 bars");
    expect(lengthText(2 * B, 4 * B)).toBe("2 beats");
    expect(lengthText(B, 4 * B)).toBe("1 beat");
  });
});

describe("moving sections", () => {
  // Three four-bar sections back to back, in ticks.
  const spans: [number, number][] = [
    [0, 16 * B],
    [16 * B, 32 * B],
    [32 * B, 48 * B],
  ];

  it("swaps two neighbours by copying the first past the second, then cutting it", () => {
    expect(sectionSwap(spans, 0, 1)).toEqual({
      copy: { from: 0, to: 16 * B, at: 32 * B },
      cut: { at: 0, ticks: 16 * B },
    });
    // Moving the second one earlier is the same move, phrased from the other side.
    expect(sectionSwap(spans, 1, -1)).toEqual(sectionSwap(spans, 0, 1));
  });

  it("has nothing to swap at the ends", () => {
    expect(sectionSwap(spans, 0, -1)).toBeNull();
    expect(sectionSwap(spans, 2, 1)).toBeNull();
    expect(sectionSwap([], 0, 1)).toBeNull();
    // A section that owns no time cannot take any with it.
    expect(sectionSwap([[0, 0], [0, 16 * B]], 0, 1)).toBeNull();
  });

  it("duplicates a section right after itself, with no cut", () => {
    expect(sectionDuplicate(spans[1])).toEqual({ copy: { from: 16 * B, to: 32 * B, at: 32 * B }, cut: null });
    expect(sectionDuplicate([8 * B, 8 * B])).toBeNull();
  });
});
