// Pins ui/src/sampler.ts to the numbers crates/dissonant-core/tests/sampler.rs pins the Rust
// helpers to, so the browser build cuts, clamps and refuses exactly what the desktop app does.

import { describe, expect, it } from "vitest";
import {
  chopRegion,
  chopsAt,
  DEFAULT_CHOP_RELEASE_SECONDS,
  MAX_CHOKE_GROUP,
  MAX_CHOP_ATTACK_SECONDS,
  MAX_CHOP_GAIN,
  MAX_CHOPS,
  MAX_ONSETS,
  MIN_CHOP_SECONDS,
  newChop,
  rechop,
  sanitizedSampler,
  sourceSeconds,
  strongestOnsets,
  suggestedRegion,
} from "./sampler";
import type { Chop, Chopper, GridStep, Onset, SampleAnalysis, SampleRegion, Sampler } from "./types";

const SOURCE_ID = "11111111-2222-4333-8444-555555555555";
const sampler = (chops: Chop[], over: Partial<Sampler> = {}): Sampler => ({ sourceId: SOURCE_ID, basePitch: 36, chops, voice: "mono", ...over });
const bounds = (chops: Chop[]) => chops.map((c) => [c.startSeconds, c.endSeconds]);
const onset = (seconds: number, strength: number): Onset => ({ seconds, strength });
const region = (startSeconds: number, endSeconds: number): SampleRegion => ({ startSeconds, endSeconds });
const analysis = (over: Partial<SampleAnalysis>): SampleAnalysis => ({ bpm: null, bpmConfidence: 0, keyRoot: null, keyScale: "major", keyConfidence: 0, onsets: [], ...over });

describe("sanitizedSampler (Sampler::sanitized)", () => {
  it("clamps every chop field into its range", () => {
    const wild = { ...newChop(-1, 99), gain: 9, attackSeconds: 7, releaseSeconds: -1, choke: 200 };
    const c = sanitizedSampler(sampler([wild]), 8)!.chops[0];
    expect([c.startSeconds, c.endSeconds]).toEqual([0, 8]);
    expect([c.gain, c.attackSeconds, c.releaseSeconds, c.choke]).toEqual([MAX_CHOP_GAIN, MAX_CHOP_ATTACK_SECONDS, 0, MAX_CHOKE_GROUP]);
  });

  it("gives a chop at least the minimum length and drops one that cannot fit", () => {
    const s = sanitizedSampler(sampler([newChop(2, 1), newChop(8 - MIN_CHOP_SECONDS / 2, 8), newChop(9, 10), newChop(Number.NaN, 1)]), 8)!;
    expect(bounds(s.chops)).toEqual([[2, 2 + MIN_CHOP_SECONDS]]);
  });

  it("keeps chop order because order is the key mapping", () => {
    const s = sanitizedSampler(sampler([newChop(4, 5), newChop(0, 1), newChop(2, 3)]), 8)!;
    expect(bounds(s.chops)).toEqual([[4, 5], [0, 1], [2, 3]]);
  });

  it("refuses more than MAX_CHOPS", () => {
    const chops = Array.from({ length: MAX_CHOPS + 1 }, (_, i) => newChop(i * 0.1, i * 0.1 + 0.05));
    expect(sanitizedSampler(sampler(chops), 60)).toBeNull();
  });

  it("lowers the base pitch until the last chop has a key", () => {
    const chops = chopsAt([1, 2, 3, 4, 5, 6, 7, 8, 9], 10);
    expect(chops).toHaveLength(10);
    expect(sanitizedSampler(sampler(chops, { basePitch: 127 }), 10)!.basePitch).toBe(118);
  });

  it("tidies the analysis", () => {
    const a = sanitizedSampler(
      sampler([], {
        analysis: {
          bpm: -3,
          bpmConfidence: 4,
          keyRoot: 12,
          keyScale: "minor",
          keyConfidence: -1,
          onsets: [onset(3, 2), onset(9, 0.5), onset(1, 0.25), onset(Number.NaN, 1)],
        },
      }),
      8,
    )!.analysis!;
    expect([a.bpm, a.bpmConfidence, a.keyRoot, a.keyScale, a.keyConfidence]).toEqual([null, 1, null, "minor", 0]);
    expect(a.onsets).toEqual([onset(1, 0.25), onset(3, 1)]);
  });

  it("keeps only the strongest onsets past the cap", () => {
    const onsets = Array.from({ length: MAX_ONSETS + 2 }, (_, i) => onset(i * 0.01, i < 2 ? 0 : 0.5));
    const kept = sanitizedSampler(
      sampler([], { analysis: { bpm: null, bpmConfidence: 0, keyRoot: null, keyScale: "major", keyConfidence: 0, onsets } }),
      60,
    )!.analysis!.onsets;
    expect(kept).toHaveLength(MAX_ONSETS);
    expect(kept.every((o) => o.strength === 0.5)).toBe(true);
  });

  it("stores no empty name, as Rust's skip_serializing_if never writes one", () => {
    const s = sanitizedSampler(sampler([{ ...newChop(0, 1), name: "" }, { ...newChop(1, 2), name: "snare" }]), 8)!;
    expect(s.chops.map((c) => "name" in c)).toEqual([false, true]);
  });

  it("leaves its argument alone", () => {
    const input = sampler([newChop(6, 30)]);
    const copy = structuredClone(input);
    sanitizedSampler(input, 8);
    expect(input).toEqual(copy);
  });

  it("holds the region inside the source and drops one too short to cut", () => {
    const withRegion = (r: SampleRegion) => sanitizedSampler(sampler([], { region: r }), 8)!.region;
    expect(withRegion(region(-1, 99))).toEqual(region(0, 8));
    expect(withRegion(region(2, 3))).toEqual(region(2, 3));
    expect(withRegion(region(7.995, 20))).toBeUndefined();
    expect(withRegion(region(5, 3))).toBeUndefined();
    expect(withRegion(region(Number.NaN, 3))).toBeUndefined();
  });

  it("holds the chopper count between one and MAX_CHOPS and keeps the voice", () => {
    const withChopper = (c: Chopper) => sanitizedSampler(sampler([], { chopper: c, voice: "poly" }), 8)!;
    expect(withChopper({ kind: "auto", count: 0 }).chopper).toEqual({ kind: "auto", count: 1 });
    expect(withChopper({ kind: "equal", count: 500 }).chopper).toEqual({ kind: "equal", count: MAX_CHOPS });
    expect(withChopper({ kind: "grid", step: "half" }).chopper).toEqual({ kind: "grid", step: "half" });
    expect(withChopper({ kind: "auto", count: 16 }).voice).toBe("poly");
  });

  it("plays a sampler from before mono/poly as mono", () => {
    const old = { sourceId: SOURCE_ID, basePitch: 36, chops: [] } as unknown as Sampler; // a pre-voice file
    const s = sanitizedSampler(old, 8)!;
    expect(s.voice).toBe("mono");
    expect("region" in s || "chopper" in s).toBe(false);
  });
});

