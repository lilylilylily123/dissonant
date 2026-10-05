import { create } from "zustand";
import { getBridge } from "./bridge";
import type { AudioStatus, Command, PlayMode, Snapshot, SongPattern, Track } from "./types";

export interface Toast {
  text: string;
  error?: boolean;
}

export type BottomPanel = "devices" | "mixer";

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
  /** Preview a pitch. `beats` defaults to the current grid, so a preview never outlasts the note. */
  audition(pitch: number, velocity?: number, beats?: number): void;
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

    async init() {
      const b = await getBridge();
      applySnapshot(await b.getState());
      await syncPlayback();
      b.onPlayhead((e) => set({ playhead: e.beat, playing: e.playing, masterPeak: e.masterPeak, trackPeaks: e.trackPeaks }));
      const audio = await b.audioStatus();
      set({ audio });
      if (!audio.running && b.isTauri) get().showToast(`audio: ${audio.error ?? "not running"}`, true);
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
    },
    selectPattern(id) {
      set({ selectedPatternId: id, selectedNoteIds: [] });
      void syncPlayback();
    },
    setSelectedNoteIds(ids) {
      set({ selectedNoteIds: ids });
    },
    setNoteLength(len) {
      set({ noteLength: len });
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

    audition(pitch, velocity = 100, beats) {
      const { selectedTrackId: id, noteLength, snapshot } = get();
      if (!id) return;
      const bpm = snapshot?.model.tempo ?? 120;
      const seconds = ((beats ?? noteLength) * 60) / bpm;
      void getBridge().then((b) => b.audition(id, pitch, velocity, seconds));
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
