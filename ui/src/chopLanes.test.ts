import { describe, expect, it } from "vitest";
import { chopOfPitch, clipHeardAt, hitSounds, laneChops, laneRowHeight, moveHits, patternTickAt, songTickOf, spanPeaks, ticksAfter, type HitSound } from "./chopLanes";
import { newChop } from "./sampler";
import { TICKS_PER_BEAT, tempoMap, type Chop, type Clip, type NoteEvent, type Sampler, type SamplerVoice, type SongPattern } from "./types";

const B = TICKS_PER_BEAT;
/** 120 bpm: a beat is half a second. */
const flat = tempoMap(120, []).secondsAt;
const chop = (start: number, end: number, patch: Partial<Chop> = {}): Chop => ({ ...newChop(start, end), ...patch });
const sampler = (chops: Chop[], voice: SamplerVoice = "mono"): Sampler => ({ sourceId: "src", basePitch: 36, chops, voice });
let n = 0;
const hit = (startTick: number, chopIndex: number, patch: Partial<NoteEvent> = {}): NoteEvent => ({
  id: `h${n++}`,
  startTick,
  lengthTicks: B / 4,
  pitch: 36 + chopIndex,
  velocity: 100,
  ...patch,
});
const byId = (sounds: HitSound[], id: string) => sounds.find((s) => s.id === id)!;

describe("rows", () => {
  it("shows every chop, chop 1 at the bottom", () => {
    const s = sampler([chop(0, 1), chop(1, 2), chop(2, 3)]);
    expect(laneChops(s, [], false)).toEqual([2, 1, 0]);
  });

  it("used only keeps the chops the pattern plays, muted hits included", () => {
    const s = sampler([chop(0, 1), chop(1, 2), chop(2, 3), chop(3, 4)]);
    expect(laneChops(s, [hit(0, 3), hit(B, 1, { muted: true }), hit(0, 9)], true)).toEqual([3, 1]);
  });

  it("maps keys to chops and nothing else", () => {
    const s = sampler([chop(0, 1), chop(1, 2)]);
    expect([35, 36, 37, 38].map((p) => chopOfPitch(s, p))).toEqual([-1, 0, 1, -1]);
  });

  it("rows share the height up to sixteen, then stop shrinking and scroll", () => {
    expect(laneRowHeight(320, 4, { min: 14, max: 48 })).toBe(48);
    expect(laneRowHeight(320, 16, { min: 14, max: 48 })).toBe(20);
    expect(laneRowHeight(320, 40, { min: 14, max: 48 })).toBe(20);
    expect(laneRowHeight(100, 40, { min: 14, max: 48 })).toBe(14);
  });
});

describe("tempo", () => {
  it("turns seconds into ticks at a steady tempo", () => {
    expect(ticksAfter(flat, 0, 1)).toBeCloseTo(2 * B, 3);
    expect(ticksAfter(flat, 3 * B, 0.25)).toBeCloseTo(3.5 * B, 3);
  });

  it("follows a tempo change inside the span", () => {
    // 120 until beat 4 (2 s), then 60: one more second is one more beat.
    const slow = tempoMap(120, [{ id: "p", tick: 4 * B, bpm: 60, ramp: false }]).secondsAt;
    expect(ticksAfter(slow, 0, 3)).toBeCloseTo(5 * B, 3);
  });
});

