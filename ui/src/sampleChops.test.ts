import { describe, expect, it } from "vitest";
import { chopsAt, MAX_CHOPS, MIN_CHOP_SECONDS, newChop } from "./sampler";
import {
  clampView,
  cleared,
  cutAt,
  DEFAULT_CHOPPER,
  deleteChop,
  dragEdge,
  dragRegionEdge,
  edgeAt,
  pageSlots,
  rechopped,
  regionEdgeAt,
  regionOf,
  sameChopper,
  secondsToX,
  sweptRegion,
  xToSeconds,
  zoomAround,
} from "./sampleChops";
import type { Chop, Sampler } from "./types";

const bounds = (chops: readonly Chop[]) => chops.map((c) => [+c.startSeconds.toFixed(6), +c.endSeconds.toFixed(6)]);
const sampler = (chops: Chop[]): Sampler => ({ sourceId: "src", basePitch: 36, chops, voice: "mono" });

describe("region", () => {
  const region = { startSeconds: 2, endSeconds: 6 };

  it("is the whole source until one is picked", () => {
    expect(regionOf(sampler([]), 8)).toEqual({ startSeconds: 0, endSeconds: 8 });
    expect(regionOf({ ...sampler([]), region }, 8)).toBe(region);
  });

  it("a tool cuts inside it and is remembered, chop i keeping its name and shaping", () => {
    const s = sampler([{ ...newChop(0, 4), name: "kick", gain: 2 }, { ...newChop(4, 8), name: "snare", choke: 3 }]);
    const next = rechopped(s, { kind: "equal", count: 4 }, region, 4)!;
    expect(bounds(next.chops)).toEqual([[2, 3], [3, 4], [4, 5], [5, 6]]);
    expect(next.region).toEqual(region);
    expect(next.chopper).toEqual({ kind: "equal", count: 4 });
    expect(next.chops.map((c) => c.name)).toEqual(["kick", "snare", undefined, undefined]);
    expect([next.chops[0].gain, next.chops[1].choke]).toEqual([2, 3]);
  });

  it("a whole drag carries names over from where it began, even through a frame that dropped a chop", () => {
    const start = rechopped(sampler([]), { kind: "equal", count: 2 }, region, 4)!;
    const named: Sampler = { ...start, chops: start.chops.map((c, i) => ({ ...c, name: `n${i}` })) };
    const auto = { ...named, analysis: { onsets: [{ seconds: 3, strength: 1 }], bpm: null, bpmConfidence: 0, keyRoot: null, keyScale: "major" as const, keyConfidence: 0 } };
    // Squeezed past the hit: one chop. Pulled back out: two again, still named.
    const squeezed = rechopped(auto, DEFAULT_CHOPPER, dragRegionEdge(region, "end", 2.5, 8), 4, auto.chops)!;
    expect(squeezed.chops.map((c) => c.name)).toEqual(["n0"]);
    const back = rechopped(squeezed, DEFAULT_CHOPPER, region, 4, auto.chops)!;
    expect(bounds(back.chops)).toEqual([[2, 3], [3, 6]]);
    expect(back.chops.map((c) => c.name)).toEqual(["n0", "n1"]);
  });

  it("a tool that cuts nothing leaves the sampler to the caller", () => {
    expect(rechopped(sampler([]), { kind: "grid", step: "bar" }, region, 4)).toBeNull();
  });

  it("clear leaves one chop over the region and keeps it one when the region moves", () => {
    const s = { ...sampler(chopsAt([3, 4], 8)), region, chopper: DEFAULT_CHOPPER };
    const c = cleared(s, 8);
    expect(bounds(c.chops)).toEqual([[2, 6]]);
    expect(c.region).toEqual(region);
    const moved = rechopped(c, c.chopper!, { startSeconds: 1, endSeconds: 7 }, 4)!;
    expect(bounds(moved.chops)).toEqual([[1, 7]]);
    expect(bounds(cleared(sampler([]), 8).chops)).toEqual([[0, 8]]);
  });

  it("drags an edge inside the source, never closer than a chop to the other edge", () => {
    expect(dragRegionEdge(region, "start", 1, 8)).toEqual({ startSeconds: 1, endSeconds: 6 });
    expect(dragRegionEdge(region, "start", -4, 8)).toEqual({ startSeconds: 0, endSeconds: 6 });
    expect(dragRegionEdge(region, "start", 9, 8)).toEqual({ startSeconds: 6 - MIN_CHOP_SECONDS, endSeconds: 6 });
    expect(dragRegionEdge(region, "end", 99, 8)).toEqual({ startSeconds: 2, endSeconds: 8 });
    expect(dragRegionEdge(region, "end", 0, 8)).toEqual({ startSeconds: 2, endSeconds: 2 + MIN_CHOP_SECONDS });
    expect(dragRegionEdge(region, "end", NaN, 8)).toBe(region);
  });

  it("sweeps either way, held in the source; a click is no region", () => {
    expect(sweptRegion(5, 3, 8)).toEqual({ startSeconds: 3, endSeconds: 5 });
    expect(sweptRegion(-1, 9, 8)).toEqual({ startSeconds: 0, endSeconds: 8 });
    expect(sweptRegion(4, 4.001, 8)).toBeNull();
  });

  it("finds the edge under the pointer, by side when both are in reach", () => {
    const view = { offset: 0, zoom: 1 };
    // 8 s over 800 px: the region's edges are at 200 and 600.
    expect(regionEdgeAt(region, 204, view, 8, 800, 7)).toBe("start");
    expect(regionEdgeAt(region, 597, view, 8, 800, 7)).toBe("end");
    expect(regionEdgeAt(region, 400, view, 8, 800, 7)).toBeNull();
    const narrow = { startSeconds: 4, endSeconds: 4.05 };
    expect(regionEdgeAt(narrow, 401, view, 8, 800, 7)).toBe("start");
    expect(regionEdgeAt(narrow, 405, view, 8, 800, 7)).toBe("end");
  });

  it("knows a tool by its kind and setting", () => {
    expect(sameChopper({ kind: "auto", count: 16 }, DEFAULT_CHOPPER)).toBe(true);
    expect(sameChopper({ kind: "equal", count: 16 }, DEFAULT_CHOPPER)).toBe(false);
    expect(sameChopper({ kind: "grid", step: "bar" }, { kind: "grid", step: "beat" })).toBe(false);
    expect(sameChopper(undefined, DEFAULT_CHOPPER)).toBe(false);
  });
});

