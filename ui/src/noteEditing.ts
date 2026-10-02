// Pure note-editing math for the piano roll. Geometry-free: works in beats and MIDI pitches.
// The roll previews with these during a drag and commits one `setNotes` on release.

import type { NoteEvent } from "./types";
import { uuid } from "./types";

export const MIN_LENGTH = 1 / 16;

export const snapFloor = (beat: number, grid: number): number => Math.floor(beat / grid + 1e-9) * grid;
export const snapRound = (beat: number, grid: number): number => Math.round(beat / grid) * grid;

export interface PitchRange {
  min: number;
  max: number;
}

export function noteAt(notes: NoteEvent[], beat: number, pitch: number): NoteEvent | null {
  for (let i = notes.length - 1; i >= 0; i--) {
    const n = notes[i];
    if (n.pitch === pitch && beat >= n.startBeat && beat < n.startBeat + n.lengthBeats) return n;
  }
  return null;
}

/** New note at a snapped slot, or null if a same-pitch note already covers it. */
export function placeNote(
  notes: NoteEvent[],
  beat: number,
  pitch: number,
  length: number,
  grid: number,
  velocity = 100,
): NoteEvent | null {
  const start = snapFloor(beat, grid);
  const overlaps = notes.some(
    (n) => n.pitch === pitch && n.startBeat < start + length && start < n.startBeat + n.lengthBeats,
  );
  if (overlaps) return null;
  return { id: uuid(), startBeat: start, lengthBeats: length, pitch, velocity };
}

/**
 * Move `ids` by a delta, clamping the delta so no moved note leaves the pitch range or starts
 * before 0 (the whole selection shifts together, so relative positions survive).
 */
export function moveNotes(
  notes: NoteEvent[],
  ids: Set<string>,
  deltaBeats: number,
  deltaPitch: number,
  range: PitchRange,
  maxEnd?: number,
): NoteEvent[] {
  const sel = notes.filter((n) => ids.has(n.id));
  if (sel.length === 0) return notes;
  const minStart = Math.min(...sel.map((n) => n.startBeat));
  const maxEndSel = Math.max(...sel.map((n) => n.startBeat + n.lengthBeats));
  const minPitch = Math.min(...sel.map((n) => n.pitch));
  const maxPitch = Math.max(...sel.map((n) => n.pitch));
  let db = Math.max(deltaBeats, -minStart);
  if (maxEnd !== undefined) db = Math.min(db, Math.max(0, maxEnd - maxEndSel));
  const dp = Math.min(Math.max(deltaPitch, range.min - minPitch), range.max - maxPitch);
  if (db === 0 && dp === 0) return notes;
  return notes.map((n) => (ids.has(n.id) ? { ...n, startBeat: n.startBeat + db, pitch: n.pitch + dp } : n));
}

export function resizeNotes(notes: NoteEvent[], ids: Set<string>, deltaBeats: number, minLength = MIN_LENGTH): NoteEvent[] {
  return notes.map((n) => (ids.has(n.id) ? { ...n, lengthBeats: Math.max(minLength, n.lengthBeats + deltaBeats) } : n));
}

/** Ids of notes overlapping a beat × pitch rectangle (inclusive pitch bounds). */
export function notesInRect(notes: NoteEvent[], beat0: number, beat1: number, pitch0: number, pitch1: number): Set<string> {
  const [b0, b1] = beat0 <= beat1 ? [beat0, beat1] : [beat1, beat0];
  const [p0, p1] = pitch0 <= pitch1 ? [pitch0, pitch1] : [pitch1, pitch0];
  const out = new Set<string>();
  for (const n of notes) {
    if (n.pitch >= p0 && n.pitch <= p1 && n.startBeat < b1 && n.startBeat + n.lengthBeats > b0) out.add(n.id);
  }
  return out;
}

export function removeNotes(notes: NoteEvent[], ids: Set<string>): NoteEvent[] {
  return notes.filter((n) => !ids.has(n.id));
}

export function adjustVelocity(notes: NoteEvent[], ids: Set<string>, delta: number): NoteEvent[] {
  return notes.map((n) => (ids.has(n.id) ? { ...n, velocity: Math.min(127, Math.max(1, n.velocity + delta)) } : n));
}

/** Copies of `ids` placed right after the selection, on the grid. Returns new notes + their ids. */
export function duplicateNotes(notes: NoteEvent[], ids: Set<string>, grid: number): { notes: NoteEvent[]; newIds: Set<string> } {
  const sel = notes.filter((n) => ids.has(n.id));
  if (sel.length === 0) return { notes, newIds: new Set() };
  const start = Math.min(...sel.map((n) => n.startBeat));
  const end = Math.max(...sel.map((n) => n.startBeat + n.lengthBeats));
  const offset = Math.max(grid, Math.ceil((end - start) / grid - 1e-9) * grid);
  const copies = sel.map((n) => ({ ...n, id: uuid(), startBeat: n.startBeat + offset }));
  return { notes: [...notes, ...copies], newIds: new Set(copies.map((c) => c.id)) };
}

/** Paste `clipboard` so its earliest note lands at `atBeat`. */
export function pasteNotes(notes: NoteEvent[], clipboard: NoteEvent[], atBeat: number): { notes: NoteEvent[]; newIds: Set<string> } {
  if (clipboard.length === 0) return { notes, newIds: new Set() };
  const start = Math.min(...clipboard.map((n) => n.startBeat));
  const copies = clipboard.map((n) => ({ ...n, id: uuid(), startBeat: n.startBeat - start + atBeat }));
  return { notes: [...notes, ...copies], newIds: new Set(copies.map((c) => c.id)) };
}

export function selectionBounds(notes: NoteEvent[], ids: Set<string>): { start: number; end: number } | null {
  const sel = notes.filter((n) => ids.has(n.id));
  if (sel.length === 0) return null;
  return {
    start: Math.min(...sel.map((n) => n.startBeat)),
    end: Math.max(...sel.map((n) => n.startBeat + n.lengthBeats)),
  };
}
