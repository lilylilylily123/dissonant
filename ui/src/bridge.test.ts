// Parity tests for the browser mock bridge.
//
// `pnpm --dir ui dev` runs the mock, so every place the mock is more permissive than
// `dissonant-core` is a feature that looks fine in the browser and breaks in the desktop app.
// These tests assert the semantics of `Document::apply` / `apply_to`
// (crates/dissonant-core/src/document.rs) and `ProjectModel` (…/src/model.rs), not the mock's
// current behavior. The mock is expected to match Rust, including rejecting invalid commands:
// over Tauri IPC an `EditError` comes back as a rejected promise carrying the plain error
// string (`lib.rs::apply` does `.map_err(|e| e.to_string())`).

import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { Bridge } from "./bridge";
import type { Command, MasterSettings, NoteEvent, ProjectModel } from "./types";
import { SOUND_PRESETS, soundPresetOf } from "./types";

const FAKE_ID = "00000000-0000-4000-8000-000000000000";

beforeAll(() => {
  // The mock drives a playhead off rAF and persists through localStorage; neither exists in
  // the node test environment.
  vi.stubGlobal("requestAnimationFrame", () => 0);
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
  });
});

/**
 * A bridge with its own state. `getBridge` memoizes its bridge in module scope, so a static
 * import would hand every test the same mock; re-importing after `vi.resetModules()` is the
 * only way to get a fresh one.
 */
async function freshBridge(): Promise<Bridge> {
  vi.resetModules();
  const { getBridge } = await import("./bridge");
  return getBridge();
}

let b: Bridge;
const model = async (): Promise<ProjectModel> => (await b.getState()).model;

/** The starter project's ids, in the order the UI sees them. */
async function ids() {
  const m = await model();
  return { melody: m.tracks[0].id, drums: m.tracks[1].id, pattern: m.patterns[0].id };
}

function note(startBeat: number, pitch: number, extra: Partial<NoteEvent> = {}): NoteEvent {
  return { id: FAKE_ID, startBeat, lengthBeats: 1, pitch, velocity: 100, ...extra };
}

/** Assert a command is rejected (as Rust's `EditError` would be) and leaves the model alone. */
async function rejectsAndKeepsModel(command: Command, reason?: string) {
  const before = await model();
  const p = b.apply(command);
  if (reason === undefined) await expect(p).rejects.toBeTruthy();
  else await expect(p).rejects.toBe(reason);
  expect(await model()).toEqual(before);
}

beforeEach(async () => {
  b = await freshBridge();
});

// ─── new project ───────────────────────────────────────────────────────────────────────────

