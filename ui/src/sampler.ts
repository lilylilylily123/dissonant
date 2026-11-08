/**
 * The sampler's rules, mirrored from `crates/dissonant-core/src/sampler.rs`: what a chop may be
 * ({@link sanitizedSampler}), how cut points become chops ({@link chopsAt}), which onsets a
 * fetch cuts at ({@link strongestOnsets}), the region a fresh link suggests
 * ({@link suggestedRegion}), what a chop tool cuts a region into ({@link chopRegion}) and what a
 * re-cut keeps ({@link rechop}). The mock bridge applies the sampler commands with these, so
 * the browser build refuses and clamps exactly what the desktop app does.
 */
import { uuid, type AudioSource, type Chop, type Chopper, type Onset, type SampleAnalysis, type SampleRegion, type Sampler } from "./types";

/** Most chops one sampler holds. */
export const MAX_CHOPS = 64;
/** Shortest chop, in seconds. */
export const MIN_CHOP_SECONDS = 0.01;
/** The key chop 0 plays on a new sampler (C1). */
export const DEFAULT_SAMPLER_BASE_PITCH = 36;
/** Loudest a chop may be boosted, linear. */
export const MAX_CHOP_GAIN = 4;
export const MAX_CHOP_ATTACK_SECONDS = 2;
export const MAX_CHOP_RELEASE_SECONDS = 5;
/** A new chop's release: just long enough that letting go of a key never clicks. */
export const DEFAULT_CHOP_RELEASE_SECONDS = 0.01;
/** Highest choke group; 0 chokes nothing. */
export const MAX_CHOKE_GROUP = 16;
/** Longest source a sampler will take, in seconds (10 minutes). */
export const MAX_SAMPLE_SECONDS = 600;
/** Most onsets an analysis keeps: the strongest. */
export const MAX_ONSETS = 512;
/** How strong an onset must be to start a suggested region: the first real downbeat. */
export const STRONG_ONSET = 0.5;
/** Bars in a suggested region when the source has a tempo. */
export const SUGGESTED_REGION_BARS = 4;
/** A suggested region's length, seconds, when the source has no tempo. */
export const SUGGESTED_REGION_SECONDS = 8;
/** Chops a fresh link is cut into: a 4 × 4 pad page. */
export const DEFAULT_CHOP_COUNT = 16;

/** `v` inside `[lo, hi]`, or `fallback` when it is not a number at all (Rust's `ranged`). */
function ranged(v: number, lo: number, hi: number, fallback: number): number {
  return Number.isFinite(v) ? Math.min(Math.max(v, lo), hi) : fallback;
}

/** A whole number in `[0, hi]`: what Rust's `u8` fields can hold. */
function byte(v: number, hi: number, fallback: number): number {
  return Number.isFinite(v) ? Math.min(Math.max(Math.round(v), 0), hi) : fallback;
}

/** `AudioSource::duration_seconds`: length at the source's own rate, 0 for a degenerate one. */
export function sourceSeconds(source: AudioSource): number {
  return source.sampleRate === 0 ? 0 : source.frames / source.sampleRate;
}

/** `Chop::new`: a chop over `[startSeconds, endSeconds)` with every other field at its default. */
export function newChop(startSeconds: number, endSeconds: number): Chop {
  return {
    id: uuid(),
    startSeconds,
    endSeconds,
    gain: 1,
    reverse: false,
    mode: "oneShot",
    attackSeconds: 0,
    releaseSeconds: DEFAULT_CHOP_RELEASE_SECONDS,
    choke: 0,
  };
}

/** Strongest first; among equals the earlier hit first (Rust's `sort_strongest_first`). */
function sortStrongestFirst(onsets: Onset[]): void {
  onsets.sort((a, b) => b.strength - a.strength || a.seconds - b.seconds);
}

/** `SampleAnalysis::sanitized` against a source `duration` seconds long. */
function sanitizedAnalysis(a: SampleAnalysis, duration: number): SampleAnalysis {
  let onsets = (a.onsets ?? [])
    .filter((o) => Number.isFinite(o.seconds) && Number.isFinite(o.strength))
    .filter((o) => o.seconds >= 0 && o.seconds <= duration)
    .map((o) => ({ seconds: o.seconds, strength: Math.min(Math.max(o.strength, 0), 1) }));
  if (onsets.length > MAX_ONSETS) {
    sortStrongestFirst(onsets);
    onsets = onsets.slice(0, MAX_ONSETS);
  }
  onsets.sort((x, y) => x.seconds - y.seconds);
  const bpm = a.bpm ?? null;
  const root = a.keyRoot ?? null;
  return {
    bpm: bpm !== null && Number.isFinite(bpm) && bpm > 0 ? bpm : null,
    bpmConfidence: ranged(a.bpmConfidence ?? 0, 0, 1, 0),
    keyRoot: root !== null && Number.isInteger(root) && root >= 0 && root < 12 ? root : null,
    keyScale: a.keyScale ?? "major",
    keyConfidence: ranged(a.keyConfidence ?? 0, 0, 1, 0),
    onsets,
  };
}

