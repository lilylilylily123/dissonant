// The one place the UI talks to the backend. In Tauri it calls Rust commands; in a plain
// browser (`pnpm --dir ui dev`) it runs an in-memory mock so the interface can be designed and
// exercised without audio. Keep the mock's command semantics aligned with dissonant-core.

import type {
  AudioStatus,
  ChordEvent,
  Command,
  MasterSettings,
  NoteEvent,
  PlayheadEvent,
  PlayMode,
  ProjectModel,
  Snapshot,
  Track,
  SongPattern,
  TrackParam,
} from "./types";
import { uuid } from "./types";
import { normalize, progression, STARTERS } from "./theory";

export interface Bridge {
  readonly isTauri: boolean;
  getState(): Promise<Snapshot>;
  apply(command: Command, transient?: boolean): Promise<Snapshot>;
  commitGesture(): Promise<Snapshot>;
  undo(): Promise<Snapshot>;
  redo(): Promise<Snapshot>;
  newProject(starter?: boolean): Promise<Snapshot>;
  openProject(path: string): Promise<Snapshot>;
  saveProject(path?: string): Promise<Snapshot>;
  setPlaybackContext(mode: PlayMode, patternId: string | null): Promise<void>;
  play(): Promise<void>;
  stop(): Promise<void>;
  seek(beat: number): Promise<void>;
  setHearChords(on: boolean): Promise<void>;
  /** `seconds` is how long the note is held before its automatic release. */
  audition(trackId: string, pitch: number, velocity?: number, seconds?: number): Promise<void>;
  auditionOff(trackId: string, pitch: number): Promise<void>;
  audioStatus(): Promise<AudioStatus>;
  restartAudio(): Promise<AudioStatus>;
  exportWav(path: string, tailSeconds?: number): Promise<string>;
  onPlayhead(cb: (e: PlayheadEvent) => void): () => void;
  pickOpenPath(): Promise<string | null>;
  pickSavePath(defaultName: string, extension: string): Promise<string | null>;
}

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ─── Tauri implementation ──────────────────────────────────────────────────────────────────

async function tauriBridge(): Promise<Bridge> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const dialog = await import("@tauri-apps/plugin-dialog");

  return {
    isTauri: true,
    getState: () => invoke<Snapshot>("get_state"),
    apply: (command, transient = false) => invoke<Snapshot>("apply", { command, transient }),
    commitGesture: () => invoke<Snapshot>("commit_gesture"),
    undo: () => invoke<Snapshot>("undo"),
    redo: () => invoke<Snapshot>("redo"),
    newProject: (starter = true) => invoke<Snapshot>("new_project", { starter }),
    openProject: (path) => invoke<Snapshot>("open_project", { path }),
    saveProject: (path) => invoke<Snapshot>("save_project", { path: path ?? null }),
    setPlaybackContext: (mode, patternId) => invoke("set_playback_context", { mode, patternId }),
    play: () => invoke("transport_play"),
    stop: () => invoke("transport_stop"),
    seek: (beat) => invoke("transport_seek", { beat }),
    setHearChords: (on) => invoke("set_hear_chords", { on }),
    audition: (trackId, pitch, velocity = 100, seconds) => invoke("audition", { trackId, pitch, velocity, seconds }),
    auditionOff: (trackId, pitch) => invoke("audition_off", { trackId, pitch }),
    audioStatus: () => invoke<AudioStatus>("audio_status"),
    restartAudio: () => invoke<AudioStatus>("restart_audio"),
    exportWav: (path, tailSeconds = 1.5) => invoke<string>("export_wav", { path, tailSeconds }),
    onPlayhead: (cb) => {
      let unlisten: (() => void) | null = null;
      let cancelled = false;
      listen<PlayheadEvent>("playhead", (e) => cb(e.payload)).then((u) => {
        if (cancelled) u();
        else unlisten = u;
      });
      return () => {
        cancelled = true;
        unlisten?.();
      };
    },
    pickOpenPath: async () => {
      const r = await dialog.open({
        multiple: false,
        directory: false,
        filters: [{ name: "dissonant project", extensions: ["dissonant", "json"] }],
      });
      return typeof r === "string" ? r : null;
    },
    pickSavePath: async (defaultName, extension) => {
      const r = await dialog.save({
        defaultPath: defaultName,
        filters: [{ name: extension, extensions: [extension] }],
      });
      return r ?? null;
    },
  };
}