describe("chopsAt (chops_at)", () => {
  it("cuts between consecutive points and always at zero", () => {
    const chops = chopsAt([3, 1, 1 + MIN_CHOP_SECONDS / 2, 8, -2, Number.NaN], 8);
    expect(bounds(chops)).toEqual([[0, 1], [1, 3], [3, 8]]);
    expect(chops[0]).toMatchObject({ gain: 1, reverse: false, mode: "oneShot", choke: 0, attackSeconds: 0, releaseSeconds: DEFAULT_CHOP_RELEASE_SECONDS });
    expect("name" in chops[0]).toBe(false);
  });

  it("never leaves a last chop shorter than the minimum", () => {
    expect(bounds(chopsAt([4, 8 - MIN_CHOP_SECONDS / 2], 8))).toEqual([[0, 4], [4, 8]]);
  });

  it("with no points is one chop over the whole source, and nothing over no source", () => {
    expect(bounds(chopsAt([], 8))).toEqual([[0, 8]]);
    expect(chopsAt([], 0)).toEqual([]);
    expect(chopsAt([1], Number.NaN)).toEqual([]);
  });

  it("stops at MAX_CHOPS with the last running to the end", () => {
    const chops = chopsAt(Array.from({ length: 199 }, (_, i) => (i + 1) * 0.1), 20);
    expect(chops).toHaveLength(MAX_CHOPS);
    expect(chops[chops.length - 1].endSeconds).toBe(20);
  });
});

describe("strongestOnsets (strongest_onsets)", () => {
  it("picks the loudest hits in time order", () => {
    const onsets = [onset(0.5, 0.2), onset(1, 0.9), onset(2, 0.5), onset(3, 0.9), onset(4, 0.1)];
    expect(strongestOnsets(onsets, 3)).toEqual([1, 2, 3]);
    expect(strongestOnsets(onsets, 0)).toEqual([]);
    expect(strongestOnsets(onsets, 99)).toHaveLength(5);
  });

  it("breaks ties toward the earlier hit and does not reorder its input", () => {
    const onsets = [onset(3, 0.5), onset(1, 0.5), onset(2, 0.5)];
    expect(strongestOnsets(onsets, 2)).toEqual([1, 2]);
    expect(onsets.map((o) => o.seconds)).toEqual([3, 1, 2]);
  });
});

describe("sourceSeconds (AudioSource::duration_seconds)", () => {
  it("is the length at the source's own rate, 0 for a degenerate source", () => {
    expect(sourceSeconds({ id: SOURCE_ID, name: "x", frames: 8 * 44_100, sampleRate: 44_100, channels: 2 })).toBe(8);
    expect(sourceSeconds({ id: SOURCE_ID, name: "x", frames: 100, sampleRate: 0, channels: 2 })).toBe(0);
  });
});

