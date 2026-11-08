/**
 * The SAMPLE screen's chop editing, kept out of the component so it can be tested: the chop
 * tools and "clear" inside the region, moving or sweeping the region, cutting and deleting one
 * chop, dragging a chop edge, and the waveform view's seconds ↔ pixels geometry. The cutting
 * itself is `chopRegion` in `./sampler`, shared with the fetch.
 *
 * Every function returns a new `Sampler` (or chop list) and never mutates its input; the page
 * sends the result as one `setSampler`. What `sanitizedSampler` would clamp anyway is clamped
 * here too, so the preview the page draws during a drag is exactly what the document will hold.
 */
import { chopRegion, DEFAULT_CHOP_COUNT, MAX_CHOPS, MIN_CHOP_SECONDS, newChop, rechop } from "./sampler";
import { uuid, type Chop, type Chopper, type SampleRegion, type Sampler } from "./types";

/** The chip counts the chop tools offer. */
export const CHOP_COUNTS = [4, 8, 16, 32] as const;

// ─── Region ────────────────────────────────────────────────────────────────────────────────

/** What a region edit re-cuts with before any tool was used: the tool a fresh link is cut by. */
export const DEFAULT_CHOPPER: Chopper = { kind: "auto", count: DEFAULT_CHOP_COUNT };

/** The tool "clear" leaves behind: moving the region afterwards keeps it one chop. */
const CLEAR_CHOPPER: Chopper = { kind: "equal", count: 1 };

/** Where the chop tools cut: the sampler's region, or the whole source without one. */
export function regionOf(s: Sampler, duration: number): SampleRegion {
  return s.region ?? { startSeconds: 0, endSeconds: duration };
}

/** The same tool, setting included: lights the chip of the tool region edits re-cut with. */
export function sameChopper(a: Chopper | undefined, b: Chopper): boolean {
  if (!a || a.kind !== b.kind) return false;
  return a.kind === "grid" ? b.kind === "grid" && a.step === b.step : b.kind !== "grid" && a.count === b.count;
}

/**
 * `s` re-cut by `chopper` inside `region`, both remembered so the next region edit cuts the
 * same way. Chop *i* keeps `from[i]`'s name and shaping (`rechop`): `from` is the chop list
 * before the gesture began, so a name survives a whole drag even through a frame where its
 * chop briefly did not exist. `null` when the tool cuts nothing there (a grid without a tempo,
 * a region too short), so the caller keeps what it has.
 */
export function rechopped(s: Sampler, chopper: Chopper, region: SampleRegion, beatsPerBar: number, from: readonly Chop[] = s.chops): Sampler | null {
  const fresh = chopRegion(chopper, region, s.analysis, beatsPerBar);
  if (fresh.length === 0) return null;
  return { ...s, region, chopper, chops: rechop(from, fresh) };
}

/**
 * "Clear": one chop over the region (the whole source without one), so there is something to
 * cut into by hand. Names and shaping go with the cuts — that is what clearing means.
 */
export function cleared(s: Sampler, duration: number): Sampler {
  const region = regionOf(s, duration);
  return { ...s, chopper: CLEAR_CHOPPER, chops: chopRegion(CLEAR_CHOPPER, region, s.analysis, 1) };
}

/**
 * The region with its `edge` dragged to `seconds`: inside the source, the other edge fixed, and
 * never shorter than {@link MIN_CHOP_SECONDS} (the edge stops rather than crossing over).
 */
export function dragRegionEdge(r: SampleRegion, edge: Edge, seconds: number, duration: number): SampleRegion {
  if (!Number.isFinite(seconds)) return r;
  return edge === "start"
    ? { startSeconds: Math.min(Math.max(seconds, 0), r.endSeconds - MIN_CHOP_SECONDS), endSeconds: r.endSeconds }
    : { startSeconds: r.startSeconds, endSeconds: Math.max(Math.min(seconds, duration), r.startSeconds + MIN_CHOP_SECONDS) };
}

/**
 * The region swept from `a` to `b`, either way round, held inside the source; `null` while it
 * is narrower than {@link MIN_CHOP_SECONDS}, so a plain click on the ruler selects nothing.
 */
export function sweptRegion(a: number, b: number, duration: number): SampleRegion | null {
  if (!Number.isFinite(a) || !Number.isFinite(b)) return null;
  const startSeconds = Math.max(0, Math.min(a, b));
  const endSeconds = Math.min(duration, Math.max(a, b));
  return endSeconds - startSeconds >= MIN_CHOP_SECONDS ? { startSeconds, endSeconds } : null;
}

/**
 * The region edge within `tolerance` pixels of viewport `x`: the nearer one, and on a region
 * so narrow both are in reach, the one on the pointer's side of its middle.
 */
