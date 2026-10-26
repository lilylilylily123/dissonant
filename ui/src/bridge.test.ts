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
import type { AudioClip, AudioSource, Command, EqModel, InsertTarget, MasterSettings, MicModel, NoteEvent, ProjectModel } from "./types";
import { chordLoop, defaultAmpRig, insertChain, layOutLoop, MAX_CLIP_GAIN, MAX_INSERTS, MAX_PEDALS, MIN_NOTE_TICKS, newPedal, PEDAL_KINDS, TICKS_PER_BEAT, trackLoop } from "./types";

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

function note(startTick: number, pitch: number, extra: Partial<NoteEvent> = {}): NoteEvent {
  return { id: FAKE_ID, startTick, lengthTicks: TICKS_PER_BEAT, pitch, velocity: 100, ...extra };
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
    expect(s.model.schemaVersion).toBe(9);
    expect(s.model.tempo).toBe(120);
    expect(s.model.key).toEqual({ rootPitchClass: null, scale: "major", isLocked: false });
    expect(s.model.tracks.map((t) => [t.name, t.isDrum])).toEqual([
      ["melody", false],
      ["drums", true],
    ]);
    expect(s.model.patterns).toHaveLength(1);
    expect(s.model.patterns[0].name).toBe("pattern 1");
    expect(s.model.patterns[0].lengthTicks).toBe(16 * TICKS_PER_BEAT);
    // I–IV–V–vi over 16 beats at 4 beats a chord.
    expect(s.model.patterns[0].chords.chords.map((c) => c.startTick)).toEqual([0, 3840, 7680, 11520]);
    expect(s.model.patterns[0].notesByTrack).toEqual({});
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
    expect(p.lengthTicks).toBe(16 * TICKS_PER_BEAT);
    expect(p.chords.chords.map((c) => c.startTick)).toEqual([0, 3840, 7680, 11520]);
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
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, ticks: 480 }, "invalid value");
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, ticks: 0 }, "invalid value");
    await rejectsAndKeepsModel({ type: "setPatternLength", id: pattern, ticks: Number.NaN });
    await rejectsAndKeepsModel({ type: "setPatternLength", id: FAKE_ID, ticks: 8 * TICKS_PER_BEAT }, "no such pattern");
    await b.apply({ type: "setPatternLength", id: pattern, ticks: 8 * TICKS_PER_BEAT });
    expect((await model()).patterns[0].lengthTicks).toBe(7680);
  });

  it("removes a deleted pattern's clips and guards the last pattern", async () => {
    const { pattern } = await ids();
    await b.apply({ type: "addPattern" });
    const second = (await model()).patterns[1].id;
    await b.apply({ type: "addClip", patternId: second, startTick: 16 * TICKS_PER_BEAT, lengthTicks: null });
    await b.apply({ type: "deletePattern", id: pattern });
    const m = await model();
    expect(m.patterns.map((p) => p.id)).toEqual([second]);
    expect(m.clips.map((c) => c.patternId)).toEqual([second]);
    await rejectsAndKeepsModel({ type: "deletePattern", id: second }, "a project needs at least one pattern");
  });
  it("trims a track and the chord track to a shortened loop, matching apply_to", async () => {
    const { melody, pattern } = await ids();
    await b.apply({
      type: "setNotes",
      patternId: pattern,
      trackId: melody,
      notes: [note(0, 60), note(6 * TICKS_PER_BEAT, 62, { lengthTicks: 4 * TICKS_PER_BEAT }), note(12 * TICKS_PER_BEAT, 64)],
    });
    await b.apply({ type: "setTrackLoop", patternId: pattern, trackId: melody, ticks: 8 * TICKS_PER_BEAT });
    const notes = (await model()).patterns[0].notesByTrack[melody];
    expect(notes.map((n) => [n.startTick, n.lengthTicks])).toEqual([
      [0, 960],
      [5760, 1920],
    ]);
    await b.apply({ type: "setChordLoop", patternId: pattern, ticks: 8 * TICKS_PER_BEAT });
    expect((await model()).patterns[0].chords.chords.map((c) => c.startTick)).toEqual([0, 3840]);
  });
  it("stores and clears per-track and chord loop lengths", async () => {
    const { melody, pattern } = await ids();
    await b.apply({ type: "setTrackLoop", patternId: pattern, trackId: melody, ticks: 8 * TICKS_PER_BEAT });
    await b.apply({ type: "setChordLoop", patternId: pattern, ticks: 4 * TICKS_PER_BEAT });
    let p = (await model()).patterns[0];
    expect(p.trackLoopTicks[melody]).toBe(7680);
    expect(p.chordLoopTicks).toBe(3840);
    expect(trackLoop(p, melody)).toBe(7680);
    expect(chordLoop(p)).toBe(3840);

    await b.apply({ type: "setTrackLoop", patternId: pattern, trackId: melody, ticks: null });
    await b.apply({ type: "setChordLoop", patternId: pattern, ticks: null });
    p = (await model()).patterns[0];
    expect(p.trackLoopTicks[melody]).toBeUndefined();
    expect(trackLoop(p, melody)).toBe(p.lengthTicks);
    expect(chordLoop(p)).toBe(p.lengthTicks);
  });

  it("rejects a loop shorter than one beat and an unknown track or pattern", async () => {
    const { melody, pattern } = await ids();
    await rejectsAndKeepsModel({ type: "setTrackLoop", patternId: pattern, trackId: melody, ticks: 480 }, "invalid value");
    await rejectsAndKeepsModel({ type: "setTrackLoop", patternId: pattern, trackId: FAKE_ID, ticks: 4 * TICKS_PER_BEAT }, "no such track");
    await rejectsAndKeepsModel({ type: "setTrackLoop", patternId: FAKE_ID, trackId: melody, ticks: 4 * TICKS_PER_BEAT }, "no such pattern");
    await rejectsAndKeepsModel({ type: "setChordLoop", patternId: pattern, ticks: Number.NaN }, "invalid value");
    await rejectsAndKeepsModel({ type: "setChordLoop", patternId: FAKE_ID, ticks: 4 * TICKS_PER_BEAT }, "no such pattern");
  });

  it("drops a deleted track's loop along with its notes", async () => {
    const { melody, pattern } = await ids();
    await b.apply({ type: "setTrackLoop", patternId: pattern, trackId: melody, ticks: 8 * TICKS_PER_BEAT });
    await b.apply({ type: "addTrack", isDrum: false });
    await b.apply({ type: "deleteTrack", id: melody });
    expect((await model()).patterns[0].trackLoopTicks[melody]).toBeUndefined();
  });

  /// Mirrors `SongPattern::looped_notes`: two turns of a 2-bar loop under a 4-bar pattern.
  it("lays a short loop out across the pattern, trimming each turn at its own end", async () => {
    const events = [
      { id: "a", startTick: 0, lengthTicks: TICKS_PER_BEAT },
      { id: "b", startTick: 6 * TICKS_PER_BEAT, lengthTicks: 4 * TICKS_PER_BEAT },
    ];
    const out = layOutLoop(events, 8 * TICKS_PER_BEAT, 16 * TICKS_PER_BEAT);
    expect(out.map((e) => [e.startTick, e.lengthTicks])).toEqual([
      [0, 960],
      [5760, 1920],
      [7680, 960],
      [13440, 1920],
    ]);
    expect(new Set(out.map((e) => e.id)).size).toBe(4);
    expect(out[0].id).toBe("a");
    expect(layOutLoop(events, 16 * TICKS_PER_BEAT, 16 * TICKS_PER_BEAT)).toBe(events);
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
        note(0, 300, { velocity: 999, lengthTicks: 0 }),
        note(-4 * TICKS_PER_BEAT, -9, { velocity: 0, lengthTicks: -1 }),
      ],
    });
    const notes = (await model()).patterns[0].notesByTrack[melody];
    expect(notes.map((n) => n.pitch)).toEqual([0, 127]);
    expect(notes.map((n) => n.velocity)).toEqual([1, 127]);
    expect(notes.map((n) => n.startTick)).toEqual([0, 0]);
    expect(notes.every((n) => n.lengthTicks === MIN_NOTE_TICKS)).toBe(true);
  });

  it("sorts by start tick then pitch", async () => {
    const { melody, pattern } = await ids();
    await b.apply({
      type: "setNotes",
      patternId: pattern,
      trackId: melody,
      notes: [note(4 * TICKS_PER_BEAT, 60), note(0, 72), note(0, 48), note(2 * TICKS_PER_BEAT, 55)],
    });
    const notes = (await model()).patterns[0].notesByTrack[melody];
    expect(notes.map((n) => [n.startTick, n.pitch])).toEqual([
      [0, 48],
      [0, 72],
      [1920, 55],
      [3840, 60],
    ]);
  });

  it("drops the track's entry when the array is empty", async () => {
    const { melody, pattern } = await ids();
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [note(0, 60)] });
    expect(Object.keys((await model()).patterns[0].notesByTrack)).toEqual([melody]);
    await b.apply({ type: "setNotes", patternId: pattern, trackId: melody, notes: [] });
    expect((await model()).patterns[0].notesByTrack).toEqual({});
  });

  it("rejects non-finite note fields instead of writing NaN into the model", async () => {
    const { melody, pattern } = await ids();
    const bad = (n: NoteEvent) => ({ type: "setNotes" as const, patternId: pattern, trackId: melody, notes: [n] });
    await rejectsAndKeepsModel(bad(note(0, Number.NaN)));
    await rejectsAndKeepsModel(bad(note(0, 60, { velocity: Number.NaN })));
    await rejectsAndKeepsModel(bad(note(Number.NaN, 60)));
    await rejectsAndKeepsModel(bad(note(0, 60, { lengthTicks: Number.NaN })));
    await rejectsAndKeepsModel(bad(note(0, Number.POSITIVE_INFINITY)));
  });
});