describe("suggestedRegion (suggested_region)", () => {
  it("starts at the first strong hit and runs four bars of the tempo", () => {
    const onsets = [onset(0.2, 0.3), onset(1, 0.8), onset(2, 1)];
    expect(suggestedRegion(analysis({ bpm: 120, onsets }), 30, 3)).toEqual(region(1, 7));
    expect(suggestedRegion(analysis({ bpm: 120, onsets }), 30, 4)).toEqual(region(1, 9));
    expect(suggestedRegion(analysis({ onsets }), 30, 3)).toEqual(region(1, 9));
  });

  it("is held inside the source", () => {
    const at = (onsets: Onset[], duration: number) => suggestedRegion(analysis({ onsets }), duration, 4);
    expect(at([onset(1, 0.8)], 5)).toEqual(region(1, 5));
    expect(at([onset(0.5, 0.2)], 30)).toEqual(region(0, 8));
    expect(at([], 30)).toEqual(region(0, 8));
    expect(at([onset(9.999, 1)], 10)).toEqual(region(2, 10));
    expect(at([onset(8, 1)], 8)).toEqual(region(0, 8));
  });
});

describe("chopRegion (chop_region)", () => {
  /** Hits around and inside the region 1…9: one on its start, one too near its end, two outside. */
  const hits = analysis({
    onsets: [onset(0.5, 1), onset(1, 0.9), onset(2, 0.5), onset(3, 0.8), onset(5, 0.7), onset(8.995, 1), onset(12, 1)],
  });

  it("auto cuts at the region start and its strongest hits inside, up to count chops", () => {
    const auto = (count: number) => bounds(chopRegion({ kind: "auto", count }, region(1, 9), hits, 4));
    expect(auto(3)).toEqual([[1, 3], [3, 5], [5, 9]]);
    expect(auto(16)).toEqual([[1, 2], [2, 3], [3, 5], [5, 9]]);
    expect(auto(1)).toEqual([[1, 9]]);
    expect(auto(0)).toEqual([[1, 9]]);
    expect(bounds(chopRegion({ kind: "auto", count: 16 }, region(1, 9), undefined, 4))).toEqual([[1, 9]]);
  });

  it("equal splits the region evenly", () => {
    const equal = (count: number, r: SampleRegion) => bounds(chopRegion({ kind: "equal", count }, r, undefined, 4));
    expect(equal(4, region(2, 6))).toEqual([[2, 3], [3, 4], [4, 5], [5, 6]]);
    expect(equal(1, region(2, 6))).toEqual([[2, 6]]);
    expect(equal(500, region(0, 600))).toHaveLength(MAX_CHOPS);
  });

  it("the grid counts bars, halves or beats of the tempo from the region start", () => {
    const a = analysis({ bpm: 120 });
    const grid = (step: GridStep, r: SampleRegion, bpb: number) => bounds(chopRegion({ kind: "grid", step }, r, a, bpb));
    expect(grid("bar", region(0.25, 6), 4)).toEqual([[0.25, 2.25], [2.25, 4.25], [4.25, 6]]);
    expect(grid("half", region(0, 2), 4)).toEqual([[0, 1], [1, 2]]);
    expect(grid("beat", region(0, 2), 4)).toHaveLength(4);
    expect(grid("beat", region(0, 2), 3)).toHaveLength(4);
    expect(grid("half", region(0, 3), 3)).toEqual([[0, 0.75], [0.75, 1.5], [1.5, 2.25], [2.25, 3]]);
    expect(chopRegion({ kind: "grid", step: "beat" }, region(0, 600), analysis({ bpm: 200 }), 4)).toHaveLength(MAX_CHOPS);
  });

  it("the grid needs a tempo and nothing cuts a region too short", () => {
    const bar: Chopper = { kind: "grid", step: "bar" };
    expect(chopRegion(bar, region(0, 8), analysis({}), 4)).toEqual([]);
    expect(chopRegion(bar, region(0, 8), undefined, 4)).toEqual([]);
    for (const c of [{ kind: "auto", count: 16 }, { kind: "equal", count: 4 }] satisfies Chopper[]) {
      expect(chopRegion(c, region(1, 1.005), hits, 4)).toEqual([]);
    }
  });
});

describe("rechop", () => {
  it("keeps each chop's settings on the new cuts", () => {
    const old = chopsAt([2], 8);
    old[0] = { ...old[0], name: "kick", gain: 2, reverse: true, mode: "gate", attackSeconds: 0.1, releaseSeconds: 0.2, choke: 3 };
    old[1] = { ...old[1], name: "snare" };
    const fresh = chopsAt([1, 3], 8);
    const out = rechop(old, fresh);
    expect(bounds(out)).toEqual([[0, 1], [1, 3], [3, 8]]);
    expect(out[0]).toMatchObject({ name: "kick", gain: 2, reverse: true, mode: "gate", attackSeconds: 0.1, releaseSeconds: 0.2, choke: 3 });
    expect(out[1].name).toBe("snare");
    expect(out[2]).toEqual({ ...newChop(3, 8), id: out[2].id });
    expect(out.map((c) => c.id)).toEqual(fresh.map((c) => c.id));
  });
});
