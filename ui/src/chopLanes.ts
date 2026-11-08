// Pure geometry for the chop lanes (`components/sample/ChopLanes.tsx`): which chop each row
// shows, how tall the rows are, how long each hit really sounds and where the next hit cuts
// it, and moving hits between chops. Works in ticks, seconds and MIDI pitches; no pixels.

import { moveHitsByRow, moveNotes } from "./noteEditing";
import { shiftedStart } from "./ropes";
import type { Chop, Clip, NoteEvent, Sampler, SongPattern } from "./types";

/** The key chop `index` plays on. */
export const chopPitch = (s: Sampler, index: number): number => s.basePitch + index;

/** The chop a pitch plays, or -1 when no chop sits on that key. */
export function chopOfPitch(s: Sampler, pitch: number): number {
  const i = pitch - s.basePitch;
  return i >= 0 && i < s.chops.length ? i : -1;
}

/**
 * The chop each row shows, top row first. Chop 1 sits at the bottom, like the lowest key of a
 * keyboard. `usedOnly` keeps the chops the pattern plays (a muted hit still counts: it is
 * there to be unmuted), so a sparse pattern over a big chop set fits on screen.
 */
export function laneChops(s: Sampler, notes: readonly NoteEvent[], usedOnly: boolean): number[] {
  const used = usedOnly ? new Set(notes.map((n) => chopOfPitch(s, n.pitch))) : null;
  const out: number[] = [];
  for (let i = s.chops.length - 1; i >= 0; i--) if (!used || used.has(i)) out.push(i);
  return out;
}

/**
 * Row height: the rows share `availPx` until there are more than `fit` of them, past which
 * they stop shrinking and the lanes scroll. Clamped to `[min, max]` so a two-chop sampler does
 * not get rows half a screen tall and a cramped panel still leaves a row you can click.
 */
export function laneRowHeight(availPx: number, rows: number, opts: { fit?: number; min: number; max: number }): number {
  const fit = opts.fit ?? 16;
  const share = Math.floor(Math.max(0, availPx) / Math.max(1, Math.min(rows, fit)));
  return Math.min(opts.max, Math.max(opts.min, share));
}

/**
 * The tick `seconds` after `fromTick` along a tempo map's `secondsAt`, found by bisection so a
 * ramp is followed exactly rather than read at one tempo.
 */
export function ticksAfter(secondsAt: (tick: number) => number, fromTick: number, seconds: number): number {
  if (!(seconds > 0)) return fromTick;
  const target = secondsAt(fromTick) + seconds;
  let lo = fromTick;
  let hi = fromTick + 1;
  // Grow until the bracket holds the target; tempo is finite and positive, so this ends.
  for (let i = 0; i < 64 && secondsAt(hi) < target; i++) {
    lo = hi;
    hi = fromTick + (hi - fromTick) * 2;
  }
  for (let i = 0; i < 48; i++) {
    const mid = (lo + hi) / 2;
    if (secondsAt(mid) < target) lo = mid;
    else hi = mid;
  }
  return hi;
}

/** How long a chop plays once struck, in seconds of source (no repitch, so also of output). */
export const chopSeconds = (c: Chop): number => Math.max(0, c.endSeconds - c.startSeconds);

/** One hit as it is heard: where its body is drawn, and where its sound really ends. */
export interface HitSound {
  id: string;
  chop: number;
  /** Where the note sounds (its grid start plus its shift). */
  start: number;
  /**
   * Where the solid body ends. A gate note's body is its own length, which is what you drag;
   * a one-shot's is its length only as far as the chop is still sounding.
   */
  bodyEnd: number;
  /** Where the sound stops, cut or not: the faded tail runs from `bodyEnd` to here. */
  end: number;
  /** Where the sound would stop if nothing cut it. */
  natural: number;
  /** The later hit's tick that cut this one short, or null when it plays out. */
  cutAt: number | null;
}

/**
 * Every unmuted hit on a chop, with its real sound. A one-shot plays the chop's length from
 * its last strike (a note with repeats strikes `repeats + 1` times across its own length, as
 * the engine schedules it); a gate holds until its note ends or the chop runs out, then rings
 * on for the chop's release. Either is cut by the next later strike that would cut it in the
 * engine: on a mono sampler any strike at all, on a poly one the same chop again or a chop in
 * the same non-zero choke group.
 *
 * `secondsAt` is the tempo map the pattern plays along, in pattern ticks.
 */
export function hitSounds(s: Sampler, notes: readonly NoteEvent[], secondsAt: (tick: number) => number): HitSound[] {
  const mono = s.voice === "mono";
  const live = notes.filter((n) => !n.muted && chopOfPitch(s, n.pitch) >= 0);
  const strikes = live
    .flatMap((n) => {
      const start = shiftedStart(n);
      const hits = Math.min(14, Math.max(0, n.repeats ?? 0)) + 1;
      return Array.from({ length: hits }, (_, h) => ({ id: n.id, chop: chopOfPitch(s, n.pitch), tick: start + (n.lengthTicks * h) / hits }));
    })
    .sort((a, b) => a.tick - b.tick);
  return live.map((n) => {
    const chop = chopOfPitch(s, n.pitch);
    const c = s.chops[chop];
    const start = shiftedStart(n);
    const hits = Math.min(14, Math.max(0, n.repeats ?? 0)) + 1;
    const last = start + (n.lengthTicks * (hits - 1)) / hits;
    const noteEnd = start + n.lengthTicks;
    const played = ticksAfter(secondsAt, last, chopSeconds(c));
    let natural: number;
    let bodyEnd: number;
    if (c.mode === "gate") {
      const held = Math.min(noteEnd, played);
      natural = ticksAfter(secondsAt, held, c.releaseSeconds);
      bodyEnd = noteEnd;
    } else {
      natural = played;
      bodyEnd = Math.min(noteEnd, played);
    }
    const cutter = strikes.find(
      (k) => k.tick > last && k.id !== n.id && (mono || k.chop === chop || (c.choke !== 0 && s.chops[k.chop].choke === c.choke)),
    );
    const cutAt = cutter && cutter.tick < natural ? cutter.tick : null;
    return { id: n.id, chop, start, bodyEnd, end: cutAt ?? natural, natural, cutAt };
  });
}

