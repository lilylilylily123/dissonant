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
  audition(trackId: string, pitch: number, velocity?: number): Promise<void>;
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
    audition: (trackId, pitch, velocity = 100) => invoke("audition", { trackId, pitch, velocity }),
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

function starterModel(): ProjectModel {
  const melody: Track = {
    id: uuid(),
    name: "melody",
    voice: "saw",
    muted: false,
    soloed: false,
    isDrum: false,
    volume: 1,
    reverbSend: 0,
    tone: 18000,
    pan: 0,
  };
  const drums: Track = { ...melody, id: uuid(), name: "drums", isDrum: true };
  const pattern: SongPattern = {
    id: uuid(),
    name: "pattern 1",
    lengthBeats: 16,
    chords: { chords: progression([0, 3, 4, 5], 0, "major", 16) },
    notesByTrack: {},
  };
  return {
    schemaVersion: 3,
    tempo: 120,
    key: { rootPitchClass: null, scale: "major", isLocked: false },
    tracks: [melody, drums],
    patterns: [pattern],
    arrangement: [pattern.id],
    master: { gain: 1, reverbWet: 0, lowCutHz: 20, highCutHz: 18000, lowEq: 1, midEq: 1, highEq: 1 },
  };
}

function clone<T>(x: T): T {
  return JSON.parse(JSON.stringify(x));
}

function reduce(m: ProjectModel, c: Command): void {
  const track = (id: string) => m.tracks.find((t) => t.id === id);
  const pattern = (id: string) => m.patterns.find((p) => p.id === id);
  switch (c.type) {
    case "setTempo":
      m.tempo = Math.min(300, Math.max(20, c.bpm));
      break;
    case "setKey":
      m.key = c.key;
      break;
    case "addTrack": {
      const t: Track = {
        id: uuid(),
        name: c.isDrum ? "drums" : `track ${m.tracks.length + 1}`,
        voice: "saw",
        muted: false,
        soloed: false,
        isDrum: c.isDrum,
        volume: 1,
        reverbSend: 0,
        tone: 18000,
        pan: 0,
      };
      m.tracks.push(t);
      break;
    }
    case "deleteTrack":
      if (m.tracks.length > 1) {
        m.tracks = m.tracks.filter((t) => t.id !== c.id);
        for (const p of m.patterns) delete p.notesByTrack[c.id];
      }
      break;
    case "renameTrack": {
      const t = track(c.id);
      if (t) t.name = c.name;
      break;
    }
    case "setTrackMuted": {
      const t = track(c.id);
      if (t) t.muted = c.muted;
      break;
    }
    case "setTrackSoloed": {
      const t = track(c.id);
      if (t) t.soloed = c.soloed;
      break;
    }
    case "moveTrack": {
      const i = m.tracks.findIndex((t) => t.id === c.id);
      const j = c.up ? i - 1 : i + 1;
      if (i >= 0 && j >= 0 && j < m.tracks.length) [m.tracks[i], m.tracks[j]] = [m.tracks[j], m.tracks[i]];
      break;
    }
    case "setTrackVoice": {
      const t = track(c.id);
      if (t) t.voice = c.voice;
      break;
    }
    case "setTrackParam": {
      const t = track(c.id);
      if (t) t[c.param] = c.value;
      break;
    }
    case "addPattern": {
      const root = m.key.rootPitchClass ?? 0;
      m.patterns.push({
        id: uuid(),
        name: `pattern ${m.patterns.length + 1}`,
        lengthBeats: 16,
        chords: { chords: progression(STARTERS[0].degrees, root, m.key.scale, 16) },
        notesByTrack: {},
      });
      break;
    }
    case "duplicatePattern": {
      const i = m.patterns.findIndex((p) => p.id === c.id);
      if (i < 0) break;
      const copy = clone(m.patterns[i]);
      copy.id = uuid();
      copy.name = `${copy.name} copy`;
      for (const notes of Object.values(copy.notesByTrack)) for (const n of notes) n.id = uuid();
      for (const ch of copy.chords.chords) ch.id = uuid();
      m.patterns.splice(i + 1, 0, copy);
      break;
    }
    case "deletePattern":
      if (m.patterns.length > 1) {
        m.patterns = m.patterns.filter((p) => p.id !== c.id);
        m.arrangement = m.arrangement.filter((id) => id !== c.id);
      }
      break;
    case "renamePattern": {
      const p = pattern(c.id);
      if (p && c.name.trim()) p.name = c.name;
      break;
    }
    case "setPatternLength": {
      const p = pattern(c.id);
      if (p) p.lengthBeats = Math.max(1, c.beats);
      break;
    }
    case "setNotes": {
      const p = pattern(c.patternId);
      if (!p) break;
      const notes: NoteEvent[] = c.notes
        .map((n) => ({
          ...n,
          pitch: Math.min(127, Math.max(0, n.pitch)),
          velocity: Math.min(127, Math.max(1, n.velocity)),
          startBeat: Math.max(0, n.startBeat),
          lengthBeats: Math.max(1 / 64, n.lengthBeats),
        }))
        .sort((a, b) => a.startBeat - b.startBeat || a.pitch - b.pitch);
      if (notes.length === 0) delete p.notesByTrack[c.trackId];
      else p.notesByTrack[c.trackId] = notes;
      break;
    }
    case "setChords": {
      const p = pattern(c.patternId);
      if (!p) break;
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
    case "setMaster":
      m.master = { ...c.master } as MasterSettings;
      break;
  }
}

function mockBridge(): Bridge {
  let model = starterModel();
  const undo: ProjectModel[] = [];
  const redo: ProjectModel[] = [];
  let pending: ProjectModel | null = null;
  let dirty = false;
  let path: string | null = null;
  let mode: PlayMode = "pattern";
  let patternId: string | null = null;
  let playing = false;
  let beat = 0;
  let last = performance.now();
  const listeners = new Set<(e: PlayheadEvent) => void>();

  const snapshot = (): Snapshot => ({ model: clone(model), canUndo: undo.length > 0 || pending !== null, canRedo: redo.length > 0, dirty, path });

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
  const blip = (pitch: number, velocity: number) => {
    try {
      ctx ??= new AudioContext();
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sawtooth";
      osc.frequency.value = 440 * Math.pow(2, (pitch - 69) / 12);
      gain.gain.setValueAtTime(0.0001, ctx.currentTime);
      gain.gain.exponentialRampToValueAtTime(0.15 * (velocity / 127), ctx.currentTime + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.5);
      osc.connect(gain).connect(ctx.destination);
      osc.start();
      osc.stop(ctx.currentTime + 0.55);
    } catch {
      /* no audio in this browser */
    }
  };

  const commitGesture = () => {
    if (pending && JSON.stringify(pending) !== JSON.stringify(model)) undo.push(pending);
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
        undo.push(before);
      }
      redo.length = 0;
      model = next;
      dirty = true;
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
        dirty = true;
      }
      return snapshot();
    },
    redo: async () => {
      const next = redo.pop();
      if (next) {
        undo.push(model);
        model = next;
        dirty = true;
      }
      return snapshot();
    },
    newProject: async () => {
      model = starterModel();
      undo.length = 0;
      redo.length = 0;
      dirty = false;
      path = null;
      beat = 0;
      return snapshot();
    },
    openProject: async (p) => {
      path = p;
      return snapshot();
    },
    saveProject: async (p) => {
      if (p) path = p;
      dirty = false;
      localStorage.setItem("dissonant.mock.project", JSON.stringify(model));
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
    audition: async (_t, pitch, velocity = 100) => blip(pitch, velocity),
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