describe("cutAt", () => {
  it("splits the chop under the cut, the tail keeping its shaping but not its name", () => {
    const shaped = { ...newChop(0, 4), name: "kick", gain: 2, reverse: true, choke: 3 };
    const s = sampler([shaped, newChop(4, 8)]);
    const cut = cutAt(s, 1.5, 8)!;
    expect(cut.index).toBe(1);
    expect(bounds(cut.sampler.chops)).toEqual([[0, 1.5], [1.5, 4], [4, 8]]);
    const [head, tail] = cut.sampler.chops;
    expect(head).toMatchObject({ id: shaped.id, name: "kick", gain: 2 });
    expect(tail).toMatchObject({ gain: 2, reverse: true, choke: 3 });
    expect(tail.name).toBeUndefined();
    expect(tail.id).not.toBe(shaped.id);
    // The input is untouched.
    expect(bounds(s.chops)).toEqual([[0, 4], [4, 8]]);
  });

  it("in a gap, a new chop runs to the next chop's start", () => {
    const cut = cutAt(sampler([newChop(0, 1), newChop(5, 6)]), 2, 8)!;
    expect([cut.index, bounds(cut.sampler.chops)]).toEqual([1, [[0, 1], [2, 5], [5, 6]]]);
    const end = cutAt(sampler([newChop(0, 1)]), 3, 8)!;
    expect(bounds(end.sampler.chops)).toEqual([[0, 1], [3, 8]]);
  });

  it("refuses a sliver, a full sampler and a point outside the source", () => {
    const s = sampler([newChop(0, 4)]);
    expect(cutAt(s, MIN_CHOP_SECONDS / 2, 8)).toBeNull();
    expect(cutAt(s, 4 - MIN_CHOP_SECONDS / 2, 8)).toBeNull();
    expect(cutAt(s, 9, 8)).toBeNull();
    expect(cutAt(s, -1, 8)).toBeNull();
    const full = chopsAt(Array.from({ length: MAX_CHOPS - 1 }, (_, i) => ((i + 1) * 8) / MAX_CHOPS), 8);
    expect(cutAt(sampler(full), 0.05, 8)).toBeNull();
  });
});

