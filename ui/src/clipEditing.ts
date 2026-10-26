// Pure clip math for the song view: group moves, trims, drops and marquee hits, all kept
// inside the song's hard end. The backend clamps too; this keeps the preview honest.

import type { Clip } from "./types";
import { TICKS_PER_BEAT } from "./types";

/** Shortest clip a trim can leave (one beat), unless the song end leaves less room. */
export const MIN_CLIP_TICKS = TICKS_PER_BEAT;

/** Round to the nearest multiple of `gridTicks`, staying on whole ticks. */
export function snapTo(tick: number, gridTicks: number): number {
  return Math.round(tick / gridTicks) * gridTicks;
}

/**
 * The shift for a group move: the anchor clip's start snaps to `gridTicks`, every clip moves by
 * the same amount, and the whole group stays inside `[0, songEnd]`. Returns 0 for an empty group.
 */
export function groupMoveDelta(anchor: Clip, group: Clip[], rawDelta: number, gridTicks: number, songEnd: number): number {
  if (group.length === 0) return 0;
  const delta = snapTo(anchor.startTick + rawDelta, gridTicks) - anchor.startTick;
  const lo = -Math.min(...group.map((c) => c.startTick));
  const hi = Math.max(0, songEnd - Math.max(...group.map((c) => c.startTick + c.lengthTicks)));
  return Math.min(Math.max(delta, lo), Math.max(lo, hi));
}

/** The group with every start shifted by `delta` ticks. */
export function moveClips(group: Clip[], delta: number): Clip[] {
  return group.map((c) => ({ ...c, startTick: c.startTick + delta }));
}

/** A right-edge trim or loop-extend: snapped, at least one beat, never past the song end. */
export function trimClip(clip: Clip, rawLength: number, gridTicks: number, songEnd: number): Clip {
  const room = songEnd - clip.startTick;
  const len = Math.max(MIN_CLIP_TICKS, snapTo(rawLength, gridTicks));
  return { ...clip, lengthTicks: Math.max(Math.min(len, room), Math.min(MIN_CLIP_TICKS, room)) };
}

/** Where a pattern dropped at `tick` lands: the bar it's in, or null outside the song. */
export function dropBar(tick: number, barTicks: number, songEnd: number): number | null {
  if (!Number.isFinite(tick) || tick < 0 || tick >= songEnd) return null;
  return Math.floor(tick / barTicks) * barTicks;
}

/**
 * Clips touched by a marquee spanning lanes `[lane0, lane1]` and ticks `[tick0, tick1]` (either
 * order). `laneOf` gives a clip's lane index, or -1 when it isn't drawn.
 */
export function clipsInBox(clips: Clip[], laneOf: (clip: Clip) => number, lane0: number, lane1: number, tick0: number, tick1: number): string[] {
  const [l0, l1] = lane0 <= lane1 ? [lane0, lane1] : [lane1, lane0];
  const [t0, t1] = tick0 <= tick1 ? [tick0, tick1] : [tick1, tick0];
  return clips
    .filter((c) => {
      const lane = laneOf(c);
      return lane >= l0 && lane <= l1 && c.startTick < t1 && c.startTick + c.lengthTicks > t0;
    })
    .map((c) => c.id);
}

/** `ids` with `id` added, or removed if it was there. */
export function toggleId(ids: string[], id: string): string[] {
  return ids.includes(id) ? ids.filter((x) => x !== id) : [...ids, id];
}
