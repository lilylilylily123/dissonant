import { create } from "zustand";
import { getBridge } from "./bridge";
import type { AudioStatus, Command, MidiStatus, PlayMode, Snapshot, SongPattern, Track } from "./types";
import { beatsPerBar as bpbOf } from "./types";

export interface Toast {
  text: string;
  error?: boolean;
}

export type BottomPanel = "devices" | "mixer";
export type LiveKeyboard = "off" | "tier" | "chromatic";

interface State {
  snapshot: Snapshot | null;
  mode: PlayMode;
  selectedTrackId: string | null;
  selectedPatternId: string | null;
  selectedNoteIds: string[];
  noteLength: number;
  zoom: number;
  arrZoom: number; // px per bar in the arrangement
  showLandscape: boolean;
  highlightRows: boolean;
  hearChords: boolean;
  bottomPanel: BottomPanel;
  playhead: number;
  playing: boolean;
  masterPeak: [number, number];
  trackPeaks: number[];
  audio: AudioStatus | null;
  toast: Toast | null;
  looping: boolean;
  loopRegion: [number, number] | null;
  armed: boolean;
  midi: MidiStatus | null;
  midiActivityAt: number;
  liveKeyboard: LiveKeyboard;
  magnet: boolean;

  init(): Promise<void>;
  dispatch(command: Command, transient?: boolean): Promise<void>;
  commitGesture(): Promise<void>;
  undo(): Promise<void>;
  redo(): Promise<void>;
  setMode(mode: PlayMode): void;
  selectTrack(id: string): void;
  selectPattern(id: string): void;
  setSelectedNoteIds(ids: string[]): void;
  setNoteLength(len: number): void;
  setZoom(zoom: number): void;
  setArrZoom(px: number): void;
  toggleLandscape(): void;
  toggleHighlight(): void;
  toggleHearChords(): void;
  setBottomPanel(p: BottomPanel): void;
  play(): void;
  stop(): void;
  togglePlay(): void;
  rewind(): void;
  seek(beat: number): void;
  tapTempo(): void;
  audition(pitch: number, velocity?: number): void;
  setLoopRegion(region: [number, number] | null): void;
  toggleLooping(): void;
  toggleRecord(): void;
  openMidi(name?: string): Promise<void>;
  closeMidi(): Promise<void>;
  refreshMidi(): Promise<void>;
  setLiveKeyboard(mode: LiveKeyboard): void;
  noteOn(pitch: number, velocity?: number): void;
  noteOff(pitch: number): void;
  toggleMagnet(): void;
  newProject(): Promise<void>;
  openProject(): Promise<void>;
  saveProject(saveAs?: boolean): Promise<void>;
  exportWav(): Promise<void>;
  restartAudio(): Promise<void>;
  showToast(text: string, error?: boolean): void;
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;
let taps: number[] = [];

export const useStore = create<State>((set, get) => {
  const syncPlayback = async () => {
    const b = await getBridge();
    const { mode, selectedPatternId } = get();
    await b.setPlaybackContext(mode, selectedPatternId);
  };

  const applySnapshot = (snapshot: Snapshot) => {
    const { selectedTrackId, selectedPatternId } = get();
    const tracks = snapshot.model.tracks;
    const patterns = snapshot.model.patterns;
    const trackOk = tracks.some((t) => t.id === selectedTrackId);
    const patternOk = patterns.some((p) => p.id === selectedPatternId);
    set({
      snapshot,
      selectedTrackId: trackOk ? selectedTrackId : (tracks[0]?.id ?? null),
      selectedPatternId: patternOk ? selectedPatternId : (patterns[0]?.id ?? null),
    });
    if (!patternOk) void syncPlayback();
  };

  return {
    snapshot: null,
    mode: "pattern",
    selectedTrackId: null,
    selectedPatternId: null,
    selectedNoteIds: [],
    noteLength: 0.25,
    zoom: 1,
    arrZoom: 30,
    showLandscape: false,
    highlightRows: true,
    hearChords: false,
    bottomPanel: "devices",
    playhead: 0,
    playing: false,
    masterPeak: [0, 0],
    trackPeaks: [],
    audio: null,
    toast: null,
    looping: true,
    loopRegion: null,
    armed: false,
    midi: null,
    midiActivityAt: 0,
    liveKeyboard: "off",
    magnet: false,

    async init() {
      const b = await getBridge();
      applySnapshot(await b.getState());
      await syncPlayback();
      await b.setLiveTrack(get().selectedTrackId);
      b.onPlayhead((e) => set({ playhead: e.beat, playing: e.playing, masterPeak: e.masterPeak, trackPeaks: e.trackPeaks }));
      b.onDocument((snap) => applySnapshot(snap));
      b.onMidiActivity(() => set({ midiActivityAt: performance.now() }));
      const audio = await b.audioStatus();
      set({ audio });
      if (!audio.running && b.isTauri) get().showToast(`audio: ${audio.error ?? "not running"}`, true);
      void get().refreshMidi();
    },

    async dispatch(command, transient = false) {
      const b = await getBridge();
      try {
        applySnapshot(await b.apply(command, transient));
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async commitGesture() {
      const b = await getBridge();
      applySnapshot(await b.commitGesture());
    },

    async undo() {
      const b = await getBridge();
      applySnapshot(await b.undo());
    },

    async redo() {
      const b = await getBridge();
      applySnapshot(await b.redo());
    },

    setMode(mode) {
      set({ mode });
      void syncPlayback();
    },
    selectTrack(id) {
      set({ selectedTrackId: id, selectedNoteIds: [] });
      void getBridge().then((b) => b.setLiveTrack(id));
    },
    selectPattern(id) {
      if (id !== get().selectedPatternId) get().setLoopRegion(null);
      set({ selectedPatternId: id, selectedNoteIds: [] });
      void syncPlayback();
    },
    setSelectedNoteIds(ids) {
      set({ selectedNoteIds: ids });
    },
    setNoteLength(len) {
      set({ noteLength: len });
      if (get().armed) void getBridge().then((b) => b.setRecord(true, len));
    },
    setZoom(zoom) {
      set({ zoom: Math.min(3, Math.max(0.25, zoom)) });
    },
    setArrZoom(px) {
      set({ arrZoom: Math.min(160, Math.max(12, px)) });
    },
    toggleLandscape() {
      set((s) => ({ showLandscape: !s.showLandscape }));
    },
    toggleHighlight() {
      set((s) => ({ highlightRows: !s.highlightRows }));
    },
    toggleHearChords() {
      const on = !get().hearChords;
      set({ hearChords: on });
      void getBridge().then((b) => b.setHearChords(on));
    },
    setBottomPanel(p) {
      set({ bottomPanel: p });
    },

    play() {
      void getBridge().then((b) => b.play());
    },
    stop() {
      void getBridge().then((b) => b.stop());
    },
    togglePlay() {
      if (get().playing) get().stop();
      else get().play();
    },
    rewind() {
      void getBridge().then((b) => b.seek(0));
    },
    seek(beat) {
      void getBridge().then((b) => b.seek(beat));
    },

    tapTempo() {
      const now = performance.now();
      taps = taps.filter((t) => now - t < 3000);
      taps.push(now);
      if (taps.length >= 2) {
        const intervals = taps.slice(1).map((t, i) => t - taps[i]);
        const avg = intervals.reduce((a, b) => a + b, 0) / intervals.length;
        const bpm = Math.round(60000 / avg);
        if (bpm >= 20 && bpm <= 300) void get().dispatch({ type: "setTempo", bpm });
      }
    },

    audition(pitch, velocity = 100) {
      const id = get().selectedTrackId;
      if (!id) return;
      void getBridge().then((b) => b.audition(id, pitch, velocity));
    },

    setLoopRegion(region) {
      set({ loopRegion: region });
      void getBridge().then((b) => (region ? b.setLoop(region[0], region[1]) : b.clearLoop()));
    },
    toggleLooping() {
      const on = !get().looping;
      set({ looping: on });
      void getBridge().then((b) => b.setLooping(on));
    },
    toggleRecord() {
      const armed = !get().armed;
      set({ armed });
      void getBridge().then(async (b) => set({ midi: await b.setRecord(armed, get().noteLength) }));
    },
    async openMidi(name) {
      const b = await getBridge();
      try {
        set({ midi: await b.openMidiInput(name) });
        get().showToast(`MIDI in: ${get().midi?.open ?? "—"}`);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async closeMidi() {
      const b = await getBridge();
      set({ midi: await b.closeMidiInput() });
    },
    async refreshMidi() {
      const b = await getBridge();
      try {
        set({ midi: await b.midiStatus() });
      } catch {
        /* no midi backend */
      }
    },
    setLiveKeyboard(mode) {
      set({ liveKeyboard: mode });
    },
    noteOn(pitch, velocity = 100) {
      void getBridge().then((b) => b.noteOn(pitch, velocity));
    },
    noteOff(pitch) {
      void getBridge().then((b) => b.noteOff(pitch));
    },
    toggleMagnet() {
      set((s) => ({ magnet: !s.magnet }));
    },

    async newProject() {
      const b = await getBridge();
      if (get().snapshot?.dirty && !confirm("Discard unsaved changes?")) return;
      applySnapshot(await b.newProject(true));
      await syncPlayback();
    },

    async openProject() {
      const b = await getBridge();
      const path = await b.pickOpenPath();
      if (!path) return;
      try {
        applySnapshot(await b.openProject(path));
        await syncPlayback();
        get().showToast(`opened ${path}`);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async saveProject(saveAs = false) {
      const b = await getBridge();
      let path: string | undefined;
      if (saveAs || !get().snapshot?.path) {
        const picked = await b.pickSavePath("untitled.dissonant", "dissonant");
        if (!picked) return;
        path = picked;
      }
      try {
        applySnapshot(await b.saveProject(path));
        get().showToast("saved");
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async exportWav() {
      const b = await getBridge();
      const path = await b.pickSavePath("song.wav", "wav");
      if (!path) return;
      get().showToast("rendering…");
      try {
        const out = await b.exportWav(path, 1.5);
        get().showToast(`exported ${out}`);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async restartAudio() {
      const b = await getBridge();
      const audio = await b.restartAudio();
      set({ audio });
      get().showToast(audio.running ? `audio running @ ${audio.sampleRate} Hz` : `audio: ${audio.error}`, !audio.running);
    },

    showToast(text, error = false) {
      set({ toast: { text, error } });
      if (toastTimer) clearTimeout(toastTimer);
      toastTimer = setTimeout(() => set({ toast: null }), error ? 6000 : 2500);
    },
  };
});

// ─── Selectors / helpers ───────────────────────────────────────────────────────────────────

export function selectedPattern(s: State): SongPattern | null {
  const m = s.snapshot?.model;
  if (!m) return null;
  return m.patterns.find((p) => p.id === s.selectedPatternId) ?? m.patterns[0] ?? null;
}

export function selectedTrack(s: State): Track | null {
  const m = s.snapshot?.model;
  if (!m) return null;
  return m.tracks.find((t) => t.id === s.selectedTrackId) ?? m.tracks[0] ?? null;
}

export function selectedTrackIndex(s: State): number {
  const m = s.snapshot?.model;
  if (!m) return 0;
  const i = m.tracks.findIndex((t) => t.id === s.selectedTrackId);
  return i < 0 ? 0 : i;
}

export const GRID_OPTIONS: [string, number][] = [
  ["1/32", 0.125],
  ["1/16", 0.25],
  ["1/8", 0.5],
  ["1/4", 1],
  ["1/2", 2],
  ["1", 4],
];

export function gridLabel(len: number): string {
  return GRID_OPTIONS.find(([, v]) => v === len)?.[0] ?? `${len}`;
}

export function dbText(gain: number): string {
  if (gain <= 0.0005) return "-inf";
  const db = 20 * Math.log10(gain);
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
}

export function peakDb(peak: number): number {
  return peak <= 0.0001 ? -60 : 20 * Math.log10(peak);
}

/** 0..1 meter position for a peak, on a -48 dB … 0 dB scale. */
export function meterPos(peak: number): number {
  return Math.min(1, Math.max(0, (peakDb(peak) + 48) / 48));
}

export function beatsPerBar(s: State): number {
  return bpbOf(s.snapshot?.model.timeSignature);
}