export function regionEdgeAt(r: SampleRegion, x: number, view: WaveView, duration: number, width: number, tolerance: number): Edge | null {
  const x0 = secondsToX(r.startSeconds, view, duration, width);
  const x1 = secondsToX(r.endSeconds, view, duration, width);
  const d0 = Math.abs(x - x0);
  const d1 = Math.abs(x - x1);
  if (d0 > tolerance && d1 > tolerance) return null;
  if (d0 <= tolerance && d1 <= tolerance) return x < (x0 + x1) / 2 ? "start" : "end";
  return d0 <= tolerance ? "start" : "end";
}

// ─── Chops ─────────────────────────────────────────────────────────────────────────────────

/** The chop drawn on top at `seconds`: the last one in key order that covers it, or -1. */
export function chopAt(chops: readonly Chop[], seconds: number): number {
  for (let i = chops.length - 1; i >= 0; i--) {
    if (seconds >= chops[i].startSeconds && seconds < chops[i].endSeconds) return i;
  }
  return -1;
}

/**
 * Cut at `seconds`. Inside a chop, that chop ends there and a copy of it (same shaping, fresh
 * id, no name) starts there, right after it in key order. In a gap, a new chop runs from there to
 * the next chop's start (or the end of the source). Refused (`null`) when the sampler is full or
 * either side would be shorter than {@link MIN_CHOP_SECONDS}. Returns the new chop's index.
 */
export function cutAt(s: Sampler, seconds: number, duration: number): { sampler: Sampler; index: number } | null {
  if (s.chops.length >= MAX_CHOPS || !(seconds >= 0 && seconds < duration)) return null;
  const at = chopAt(s.chops, seconds);
  if (at >= 0) {
    const c = s.chops[at];
    if (seconds - c.startSeconds < MIN_CHOP_SECONDS || c.endSeconds - seconds < MIN_CHOP_SECONDS) return null;
    const { name: _name, ...shape } = c;
    const tail: Chop = { ...shape, id: uuid(), startSeconds: seconds };
    const chops = [...s.chops];
    chops.splice(at, 1, { ...c, endSeconds: seconds }, tail);
    return { sampler: { ...s, chops }, index: at + 1 };
  }
  const next = s.chops.reduce((m, c) => (c.startSeconds > seconds ? Math.min(m, c.startSeconds) : m), duration);
  if (next - seconds < MIN_CHOP_SECONDS) return null;
  // Placed after every chop that starts earlier, so the pads still read left to right.
  const index = s.chops.filter((c) => c.startSeconds < seconds).length;
  const chops = [...s.chops];
  chops.splice(index, 0, newChop(seconds, next));
  return { sampler: { ...s, chops }, index };
}

/** Without chop `index`; the keys of the chops after it move down by one. */
export function deleteChop(s: Sampler, index: number): Sampler {
  if (index < 0 || index >= s.chops.length) return s;
  return { ...s, chops: s.chops.filter((_, i) => i !== index) };
}

export type Edge = "start" | "end";

/** Two edges this close (seconds) are one cut: dragging it moves both chops. */
const SHARED_EDGE_SECONDS = 1e-6;

/**
 * Chop `index`'s `edge` dragged to `seconds`. The edge stays inside the source and at least
 * {@link MIN_CHOP_SECONDS} from the chop's other edge. When `linked` and a neighbour shares that
 * edge (the cut between two chops), the neighbour's edge moves with it and the clamp keeps the
 * neighbour long enough too — moving a cut never opens a gap or an overlap.
 */
export function dragEdge(s: Sampler, index: number, edge: Edge, seconds: number, duration: number, linked = true): Sampler {
  const c = s.chops[index];
  if (!c || !Number.isFinite(seconds)) return s;
  const at = edge === "start" ? c.startSeconds : c.endSeconds;
  const partner = linked
    ? s.chops.findIndex((o, i) => i !== index && Math.abs((edge === "start" ? o.endSeconds : o.startSeconds) - at) < SHARED_EDGE_SECONDS)
    : -1;
  const p = partner >= 0 ? s.chops[partner] : null;
  let lo: number;
  let hi: number;
  if (edge === "start") {
    lo = p ? p.startSeconds + MIN_CHOP_SECONDS : 0;
    hi = c.endSeconds - MIN_CHOP_SECONDS;
  } else {
    lo = c.startSeconds + MIN_CHOP_SECONDS;
    hi = p ? p.endSeconds - MIN_CHOP_SECONDS : duration;
  }
  const t = Math.min(Math.max(seconds, Math.max(0, lo)), Math.min(duration, hi));
  const chops = s.chops.map((o, i) => {
    if (i === index) return edge === "start" ? { ...o, startSeconds: t } : { ...o, endSeconds: t };
    if (i === partner) return edge === "start" ? { ...o, endSeconds: t } : { ...o, startSeconds: t };
    return o;
  });
  return { ...s, chops };
}