describe("newProject", () => {
  it("matches ProjectModel::starter()", async () => {
    const s = await b.newProject(true);
    expect(s.model.schemaVersion).toBe(5);
    expect(s.model.tempo).toBe(120);
    expect(s.model.key).toEqual({ rootPitchClass: 0, scale: "major", isLocked: true });
    expect(s.model.tracks.map((t) => [t.name, t.isDrum])).toEqual([
      ["melody", false],
      ["drums", true],
    ]);
    expect(s.model.patterns).toHaveLength(1);
    expect(s.model.patterns[0].name).toBe("pattern 1");
    expect(s.model.patterns[0].lengthBeats).toBe(16);
    // I–IV–V–vi over 16 beats at 4 beats a chord.
    expect(s.model.patterns[0].chords.chords.map((c) => c.startBeat)).toEqual([0, 4, 8, 12]);
    // A melody (STARTER_MELODY) and a groove (starter_groove), so space plays music at once.
    const [melody, drums] = s.model.tracks;
    expect(melody.voice).toBe("pluck");
    const notes = s.model.patterns[0].notesByTrack;
    expect(notes[melody.id].map((n) => n.pitch)).toEqual([76, 74, 72, 69, 72, 77, 74, 71, 67, 72, 71, 69]);
    const count = (pitch: number) => notes[drums.id].filter((n) => n.pitch === pitch).length;
    expect([count(36), count(38), count(42)]).toEqual([8, 8, 32]);
    expect(s.model.clips.map((c) => c.patternId)).toEqual([s.model.patterns[0].id]);
    expect(s.model.master).toEqual({ gain: 1, reverbWet: 0, lowCutHz: 20, highCutHz: 18000, lowEq: 1, midEq: 1, highEq: 1 });
    expect([s.canUndo, s.canRedo, s.dirty, s.path]).toEqual([false, false, false, null]);
  });

  it("matches ProjectModel::empty() when starter is false", async () => {
    const s = await b.newProject(false);
    expect(s.model.tracks.map((t) => t.name)).toEqual(["melody"]);
    expect(s.model.tracks[0].isDrum).toBe(false);
    expect(s.model.patterns).toHaveLength(1);
    expect(s.model.patterns[0].chords.chords).toEqual([]);
    expect(s.model.clips).toEqual([]);
  });

  it("follows the new-project setting when no choice is passed (new_project's fallback)", async () => {
    const settings = await b.getSettings();
    await b.setSettings({ ...settings, editing: { ...settings.editing, newProject: "empty" } });
    expect((await b.newProject()).model.tracks).toHaveLength(1);
    await b.setSettings({ ...settings, editing: { ...settings.editing, newProject: "starter" } });
    expect((await b.newProject()).model.tracks).toHaveLength(2);
  });

  it("drops a pending transient gesture (Document::replace clears `pending`)", async () => {
    const { melody } = await ids();
    await b.apply({ type: "setTrackParam", id: melody, param: "volume", value: 0.4 }, true);
    expect((await b.getState()).canUndo).toBe(true);
    const s = await b.newProject(true);
    expect(s.canUndo).toBe(false);
    expect(s.dirty).toBe(false);
  });
});

// ─── scalar edits and clamping ─────────────────────────────────────────────────────────────

describe("setTempo", () => {
  it("clamps to Tempo::MIN_BPM..MAX_BPM", async () => {
    await b.apply({ type: "setTempo", bpm: 1000 });
    expect((await model()).tempo).toBe(300);
    await b.apply({ type: "setTempo", bpm: 1 });
    expect((await model()).tempo).toBe(20);
  });

  it("rejects a non-finite bpm instead of storing NaN", async () => {
    await rejectsAndKeepsModel({ type: "setTempo", bpm: Number.NaN });
    await rejectsAndKeepsModel({ type: "setTempo", bpm: Number.POSITIVE_INFINITY });
  });
});

describe("setKey", () => {
  it("stores a copy, not a reference to the caller's command", async () => {
    const key = { rootPitchClass: 0, scale: "major" as const, isLocked: true };
    await b.apply({ type: "setKey", key });
    key.rootPitchClass = 7;
    expect((await model()).key.rootPitchClass).toBe(0);
  });
});

describe("setTrackParam", () => {
  it("clamps each parameter to the engine's range", async () => {
    const { melody } = await ids();
    const set = async (param: "volume" | "reverbSend" | "tone" | "pan", value: number) => {
      await b.apply({ type: "setTrackParam", id: melody, param, value });
      return (await model()).tracks[0][param];
    };
    expect(await set("volume", 9)).toBe(1.5);
    expect(await set("volume", -2)).toBe(0);
    expect(await set("reverbSend", 3)).toBe(1);
    expect(await set("reverbSend", -1)).toBe(0);
    expect(await set("tone", 1)).toBe(200);
    expect(await set("tone", 99_999)).toBe(20_000);
    expect(await set("pan", -5)).toBe(-1);
    expect(await set("pan", 5)).toBe(1);
  });

  it("rejects a non-finite value and an unknown track", async () => {
    const { melody } = await ids();
    await rejectsAndKeepsModel({ type: "setTrackParam", id: melody, param: "volume", value: Number.NaN });
    await rejectsAndKeepsModel({ type: "setTrackParam", id: FAKE_ID, param: "volume", value: 0.5 }, "no such track");
  });
});