// ─── Browser mock ──────────────────────────────────────────────────────────────────────────

/**
 * `dissonant_core::document::EditError`. Over Tauri IPC `apply` maps the error with
 * `to_string`, so `invoke` rejects with the bare message — the mock rejects with the same
 * value so a rejected command looks identical in both bridges.
 */
const NO_SUCH_TRACK = "no such track";
const NO_SUCH_PATTERN = "no such pattern";
const LAST_TRACK = "a project needs at least one track";
const LAST_PATTERN = "a project needs at least one pattern";
const INVALID_VALUE = "invalid value";

function fail(message: string): never {
  // A plain string, not an `Error`: that is what Tauri's `invoke` rejects with.
  throw message;
}

const clamp = (x: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, x));
const allFinite = (...xs: number[]) => xs.every((x) => Number.isFinite(x));

/** `TrackParam` ranges, from `apply_to`'s `SetTrackParam` arm. */
const TRACK_PARAM_RANGE: Record<TrackParam, [number, number]> = {
  volume: [0, 1.5],
  reverbSend: [0, 1],
  tone: [200, 20_000],
  pan: [-1, 1],
};

function newTrack(isDrum: boolean, name: string): Track {
  return {
    id: uuid(),
    name,
    voice: "saw",
    muted: false,
    soloed: false,
    isDrum,
    volume: 1,
    reverbSend: 0,
    tone: 18000,
    pan: 0,
    color: null,
  };
}

function newPattern(name: string): SongPattern {
  return { id: uuid(), name, lengthBeats: 16, chords: { chords: [] }, notesByTrack: {} };
}

/** `ProjectModel::empty()`: one melody track, one empty pattern, no key, nothing arranged. */
function emptyModel(): ProjectModel {
  return {
    schemaVersion: 3,
    tempo: 120,
    key: { rootPitchClass: null, scale: "major", isLocked: false },
    tracks: [newTrack(false, "melody")],
    patterns: [newPattern("pattern 1")],
    arrangement: [],
    master: { gain: 1, reverbWet: 0, lowCutHz: 20, highCutHz: 18000, lowEq: 1, midEq: 1, highEq: 1 },
  };
}

/** `ProjectModel::starter()`: `empty()` plus a I–IV–V–vi progression, a drum track and a song. */
function starterModel(): ProjectModel {
  const m = emptyModel();
  m.patterns[0].chords = { chords: progression([0, 3, 4, 5], 0, "major", 16) };
  m.tracks.push(newTrack(true, "drums"));
  m.arrangement = [m.patterns[0].id];
  return m;
}

/**
 * `NoteEvent::sanitized`. Pitch and velocity are `i32` in Rust and every number crosses the IPC
 * boundary as JSON, so a non-finite or fractional value is rejected outright rather than
 * silently stored — in the desktop app serde would refuse the whole payload.
 */
function sanitizeNote(n: NoteEvent): NoteEvent {
  if (!allFinite(n.startBeat, n.lengthBeats) || !Number.isInteger(n.pitch) || !Number.isInteger(n.velocity)) {
    fail(INVALID_VALUE);
  }
  return {
    ...n,
    pitch: clamp(n.pitch, 0, 127),
    velocity: clamp(n.velocity, 1, 127),
    startBeat: Math.max(0, n.startBeat),
    lengthBeats: Math.max(1 / 64, n.lengthBeats),
  };
}

/** `ProjectModel::normalized`, run on anything read back from storage. */
function normalizedModel(m: ProjectModel): ProjectModel {
  m.schemaVersion = 3;
  if (m.tracks.length === 0) m.tracks = [newTrack(false, "melody")];
  if (m.patterns.length === 0) m.patterns = [newPattern("pattern 1")];
  for (const p of m.patterns) {
    for (const notes of Object.values(p.notesByTrack)) {
      for (let i = 0; i < notes.length; i++) notes[i] = sanitizeNote(notes[i]);
    }
  }
  m.arrangement = m.arrangement.filter((id) => m.patterns.some((p) => p.id === id));
  return m;
}