/**
 * `Sampler::sanitized`: every chop held inside a source `sourceSeconds` long and its own
 * ranges, in the same order, a chop that cannot fit dropped; `basePitch` lowered until the last
 * chop still has a MIDI key; the region held inside the source (dropped when shorter than
 * {@link MIN_CHOP_SECONDS}) and the chopper's count in 1…{@link MAX_CHOPS}. `null` when there
 * are more than {@link MAX_CHOPS} chops. Never mutates its argument.
 */
export function sanitizedSampler(s: Sampler, sourceSeconds: number): Sampler | null {
  const input = s.chops ?? [];
  if (input.length > MAX_CHOPS) return null;
  const duration = ranged(sourceSeconds, 0, Number.MAX_VALUE, 0);
  const chops: Chop[] = [];
  for (const c of input) {
    if (!Number.isFinite(c.startSeconds) || !Number.isFinite(c.endSeconds)) continue;
    const start = Math.min(Math.max(c.startSeconds, 0), duration);
    if (start + MIN_CHOP_SECONDS > duration) continue;
    const chop: Chop = {
      id: c.id ?? uuid(),
      startSeconds: start,
      endSeconds: Math.min(Math.max(c.endSeconds, start + MIN_CHOP_SECONDS), duration),
      gain: ranged(c.gain ?? 1, 0, MAX_CHOP_GAIN, 1),
      reverse: c.reverse ?? false,
      mode: c.mode === "gate" ? "gate" : "oneShot",
      attackSeconds: ranged(c.attackSeconds ?? 0, 0, MAX_CHOP_ATTACK_SECONDS, 0),
      releaseSeconds: ranged(
        c.releaseSeconds ?? DEFAULT_CHOP_RELEASE_SECONDS,
        0,
        MAX_CHOP_RELEASE_SECONDS,
        DEFAULT_CHOP_RELEASE_SECONDS,
      ),
      choke: byte(c.choke ?? 0, MAX_CHOKE_GROUP, 0),
    };
    // Rust skips an empty name on the wire, so the mock never stores one either.
    if (c.name) chop.name = c.name;
    chops.push(chop);
  }
  const top = 127 - Math.max(chops.length - 1, 0);
  const out: Sampler = {
    sourceId: s.sourceId,
    basePitch: Math.min(byte(s.basePitch ?? DEFAULT_SAMPLER_BASE_PITCH, 127, DEFAULT_SAMPLER_BASE_PITCH), top),
    chops,
    // A sampler from before mono/poly plays mono, like Rust's `#[serde(default)]`.
    voice: s.voice === "poly" ? "poly" : "mono",
  };
  if (s.analysis) out.analysis = sanitizedAnalysis(s.analysis, duration);
  const region = s.region && sanitizedRegion(s.region, duration);
  if (region) out.region = region;
  if (s.chopper) out.chopper = sanitizedChopper(s.chopper);
  return out;
}

/** `SampleRegion::sanitized`: held inside the source, or `null` when too short to cut. */
function sanitizedRegion(r: SampleRegion, duration: number): SampleRegion | null {
  if (!Number.isFinite(r.startSeconds) || !Number.isFinite(r.endSeconds)) return null;
  const startSeconds = Math.min(Math.max(r.startSeconds, 0), duration);
  const endSeconds = Math.min(Math.max(r.endSeconds, 0), duration);
  return endSeconds - startSeconds >= MIN_CHOP_SECONDS ? { startSeconds, endSeconds } : null;
}

/** `chop_count`: a whole chop count in 1…{@link MAX_CHOPS} (Rust's `u32` holds no fraction). */
function chopCount(count: number): number {
  return Number.isFinite(count) ? Math.min(Math.max(Math.floor(count), 1), MAX_CHOPS) : 1;
}

/** `Chopper::sanitized`: the same tool with its count in 1…{@link MAX_CHOPS}. */
function sanitizedChopper(c: Chopper): Chopper {
  return c.kind === "grid" ? { kind: "grid", step: c.step } : { kind: c.kind, count: chopCount(c.count) };
}

/**
 * `chops_at`: chops between consecutive cut points over a source `duration` seconds long, the
 * last ending at `duration`. 0 is always a cut; points outside the source, not numbers, or
 * closer than {@link MIN_CHOP_SECONDS} to the previous cut (or to the end) are ignored. At most
 * {@link MAX_CHOPS} chops, the last running to the end.
 */
export function chopsAt(points: readonly number[], duration: number): Chop[] {
  return chopsBetween(points, 0, duration);
}

/** `chops_between`: {@link chopsAt} over `[start, end)`; `start` is always a cut. */
function chopsBetween(points: readonly number[], start: number, end: number): Chop[] {
  if (!Number.isFinite(start) || !Number.isFinite(end) || end - start < MIN_CHOP_SECONDS) return [];
  const sorted = points.filter((p) => Number.isFinite(p) && p > start && p < end).sort((a, b) => a - b);
  const cuts = [start];
  for (const p of sorted) {
    if (cuts.length === MAX_CHOPS) break;
    if (p - cuts[cuts.length - 1] >= MIN_CHOP_SECONDS && end - p >= MIN_CHOP_SECONDS) cuts.push(p);
  }
  return cuts.map((from, i) => newChop(from, cuts[i + 1] ?? end));
}