describe("setTrackColor", () => {
  it("accepts #rrggbb, lowercasing it, and accepts null", async () => {
    const { melody } = await ids();
    await b.apply({ type: "setTrackColor", id: melody, color: "#ABCDEF" });
    expect((await model()).tracks[0].color).toBe("#abcdef");
    await b.apply({ type: "setTrackColor", id: melody, color: null });
    expect((await model()).tracks[0].color).toBeNull();
  });

  it("rejects anything that is not #rrggbb", async () => {
    const { melody } = await ids();
    for (const color of ["red", "#abc", "#abcdeg", "abcdef", "#abcdef0"]) {
      await rejectsAndKeepsModel({ type: "setTrackColor", id: melody, color }, "invalid value");
    }
  });

  it("validates the color before looking up the track", async () => {
    await rejectsAndKeepsModel({ type: "setTrackColor", id: FAKE_ID, color: "nope" }, "invalid value");
    await rejectsAndKeepsModel({ type: "setTrackColor", id: FAKE_ID, color: "#abcdef" }, "no such track");
  });
});

// ─── tracks ────────────────────────────────────────────────────────────────────────────────

describe("tracks", () => {
  it("names a new track after its position", async () => {
    await b.apply({ type: "addTrack", isDrum: false });
    const m = await model();
    expect(m.tracks[2]).toMatchObject({ name: "track 3", voice: "saw", isDrum: false, volume: 1, reverbSend: 0, tone: 18000, pan: 0, color: null });
    await b.apply({ type: "addTrack", isDrum: true });
    expect((await model()).tracks[3]).toMatchObject({ name: "drums", isDrum: true });
  });

  it("rejects edits to an unknown track", async () => {
    await rejectsAndKeepsModel({ type: "renameTrack", id: FAKE_ID, name: "x" }, "no such track");
    await rejectsAndKeepsModel({ type: "setTrackMuted", id: FAKE_ID, muted: true }, "no such track");
    await rejectsAndKeepsModel({ type: "setTrackSoloed", id: FAKE_ID, soloed: true }, "no such track");
    await rejectsAndKeepsModel({ type: "setTrackVoice", id: FAKE_ID, voice: "pad" }, "no such track");
    await rejectsAndKeepsModel({ type: "moveTrack", id: FAKE_ID, up: true }, "no such track");
    await rejectsAndKeepsModel({ type: "deleteTrack", id: FAKE_ID }, "no such track");
  });

  it("moves within bounds and no-ops (without erroring) at the edges", async () => {
    const { melody, drums } = await ids();
    await b.apply({ type: "moveTrack", id: melody, up: true });
    expect((await model()).tracks.map((t) => t.id)).toEqual([melody, drums]);
    await b.apply({ type: "moveTrack", id: drums, up: false });
    expect((await model()).tracks.map((t) => t.id)).toEqual([melody, drums]);
    await b.apply({ type: "moveTrack", id: drums, up: true });
    expect((await model()).tracks.map((t) => t.id)).toEqual([drums, melody]);
  });

  it("purges the deleted track's notes from every pattern", async () => {
    const { melody, drums, pattern } = await ids();
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [note(0, 60)] });
    await b.apply({ type: "setNotes", patternId: pattern, trackId: drums, notes: [note(0, 36)] });
    await b.apply({ type: "deleteTrack", id: melody });
    const m = await model();
    expect(m.tracks.map((t) => t.id)).toEqual([drums]);
    expect(Object.keys(m.patterns[0].notesByTrack)).toEqual([drums]);
  });

  it("refuses to delete the last track", async () => {
    const { melody, drums } = await ids();
    await b.apply({ type: "deleteTrack", id: melody });
    await rejectsAndKeepsModel({ type: "deleteTrack", id: drums }, "a project needs at least one track");
    expect((await model()).tracks).toHaveLength(1);
  });
});

