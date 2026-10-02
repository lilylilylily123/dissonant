// A small mirror of dissonant-core's theory, used for 60 fps rendering so the roll never has
// to round-trip to Rust per frame. Rust remains the source of truth; tests pin both to the
// same expectations.

import type { ChordEvent, KeyState, ScaleType, Tier } from "./types";
import { uuid } from "./types";

export const NOTE_NAMES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
const MAJOR_STEPS = [0, 2, 4, 5, 7, 9, 11];
const MINOR_STEPS = [0, 2, 3, 5, 7, 8, 10];

export const normalize = (pc: number): number => ((pc % 12) + 12) % 12;

export function semitoneDistance(a: number, b: number): number {
  const d = Math.abs(a - b) % 12;
  return Math.min(d, 12 - d);
}

export const noteName = (pc: number): string => NOTE_NAMES[normalize(pc)];
export const midiName = (pitch: number): string => `${noteName(pitch)}${Math.floor(pitch / 12) - 1}`;
export const keyName = (root: number, scale: ScaleType): string =>
  `${noteName(root)} ${scale === "major" ? "maj" : "min"}`;

export function scalePitchClasses(root: number, scale: ScaleType): number[] {
  return (scale === "major" ? MAJOR_STEPS : MINOR_STEPS).map((s) => normalize(root + s));
}

export function keyPitchClasses(key: KeyState): number[] | null {
  return key.rootPitchClass === null ? null : scalePitchClasses(key.rootPitchClass, key.scale);
}

export interface ChordSuggestion {
  pitchClasses: number[];
  name: string;
  romanNumeral: string;
}

type Quality = "major" | "minor" | "dim" | "aug" | "other";

function qualityOf(third: number, fifth: number): Quality {
  if (third === 4 && fifth === 7) return "major";
  if (third === 3 && fifth === 7) return "minor";
  if (third === 3 && fifth === 6) return "dim";
  if (third === 4 && fifth === 8) return "aug";
  return "other";
}

const SUFFIX: Record<Quality, string> = { major: "", minor: "m", dim: "dim", aug: "aug", other: "?" };

function roman(q: Quality, degree: number): string {
  const base = ["I", "II", "III", "IV", "V", "VI", "VII"][degree % 7];
  switch (q) {
    case "major":
      return base;
    case "aug":
      return `${base}+`;
    case "minor":
      return base.toLowerCase();
    case "dim":
      return `${base.toLowerCase()}°`;
    default:
      return `${base}?`;
  }
}

export function diatonicChords(root: number, scale: ScaleType): ChordSuggestion[] {
  const pcs = scalePitchClasses(root, scale);
  return Array.from({ length: 7 }, (_, degree) => {
    const chord = [degree, degree + 2, degree + 4].map((d) => pcs[d % 7]);
    const q = qualityOf(normalize(chord[1] - chord[0]), normalize(chord[2] - chord[0]));
    return { pitchClasses: chord, name: `${noteName(chord[0])}${SUFFIX[q]}`, romanNumeral: roman(q, degree) };
  });
}

export function chordName(pitchClasses: number[]): string {
  const pcs = [...new Set(pitchClasses.map(normalize))].sort((a, b) => a - b);
  if (pcs.length === 0) return "—";
  if (pcs.length === 3) {
    for (const root of pcs) {
      const iv = pcs.map((p) => normalize(p - root)).sort((a, b) => a - b);
      const q = qualityOf(iv[1], iv[2]);
      if (q !== "other") return `${noteName(root)}${SUFFIX[q]}`;
    }
  }
  return pcs.map(noteName).join("·");
}

export interface Starter {
  name: string;
  degrees: number[];
}

export const STARTERS: Starter[] = [
  { name: "I–IV–V–vi", degrees: [0, 3, 4, 5] },
  { name: "I–V–vi–IV", degrees: [0, 4, 5, 3] },
  { name: "vi–IV–I–V", degrees: [5, 3, 0, 4] },
  { name: "ii–V–I", degrees: [1, 4, 0, 0] },
  { name: "I–vi–IV–V", degrees: [0, 5, 3, 4] },
  { name: "i–VI–III–VII", degrees: [0, 5, 2, 6] },
];

export function progression(
  degrees: number[],
  root: number,
  scale: ScaleType,
  totalBeats: number,
  chordBeats = 4,
): ChordEvent[] {
  if (degrees.length === 0 || chordBeats <= 0) return [];
  const diatonic = diatonicChords(root, scale);
  const slots = Math.max(Math.floor(totalBeats / chordBeats), degrees.length);
  return Array.from({ length: slots }, (_, i) => {
    const c = diatonic[((degrees[i % degrees.length] % 7) + 7) % 7];
    return {
      id: uuid(),
      startBeat: i * chordBeats,
      lengthBeats: chordBeats,
      pitchClasses: [...c.pitchClasses],
      name: c.name,
    };
  });
}

// ─── Tiers ─────────────────────────────────────────────────────────────────────────────────

export function tier(pitchClass: number, chord: number[], key: number[] | null): Tier {
  const p = normalize(pitchClass);
  const chordSet = chord.map(normalize);
  if (chordSet.includes(p)) return "chordTone";
  const nearest = chordSet.reduce((m, c) => Math.min(m, semitoneDistance(p, c)), 12);
  if (nearest === 1) return "dissonance";
  if (key && !key.some((k) => normalize(k) === p)) return "dissonance";
  return "tension";
}