// ─── chords and arrangement ────────────────────────────────────────────────────────────────

describe("setChords", () => {
  const chord = (startTick: number, lengthTicks: number, pitchClasses: number[]) => ({
    id: FAKE_ID,
    startTick,
    lengthTicks,
    pitchClasses,
    name: null,
  });

  it("drops empty or negatively placed chords and normalizes pitch classes", async () => {
    const { pattern } = await ids();
    await b.apply({
      type: "setChords",
      patternId: pattern,
      chords: [chord(8 * TICKS_PER_BEAT, 4 * TICKS_PER_BEAT, [7, 23, -1, 11, 7]), chord(2 * TICKS_PER_BEAT, 0, [0]), chord(-1, 4 * TICKS_PER_BEAT, [0]), chord(0, 4 * TICKS_PER_BEAT, [4, 0, 7])],
    });
    const chords = (await model()).patterns[0].chords.chords;
    expect(chords.map((c) => c.startTick)).toEqual([0, 7680]);
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
    await b.apply({ type: "addClip", patternId: pattern, startTick: 16 * TICKS_PER_BEAT, lengthTicks: null });
    expect((await model()).clips.map((c) => c.startTick)).toEqual([0, 15360]);
    // Clip length defaults to the pattern's own length.
    expect((await model()).clips[1].lengthTicks).toBe(16 * TICKS_PER_BEAT);
    await rejectsAndKeepsModel({ type: "addClip", patternId: FAKE_ID, startTick: 32 * TICKS_PER_BEAT, lengthTicks: null }, "no such pattern");
    await rejectsAndKeepsModel({ type: "addClip", patternId: pattern, startTick: -1, lengthTicks: null }, "invalid value");
    await rejectsAndKeepsModel({ type: "addClip", patternId: pattern, startTick: 32 * TICKS_PER_BEAT, lengthTicks: 0 }, "invalid value");
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
      await b.apply({ type: "setPatternLength", id: pattern, ticks: (i + 2) * TICKS_PER_BEAT });
    }
    for (let i = 0; i < 200; i++) await b.undo();
    const s = await b.getState();
    expect(s.model.patterns[0].lengthTicks).toBe(2 * TICKS_PER_BEAT);
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

// ─── plugins ───────────────────────────────────────────────────────────────────────────────

describe("track plugins", () => {
  const anyPlugin = async () => (await b.scanPlugins())[0];

  it("refuses a plugin on a track that does not exist", async () => {
    await rejectsAndKeepsModel({ type: "setTrackPlugin", id: FAKE_ID, plugin: { uid: "clap:/x.clap:x", name: "x", state: null } }, "no such track");
  });

  it("refuses a blank uid", async () => {
    const { melody } = await ids();
    await rejectsAndKeepsModel({ type: "setTrackPlugin", id: melody, plugin: { uid: "   ", name: "x", state: null } }, "invalid value");
  });

  it("keeps the saved state while the plugin stays the same and drops it when it changes", async () => {
    const { melody } = await ids();
    const [a, b2] = await b.scanPlugins();
    await b.apply({ type: "setTrackPlugin", id: melody, plugin: { uid: a.uid, name: a.name, state: null } });
    await b.apply({ type: "setPluginState", id: melody, state: "c2F2ZWQ=" });

    await b.apply({ type: "setTrackPlugin", id: melody, plugin: { uid: a.uid, name: a.name, state: null } });
    expect((await model()).tracks[0].plugin?.state).toBe("c2F2ZWQ=");

    await b.apply({ type: "setTrackPlugin", id: melody, plugin: { uid: b2.uid, name: b2.name, state: null } });
    expect((await model()).tracks[0].plugin).toEqual({ uid: b2.uid, name: b2.name, state: null });
  });

  it("refuses plugin state on a track that has no plugin", async () => {
    const { melody } = await ids();
    await rejectsAndKeepsModel({ type: "setPluginState", id: melody, state: "eA==" }, "track has no plugin");
  });

  it("goes back to the built-in voice when the plugin is cleared", async () => {
    const { melody } = await ids();
    const p = await anyPlugin();
    await b.setTrackPlugin(melody, p.uid);
    expect((await model()).tracks[0].plugin?.name).toBe(p.name);
    await b.setTrackPlugin(melody, null);
    expect((await model()).tracks[0].plugin).toBeNull();
    expect((await model()).tracks[0].voice).toBe("saw");
  });

  it("reports no parameters for a track with no plugin", async () => {
    const { melody } = await ids();
    expect(await b.pluginParams(melody)).toEqual([]);
  });

  it("clamps a parameter to the range the plugin published", async () => {
    const { melody } = await ids();
    await b.setTrackPlugin(melody, (await anyPlugin()).uid);
    const cutoff = (await b.pluginParams(melody))[0];
    await b.setPluginParam(melody, cutoff.id, 1e9);
    expect((await b.pluginParams(melody))[0].value).toBe(cutoff.max);
  });

  it("rounds a stepped parameter", async () => {
    const { melody } = await ids();
    await b.setTrackPlugin(melody, (await anyPlugin()).uid);
    const stepped = (await b.pluginParams(melody)).find((p) => p.isStepped)!;
    await b.setPluginParam(melody, stepped.id, 7.4);
    expect((await b.pluginParams(melody)).find((p) => p.id === stepped.id)!.value).toBe(7);
  });

  it("drags without touching the document and commits one undo step", async () => {
    const { melody } = await ids();
    await b.setTrackPlugin(melody, (await anyPlugin()).uid);
    const res = (await b.pluginParams(melody)).find((p) => p.name === "resonance")!;
    const before = await b.getState();

    await b.setPluginParam(melody, res.id, 0.9, false);
    const dragging = await b.getState();
    // The plugin heard it; the document did not, so there is nothing to undo yet.
    expect((await b.pluginParams(melody)).find((p) => p.id === res.id)!.value).toBe(0.9);
    expect(dragging.model).toEqual(before.model);
    expect(dragging.undoLabel).toBe("change plugin");

    await b.setPluginParam(melody, res.id, 0.9, true);
    const committed = await b.getState();
    expect(committed.undoLabel).toBe("edit plugin");
    expect(committed.model.tracks[0].plugin?.state).toEqual(expect.any(String));

    const undone = await b.undo();
    expect(undone.model).toEqual(before.model);
    // Undo has to reach the plugin too, not just the document.
    expect((await b.pluginParams(melody)).find((p) => p.id === res.id)!.value).toBe(res.default);
  });

  it("restores the saved values when a project is reopened", async () => {
    const { melody } = await ids();
    await b.setTrackPlugin(melody, (await anyPlugin()).uid);
    const res = (await b.pluginParams(melody)).find((p) => p.name === "resonance")!;
    await b.setPluginParam(melody, res.id, 0.77, true);
    await b.saveProject("/tmp/plugged.dissonant");

    await b.newProject(true);
    await b.openProject("/tmp/plugged.dissonant");
    const reopened = (await model()).tracks[0].id;
    expect((await b.pluginParams(reopened)).find((p) => p.id === res.id)!.value).toBe(0.77);
  });
});

// ─── insert chains ─────────────────────────────────────────────────────────────────────────

describe("insert chains", () => {
  /** The two effects the mock's plugin folder holds, in scan order. */
  const effects = async () => (await b.scanPlugins()).filter((p) => p.kind === "effect");
  const chain = async (target: InsertTarget) => insertChain(await model(), target);
  const names = async (target: InsertTarget) => (await chain(target)).map((i) => i.name);

  /** A chain of `n` effects, alternating the two so order is readable. */
  async function fill(target: InsertTarget, n: number) {
    const es = await effects();
    for (let i = 0; i < n; i++) await b.addInsert(target, es[i % es.length].uid);
    return chain(target);
  }

  it("appends by default and inserts at an index when given one", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const [tape, fuzz] = [...(await effects())].reverse();
    await b.addInsert(target, tape.uid);
    await b.addInsert(target, fuzz.uid);
    await b.addInsert(target, fuzz.uid, 0);
    expect(await names(target)).toEqual([fuzz.name, tape.name, fuzz.name]);
    // Past the end is the end, not a hole.
    await b.addInsert(target, tape.uid, 99);
    expect((await names(target)).at(-1)).toBe(tape.name);
  });

  it("starts every effect enabled with no state of its own", async () => {
    const { melody } = await ids();
    const [e] = await effects();
    await b.addInsert({ track: melody }, e.uid);
    const [ins] = await chain({ track: melody });
    expect([ins.uid, ins.name, ins.enabled]).toEqual([e.uid, e.name, true]);
    expect(ins.state ?? null).toBeNull();
  });

  it("refuses a ninth effect and leaves the chain as it was", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const full = await fill(target, MAX_INSERTS);
    expect(full).toHaveLength(MAX_INSERTS);
    const [e] = await effects();
    await expect(b.addInsert(target, e.uid)).rejects.toBe(
      `an insert chain holds at most ${MAX_INSERTS} effects`,
    );
    expect(await chain(target)).toEqual(full);
  });

  it("refuses a blank uid and an unknown track", async () => {
    const { melody } = await ids();
    await rejectsAndKeepsModel(
      { type: "addInsert", target: { track: melody }, uid: "  ", name: "x", index: null },
      "invalid value",
    );
    await rejectsAndKeepsModel(
      { type: "addInsert", target: { track: FAKE_ID }, uid: "clap:/x.clap:x", name: "x", index: null },
      "no such track",
    );
  });

  it("refuses an effect this computer does not have", async () => {
    const { melody } = await ids();
    await expect(b.addInsert({ track: melody }, "clap:/nowhere.clap:nope")).rejects.toBe("no such plugin");
  });

  it("moves one effect and shuffles the rest up or down around it", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const built = await fill(target, 3);
    await b.moveInsert(target, built[0].id, 2);
    expect((await chain(target)).map((i) => i.id)).toEqual([built[1].id, built[2].id, built[0].id]);
    await b.moveInsert(target, built[0].id, 1);
    expect((await chain(target)).map((i) => i.id)).toEqual([built[1].id, built[0].id, built[2].id]);
  });

  it("clamps a move past either end instead of failing", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const built = await fill(target, 3);
    const order = built.map((i) => i.id);

    // The arrows on the first and last rows are pressed anyway; they must do nothing rather
    // than throw, because the UI's drop targets and the chain's bounds need not agree.
    await b.moveInsert(target, built[0].id, -4);
    expect((await chain(target)).map((i) => i.id)).toEqual(order);
    await b.moveInsert(target, built[2].id, 99);
    expect((await chain(target)).map((i) => i.id)).toEqual(order);
  });

  it("bypasses in place and switches back on", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const built = await fill(target, 3);

    await b.setInsertEnabled(target, built[1].id, false);
    expect((await chain(target)).map((i) => i.enabled)).toEqual([true, false, true]);
    expect((await chain(target)).map((i) => i.id)).toEqual(built.map((i) => i.id));

    await b.setInsertEnabled(target, built[1].id, true);
    expect((await chain(target)).every((i) => i.enabled)).toBe(true);
  });

  it("removes the slot it names and keeps the order of the rest", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const built = await fill(target, 3);
    await b.removeInsert(target, built[1].id);
    expect((await chain(target)).map((i) => i.id)).toEqual([built[0].id, built[2].id]);
  });

  it("refuses to remove, move or bypass an insert that is not there", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    await fill(target, 1);
    await expect(b.removeInsert(target, FAKE_ID)).rejects.toBe("no such insert");
    await expect(b.moveInsert(target, FAKE_ID, 0)).rejects.toBe("no such insert");
    await expect(b.setInsertEnabled(target, FAKE_ID, false)).rejects.toBe("no such insert");
  });

  it("keeps the master chain and a track's chain apart", async () => {
    const { melody } = await ids();
    const [fuzz, tape] = await effects();
    await b.addInsert({ track: melody }, fuzz.uid);
    await b.addInsert("master", tape.uid);
    expect(await names({ track: melody })).toEqual([fuzz.name]);
    expect(await names("master")).toEqual([tape.name]);
    expect((await model()).masterInserts).toHaveLength(1);
  });

  it("takes the chain away with the track it belonged to", async () => {
    const { melody, drums } = await ids();
    const [e] = await effects();
    await b.addInsert({ track: drums }, e.uid);
    await b.apply({ type: "deleteTrack", id: drums });
    expect(await names({ track: drums })).toEqual([]);
    expect(await names({ track: melody })).toEqual([]);
  });

  it("puts a chain back on undo and takes it away again on redo", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const built = await fill(target, 2);
    await b.removeInsert(target, built[0].id);
    expect(await chain(target)).toHaveLength(1);

    await b.undo();
    expect((await chain(target)).map((i) => i.id)).toEqual(built.map((i) => i.id));
    await b.redo();
    expect((await chain(target)).map((i) => i.id)).toEqual([built[1].id]);
  });

  it("drops inserts that name nothing, repeat an id, or run past the cap when a file is read", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    await fill(target, 2);
    await b.saveProject("/tmp/chained.dissonant");

    // What a hand-edited project can carry. `openProject` runs the mock's `normalizedModel`,
    // which stands in for `ProjectModel::normalized`.
    const raw = JSON.parse(localStorage.getItem("dissonant.mock.project")!) as ProjectModel;
    const slot = (id: string, uid: string) => ({ id, uid, name: "x", enabled: true });
    raw.tracks[0].inserts = [
      slot(FAKE_ID, "   "),
      slot("11111111-0000-4000-8000-000000000000", "clap:/a.clap:a"),
      slot("11111111-0000-4000-8000-000000000000", "clap:/a.clap:a"),
      ...Array.from({ length: MAX_INSERTS + 2 }, (_, i) =>
        slot(`2222222${i}-0000-4000-8000-000000000000`, "clap:/b.clap:b"),
      ),
    ];
    localStorage.setItem("dissonant.mock.project", JSON.stringify(raw));

    await b.openProject("/tmp/chained.dissonant");
    const loaded = await chain(target);
    expect(loaded).toHaveLength(MAX_INSERTS);
    expect(loaded.every((i) => i.uid.trim().length > 0)).toBe(true);
    expect(new Set(loaded.map((i) => i.id)).size).toBe(MAX_INSERTS);
  });

  it("reads an effect's parameters and drags without touching the document", async () => {
    const { melody } = await ids();
    const target: InsertTarget = { track: melody };
    const [built] = await fill(target, 1);
    const res = (await b.insertParams(built.id)).find((p) => p.name === "resonance")!;
    const before = await b.getState();

    await b.setInsertParam(built.id, res.id, 0.9, false);
    expect((await b.insertParams(built.id)).find((p) => p.id === res.id)!.value).toBe(0.9);
    expect((await b.getState()).model).toEqual(before.model);

    const committed = await b.setInsertParam(built.id, res.id, 0.9, true);
    expect(committed?.model.tracks[0].inserts?.[0].state).toEqual(expect.any(String));

    // Undo has to reach the effect too, not just the document.
    await b.undo();
    expect((await b.insertParams(built.id)).find((p) => p.id === res.id)!.value).toBe(res.default);
  });

  it("clamps an effect's parameter to the range it published", async () => {
    const { melody } = await ids();
    const [built] = await fill({ track: melody }, 1);
    const cutoff = (await b.insertParams(built.id))[0];
    await b.setInsertParam(built.id, cutoff.id, 1e9);
    expect((await b.insertParams(built.id))[0].value).toBe(cutoff.max);
  });

  it("reports no parameters for an insert that is gone", async () => {
    const { melody } = await ids();
    const [built] = await fill({ track: melody }, 1);
    await b.removeInsert({ track: melody }, built.id);
    expect(await b.insertParams(built.id)).toEqual([]);
  });

  it("keeps a restored project's dialled-in values", async () => {
    const { melody } = await ids();
    const [built] = await fill({ track: melody }, 1);
    const res = (await b.insertParams(built.id)).find((p) => p.name === "resonance")!;
    await b.setInsertParam(built.id, res.id, 0.77, true);
    await b.saveProject("/tmp/dialled.dissonant");

    await b.newProject(true);
    await b.openProject("/tmp/dialled.dissonant");
    const reopened = (await model()).tracks[0].inserts![0];
    expect((await b.insertParams(reopened.id)).find((p) => p.id === res.id)!.value).toBe(0.77);
  });
});