describe("tails", () => {
  it("a one-shot rings for its chop's length at the song's tempo", () => {
    const s = sampler([chop(0, 1)]);
    const a = hit(0, 0);
    const [snd] = hitSounds(s, [a], flat);
    expect(snd.bodyEnd).toBe(B / 4);
    expect(snd.natural).toBeCloseTo(2 * B, 3);
    expect(snd.end).toBeCloseTo(2 * B, 3);
    expect(snd.cutAt).toBeNull();
  });

  it("a one-shot shorter than its note draws no more body than it sounds", () => {
    const s = sampler([chop(0, 0.25)]);
    const [snd] = hitSounds(s, [hit(0, 0, { lengthTicks: 2 * B })], flat);
    expect(snd.bodyEnd).toBeCloseTo(B / 2, 3);
    expect(snd.end).toBeCloseTo(B / 2, 3);
  });

  it("on a mono sampler any later hit cuts the tail", () => {
    const s = sampler([chop(0, 1), chop(1, 2)]);
    const a = hit(0, 0);
    const b = hit(B, 1);
    const sounds = hitSounds(s, [a, b], flat);
    expect(byId(sounds, a.id).cutAt).toBe(B);
    expect(byId(sounds, a.id).end).toBe(B);
    expect(byId(sounds, b.id).cutAt).toBeNull();
  });

  it("on a poly sampler only the same chop or its choke group cuts", () => {
    const s = sampler([chop(0, 1, { choke: 2 }), chop(1, 2), chop(2, 3, { choke: 2 })], "poly");
    const a = hit(0, 0);
    const other = hit(B / 2, 1);
    const again = hit(B, 0);
    const sounds = hitSounds(s, [a, other, again], flat);
    expect(byId(sounds, a.id).cutAt).toBe(B);
    const choked = hitSounds(s, [a, other, hit(1.5 * B, 2)], flat);
    expect(byId(choked, a.id).cutAt).toBe(1.5 * B);
    const free = hitSounds(sampler([chop(0, 1), chop(1, 2)], "poly"), [a, other], flat);
    expect(byId(free, a.id).cutAt).toBeNull();
  });

  it("a choke group of zero chokes nothing", () => {
    const s = sampler([chop(0, 1), chop(1, 2)], "poly");
    const a = hit(0, 0);
    expect(byId(hitSounds(s, [a, hit(B, 1)], flat), a.id).cutAt).toBeNull();
  });

  it("a muted hit neither sounds nor cuts", () => {
    const s = sampler([chop(0, 1), chop(1, 2)]);
    const a = hit(0, 0);
    const m = hit(B, 1, { muted: true });
    const sounds = hitSounds(s, [a, m], flat);
    expect(sounds.map((x) => x.id)).toEqual([a.id]);
    expect(sounds[0].cutAt).toBeNull();
  });

  it("a hit that plays out before the next one is not marked cut", () => {
    const s = sampler([chop(0, 0.1), chop(1, 2)]);
    const a = hit(0, 0);
    expect(byId(hitSounds(s, [a, hit(B, 1)], flat), a.id).cutAt).toBeNull();
  });

  it("a gate holds its own length, then rings for its release", () => {
    const s = sampler([chop(0, 4, { mode: "gate", releaseSeconds: 0.5 })]);
    const [snd] = hitSounds(s, [hit(0, 0, { lengthTicks: 2 * B })], flat);
    expect(snd.bodyEnd).toBe(2 * B);
    expect(snd.natural).toBeCloseTo(3 * B, 3);
  });

  it("a gate held past its chop releases where the chop runs out", () => {
    const s = sampler([chop(0, 0.5, { mode: "gate", releaseSeconds: 0.5 })]);
    const [snd] = hitSounds(s, [hit(0, 0, { lengthTicks: 4 * B })], flat);
    expect(snd.bodyEnd).toBe(4 * B);
    expect(snd.natural).toBeCloseTo(2 * B, 3);
  });

  it("repeats ring from the last strike, and the hit is placed where it is shifted to", () => {
    const s = sampler([chop(0, 1)]);
    const [snd] = hitSounds(s, [hit(0, 0, { lengthTicks: B, repeats: 3, shiftTicks: B / 2 })], flat);
    expect(snd.start).toBe(B / 2);
    // Strikes at 0.5, 0.75, 1, 1.25 beats; one second (two beats) from the last.
    expect(snd.natural).toBeCloseTo(3.25 * B, 3);
  });

  it("another hit's repeat cuts like a hit of its own", () => {
    const s = sampler([chop(0, 1), chop(1, 2)]);
    const a = hit(B / 2, 0);
    const rolled = hit(0, 1, { lengthTicks: B, repeats: 1 });
    expect(byId(hitSounds(s, [a, rolled], flat), a.id).cutAt).toBeNull();
    const later = hit(B / 4, 0);
    expect(byId(hitSounds(s, [later, rolled], flat), later.id).cutAt).toBe(B / 2);
  });
});