export function chordAt(chords: ChordEvent[], beat: number): ChordEvent | null {
  for (let i = chords.length - 1; i >= 0; i--) {
    const c = chords[i];
    if (beat >= c.startBeat && beat < c.startBeat + c.lengthBeats) return c;
  }
  return null;
}

/** Tier for every pitch class at `beat`; `null` entries mean "no guidance". */
export function tierMap(beat: number, chords: ChordEvent[], key: KeyState): (Tier | null)[] {
  const keyPcs = keyPitchClasses(key);
  const chord = chordAt(chords, beat);
  if (chord) return Array.from({ length: 12 }, (_, pc) => tier(pc, chord.pitchClasses, keyPcs));
  if (keyPcs) return Array.from({ length: 12 }, (_, pc) => (keyPcs.includes(pc) ? "chordTone" : "dissonance"));
  return Array(12).fill(null);
}

// ─── Key detection (Krumhansl–Schmuckler) ─────────────────────────────────────────────────

const MAJOR_PROFILE = [6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88];
const MINOR_PROFILE = [6.33, 2.68, 3.52, 5.38, 2.6, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17];

export interface KeyCandidate {
  rootPitchClass: number;
  scale: ScaleType;
  score: number;
}

export interface KeyDetectionResult {
  candidates: KeyCandidate[];
  isConfident: boolean;
}

function pearson(x: number[], y: number[]): number {
  const n = x.length;
  const mx = x.reduce((a, b) => a + b, 0) / n;
  const my = y.reduce((a, b) => a + b, 0) / n;
  let num = 0;
  let dx = 0;
  let dy = 0;
  for (let i = 0; i < n; i++) {
    const a = x[i] - mx;
    const b = y[i] - my;
    num += a * b;
    dx += a * a;
    dy += b * b;
  }
  const den = Math.sqrt(dx * dy);
  return den === 0 ? 0 : num / den;
}

export function detectKey(pitchClasses: number[], minNotes = 10, minGap = 0.04): KeyDetectionResult {
  if (pitchClasses.length === 0) return { candidates: [], isConfident: false };
  const hist = Array(12).fill(0);
  for (const pc of pitchClasses) hist[normalize(pc)] += 1;
  const candidates: KeyCandidate[] = [];
  for (let root = 0; root < 12; root++) {
    for (const scale of ["major", "minor"] as ScaleType[]) {
      const base = scale === "major" ? MAJOR_PROFILE : MINOR_PROFILE;
      const profile = Array.from({ length: 12 }, (_, i) => base[normalize(i - root)]);
      candidates.push({ rootPitchClass: root, scale, score: pearson(hist, profile) });
    }
  }
  candidates.sort((a, b) => b.score - a.score);
  const gap = candidates[0].score - candidates[1].score;
  return { candidates, isConfident: pitchClasses.length >= minNotes && gap >= minGap };
}

// ─── Explain this note ─────────────────────────────────────────────────────────────────────

const INTERVAL_NAMES = ["the root", "a ♭9", "the 9th", "a ♭3 / ♯9", "the 3rd", "the 11th", "a ♯11", "the 5th", "a ♭13", "the 13th", "the ♭7", "the 7th"];

/** Root pitch class of a chord: the named root if the chord is a recognizable triad, else its lowest pitch class. */
export function chordRoot(chord: ChordEvent): number {
  const name = chord.name ?? chordName(chord.pitchClasses);
  const m = /^([A-G]#?)/.exec(name);
  if (m) {
    const idx = NOTE_NAMES.indexOf(m[1]);
    if (idx >= 0) return idx;
  }
  return chord.pitchClasses[0] ?? 0;
}

/**
 * Plain words for why a pitch is tiered the way it is against the chord at `beat`.
 * The voice from PRODUCT.md: names things plainly, never condescends, never forbids.
 */
export function explainNote(pitch: number, chords: ChordEvent[], key: KeyState, beat: number): string {
  const chord = chordAt(chords, beat);
  const keyPcs = keyPitchClasses(key);
  const pc = normalize(pitch);
  const name = noteName(pc);
  if (!chord) {
    if (!keyPcs) return "no chord or key yet — anything goes";
    return keyPcs.includes(pc)
      ? `${name} is in ${keyName(key.rootPitchClass!, key.scale)}`
      : `${name} is outside ${keyName(key.rootPitchClass!, key.scale)} — flagged, not forbidden`;
  }
  const label = chord.name ?? chordName(chord.pitchClasses);
  const root = chordRoot(chord);
  const interval = INTERVAL_NAMES[normalize(pc - root)];
  const t = tier(pc, chord.pitchClasses, keyPcs);
  if (t === "chordTone") return `${name} over ${label}: ${interval}. Solid.`;
  if (t === "tension") return `${name} over ${label}: ${interval}. Spicy but good.`;
  // Dissonance: say which chord tone it rubs against and where it wants to go.
  const above = chord.pitchClasses.map(normalize).find((c) => normalize(pc - c) === 1);
  const below = chord.pitchClasses.map(normalize).find((c) => normalize(c - pc) === 1);
  if (above !== undefined) return `${name} over ${label}: a half step above ${noteName(above)}. It wants to fall to ${noteName(above)} — or stay, on purpose.`;
  if (below !== undefined) return `${name} over ${label}: a half step below ${noteName(below)}. It leans up into ${noteName(below)} — or stays, on purpose.`;
  return `${name} over ${label}: outside the key. Flagged, not forbidden.`;
}