describe("plugin catalog", () => {
  it("leads every entry with plain words and tags, not format jargon", async () => {
    const catalog = await b.pluginCatalog();
    expect(catalog.length).toBeGreaterThan(0);
    for (const e of catalog) {
      expect(e.summary.trim().length).toBeGreaterThan(0);
      expect(e.tags.length).toBeGreaterThan(0);
      expect(e.license.trim().length).toBeGreaterThan(0);
    }
  });

  it("installs an entry and the plugin then shows up in a scan", async () => {
    const seen: { phase: string; done: number; total: number }[] = [];
    b.onPluginInstall((p) => seen.push({ phase: p.phase, done: p.done, total: p.total }));
    expect((await b.scanPlugins()).map((p) => p.name)).not.toContain("Dexed");

    await b.installPlugin("dexed");

    expect(seen[0].phase).toBe("downloading");
    expect(seen.at(-1)?.phase).toBe("installed");
    // A progress bar drawn from these must never run past its end.
    for (const s of seen) expect(s.done).toBeLessThanOrEqual(s.total);
    expect((await b.scanPlugins()).map((p) => p.name)).toContain("Dexed");
  });

  it("refuses an entry that is not in the catalog", async () => {
    await expect(b.installPlugin("not-a-plugin")).rejects.toBe("no such catalog entry");
  });
});