describe("moving hits", () => {
  const rows = [36, 37, 38, 39];

  it("dragging up a row hands the hit to the next chop", () => {
    const a = hit(0, 0);
    const [moved] = moveHits([a], new Set([a.id]), 0, 1, rows, 4 * B);
    expect(moved.pitch).toBe(37);
    expect(moved.startTick).toBe(0);
  });

  it("moves in time and across chops at once", () => {
    const a = hit(B, 1);
    const [moved] = moveHits([a], new Set([a.id]), B, -1, rows, 4 * B);
    expect([moved.startTick, moved.pitch]).toEqual([2 * B, 36]);
  });

  it("a group keeps its shape against the outer rows and the pattern's edges", () => {
    const a = hit(0, 0);
    const b = hit(B, 2);
    const ids = new Set([a.id, b.id]);
    const up = moveHits([a, b], ids, -B, 5, rows, 4 * B);
    expect(up.map((x) => [x.startTick, x.pitch])).toEqual([[0, 37], [B, 39]]);
  });

  it("walks the shown rows, skipping hidden chops", () => {
    const a = hit(0, 0);
    const [moved] = moveHits([a], new Set([a.id]), 0, 1, [36, 39], 4 * B);
    expect(moved.pitch).toBe(39);
  });
});

describe("row waveform", () => {
  const peaks: [number, number][] = Array.from({ length: 10 }, (_, i) => [-i / 10, i / 10]);

  it("takes the chop's span out of the whole source", () => {
    // 10 buckets over 10 s: a bucket a second.
    expect(spanPeaks(peaks, 10, 2, 4, 2)).toEqual([[-0.2, 0.2], [-0.3, 0.3]]);
  });

  it("folds many buckets into one column by their extremes", () => {
    expect(spanPeaks(peaks, 10, 0, 10, 1)).toEqual([[-0.9, 0.9]]);
  });

  it("draws a reversed chop back to front", () => {
    expect(spanPeaks(peaks, 10, 2, 4, 2, true)).toEqual([[-0.3, 0.3], [-0.2, 0.2]]);
  });

  it("is empty with nothing to draw from", () => {
    expect(spanPeaks([], 10, 0, 1, 8)).toEqual([]);
    expect(spanPeaks(peaks, 0, 0, 1, 8)).toEqual([]);
  });
});

describe("the SAMPLE strip's song time", () => {
  const bar = 4 * B;
  // A 2-bar pattern, placed at bar 3 for 4 bars (looping once) and again at bar 9 entering one bar in.
  const pattern: SongPattern = { id: "p", trackId: "t", name: "p", lengthTicks: 2 * bar, notes: [] };
  const clip = (id: string, startTick: number, lengthTicks: number, offsetTicks = 0, patternId = "p"): Clip => ({ id, patternId, startTick, lengthTicks, offsetTicks, muted: false });
  const clips = [clip("a", 2 * bar, 4 * bar), clip("b", 8 * bar, 2 * bar, bar), clip("other", 0, 20 * bar, 0, "q")];

  it("reads the playhead through the clip heard, looping the pattern inside it", () => {
    expect(patternTickAt(clips, pattern, 2 * bar)).toBe(0);
    expect(patternTickAt(clips, pattern, 4 * bar + B)).toBe(B);
    expect(patternTickAt(clips, pattern, 8 * bar)).toBe(bar);
    expect(patternTickAt(clips, pattern, 9 * bar)).toBe(0);
  });

  it("has no playhead where the pattern is not playing, rather than folding the song in", () => {
    expect(patternTickAt(clips, pattern, bar)).toBeNull();
    expect(patternTickAt(clips, pattern, 6 * bar)).toBeNull();
    expect(patternTickAt([], pattern, 0)).toBeNull();
  });

  it("the clip on top wins where two overlap", () => {
    const overlapping = [clip("a", 0, 8 * bar), clip("late", 4 * bar, 2 * bar, B)];
    expect(clipHeardAt(overlapping, "p", 5 * bar)?.id).toBe("late");
    expect(patternTickAt(overlapping, pattern, 4 * bar)).toBe(B);
  });

  it("seeks to that tick in the pass being heard, else the first place it plays", () => {
    // Heard at bar 5 (the second pass of clip a): beat 2 of the pattern is in that pass.
    expect(songTickOf(clips, pattern, B, 4 * bar + 3 * B)).toBe(4 * bar + B);
    // Not playing at bar 1: the first clip's first pass.
    expect(songTickOf(clips, pattern, B, 0)).toBe(2 * bar + B);
    // Clip b enters a bar in, so the pattern's bar 2 is where it starts.
    expect(songTickOf([clips[1]], pattern, bar, 0)).toBe(8 * bar);
    expect(songTickOf([clips[1]], pattern, 0, 0)).toBe(9 * bar);
  });

  it("finds nowhere for a tick no clip plays, or an unplaced pattern", () => {
    expect(songTickOf([clip("short", 0, B)], pattern, bar, 0)).toBeNull();
    expect(songTickOf([], pattern, 0, 0)).toBeNull();
  });
});
