import { create } from "zustand";
import { getBridge } from "./bridge";
import type { AudioStatus, Command, PlayMode, Snapshot, SongPattern, Track } from "./types";

export interface Toast {
  text: string;
  error?: boolean;
}

interface State {
  snapshot: Snapshot | null;
  mode: PlayMode;
  selectedTrackId: string | null;
  selectedPatternId: string | null;
  noteLength: number;
  zoom: number;
  showLandscape: boolean;
  hearChords: boolean;
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
  setNoteLength(len: number): void;
  setZoom(zoom: number): void;
  toggleLandscape(): void;
  toggleHearChords(): void;
  play(): void;
  stop(): void;
  togglePlay(): void;
  rewind(): void;
  seek(beat: number): void;
  audition(pitch: number, velocity?: number): void;
  newProject(): Promise<void>;
  openProject(): Promise<void>;
  saveProject(saveAs?: boolean): Promise<void>;
  exportWav(): Promise<void>;
  restartAudio(): Promise<void>;
  showToast(text: string, error?: boolean): void;
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;

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
    noteLength: 1,
    zoom: 1,
    showLandscape: false,
    hearChords: false,
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
      set({ selectedTrackId: id });
    },

    selectPattern(id) {
      set({ selectedPatternId: id });
      void syncPlayback();
    },

    setNoteLength(len) {
      set({ noteLength: len });
    },

    setZoom(zoom) {
      set({ zoom: Math.min(4, Math.max(0.35, zoom)) });
    },

    toggleLandscape() {
      set((s) => ({ showLandscape: !s.showLandscape }));
    },

    toggleHearChords() {
      const on = !get().hearChords;
      set({ hearChords: on });
      void getBridge().then((b) => b.setHearChords(on));
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

    audition(pitch, velocity = 100) {
      const id = get().selectedTrackId;
      if (!id) return;
      void getBridge().then((b) => b.audition(id, pitch, velocity));
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

// ─── Selectors ─────────────────────────────────────────────────────────────────────────────

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

export const LENGTH_OPTIONS: [string, number][] = [
  ["1/16", 0.25],
  ["1/8", 0.5],
  ["1/4", 1],
  ["1/2", 2],
  ["1", 4],
];