// ─── patterns ──────────────────────────────────────────────────────────────────────────────

describe("patterns", () => {
  it("duplicates in place with fresh ids", async () => {
    const { melody, pattern } = await ids();
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [note(0, 60)] });
    await b.apply({ type: "addPattern" });
    await b.apply({ type: "duplicatePattern", id: pattern });
    const m = await model();
    expect(m.patterns.map((p) => p.name)).toEqual(["pattern 1", "pattern 1 copy", "pattern 2"]);
    const [orig, copy] = m.patterns;
    expect(copy.id).not.toBe(orig.id);
    expect(copy.notesByTrack[melody][0].id).not.toBe(orig.notesByTrack[melody][0].id);
    expect(copy.chords.chords.map((c) => c.id)).not.toEqual(orig.chords.chords.map((c) => c.id));
    expect(copy.chords.chords.map((c) => c.pitchClasses)).toEqual(orig.chords.chords.map((c) => c.pitchClasses));
  });

  it("seeds a new pattern from the project key", async () => {
    await b.apply({ type: "setKey", key: { rootPitchClass: 2, scale: "minor", isLocked: true } });
    await b.apply({ type: "addPattern" });
    const p = (await model()).patterns[1];
    expect(p.name).toBe("pattern 2");
    expect(p.lengthBeats).toBe(16);
    expect(p.chords.chords.map((c) => c.startBeat)).toEqual([0, 4, 8, 12]);
    expect(p.chords.chords[0].pitchClasses).toContain(2);
  });

  it("refuses a blank rename and an unknown pattern", async () => {
    const { pattern } = await ids();
    await rejectsAndKeepsModel({ type: "renamePattern", id: pattern, name: "   " }, "invalid value");
    await rejectsAndKeepsModel({ type: "renamePattern", id: FAKE_ID, name: "verse" }, "no such pattern");
    await rejectsAndKeepsModel({ type: "duplicatePattern", id: FAKE_ID }, "no such pattern");
    // The last-pattern guard runs before the existence check, so an unknown id only reports
    // "no such pattern" once there is more than one pattern to delete from.
    await rejectsAndKeepsModel({ type: "deletePattern", id: FAKE_ID }, "a project needs at least one pattern");
    await b.apply({ type: "addPattern" });
    await rejectsAndKeepsModel({ type: "deletePattern", id: FAKE_ID }, "no such pattern");
  });

  it("rejects a pattern shorter than one beat rather than clamping it", async () => {
    const { pattern } = await ids();
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, beats: 0.5 }, "invalid value");
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, beats: 0 }, "invalid value");
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, beats: Number.NaN });
    await rejectsAndKeepsModel({ type: "setPatternLength", id: FAKE_ID, beats: 8 }, "no such pattern");
    await b.apply({ type: "setPatternLength", id: pattern, beats: 8 });
    expect((await model()).patterns[0].lengthBeats).toBe(8);
  });

  it("removes a deleted pattern's clips and guards the last pattern", async () => {
    const { pattern } = await ids();
    await b.apply({ type: "addPattern" });
    const second = (await model()).patterns[1].id;
    await b.apply({ type: "addClip", patternId: second, startBeat: 16, lengthBeats: null });
    await b.apply({ type: "deletePattern", id: pattern });
    const m = await model();
    expect(m.patterns.map((p) => p.id)).toEqual([second]);
    expect(m.clips.map((c) => c.patternId)).toEqual([second]);
    await rejectsAndKeepsModel({ type: "deletePattern", id: second }, "a project needs at least one pattern");
  });
});

// ─── notes ─────────────────────────────────────────────────────────────────────────────────

