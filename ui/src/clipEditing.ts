// Pure clip math for the song view: snapping, group moves, trims, drops, clipboard and marquee
// hits, all kept inside the song's hard end. The backend clamps too; this keeps the preview
// honest.

import type { Clip } from "./types";
import { TICKS_PER_BEAT } from "./types";

/** Shortest clip a trim can leave (one beat), unless the song end leaves less room. */
export const MIN_CLIP_TICKS = TICKS_PER_BEAT;

/** What the song view snaps to: a division in ticks, one bar of the project's meter, or nothing. */
export type ArrSnap = number | "bar" | "off";

/**
 * The tick grid a gesture lands on. "off" (and ⇧, which bypasses whatever is set) answers 1:
 * every helper below already rounds to a grid, and a grid of one tick only rounds to a whole
 * tick — so snap-off needs no second code path.
 */
export function snapGrid(snap: ArrSnap, barTicks: number, bypass = false): number {
  if (bypass || snap === "off") return 1;
  if (snap === "bar") return Math.max(1, Math.round(barTicks));
  return Math.max(1, Math.round(snap));
}

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

/**
 * Where a pattern dropped at `tick` lands: the start of the `gridTicks` slot it fell in, or null
 * outside the song. It floors rather than rounds, so a drop never jumps past the pointer.
 */
export function dropAt(tick: number, gridTicks: number, songEnd: number): number | null {
  if (!Number.isFinite(tick) || tick < 0 || tick >= songEnd) return null;
  return Math.floor(tick / gridTicks) * gridTicks;
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

/**
 * A group muted or unmuted as one step: any clip still audible means "mute all", so a mixed
 * selection takes one keystroke to silence and a second to bring back.
 */
export function toggleMuted(group: Clip[]): Clip[] {
  const muted = group.some((c) => !c.muted);
  return group.map((c) => ({ ...c, muted }));
}

/**
 * Where pasted clips land: the clipboard shifted so its earliest clip starts at `atTick`, each
 * keeping its own pattern (and so its own track). Copies that would start outside the song are
 * dropped rather than piled up at the end. The result feeds `addClip`, which clamps the lengths.
 */
export function pasteClips(clipboard: Clip[], atTick: number, songEnd: number): { src: Clip; startTick: number }[] {
  if (clipboard.length === 0) return [];
  const first = Math.min(...clipboard.map((c) => c.startTick));
  const at = Math.max(0, Math.round(atTick));
  return clipboard
    .map((src) => ({ src, startTick: src.startTick - first + at }))
    .filter((x) => x.startTick < songEnd);
}

/**
 * Where duplicated clips land: one group-span later, so repeating the gesture marches copies
 * along the timeline instead of stacking them. Copies past the song end are dropped.
 */
export function duplicateClips(group: Clip[], songEnd: number): { src: Clip; startTick: number }[] {
  if (group.length === 0) return [];
  const first = Math.min(...group.map((c) => c.startTick));
  const span = Math.max(...group.map((c) => c.startTick + c.lengthTicks)) - first;
  return group.map((src) => ({ src, startTick: src.startTick + span })).filter((x) => x.startTick < songEnd);
}