// ─── audio tracks ──────────────────────────────────────────────────────────────────────────

describe("audio tracks", () => {
  const SOURCE_ID = "11111111-2222-3333-4444-555555555555";
  /** An id nothing in these tests ever uses. */
  const MISSING_ID = "99999999-9999-4999-8999-999999999999";

  /** Add an audio track and return its id. */
  async function addAudioTrack(name = "guitar"): Promise<string> {
    await b.apply({ type: "addAudioTrack", name });
    const m = await model();
    return m.tracks[m.tracks.length - 1].id;
  }

  const source = (over: Partial<AudioSource> = {}): AudioSource => ({
    id: SOURCE_ID,
    name: "take 1",
    frames: 48_000,
    sampleRate: 48_000,
    channels: 1,
    ...over,
  });

  const clip = (trackId: string, over: Partial<AudioClip> = {}): AudioClip => ({
    id: FAKE_ID,
    trackId,
    sourceId: SOURCE_ID,
    startTick: 0,
    lengthSeconds: 1,
    offsetSeconds: 0,
    gain: 1,
    fadeInSeconds: 0,
    fadeOutSeconds: 0,
    muted: false,
    ...over,
  });

  it("creates an audio track with a rig and no drum kit", async () => {
    const id = await addAudioTrack();
    const t = (await model()).tracks.find((x) => x.id === id)!;
    expect(t.isAudio).toBe(true);
    expect(t.isDrum).toBe(false);
    expect(t.amp).not.toBeNull();
    expect(t.amp!.amp.model).toBe("crunch");
    // A note track never carries one.
    expect((await model()).tracks[0].amp).toBeNull();
  });

  it("names a blank audio track rather than leaving it unidentifiable", async () => {
    await b.apply({ type: "addAudioTrack", name: "   " });
    const m = await model();
    expect(m.tracks[m.tracks.length - 1].name).toBe("audio");
  });

  it("lands a take as one source and one clip", async () => {
    const id = await addAudioTrack();
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id) });
    const m = await model();
    expect(m.audioSources).toHaveLength(1);
    expect(m.audioClips).toHaveLength(1);
    expect(m.audioClips![0].trackId).toBe(id);
  });

  it("refuses a take whose clip points somewhere else", async () => {
    const id = await addAudioTrack();
    // Both of these would be silently dropped by `normalized`, so they are refused outright.
    await rejectsAndKeepsModel(
      { type: "addAudioTake", trackId: id, source: source(), clip: clip(FAKE_ID) },
      "invalid value",
    );
    await rejectsAndKeepsModel(
      { type: "addAudioTake", trackId: id, source: source(), clip: clip(id, { sourceId: FAKE_ID }) },
      "invalid value",
    );
    await rejectsAndKeepsModel(
      { type: "addAudioTake", trackId: id, source: source({ frames: 0 }), clip: clip(id) },
      "invalid value",
    );
  });

  it("refuses audio on a note track", async () => {
    const { melody } = await ids();
    await rejectsAndKeepsModel({ type: "setTrackAmp", id: melody, amp: defaultAmpRig() }, "not an audio track");
    await rejectsAndKeepsModel(
      { type: "addAudioTake", trackId: melody, source: source(), clip: clip(melody) },
      "not an audio track",
    );
  });

  it("clamps a rig instead of storing nonsense, and rejects NaN outright", async () => {
    const id = await addAudioTrack();
    const base = defaultAmpRig();
    await b.apply({
      type: "setTrackAmp",
      id,
      amp: {
        ...base,
        inputDb: 99,
        amp: { ...base.amp, gain: -5, master: 50 },
        cab: { ...base.cab, posX: 99, posZ: -4 },
      },
    });
    const rig = (await model()).tracks.find((t) => t.id === id)!.amp!;
    expect(rig.inputDb).toBe(24);
    expect(rig.amp.gain).toBe(0);
    expect(rig.amp.master).toBe(10);
    // Mic placement is a 0…10 knob like the rest of the rig.
    expect(rig.cab.posX).toBe(10);
    expect(rig.cab.posZ).toBe(0);

    await rejectsAndKeepsModel(
      { type: "setTrackAmp", id, amp: { ...base, inputDb: Number.NaN } },
      "invalid value",
    );
    // A placement that cannot be made sane sinks the whole command, like any other knob.
    await rejectsAndKeepsModel(
      { type: "setTrackAmp", id, amp: { ...base, cab: { ...base.cab, posZ: Number.POSITIVE_INFINITY } } },
      "invalid value",
    );
  });

  it("defaults an unknown eq or mic instead of refusing the rig", async () => {
    const id = await addAudioTrack();
    const base = defaultAmpRig();
    // What a file from another build looks like: a component this build has never heard of, and
    // one the old shape never wrote at all.
    await b.apply({
      type: "setTrackAmp",
      id,
      amp: {
        ...base,
        eq: "tweedBright" as unknown as EqModel,
        cab: { ...base.cab, micModel: undefined as unknown as MicModel },
      },
    });
    const rig = (await model()).tracks.find((t) => t.id === id)!.amp!;
    expect(rig.eq).toBe("usClassic");
    expect(rig.cab.micModel).toBe("dynamic");
  });

  it("keeps at most MAX_PEDALS pedals", async () => {
    const id = await addAudioTrack();
    const pedals = PEDAL_KINDS.map(newPedal);
    expect(pedals.length).toBeGreaterThan(MAX_PEDALS);
    await b.apply({ type: "setTrackAmp", id, amp: { ...defaultAmpRig(), pedals } });
    expect((await model()).tracks.find((t) => t.id === id)!.amp!.pedals).toHaveLength(MAX_PEDALS);
  });

  it("clamps clip gain and fades, and refuses a clip with no length", async () => {
    const id = await addAudioTrack();
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id, { lengthSeconds: 2 }) });
    const placed = (await model()).audioClips![0];

    await b.apply({
      type: "updateAudioClip",
      clip: { ...placed, gain: 99, fadeInSeconds: 99, fadeOutSeconds: -1, offsetSeconds: -3 },
    });
    const edited = (await model()).audioClips![0];
    expect(edited.gain).toBe(MAX_CLIP_GAIN);
    // Fades may not overlap, so each is capped at half the clip.
    expect(edited.fadeInSeconds).toBe(1);
    expect(edited.fadeOutSeconds).toBe(0);
    expect(edited.offsetSeconds).toBe(0);

    await rejectsAndKeepsModel({ type: "updateAudioClip", clip: { ...placed, lengthSeconds: 0 } }, "invalid value");
    await rejectsAndKeepsModel({ type: "updateAudioClip", clip: { ...placed, startTick: -1 } }, "invalid value");
    // Not `FAKE_ID`: the helper gives the placed clip that id, so it names a real clip here.
    await rejectsAndKeepsModel({ type: "removeAudioClip", id: MISSING_ID }, "no such audio clip");
  });

  it("keeps audio clips in start order", async () => {
    const id = await addAudioTrack();
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id, { startTick: 8 * TICKS_PER_BEAT }) });
    await b.apply({
      type: "addAudioTake",
      trackId: id,
      source: source(),
      clip: clip(id, { id: "22222222-2222-2222-2222-222222222222", startTick: 2 * TICKS_PER_BEAT }),
    });
    expect((await model()).audioClips!.map((c) => c.startTick)).toEqual([1920, 7680]);
    // The same source twice is stored once.
    expect((await model()).audioSources).toHaveLength(1);
  });

  it("purges a deleted audio track's clips", async () => {
    const id = await addAudioTrack();
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id) });
    await b.apply({ type: "deleteTrack", id });
    const m = await model();
    expect(m.audioClips).toHaveLength(0);
    // The source survives: undoing the delete must not force a re-decode.
    expect(m.audioSources).toHaveLength(1);
  });

  it("records one undo step for a rig gesture", async () => {
    const id = await addAudioTrack();
    const before = (await model()).tracks.find((t) => t.id === id)!.amp!;
    for (const gain of [6, 7, 8, 9]) {
      await b.apply({ type: "setTrackAmp", id, amp: { ...before, amp: { ...before.amp, gain } } }, true);
    }
    await b.commitGesture();
    const s = await b.undo();
    expect(s.model.tracks.find((t) => t.id === id)!.amp!.amp.gain).toBe(before.amp.gain);
  });

  it("survives a save and reopen", async () => {
    const id = await addAudioTrack();
    await b.apply({
      type: "setTrackAmp",
      id,
      amp: {
        ...defaultAmpRig(),
        pedals: [newPedal("delay"), newPedal("wah")],
        eq: "britishBright",
        cab: { model: "twoByTwelve", micModel: "ribbon", posX: 7.5, posZ: 1 },
      },
    });
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id) });
    await b.saveProject("/tmp/guitar.dissonant");
    await b.newProject(false);
    await b.openProject("/tmp/guitar.dissonant");

    const m = await model();
    const t = m.tracks.find((x) => x.isAudio)!;
    expect(t.amp!.pedals.map((p) => p.kind)).toEqual(["delay", "wah"]);
    expect(t.amp!.eq).toBe("britishBright");
    expect(t.amp!.cab).toEqual({ model: "twoByTwelve", micModel: "ribbon", posX: 7.5, posZ: 1 });
    expect(m.audioClips).toHaveLength(1);
  });

  it("drops a clip whose source is gone when the project is read back", async () => {
    const id = await addAudioTrack();
    await b.apply({ type: "addAudioTake", trackId: id, source: source(), clip: clip(id) });
    await b.saveProject("/tmp/orphan.dissonant");
    // Simulate a file hand-edited to lose its source list.
    const m = await model();
    m.audioSources = [];
    await b.newProject(false);
    await b.openProject("/tmp/orphan.dissonant");
    expect((await model()).audioClips).toHaveLength(1);
  });

  it("says so plainly when recording needs the desktop app", async () => {
    await expect(b.startAudioRecord()).rejects.toThrow("desktop app");
    await expect(b.stopAudioRecord()).rejects.toThrow("desktop app");
    await expect(b.importAudio("/tmp/riff.wav", null)).rejects.toThrow("desktop app");
    expect(await b.inputDevices()).toEqual([]);
  });

  it("draws a deterministic waveform so the screen can be designed without audio", async () => {
    const a = await b.audioPeaks(SOURCE_ID, 64);
    const again = await b.audioPeaks(SOURCE_ID, 64);
    expect(a).toHaveLength(64);
    expect(a).toEqual(again);
    expect(await b.audioPeaks("a-different-id", 64)).not.toEqual(a);
    for (const [lo, hi] of a) {
      expect(lo).toBeLessThanOrEqual(hi);
      expect(lo).toBeGreaterThanOrEqual(-1);
      expect(hi).toBeLessThanOrEqual(1);
    }
  });
});