describe("setNotes", () => {
  it("requires the track to exist, checked before the pattern", async () => {
    const { melody, pattern } = await ids();
    await rejectsAndKeepsModel({ type: "setNotes", patternId: pattern, trackId: FAKE_ID, notes: [note(0, 60)] }, "no such track");
    await rejectsAndKeepsModel({ type: "setNotes", patternId: FAKE_ID, trackId: FAKE_ID, notes: [note(0, 60)] }, "no such track");
    await rejectsAndKeepsModel({ type: "setNotes", patternId: FAKE_ID, trackId: melody, notes: [note(0, 60)] }, "no such pattern");
  });

  it("clamps every note into NoteEvent::sanitized's ranges", async () => {
    const { melody, pattern } = await ids();
    await b.apply({
      type: "setNotes",
      patternId: pattern,
      trackId: melody,
      notes: [
        note(0, 300, { velocity: 999, lengthBeats: 0 }),
        note(-4, -9, { velocity: 0, lengthBeats: -1 }),
      ],
    });
    const notes = (await model()).patterns[0].notesByTrack[melody];
    expect(notes.map((n) => n.pitch)).toEqual([0, 127]);
    expect(notes.map((n) => n.velocity)).toEqual([1, 127]);
    expect(notes.map((n) => n.startBeat)).toEqual([0, 0]);
    expect(notes.every((n) => n.lengthBeats === 1 / 64)).toBe(true);
  });

  it("sorts by start beat then pitch", async () => {
    const { melody, pattern } = await ids();
    await b.apply({
      type: "setNotes",
      patternId: pattern,
      trackId: melody,
      notes: [note(4, 60), note(0, 72), note(0, 48), note(2, 55)],
    });
    const notes = (await model()).patterns[0].notesByTrack[melody];
    expect(notes.map((n) => [n.startBeat, n.pitch])).toEqual([
      [0, 48],
      [0, 72],
      [2, 55],
      [4, 60],
    ]);
  });

  it("drops the track's entry when the array is empty", async () => {
    const { melody, pattern } = await ids();
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [note(0, 60)] });
    expect(Object.keys((await model()).patterns[0].notesByTrack)).toContain(melody);
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [] });
    expect(Object.keys((await model()).patterns[0].notesByTrack)).not.toContain(melody);
  });

  it("rejects non-finite note fields instead of writing NaN into the model", async () => {
    const { melody, pattern } = await ids();
    const bad = (n: NoteEvent) => ({ type: "setNotes" as const, patternId: pattern, trackId: melody, notes: [n] });
    await rejectsAndKeepsModel(bad(note(0, Number.NaN)));
    await rejectsAndKeepsModel(bad(note(0, 60, { velocity: Number.NaN })));
    await rejectsAndKeepsModel(bad(note(Number.NaN, 60)));
    await rejectsAndKeepsModel(bad(note(0, 60, { lengthBeats: Number.NaN })));
    await rejectsAndKeepsModel(bad(note(0, Number.POSITIVE_INFINITY)));
  });
});

// ─── chords and arrangement ────────────────────────────────────────────────────────────────

describe("setChords", () => {
  const chord = (startBeat: number, lengthBeats: number, pitchClasses: number[]) => ({
    id: FAKE_ID,
    startBeat,
    lengthBeats,
    pitchClasses,
    name: null,
  });

  it("drops empty or negatively placed chords and normalizes pitch classes", async () => {
    const { pattern } = await ids();
    await b.apply({
      type: "setChords",
      patternId: pattern,
      chords: [chord(8, 4, [7, 23, -1, 11, 7]), chord(2, 0, [0]), chord(-1, 4, [0]), chord(0, 4, [4, 0, 7])],
    });
    const chords = (await model()).patterns[0].chords.chords;
    expect(chords.map((c) => c.startBeat)).toEqual([0, 8]);
    expect(chords[0].pitchClasses).toEqual([0, 4, 7]);
    expect(chords[1].pitchClasses).toEqual([7, 11]);
  });

  it("rejects an unknown pattern", async () => {
    await rejectsAndKeepsModel({ type: "setChords", patternId: FAKE_ID, chords: [] }, "no such pattern");
  });
});