function clone<T>(x: T): T {
  return JSON.parse(JSON.stringify(x));
}

/**
 * Mirrors `apply_to` in crates/dissonant-core/src/document.rs: same validation, same clamping,
 * same errors. It mutates a throwaway clone, so a thrown error leaves the model untouched —
 * exactly like the Rust version, which discards `next` on `Err`.
 */
function reduce(m: ProjectModel, c: Command): void {
  const track = (id: string) => m.tracks.find((t) => t.id === id) ?? fail(NO_SUCH_TRACK);
  const pattern = (id: string) => m.patterns.find((p) => p.id === id) ?? fail(NO_SUCH_PATTERN);
  switch (c.type) {
    case "setTempo":
      if (!allFinite(c.bpm)) fail(INVALID_VALUE);
      m.tempo = clamp(c.bpm, 20, 300);
      break;
    case "setKey":
      m.key = { ...c.key };
      break;
    case "addTrack":
      m.tracks.push(newTrack(c.isDrum, c.isDrum ? "drums" : `track ${m.tracks.length + 1}`));
      break;
    case "setTrackColor":
      if (c.color !== null && !/^#[0-9a-f]{6}$/i.test(c.color)) fail(INVALID_VALUE);
      track(c.id).color = c.color === null ? null : c.color.toLowerCase();
      break;
    case "deleteTrack": {
      if (m.tracks.length <= 1) fail(LAST_TRACK);
      const before = m.tracks.length;
      m.tracks = m.tracks.filter((t) => t.id !== c.id);
      if (m.tracks.length === before) fail(NO_SUCH_TRACK);
      for (const p of m.patterns) delete p.notesByTrack[c.id];
      break;
    }
    case "renameTrack":
      track(c.id).name = c.name;
      break;
    case "setTrackMuted":
      track(c.id).muted = c.muted;
      break;
    case "setTrackSoloed":
      track(c.id).soloed = c.soloed;
      break;
    case "moveTrack": {
      const i = m.tracks.findIndex((t) => t.id === c.id);
      if (i < 0) fail(NO_SUCH_TRACK);
      const j = c.up ? i - 1 : i + 1;
      if (j >= 0 && j < m.tracks.length) [m.tracks[i], m.tracks[j]] = [m.tracks[j], m.tracks[i]];
      break;
    }
    case "setTrackVoice":
      track(c.id).voice = c.voice;
      break;
    case "setTrackParam": {
      if (!allFinite(c.value)) fail(INVALID_VALUE);
      const [lo, hi] = TRACK_PARAM_RANGE[c.param];
      track(c.id)[c.param] = clamp(c.value, lo, hi);
      break;
    }
    case "addPattern": {
      const p = newPattern(`pattern ${m.patterns.length + 1}`);
      const root = m.key.rootPitchClass ?? 0;
      p.chords = { chords: progression(STARTERS[0].degrees, root, m.key.scale, p.lengthBeats) };
      m.patterns.push(p);
      break;
    }
    case "duplicatePattern": {
      const i = m.patterns.findIndex((p) => p.id === c.id);
      if (i < 0) fail(NO_SUCH_PATTERN);
      const copy = clone(m.patterns[i]);
      copy.id = uuid();
      copy.name = `${copy.name} copy`;
      for (const notes of Object.values(copy.notesByTrack)) for (const n of notes) n.id = uuid();
      for (const ch of copy.chords.chords) ch.id = uuid();
      m.patterns.splice(i + 1, 0, copy);
      break;
    }
    case "deletePattern": {
      if (m.patterns.length <= 1) fail(LAST_PATTERN);
      const before = m.patterns.length;
      m.patterns = m.patterns.filter((p) => p.id !== c.id);
      if (m.patterns.length === before) fail(NO_SUCH_PATTERN);
      m.arrangement = m.arrangement.filter((id) => id !== c.id);
      break;
    }
    case "renamePattern":
      if (!c.name.trim()) fail(INVALID_VALUE);
      pattern(c.id).name = c.name;
      break;
    case "setPatternLength":
      if (!allFinite(c.beats) || c.beats < 1) fail(INVALID_VALUE);
      pattern(c.id).lengthBeats = c.beats;
      break;
    case "setNotes": {
      track(c.trackId); // Rust checks the track before the pattern.
      const p = pattern(c.patternId);
      const notes: NoteEvent[] = c.notes
        .map(sanitizeNote)
        .sort((a, b) => a.startBeat - b.startBeat || a.pitch - b.pitch);
      if (notes.length === 0) delete p.notesByTrack[c.trackId];
      else p.notesByTrack[c.trackId] = notes;
      break;
    }
    case "setChords": {
      const p = pattern(c.patternId);
      const chords: ChordEvent[] = c.chords
        .filter((ch) => ch.lengthBeats > 0 && ch.startBeat >= 0)
        .map((ch) => ({ ...ch, pitchClasses: [...new Set(ch.pitchClasses.map(normalize))].sort((a, b) => a - b) }))
        .sort((a, b) => a.startBeat - b.startBeat);
      p.chords = { chords };
      break;
    }
    case "setArrangement":
      m.arrangement = c.arrangement.filter((id) => m.patterns.some((p) => p.id === id));
      break;
    case "setMaster": {
      const v = c.master;
      if (!allFinite(v.gain, v.reverbWet, v.lowCutHz, v.highCutHz, v.lowEq, v.midEq, v.highEq)) fail(INVALID_VALUE);
      const master: MasterSettings = {
        gain: clamp(v.gain, 0, 1.5),
        reverbWet: clamp(v.reverbWet, 0, 1),
        lowCutHz: clamp(v.lowCutHz, 10, 2_000),
        highCutHz: clamp(v.highCutHz, 500, 20_000),
        lowEq: clamp(v.lowEq, 0, 2),
        midEq: clamp(v.midEq, 0, 2),
        highEq: clamp(v.highEq, 0, 2),
      };
      m.master = master;
      break;
    }
  }
}

/** `document::MAX_UNDO`. */
const MAX_UNDO = 200;

/** The mock's stand-in for the filesystem: one slot, written by save, read by open. */
const STORAGE_KEY = "dissonant.mock.project";

function mockBridge(): Bridge {
  let model = starterModel();
  const undo: ProjectModel[] = [];
  const redo: ProjectModel[] = [];
  let pending: ProjectModel | null = null;
  // Mirrors Document's save point: dirty is "differs from what was saved", not a sticky flag,
  // so undoing back to the saved state (or a round-trip slider drag) leaves the project clean.
  // `Document::new` takes a save point immediately, so a fresh project is clean until edited.
  let saved: string | null = JSON.stringify(model);
  let path: string | null = null;
  let mode: PlayMode = "pattern";
  let patternId: string | null = null;
  let playing = false;
  let beat = 0;
  let last = performance.now();
  const listeners = new Set<(e: PlayheadEvent) => void>();

  const snapshot = (): Snapshot => ({
    model: clone(model),
    canUndo: undo.length > 0 || (pending !== null && JSON.stringify(pending) !== JSON.stringify(model)),
    canRedo: redo.length > 0,
    dirty: saved !== JSON.stringify(model),
    path,
  });

  const loopLength = () => {
    if (mode === "song" && model.arrangement.length > 0) {
      return model.arrangement.reduce((sum, id) => sum + (model.patterns.find((p) => p.id === id)?.lengthBeats ?? 0), 0);
    }
    const p = model.patterns.find((x) => x.id === patternId) ?? model.patterns[0];
    return p?.lengthBeats ?? 16;
  };

  const tick = () => {
    const now = performance.now();
    if (playing) {
      beat += ((now - last) / 1000) * (model.tempo / 60);
      const len = loopLength();
      if (beat >= len) beat = beat % len;
    }
    last = now;
    const e: PlayheadEvent = { beat, playing, masterPeak: [0, 0], trackPeaks: [] };
    for (const cb of listeners) cb(e);
    requestAnimationFrame(tick);
  };
  requestAnimationFrame(tick);

  // A tiny WebAudio blip so placing notes gives feedback in the browser mock.
  let ctx: AudioContext | null = null;
  const blip = (pitch: number, velocity: number, seconds = 0.25) => {
    try {
      ctx ??= new AudioContext();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      const hold = Math.min(4, Math.max(0.12, seconds));
      osc.type = "sawtooth";
      osc.frequency.value = 440 * Math.pow(2, (pitch - 69) / 12);
      gain.gain.setValueAtTime(0.0001, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.15 * (velocity / 127), ctx.currentTime + 0.01);
      gain.gain.setValueAtTime(0.15 * (velocity / 127), ctx.currentTime + hold);
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + hold + 0.08);
      osc.connect(gain).connect(ctx.destination);
      osc.start();
      osc.stop(ctx.currentTime + hold + 0.1);
    } catch {
      /* no audio in this browser */
    }
  };

  /** `Document::push_undo` — the Rust history is capped at `MAX_UNDO` snapshots. */
  const pushUndo = (snap: ProjectModel) => {
    undo.push(snap);
    if (undo.length > MAX_UNDO) undo.shift();
  };

  /** `Document::replace` — new/open install a model and drop the whole history, pending included. */
  const replace = (next: ProjectModel, to: string | null) => {
    model = next;
    undo.length = 0;
    redo.length = 0;
    pending = null;
    path = to;
    saved = JSON.stringify(model);
    beat = 0;
  };

  const commitGesture = () => {
    if (pending && JSON.stringify(pending) !== JSON.stringify(model)) pushUndo(pending);
    pending = null;
  };

  return {
    isTauri: false,
    getState: async () => snapshot(),
    apply: async (command, transient = false) => {
      const before = clone(model);
      const next = clone(model);
      reduce(next, command);
      if (JSON.stringify(next) === JSON.stringify(before)) return snapshot();
      if (transient) pending ??= before;
      else {
        commitGesture();
        pushUndo(before);
      }
      redo.length = 0;
      model = next;
      return snapshot();
    },
    commitGesture: async () => {
      commitGesture();
      return snapshot();
    },
    undo: async () => {
      commitGesture();
      const prev = undo.pop();
      if (prev) {
        redo.push(model);
        model = prev;
      }
      return snapshot();
    },
    redo: async () => {
      commitGesture();
      const next = redo.pop();
      if (next) {
        pushUndo(model);
        model = next;
      }
      return snapshot();
    },
    newProject: async (starter = true) => {
      replace(starter ? starterModel() : emptyModel(), null);
      return snapshot();
    },
    openProject: async (p) => {
      const stored = localStorage.getItem(STORAGE_KEY);
      if (!stored) fail(`could not read ${p}`);
      replace(normalizedModel(JSON.parse(stored) as ProjectModel), p);
      return snapshot();
    },
    saveProject: async (p) => {
      const target = p ?? path ?? fail("no file path yet — use Save As");
      path = target;
      saved = JSON.stringify(model);
      localStorage.setItem(STORAGE_KEY, saved);
      return snapshot();
    },
    setPlaybackContext: async (m, pid) => {
      if (m !== mode || pid !== patternId) beat = 0;
      mode = m;
      patternId = pid;
    },
    play: async () => {
      playing = true;
      last = performance.now();
    },
    stop: async () => {
      playing = false;
    },
    seek: async (b) => {
      beat = Math.max(0, b);
    },
    setHearChords: async () => {},
    audition: async (_t, pitch, velocity = 100, seconds) => blip(pitch, velocity, seconds),
    auditionOff: async () => {},
    audioStatus: async () => ({ running: false, sampleRate: null, error: "browser mock: no engine" }),
    restartAudio: async () => ({ running: false, sampleRate: null, error: "browser mock: no engine" }),
    exportWav: async (p) => {
      alert("Export needs the desktop app (Tauri). In the browser mock nothing is rendered.");
      return p;
    },
    onPlayhead: (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    pickOpenPath: async () => null,
    pickSavePath: async (name, ext) => `${name}.${ext}`,
  };
}

let bridgePromise: Promise<Bridge> | null = null;

export function getBridge(): Promise<Bridge> {
  bridgePromise ??= isTauri ? tauriBridge() : Promise.resolve(mockBridge());
  return bridgePromise;
}