/** `strongest_onsets`: the `count` strongest onset times, ascending; ties go to the earlier hit. */
export function strongestOnsets(onsets: readonly Onset[], count: number): number[] {
  const picked = onsets.filter((o) => Number.isFinite(o.seconds) && Number.isFinite(o.strength));
  sortStrongestFirst(picked);
  return picked
    .slice(0, Math.max(0, count))
    .map((o) => o.seconds)
    .sort((a, b) => a - b);
}

/**
 * `suggested_region`: from the first hit at least {@link STRONG_ONSET} strong (else the start)
 * for {@link SUGGESTED_REGION_BARS} bars of `beatsPerBar` at the detected tempo, or
 * {@link SUGGESTED_REGION_SECONDS} without one; held inside a source `duration` long, sliding
 * back to keep its length when a hit near the end would leave a sliver.
 */
export function suggestedRegion(analysis: SampleAnalysis, duration: number, beatsPerBar: number): SampleRegion {
  const d = ranged(duration, 0, Number.MAX_VALUE, 0);
  const strong = analysis.onsets.filter((o) => Number.isFinite(o.seconds) && o.strength >= STRONG_ONSET);
  const first = strong.reduce((m, o) => Math.min(m, o.seconds), Infinity);
  const downbeat = Number.isFinite(first) ? Math.min(Math.max(first, 0), d) : 0;
  const bpm = analysis.bpm;
  const length =
    bpm != null && Number.isFinite(bpm) && bpm > 0
      ? (SUGGESTED_REGION_BARS * Math.max(1, Math.floor(beatsPerBar)) * 60) / bpm
      : SUGGESTED_REGION_SECONDS;
  const endSeconds = Math.min(downbeat + length, d);
  const startSeconds = endSeconds - downbeat >= MIN_CHOP_SECONDS ? downbeat : Math.max(endSeconds - length, 0);
  return { startSeconds, endSeconds };
}

/**
 * `chop_region`: what `chopper` cuts `region` into — `auto` at most `count` chops at the
 * region's start and its `count - 1` strongest hits inside; `equal` `count` equal chops;
 * `grid` a cut every bar (of `beatsPerBar`), half bar or beat of the analysis tempo from the
 * region's start, nothing without a tempo. A region shorter than {@link MIN_CHOP_SECONDS} gives
 * nothing. `beatsPerBar` is a whole count, as Rust's `u32`.
 */
export function chopRegion(chopper: Chopper, region: SampleRegion, analysis: SampleAnalysis | undefined, beatsPerBar: number): Chop[] {
  const { startSeconds: start, endSeconds: end } = region;
  if (!Number.isFinite(start) || !Number.isFinite(end) || end - start < MIN_CHOP_SECONDS) return [];
  switch (chopper.kind) {
    case "auto": {
      const inside = (analysis?.onsets ?? []).filter((o) => o.seconds - start >= MIN_CHOP_SECONDS && end - o.seconds >= MIN_CHOP_SECONDS);
      return chopsBetween(strongestOnsets(inside, chopCount(chopper.count) - 1), start, end);
    }
    case "equal": {
      const n = chopCount(chopper.count);
      const length = end - start;
      return chopsBetween(
        Array.from({ length: n - 1 }, (_, i) => start + ((i + 1) * length) / n),
        start,
        end,
      );
    }
    case "grid": {
      const bpm = analysis?.bpm;
      if (bpm == null || !Number.isFinite(bpm) || bpm <= 0) return [];
      const bar = Math.max(1, Math.floor(beatsPerBar));
      const beats = chopper.step === "bar" ? bar : chopper.step === "half" ? bar / 2 : 1;
      const period = (beats * 60) / bpm;
      if (period < MIN_CHOP_SECONDS) return [];
      const points: number[] = [];
      for (let k = 1; k < MAX_CHOPS; k++) {
        const t = start + k * period;
        if (t >= end) break;
        points.push(t);
      }
      return chopsBetween(points, start, end);
    }
  }
}

/**
 * `rechop`: `fresh` cuts carrying chop *i*'s name, gain, reverse, mode, attack, release and
 * choke from `old` where there is a chop *i*; the rest stay at their defaults. Ids are fresh.
 */
export function rechop(old: readonly Chop[], fresh: readonly Chop[]): Chop[] {
  return fresh.map((c, i) => {
    const o = old[i];
    if (!o) return c;
    const out: Chop = {
      id: c.id,
      startSeconds: c.startSeconds,
      endSeconds: c.endSeconds,
      gain: o.gain,
      reverse: o.reverse,
      mode: o.mode,
      attackSeconds: o.attackSeconds,
      releaseSeconds: o.releaseSeconds,
      choke: o.choke,
    };
    if (o.name) out.name = o.name;
    return out;
  });
}