describe("clips", () => {
  it("places clips in start order and validates like AddClip", async () => {
    const { pattern } = await ids();
    await b.apply({ type: "addClip", patternId: pattern, startBeat: 16, lengthBeats: null });
    expect((await model()).clips.map((c) => c.startBeat)).toEqual([0, 16]);
    // Clip length defaults to the pattern's own length.
    expect((await model()).clips[1].lengthBeats).toBe(16);
    await rejectsAndKeepsModel({ type: "addClip", patternId: FAKE_ID, startBeat: 32, lengthBeats: null }, "no such pattern");
    await rejectsAndKeepsModel({ type: "addClip", patternId: pattern, startBeat: -1, lengthBeats: null }, "invalid value");
    await rejectsAndKeepsModel({ type: "addClip", patternId: pattern, startBeat: 32, lengthBeats: 0 }, "invalid value");
    await rejectsAndKeepsModel({ type: "removeClip", id: FAKE_ID }, "no such clip");
  });
});

describe("setMaster", () => {
  const master = (over: Partial<MasterSettings> = {}): MasterSettings => ({
    gain: 1,
    reverbWet: 0,
    lowCutHz: 20,
    highCutHz: 18000,
    lowEq: 1,
    midEq: 1,
    highEq: 1,
    ...over,
  });

  it("clamps every field", async () => {
    await b.apply({
      type: "setMaster",
      master: master({ gain: 9, reverbWet: 3, lowCutHz: 1, highCutHz: 99_999, lowEq: 5, midEq: -1, highEq: 5 }),
    });
    expect((await model()).master).toEqual({ gain: 1.5, reverbWet: 1, lowCutHz: 10, highCutHz: 20_000, lowEq: 2, midEq: 0, highEq: 2 });
    await b.apply({ type: "setMaster", master: master({ lowCutHz: 9_999, highCutHz: 1 }) });
    expect((await model()).master).toMatchObject({ lowCutHz: 2_000, highCutHz: 500 });
  });

  it("rejects a non-finite field", async () => {
    await rejectsAndKeepsModel({ type: "setMaster", master: master({ gain: Number.NaN }) });
  });
});

// ─── undo / redo / gestures ────────────────────────────────────────────────────────────────

describe("undo, redo and transient gestures", () => {
  it("does not record an undo step for a command that changes nothing", async () => {
    const { melody } = await ids();
    const s = await b.apply({ type: "setTrackMuted", id: melody, muted: false });
    expect(s.canUndo).toBe(false);
    expect(s.dirty).toBe(false);
  });

  it("round-trips a single edit and clears redo on the next edit", async () => {
    const { melody } = await ids();
    await b.apply({ type: "renameTrack", id: melody, name: "lead" });
    expect((await b.getState()).canUndo).toBe(true);
    const undone = await b.undo();
    expect(undone.model.tracks[0].name).toBe("melody");
    expect(undone.canRedo).toBe(true);
    expect((await b.redo()).model.tracks[0].name).toBe("lead");
    await b.undo();
    await b.apply({ type: "renameTrack", id: melody, name: "bass" });
    expect((await b.getState()).canRedo).toBe(false);
  });

  it("coalesces a run of transient edits into one undo step", async () => {
    const { melody } = await ids();
    for (const value of [0.9, 0.8, 0.7]) {
      await b.apply({ type: "setTrackParam", id: melody, param: "volume", value }, true);
    }
    expect((await b.getState()).canUndo).toBe(true);
    await b.commitGesture();
    expect((await b.undo()).model.tracks[0].volume).toBe(1);
    expect((await b.getState()).canUndo).toBe(false);
  });

  it("closes the open gesture when a non-transient command arrives", async () => {
    const { melody } = await ids();
    await b.apply({ type: "setTrackParam", id: melody, param: "volume", value: 0.5 }, true);
    await b.apply({ type: "renameTrack", id: melody, name: "lead" });
    expect((await b.undo()).model.tracks[0]).toMatchObject({ name: "melody", volume: 0.5 });
    expect((await b.undo()).model.tracks[0]).toMatchObject({ name: "melody", volume: 1 });
  });

  it("closes the open gesture on redo, like Document::redo", async () => {
    const { melody } = await ids();
    await b.apply({ type: "setTrackParam", id: melody, param: "volume", value: 0.5 }, true);
    await b.redo(); // nothing to redo, but it still ends the gesture
    await b.apply({ type: "setTrackParam", id: melody, param: "volume", value: 0.7 }, true);
    await b.commitGesture();
    expect((await b.undo()).model.tracks[0].volume).toBe(0.5);
  });

  it("keeps at most MAX_UNDO (200) steps", async () => {
    const { pattern } = await ids();
    for (let i = 0; i < 201; i++) {
      await b.apply({ type: "setPatternLength", id: pattern, beats: i + 2 });
    }
    for (let i = 0; i < 200; i++) await b.undo();
    const s = await b.getState();
    expect(s.model.patterns[0].lengthBeats).toBe(2);
    expect(s.canUndo).toBe(false);
  });
});