describe("deleteChop", () => {
  it("drops one chop and leaves the rest in order", () => {
    const s = sampler(chopsAt([2, 4], 8));
    expect(bounds(deleteChop(s, 1).chops)).toEqual([[0, 2], [4, 8]]);
    expect(deleteChop(s, 7)).toBe(s);
  });
});

describe("dragEdge", () => {
  const s = sampler(chopsAt([2, 4], 8));

  it("moves a shared cut on both chops", () => {
    expect(bounds(dragEdge(s, 1, "start", 2.5, 8).chops)).toEqual([[0, 2.5], [2.5, 4], [4, 8]]);
    expect(bounds(dragEdge(s, 0, "end", 1, 8).chops)).toEqual([[0, 1], [1, 4], [4, 8]]);
  });

  it("unlinked, only the dragged chop moves", () => {
    expect(bounds(dragEdge(s, 1, "start", 1, 8, false).chops)).toEqual([[0, 2], [1, 4], [4, 8]]);
  });

  it("clamps inside the source and keeps both chops at least MIN_CHOP_SECONDS long", () => {
    // Past the chop's own end, and past the neighbour's start.
    expect(bounds(dragEdge(s, 1, "start", 7, 8).chops)[1]).toEqual([4 - MIN_CHOP_SECONDS, 4]);
    expect(bounds(dragEdge(s, 1, "start", -3, 8).chops)[0]).toEqual([0, MIN_CHOP_SECONDS]);
    expect(bounds(dragEdge(s, 2, "end", 99, 8).chops)[2]).toEqual([4, 8]);
    expect(bounds(dragEdge(s, 2, "end", 1, 8).chops)[2]).toEqual([4, 4 + MIN_CHOP_SECONDS]);
    expect(bounds(dragEdge(s, 0, "start", -1, 8).chops)[0]).toEqual([0, 2]);
    expect(dragEdge(s, 0, "start", NaN, 8)).toBe(s);
  });
});

describe("view geometry", () => {
  it("maps seconds to pixels and back", () => {
    const view = { offset: 2, zoom: 2 };
    // 8 s over 800 px at zoom 2: 200 px per second.
    expect(secondsToX(3, view, 8, 800)).toBe(200);
    expect(xToSeconds(200, view, 8, 800)).toBe(3);
  });

  it("clamps zoom and keeps the window inside the source", () => {
    expect(clampView({ offset: 7, zoom: 2 }, 8)).toEqual({ offset: 4, zoom: 2 });
    expect(clampView({ offset: -1, zoom: 0.2 }, 8)).toEqual({ offset: 0, zoom: 1 });
    expect(clampView({ offset: 0, zoom: 1e6 }, 8).zoom).toBe(64);
  });

  it("zooms around the pointer", () => {
    const next = zoomAround({ offset: 0, zoom: 1 }, 4, 400, 8, 800);
    // The second under x=400 (4 s) stays under x=400.
    expect(secondsToX(4, next, 8, 800)).toBeCloseTo(400);
    expect(next.zoom).toBe(4);
  });

  it("finds the edge under the pointer, the start winning on a shared cut", () => {
    const chops = chopsAt([2, 4], 8);
    const view = { offset: 0, zoom: 1 };
    expect(edgeAt(chops, 201, view, 8, 800, 5)).toEqual({ index: 1, edge: "start" });
    expect(edgeAt(chops, 797, view, 8, 800, 5)).toEqual({ index: 2, edge: "end" });
    expect(edgeAt(chops, 300, view, 8, 800, 5)).toBeNull();
  });
});

describe("pad pages", () => {
  it("lays pad 1 bottom-left and leaves missing chops empty", () => {
    expect(pageSlots(0, 16)).toEqual([12, 13, 14, 15, 8, 9, 10, 11, 4, 5, 6, 7, 0, 1, 2, 3]);
    expect(pageSlots(1, 18).slice(12)).toEqual([16, 17, -1, -1]);
  });
});