/**
 * Move hits `dTicks` in time and `dRows` rows along `rowPitches` (the shown rows' keys, bottom
 * row first, so a positive `dRows` moves up to a later chop). Both are clamped for the whole
 * selection at once, so a group keeps its shape against the pattern's edges and the outer rows.
 */
export function moveHits(
  notes: NoteEvent[],
  ids: Set<string>,
  dTicks: number,
  dRows: number,
  rowPitches: number[],
  maxEnd: number,
): NoteEvent[] {
  const timed = moveNotes(notes, ids, dTicks, 0, { min: 0, max: 127 }, maxEnd);
  return moveHitsByRow(timed, ids, rowPitches, dRows);
}

/**
 * The waveform of `[startSeconds, endSeconds)` out of a whole source's `peaks` (min/max pairs
 * over `duration` seconds), folded into `width` columns for a row header. A reversed chop is
 * drawn back to front, as it plays.
 */
export function spanPeaks(
  peaks: readonly [number, number][],
  duration: number,
  startSeconds: number,
  endSeconds: number,
  width: number,
  reverse = false,
): [number, number][] {
  const cols = Math.max(0, Math.floor(width));
  if (!peaks.length || !(duration > 0) || !cols) return [];
  const per = peaks.length / duration;
  const b0 = Math.max(0, Math.min(peaks.length, startSeconds * per));
  const b1 = Math.max(b0, Math.min(peaks.length, endSeconds * per));
  const out: [number, number][] = [];
  for (let x = 0; x < cols; x++) {
    const from = b0 + ((b1 - b0) * x) / cols;
    const to = b0 + ((b1 - b0) * (x + 1)) / cols;
    let lo = 0;
    let hi = 0;
    // Every bucket the column touches, or the one it sits inside when it is narrower than one.
    const first = Math.min(peaks.length - 1, Math.floor(from));
    const last = Math.max(first, Math.min(peaks.length - 1, Math.ceil(to) - 1));
    for (let i = first; i <= last; i++) {
      lo = Math.min(lo, peaks[i][0]);
      hi = Math.max(hi, peaks[i][1]);
    }
    out.push([lo, hi]);
  }
  return reverse ? out.reverse() : out;
}

// ─── The SAMPLE strip's song time ──────────────────────────────────────────────────────────
// SAMPLE plays the song, so its playhead is a song tick, while the strip edits one pattern.
// The two meet only where a clip of that pattern is heard: the same clip `place_recorded_note`
// files a recorded hit through (the latest-starting clip covering the tick). Anywhere else the
// pattern is not playing, and the strip says so by showing no playhead rather than folding the
// song tick in with a modulo.

/** The clip of `patternId` heard at song tick `tick`: of those covering it, the one starting last. */
export function clipHeardAt(clips: readonly Clip[], patternId: string, tick: number): Clip | null {
  let heard: Clip | null = null;
  for (const c of clips) {
    if (c.patternId !== patternId || tick < c.startTick || tick >= c.startTick + c.lengthTicks) continue;
    if (!heard || c.startTick >= heard.startTick) heard = c;
  }
  return heard;
}

/** Song tick → pattern tick, through the clip heard there; `null` where the pattern is not playing. */
export function patternTickAt(clips: readonly Clip[], pattern: SongPattern, songTick: number): number | null {
  const clip = clipHeardAt(clips, pattern.id, songTick);
  if (!clip) return null;
  const len = Math.max(1, pattern.lengthTicks);
  return (((songTick - clip.startTick + clip.offsetTicks) % len) + len) % len;
}

/**
 * Pattern tick → song tick, for a click on the strip's ruler: in the pass of the pattern being
 * heard at `songTick` when it is, else the first place any clip of it plays that tick (clips in
 * song order). `null` when no clip of the pattern ever plays that tick.
 */
export function songTickOf(clips: readonly Clip[], pattern: SongPattern, patternTick: number, songTick: number): number | null {
  const len = Math.max(1, pattern.lengthTicks);
  const p = Math.min(Math.max(patternTick, 0), len);
  const end = (c: Clip) => c.startTick + c.lengthTicks;
  const heard = clipHeardAt(clips, pattern.id, songTick);
  if (heard) {
    let t = songTick - (patternTickAt(clips, pattern, songTick) ?? 0) + p;
    if (t < heard.startTick) t += len;
    if (t < end(heard)) return t;
  }
  const mine = clips.filter((c) => c.patternId === pattern.id).sort((a, b) => a.startTick - b.startTick);
  for (const c of mine) {
    const t = c.startTick + ((((p - c.offsetTicks) % len) + len) % len);
    if (t < end(c)) return t;
  }
  return null;
}