// ─── View geometry ─────────────────────────────────────────────────────────────────────────

/** The waveform's window onto the source: zoom 1 fits the whole source in `width` pixels. */
export interface WaveView {
  /** Source seconds at the left edge of the viewport. */
  offset: number;
  /** Multiple of the fit-to-width scale, ≥ 1. */
  zoom: number;
}

/** Deepest zoom: an 8 s source at 1000 px is then ~8 px per millisecond-ish detail. */
export const MAX_WAVE_ZOOM = 64;

/** Pixels per source second at this view. */
export function pxPerSecond(view: WaveView, duration: number, width: number): number {
  return duration > 0 ? (width / duration) * view.zoom : 0;
}

export function secondsToX(seconds: number, view: WaveView, duration: number, width: number): number {
  return (seconds - view.offset) * pxPerSecond(view, duration, width);
}

export function xToSeconds(x: number, view: WaveView, duration: number, width: number): number {
  const pps = pxPerSecond(view, duration, width);
  return pps > 0 ? view.offset + x / pps : 0;
}

/** `view` with its zoom in range and its offset keeping the viewport inside the source. */
export function clampView(view: WaveView, duration: number): WaveView {
  const zoom = Math.min(MAX_WAVE_ZOOM, Math.max(1, Number.isFinite(view.zoom) ? view.zoom : 1));
  const span = duration / zoom;
  const offset = Math.min(Math.max(0, Number.isFinite(view.offset) ? view.offset : 0), Math.max(0, duration - span));
  return { offset, zoom };
}

/** Zoom to `zoom` keeping the source second under viewport pixel `anchorX` where it is. */
export function zoomAround(view: WaveView, zoom: number, anchorX: number, duration: number, width: number): WaveView {
  const at = xToSeconds(anchorX, view, duration, width);
  const next = clampView({ offset: 0, zoom }, duration);
  const pps = pxPerSecond(next, duration, width);
  return clampView({ zoom: next.zoom, offset: pps > 0 ? at - anchorX / pps : 0 }, duration);
}

/** The view that shows chop `c` with a little room either side (never zooming out). */
export function viewShowing(view: WaveView, c: Chop, duration: number): WaveView {
  const span = duration / view.zoom;
  if (c.startSeconds >= view.offset && c.endSeconds <= view.offset + span) return view;
  return clampView({ zoom: view.zoom, offset: c.startSeconds - span * 0.1 }, duration);
}

/**
 * The chop edge within `tolerance` pixels of viewport `x`, nearest first; on a shared cut the
 * chop to the right of it (its start) wins, so a plain drag moves the cut.
 */
export function edgeAt(chops: readonly Chop[], x: number, view: WaveView, duration: number, width: number, tolerance: number): { index: number; edge: Edge } | null {
  let best: { index: number; edge: Edge; d: number } | null = null;
  for (let index = 0; index < chops.length; index++) {
    for (const edge of ["start", "end"] as const) {
      const c = chops[index];
      const d = Math.abs(secondsToX(edge === "start" ? c.startSeconds : c.endSeconds, view, duration, width) - x);
      if (d > tolerance) continue;
      const tie = best !== null && Math.abs(d - best.d) <= 0.5;
      if (!best || (!tie && d < best.d) || (tie && edge === "start" && best.edge === "end")) best = { index, edge, d };
    }
  }
  return best && { index: best.index, edge: best.edge };
}

// ─── Pads ──────────────────────────────────────────────────────────────────────────────────

/** Pads per page of the pad grid (4 × 4). */
export const PADS_PER_PAGE = 16;

/**
 * The computer keys that play a page of pads, in pad order: pad 1 is bottom-left, as on an
 * MPC — Z X C V is the bottom row, then A S D F, Q W E R, and 1 2 3 4 on top.
 */
export const PAD_KEYS = ["z", "x", "c", "v", "a", "s", "d", "f", "q", "w", "e", "r", "1", "2", "3", "4"] as const;

/** The 16 slots of one page in grid order (top row first), as chop indices; -1 is an empty slot. */
export function pageSlots(page: number, chopCount: number): number[] {
  return [3, 2, 1, 0].flatMap((row) =>
    [0, 1, 2, 3].map((col) => {
      const i = page * PADS_PER_PAGE + row * 4 + col;
      return i < chopCount ? i : -1;
    }),
  );
}
