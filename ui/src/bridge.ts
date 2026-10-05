// The one place the UI talks to the backend. In Tauri it calls Rust commands; in a plain
// browser (`pnpm --dir ui dev`) it runs an in-memory mock so the interface can be designed and
// exercised without audio. Keep the mock's command semantics aligned with dissonant-core.

import type {
  AudioStatus,
  AudioStatusEvent,
  ChordEvent,
  ExportProgress,
  ExportRequest,
  ExportResult,
  Command,
  MasterSettings,
  MidiStatus,
  NoteEvent,
  OutputDevice,
  Settings,
  PlayheadEvent,
  PlayMode,
  ProjectModel,
  RecoveryCandidate,
  Snapshot,
  Track,
  SongPattern,
  TemplateInfo,
} from "./types";
import { uuid, withDefaults } from "./types";
import { normalize, progression, STARTERS } from "./theory";

export interface Bridge {
  readonly isTauri: boolean;
  getState(): Promise<Snapshot>;
  /** `label` names the edit for the Edit menu ("move notes"); the command's own name otherwise. */
  apply(command: Command, transient?: boolean, label?: string): Promise<Snapshot>;
  commitGesture(): Promise<Snapshot>;
  undo(): Promise<Snapshot>;
  redo(): Promise<Snapshot>;
  newProject(starter?: boolean): Promise<Snapshot>;
  openProject(path: string): Promise<Snapshot>;
  saveProject(path?: string): Promise<Snapshot>;
  setPlaybackContext(mode: PlayMode, patternId: string | null): Promise<void>;
  /** Start playing; with `countInBars` the metronome counts first and the transport starts after. */
  play(countInBars?: number): Promise<void>;
  stop(): Promise<void>;
  setMetronome(on: boolean, volume?: number): Promise<void>;
  seek(beat: number): Promise<void>;
  setHearChords(on: boolean): Promise<void>;
  audition(trackId: string, pitch: number, velocity?: number): Promise<void>;
  auditionOff(trackId: string, pitch: number): Promise<void>;
  audioStatus(): Promise<AudioStatus>;
  restartAudio(): Promise<AudioStatus>;
  exportWav(request: ExportRequest): Promise<ExportResult>;
  onExportProgress(cb: (p: ExportProgress) => void): () => void;
  exportMidi(path: string, scope: "song" | "pattern"): Promise<string>;
  importMidi(path: string, patternId: string): Promise<Snapshot>;
  listTemplates(): Promise<TemplateInfo[]>;
  saveTemplate(name: string): Promise<TemplateInfo>;
  deleteTemplate(name: string): Promise<void>;
  newFromTemplate(name: string): Promise<Snapshot>;
  onPlayhead(cb: (e: PlayheadEvent) => void): () => void;
  /** The backend changed the document on its own (e.g. a recorded note landed). */
  onDocument(cb: (s: Snapshot) => void): () => void;
  onMidiActivity(cb: (pitch: number) => void): () => void;
  midiStatus(): Promise<MidiStatus>;
  openMidiInput(name?: string): Promise<MidiStatus>;
  closeMidiInput(): Promise<MidiStatus>;
  setLiveTrack(trackId: string | null): Promise<void>;
  setRecord(armed: boolean, quantize?: number): Promise<MidiStatus>;
  noteOn(pitch: number, velocity?: number): Promise<void>;
  noteOff(pitch: number): Promise<void>;
  setLoop(start: number, end: number): Promise<void>;
  clearLoop(): Promise<void>;
  setLooping(on: boolean): Promise<void>;
  /** Native open dialog; `extensions` defaults to the project formats. */
  pickOpenPath(extensions?: string[], label?: string): Promise<string | null>;
  pickSavePath(defaultName: string, extension: string): Promise<string | null>;
  /** OS window title (file name + dirty mark). */
  setTitle(title: string): Promise<void>;
  /** Close the app for real; the backend only asks first while the document is dirty. */
  quit(): Promise<void>;
  /** The window's close button was pressed while the document was dirty. */
  onCloseRequested(cb: () => void): () => void;
  /** A `.dissonant` file was dropped onto the window. */
  onFileDropped(cb: (path: string) => void): () => void;
  /** The audio device changed under us (lost, switched, reappeared). */
  onAudioStatus(cb: (e: AudioStatusEvent) => void): () => void;
  recentFiles(): Promise<string[]>;
  clearRecent(): Promise<void>;
  recoveryCandidates(): Promise<RecoveryCandidate[]>;
  restoreAutosave(autosavePath: string): Promise<Snapshot>;
  discardAutosave(autosavePath: string): Promise<void>;
  autosaveNow(): Promise<string | null>;
  getSettings(): Promise<Settings>;
  /** Persist settings; the backend restarts audio / reopens MIDI as needed and returns the audio status. */
  setSettings(settings: Settings): Promise<AudioStatus>;
  outputDevices(): Promise<OutputDevice[]>;
  testTone(): Promise<void>;
  /** The backend opened or dropped a MIDI input on its own. */
  onMidiStatus(cb: (s: MidiStatus) => void): () => void;
}

const isTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

// ─── Tauri implementation ──────────────────────────────────────────────────────────────────

async function tauriBridge(): Promise<Bridge> {
  const { invoke } = await import("@tauri-apps/api/core");
  const { listen } = await import("@tauri-apps/api/event");
  const { getCurrentWindow } = await import("@tauri-apps/api/window");
  const dialog = await import("@tauri-apps/plugin-dialog");

  const subscribe = <T,>(name: string, cb: (payload: T) => void): (() => void) => {
    let unlisten: (() => void) | null = null;
    let cancelled = false;
    listen<T>(name, (e) => cb(e.payload)).then((u) => {
      if (cancelled) u();
      else unlisten = u;
    });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  };

  return {
    isTauri: true,
    getState: () => invoke<Snapshot>("get_state"),
    apply: (command, transient = false, label) => invoke<Snapshot>("apply", { command, transient, label: label ?? null }),
    commitGesture: () => invoke<Snapshot>("commit_gesture"),
    undo: () => invoke<Snapshot>("undo"),
    redo: () => invoke<Snapshot>("redo"),
    newProject: (starter = true) => invoke<Snapshot>("new_project", { starter }),
    openProject: (path) => invoke<Snapshot>("open_project", { path }),
    saveProject: (path) => invoke<Snapshot>("save_project", { path: path ?? null }),
    setPlaybackContext: (mode, patternId) => invoke("set_playback_context", { mode, patternId }),
    play: (countInBars) => invoke("transport_play", { countInBars: countInBars ?? null }),
    stop: () => invoke("transport_stop"),
    setMetronome: (on, volume) => invoke("set_metronome", { on, volume: volume ?? null }),
    seek: (beat) => invoke("transport_seek", { beat }),
    setHearChords: (on) => invoke("set_hear_chords", { on }),
    audition: (trackId, pitch, velocity = 100) => invoke("audition", { trackId, pitch, velocity }),
    auditionOff: (trackId, pitch) => invoke("audition_off", { trackId, pitch }),
    audioStatus: () => invoke<AudioStatus>("audio_status"),
    restartAudio: () => invoke<AudioStatus>("restart_audio"),
    exportWav: (request) => invoke<ExportResult>("export_wav", { request }),
    onExportProgress: (cb) => subscribe<ExportProgress>("export-progress", cb),
    exportMidi: (path, scope) => invoke<string>("export_midi", { path, scope }),
    importMidi: (path, patternId) => invoke<Snapshot>("import_midi", { path, patternId }),
    listTemplates: () => invoke<TemplateInfo[]>("list_templates"),
    saveTemplate: (name) => invoke<TemplateInfo>("save_template", { name }),
    deleteTemplate: (name) => invoke("delete_template", { name }),
    newFromTemplate: (name) => invoke<Snapshot>("new_from_template", { name }),
    onPlayhead: (cb) => subscribe<PlayheadEvent>("playhead", cb),
    onDocument: (cb) => subscribe<Snapshot>("document", cb),
    onMidiActivity: (cb) => subscribe<number>("midi-activity", cb),
    midiStatus: () => invoke<MidiStatus>("midi_status"),
    openMidiInput: (name) => invoke<MidiStatus>("open_midi_input", { name: name ?? null }),
    closeMidiInput: () => invoke<MidiStatus>("close_midi_input"),
    setLiveTrack: (trackId) => invoke("set_live_track", { trackId }),
    setRecord: (armed, quantize) => invoke<MidiStatus>("set_record", { armed, quantize: quantize ?? null }),
    noteOn: (pitch, velocity = 100) => invoke("note_on", { pitch, velocity }),
    noteOff: (pitch) => invoke("note_off", { pitch }),
    setLoop: (start, end) => invoke("set_loop", { start, end }),
    clearLoop: () => invoke("clear_loop"),
    setLooping: (on) => invoke("set_looping", { on }),
    pickOpenPath: async (extensions = ["dissonant", "json"], label = "dissonant project") => {
      const r = await dialog.open({
        multiple: false,
        directory: false,
        filters: [{ name: label, extensions }],
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
    setTitle: (title) => getCurrentWindow().setTitle(title),
    quit: () => invoke("quit"),
    onCloseRequested: (cb) => subscribe<null>("close-requested", () => cb()),
    onFileDropped: (cb) => subscribe<string>("file-dropped", cb),
    onAudioStatus: (cb) => subscribe<AudioStatusEvent>("audio-status", cb),
    recentFiles: () => invoke<string[]>("recent_files"),
    clearRecent: () => invoke("clear_recent"),
    recoveryCandidates: () => invoke<RecoveryCandidate[]>("recovery_candidates"),
    restoreAutosave: (autosavePath) => invoke<Snapshot>("restore_autosave", { autosavePath }),
    discardAutosave: (autosavePath) => invoke("discard_autosave", { autosavePath }),
    autosaveNow: () => invoke<string | null>("autosave_now"),
    getSettings: async () => withDefaults(await invoke<Partial<Settings>>("get_settings")),
    setSettings: (settings) => invoke<AudioStatus>("set_settings", { settings }),
    outputDevices: () => invoke<OutputDevice[]>("output_devices"),
    testTone: () => invoke("test_tone"),
    onMidiStatus: (cb) => subscribe<MidiStatus>("midi-status", cb),
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
    color: null,
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
    clips: [{ id: uuid(), patternId: pattern.id, startBeat: 0, lengthBeats: 16, offsetBeats: 0, muted: false }],
    sections: [],
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
        color: null,
      };
      m.tracks.push(t);
      break;
    }
    case "setTrackColor": {
      const t = track(c.id);
      if (t) t.color = c.color;
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
        m.clips = m.clips.filter((cl) => cl.patternId !== c.id);
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
    case "addClip": {
      const p = pattern(c.patternId);
      if (!p || c.startBeat < 0) break;
      m.clips.push({ id: uuid(), patternId: p.id, startBeat: c.startBeat, lengthBeats: c.lengthBeats ?? p.lengthBeats, offsetBeats: 0, muted: false });
      m.clips.sort((a, b) => a.startBeat - b.startBeat);
      break;
    }
    case "updateClip": {
      const i = m.clips.findIndex((cl) => cl.id === c.clip.id);
      if (i < 0 || c.clip.lengthBeats <= 0 || c.clip.startBeat < 0) break;
      m.clips[i] = { ...c.clip };
      m.clips.sort((a, b) => a.startBeat - b.startBeat);
      break;
    }
    case "removeClip":
      m.clips = m.clips.filter((cl) => cl.id !== c.id);
      break;
    case "addSection":
      m.sections.push({ id: uuid(), name: c.name.trim() || `section ${m.sections.length + 1}`, startBeat: Math.max(0, c.startBeat), key: null, color: null });
      m.sections.sort((a, b) => a.startBeat - b.startBeat);
      break;
    case "updateSection": {
      const i = m.sections.findIndex((x) => x.id === c.section.id);
      if (i >= 0 && c.section.name.trim()) m.sections[i] = { ...c.section };
      m.sections.sort((a, b) => a.startBeat - b.startBeat);
      break;
    }
    case "removeSection":
      m.sections = m.sections.filter((x) => x.id !== c.id);
      break;
    case "setMaster":
      m.master = { ...c.master } as MasterSettings;
      break;
    case "setTimeSignature":
      m.timeSignature = { numerator: c.numerator, denominator: c.denominator };
      break;
    case "setSwing":
      m.swing = Math.min(75, Math.max(50, c.swing));
      m.swingGrid = c.grid === 0.25 ? 0.25 : 0.5;
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
  let loop: [number, number] | null = null;
  let looping = true;
  let armed = false;
  let quantize = 0.25;
  let liveTrack: string | null = null;
  const held = new Map<number, { start: number; velocity: number }>();
  const listeners = new Set<(e: PlayheadEvent) => void>();
  const docListeners = new Set<(s: Snapshot) => void>();
  const midiListeners = new Set<(p: number) => void>();

  const undoLabels: string[] = [];
  const redoLabels: string[] = [];
  let pendingLabel = "edit";
  const closeListeners = new Set<() => void>();
  const snapshot = (): Snapshot => ({
    model: clone(model),
    canUndo: undo.length > 0 || pending !== null,
    canRedo: redo.length > 0,
    dirty,
    path,
    undoLabel: pending ? pendingLabel : (undoLabels[undoLabels.length - 1] ?? null),
    redoLabel: redoLabels[redoLabels.length - 1] ?? null,
  });
  const defaultLabel = (c: Command) => c.type.replace(/([A-Z])/g, " $1").toLowerCase().trim();
  const recentKey = "dissonant.mock.recent";
  const loadRecent = (): string[] => {
    try {
      return JSON.parse(localStorage.getItem(recentKey) ?? "[]") as string[];
    } catch {
      return [];
    }
  };
  const remember = (p: string) => {
    const list = [p, ...loadRecent().filter((x) => x !== p)].slice(0, 12);
    try {
      localStorage.setItem(recentKey, JSON.stringify(list));
    } catch {
      /* storage unavailable */
    }
  };
  // The browser stands in for the OS close button.
  window.addEventListener("beforeunload", (e) => {
    if (dirty) {
      e.preventDefault();
      for (const cb of closeListeners) cb();
    }
  });

  const loopLength = () => {
    if (mode === "song" && model.clips.length > 0) {
      return model.clips.reduce((max, c) => Math.max(max, c.startBeat + c.lengthBeats), 0);
    }
    const p = model.patterns.find((x) => x.id === patternId) ?? model.patterns[0];
    return p?.lengthBeats ?? 16;
  };

  const tick = () => {
    const now = performance.now();
    if (playing) {
      beat += ((now - last) / 1000) * (model.tempo / 60);
      const len = loopLength();
      const [ls, le] = loop && loop[1] > loop[0] ? [Math.min(loop[0], len), Math.min(loop[1], len)] : [0, len];
      if (looping && beat >= le && le > ls) beat = ls + ((beat - le) % (le - ls));
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
    if (pending && JSON.stringify(pending) !== JSON.stringify(model)) {
      undo.push(pending);
      undoLabels.push(pendingLabel);
    }
    pending = null;
  };

  return {
    isTauri: false,
    getState: async () => snapshot(),
    apply: async (command, transient = false, label) => {
      const before = clone(model);
      const next = clone(model);
      reduce(next, command);
      if (JSON.stringify(next) === JSON.stringify(before)) return snapshot();
      const name = label?.trim() || defaultLabel(command);
      if (transient) {
        if (!pending) {
          pending = before;
          pendingLabel = name;
        }
      } else {
        commitGesture();
        undo.push(before);
        undoLabels.push(name);
      }
      redo.length = 0;
      redoLabels.length = 0;
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
        redoLabels.push(undoLabels.pop() ?? "edit");
        model = prev;
        dirty = true;
      }
      return snapshot();
    },
    redo: async () => {
      const next = redo.pop();
      if (next) {
        undo.push(model);
        undoLabels.push(redoLabels.pop() ?? "edit");
        model = next;
        dirty = true;
      }
      return snapshot();
    },
    newProject: async () => {
      model = starterModel();
      undo.length = 0;
      redo.length = 0;
      undoLabels.length = 0;
      redoLabels.length = 0;
      dirty = false;
      path = null;
      beat = 0;
      return snapshot();
    },
    openProject: async (p) => {
      path = p;
      remember(p);
      return snapshot();
    },
    saveProject: async (p) => {
      if (p) path = p;
      dirty = false;
      if (path) remember(path);
      try {
        localStorage.setItem("dissonant.mock.project", JSON.stringify(model));
      } catch {
        /* storage unavailable */
      }
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
    setMetronome: async () => {},
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
    exportWav: async () => {
      throw new Error("export needs the desktop app — the browser mock renders nothing");
    },
    onExportProgress: () => () => {},
    exportMidi: async () => {
      throw new Error("MIDI export needs the desktop app");
    },
    importMidi: async () => {
      throw new Error("MIDI import needs the desktop app");
    },
    listTemplates: async () => {
      try {
        return Object.keys(JSON.parse(localStorage.getItem("dissonant.mock.templates") ?? "{}")).map((name) => ({ name, path: name }));
      } catch {
        return [];
      }
    },
    saveTemplate: async (name) => {
      const all = JSON.parse(localStorage.getItem("dissonant.mock.templates") ?? "{}") as Record<string, ProjectModel>;
      all[name] = clone(model);
      localStorage.setItem("dissonant.mock.templates", JSON.stringify(all));
      return { name, path: name };
    },
    deleteTemplate: async (name) => {
      const all = JSON.parse(localStorage.getItem("dissonant.mock.templates") ?? "{}") as Record<string, ProjectModel>;
      delete all[name];
      localStorage.setItem("dissonant.mock.templates", JSON.stringify(all));
    },
    newFromTemplate: async (name) => {
      const all = JSON.parse(localStorage.getItem("dissonant.mock.templates") ?? "{}") as Record<string, ProjectModel>;
      if (all[name]) {
        model = clone(all[name]);
        undo.length = 0;
        redo.length = 0;
        undoLabels.length = 0;
        redoLabels.length = 0;
        dirty = false;
        path = null;
        beat = 0;
      }
      return snapshot();
    },
    onPlayhead: (cb) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    onDocument: (cb) => {
      docListeners.add(cb);
      return () => docListeners.delete(cb);
    },
    onMidiActivity: (cb) => {
      midiListeners.add(cb);
      return () => midiListeners.delete(cb);
    },
    midiStatus: async () => ({ inputs: [], open: null, armed }),
    openMidiInput: async () => ({ inputs: [], open: null, armed }),
    closeMidiInput: async () => ({ inputs: [], open: null, armed }),
    setLiveTrack: async (id) => {
      liveTrack = id;
    },
    setRecord: async (a, q) => {
      armed = a;
      if (q !== undefined) quantize = q;
      if (!a) held.clear();
      return { inputs: [], open: null, armed };
    },
    noteOn: async (pitch, velocity = 100) => {
      blip(pitch, velocity);
      for (const cb of midiListeners) cb(pitch);
      if (armed && playing) held.set(pitch, { start: beat, velocity });
    },
    noteOff: async (pitch) => {
      const h = held.get(pitch);
      held.delete(pitch);
      if (!h) return;
      const track = liveTrack ?? model.tracks[0]?.id;
      const p = model.patterns.find((x) => x.id === patternId) ?? model.patterns[0];
      if (!track || !p) return;
      const grid = quantize > 0 ? quantize : 1 / 32;
      let len = beat - h.start;
      if (len < 0) len += p.lengthBeats;
      const start = Math.round((h.start % p.lengthBeats) / grid) * grid;
      const notes = (p.notesByTrack[track] ?? []).filter((n) => !(n.pitch === pitch && Math.abs(n.startBeat - start) < grid / 2));
      notes.push({ id: uuid(), startBeat: Math.min(start, p.lengthBeats - grid), lengthBeats: Math.max(grid, Math.round(len / grid) * grid), pitch, velocity: h.velocity });
      undo.push(clone(model));
      redo.length = 0;
      reduce(model, { type: "setNotes", patternId: p.id, trackId: track, notes });
      dirty = true;
      for (const cb of docListeners) cb(snapshot());
    },
    setLoop: async (s, e) => {
      loop = [s, e];
    },
    clearLoop: async () => {
      loop = null;
    },
    setLooping: async (on) => {
      looping = on;
    },
    pickOpenPath: async () => null,
    pickSavePath: async (name, ext) => (name.endsWith(`.${ext}`) ? name : `${name}.${ext}`),
    setTitle: async (title) => {
      document.title = title;
    },
    quit: async () => {
      dirty = false;
      window.close();
    },
    onCloseRequested: (cb) => {
      closeListeners.add(cb);
      return () => closeListeners.delete(cb);
    },
    onFileDropped: () => () => {},
    onAudioStatus: () => () => {},
    recentFiles: async () => loadRecent(),
    clearRecent: async () => {
      try {
        localStorage.removeItem(recentKey);
      } catch {
        /* storage unavailable */
      }
    },
    recoveryCandidates: async () => [],
    restoreAutosave: async () => snapshot(),
    discardAutosave: async () => {},
    autosaveNow: async () => null,
    getSettings: async () => {
      try {
        return withDefaults(JSON.parse(localStorage.getItem("dissonant.mock.settings") ?? "null"));
      } catch {
        return withDefaults(null);
      }
    },
    setSettings: async (settings) => {
      try {
        localStorage.setItem("dissonant.mock.settings", JSON.stringify(settings));
      } catch {
        /* storage unavailable */
      }
      return { running: false, sampleRate: null, error: "browser mock: no engine" };
    },
    outputDevices: async () => [
      { name: "System default", isDefault: true, sampleRates: [44100, 48000, 96000], defaultSampleRate: 48000 },
      { name: "Mock interface", isDefault: false, sampleRates: [44100, 48000, 88200, 96000, 192000], defaultSampleRate: 44100 },
    ],
    testTone: async () => blip(69, 100),
    onMidiStatus: () => () => {},
  };
}

let bridgePromise: Promise<Bridge> | null = null;

export function getBridge(): Promise<Bridge> {
  bridgePromise ??= isTauri ? tauriBridge() : Promise.resolve(mockBridge());
  return bridgePromise;
}