// ─── files ─────────────────────────────────────────────────────────────────────────────────

describe("save and open", () => {
  it("refuses to save with no path, like save_project", async () => {
    const { melody } = await ids();
    await b.apply({ type: "renameTrack", id: melody, name: "lead" });
    await expect(b.saveProject()).rejects.toBe("no file path yet — use Save As");
    expect((await b.getState()).dirty).toBe(true);
  });

  it("stores the path and clears the dirty flag", async () => {
    const { melody } = await ids();
    await b.apply({ type: "renameTrack", id: melody, name: "lead" });
    const s = await b.saveProject("/tmp/song.dissonant");
    expect(s.path).toBe("/tmp/song.dissonant");
    expect(s.dirty).toBe(false);
    expect((await b.saveProject()).path).toBe("/tmp/song.dissonant");
  });

  it("replaces the document on open: history cleared, not dirty", async () => {
    const { melody } = await ids();
    await b.apply({ type: "renameTrack", id: melody, name: "lead" });
    await b.saveProject("/tmp/song.dissonant");
    await b.apply({ type: "renameTrack", id: melody, name: "scratch" });
    const s = await b.openProject("/tmp/song.dissonant");
    expect(s.path).toBe("/tmp/song.dissonant");
    expect(s.dirty).toBe(false);
    expect(s.canUndo).toBe(false);
    expect(s.canRedo).toBe(false);
    expect(s.model.tracks[0].name).toBe("lead");
  });
});

// ─── sound presets ─────────────────────────────────────────────────────────────────────────

describe("sound presets (store.applySoundPreset)", () => {
  it("voice, tone and reverb as transient edits undo as one labelled step", async () => {
    const { melody } = await ids();
    const before = (await model()).tracks[0];
    const preset = SOUND_PRESETS.find((p) => p.name === "warm pad")!;
    const label = `sound: ${preset.name}`;
    await b.apply({ type: "setTrackVoice", id: melody, voice: preset.voice }, true, label);
    await b.apply({ type: "setTrackParam", id: melody, param: "tone", value: preset.tone }, true, label);
    await b.apply({ type: "setTrackParam", id: melody, param: "reverbSend", value: preset.reverbSend }, true, label);
    const s = await b.commitGesture();
    expect(soundPresetOf(s.model.tracks[0])?.name).toBe("warm pad");
    expect(s.undoLabel).toBe(label);
    const back = await b.undo();
    expect(back.model.tracks[0]).toEqual(before);
    expect(back.canUndo).toBe(false);
  });
});
