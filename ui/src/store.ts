import { create } from "zustand";
import { getBridge } from "./bridge";
import type { AudioStatus, Command, KeyState, MidiStatus, PlayMode, RecoveryCandidate, Snapshot, SongPattern, Track } from "./types";
import { beatsPerBar as bpbOf, fileNameOf, keyAt } from "./types";

export interface Toast {
  text: string;
  error?: boolean;
}

/** One button of an in-app dialog. `value` is what `ask` resolves with. */
export interface DialogButton {
  label: string;
  value: string;
  kind?: "primary" | "danger" | "quiet";
}

export interface DialogSpec {
  title: string;
  message?: string;
  /** Optional monospace detail line (a path, a count). */
  detail?: string;
  buttons: DialogButton[];
  /** Resolved with when the dialog is dismissed with Esc / the backdrop. */
  cancelValue?: string;
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
  dialog: DialogSpec | null;
  recent: string[];
  looping: boolean;
  loopRegion: [number, number] | null;
  armed: boolean;
  midi: MidiStatus | null;
  midiActivityAt: number;
  liveKeyboard: LiveKeyboard;
  magnet: boolean;
  stamp: boolean;

  init(): Promise<void>;
  /** `label` names the edit in the Edit menu ("move notes"); defaults to the command's name. */
  dispatch(command: Command, transient?: boolean, label?: string): Promise<void>;
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
  toggleStamp(): void;
  newProject(): Promise<void>;
  openProject(): Promise<void>;
  /** Open a known path (recent file, dropped file), asking about unsaved work first. */
  openPath(path: string): Promise<void>;
  /** Save; resolves true when the document ended up saved (false on cancel / error). */
  saveProject(saveAs?: boolean): Promise<boolean>;
  exportWav(): Promise<void>;
  restartAudio(): Promise<void>;
  showToast(text: string, error?: boolean): void;
  /** Show an in-app dialog and resolve with the chosen button's value. */
  ask(spec: DialogSpec): Promise<string>;
  /** Yes/no convenience over `ask`. */
  confirm(title: string, message?: string, okLabel?: string, danger?: boolean): Promise<boolean>;
  /** Called by a dialog button; resolves the pending `ask`. */
  answerDialog(value: string): void;
  /** Unsaved changes stand in the way of `what`; returns true when it is OK to proceed. */
  confirmDiscard(what: string): Promise<boolean>;
  /** The OS close button: save / discard / cancel, then quit. */
  requestClose(): Promise<void>;
  refreshRecent(): Promise<void>;
  clearRecent(): Promise<void>;
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;
let taps: number[] = [];
let dialogResolve: ((value: string) => void) | null = null;
let lastTitle = "";

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
    // OS window title: "song.dissonant • — dissonant"
    const title = `${fileNameOf(snapshot.path)}${snapshot.dirty ? " •" : ""} — dissonant`;
    if (title !== lastTitle) {
      lastTitle = title;
      void getBridge().then((b) => b.setTitle(title));
    }
  };

  /** Offer to restore autosaves left by an earlier run, newest first. */
  const offerRecovery = async (candidates: RecoveryCandidate[]) => {
    const b = await getBridge();
    for (const c of candidates) {
      const when = new Date(c.savedAtMs).toLocaleString();
      const answer = await get().ask({
        title: "Recover unsaved work?",
        message: `An autosave of ${c.projectPath ? `“${fileNameOf(c.projectPath)}”` : "an untitled project"} from ${when} was found. It was never saved.`,
        detail: c.projectPath ?? undefined,
        buttons: [
          { label: "Restore", value: "restore", kind: "primary" },
          { label: "Delete autosave", value: "discard", kind: "danger" },
          { label: "Later", value: "later", kind: "quiet" },
        ],
        cancelValue: "later",
      });
      if (answer === "restore") {
        try {
          applySnapshot(await b.restoreAutosave(c.autosavePath));
          await syncPlayback();
          get().showToast("restored from autosave — save to keep it");
        } catch (e) {
          get().showToast(String(e), true);
        }
        return; // one document at a time; the rest stay on disk for next launch
      }
      if (answer === "discard") await b.discardAutosave(c.autosavePath);
    }
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
    dialog: null,
    recent: [],
    looping: true,
    loopRegion: null,
    armed: false,
    midi: null,
    midiActivityAt: 0,
    liveKeyboard: "off",
    magnet: false,
    stamp: false,

    async init() {
      const b = await getBridge();
      applySnapshot(await b.getState());
      await syncPlayback();
      await b.setLiveTrack(get().selectedTrackId);
      b.onPlayhead((e) => set({ playhead: e.beat, playing: e.playing, masterPeak: e.masterPeak, trackPeaks: e.trackPeaks }));
      b.onDocument((snap) => applySnapshot(snap));
      b.onMidiActivity(() => set({ midiActivityAt: performance.now() }));
      b.onCloseRequested(() => void get().requestClose());
      b.onFileDropped((path) => void get().openPath(path));
      b.onAudioStatus((e) => {
        set({ audio: e.status });
        get().showToast(e.message, !e.status.running);
      });
      const audio = await b.audioStatus();
      set({ audio });
      if (!audio.running && b.isTauri) get().showToast(`audio: ${audio.error ?? "not running"}`, true);
      void get().refreshMidi();
      void get().refreshRecent();
      try {
        const candidates = await b.recoveryCandidates();
        if (candidates.length) void offerRecovery(candidates);
      } catch {
        /* no recovery backend */
      }
    },

    async dispatch(command, transient = false, label) {
      const b = await getBridge();
      try {
        applySnapshot(await b.apply(command, transient, label));
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
    toggleStamp() {
      set((s) => ({ stamp: !s.stamp }));
    },

    async newProject() {
      const b = await getBridge();
      if (!(await get().confirmDiscard("creating a new project"))) return;
      applySnapshot(await b.newProject(true));
      await syncPlayback();
    },

    async openProject() {
      const b = await getBridge();
      if (!(await get().confirmDiscard("opening another project"))) return;
      const path = await b.pickOpenPath();
      if (!path) return;
      try {
        applySnapshot(await b.openProject(path));
        await syncPlayback();
        get().showToast(`opened ${fileNameOf(path)}`);
        void get().refreshRecent();
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async openPath(path) {
      const b = await getBridge();
      if (path === get().snapshot?.path && !get().snapshot?.dirty) return;
      if (!(await get().confirmDiscard(`opening “${fileNameOf(path)}”`))) return;
      try {
        applySnapshot(await b.openProject(path));
        await syncPlayback();
        get().showToast(`opened ${fileNameOf(path)}`);
      } catch (e) {
        get().showToast(String(e), true);
      }
      void get().refreshRecent();
    },

    async saveProject(saveAs = false) {
      const b = await getBridge();
      let path: string | undefined;
      if (saveAs || !get().snapshot?.path) {
        const picked = await b.pickSavePath(fileNameOf(get().snapshot?.path), "dissonant");
        if (!picked) return false;
        path = picked;
      }
      try {
        applySnapshot(await b.saveProject(path));
        get().showToast("saved");
        void get().refreshRecent();
        return true;
      } catch (e) {
        get().showToast(String(e), true);
        return false;
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

    ask(spec) {
      // One dialog at a time: a new one cancels the previous.
      dialogResolve?.(spec.cancelValue ?? "cancel");
      return new Promise<string>((resolve) => {
        dialogResolve = resolve;
        set({ dialog: spec });
      });
    },
    answerDialog(value) {
      const resolve = dialogResolve;
      dialogResolve = null;
      set({ dialog: null });
      resolve?.(value);
    },
    async confirm(title, message, okLabel = "OK", danger = false) {
      const v = await get().ask({
        title,
        message,
        buttons: [
          { label: okLabel, value: "ok", kind: danger ? "danger" : "primary" },
          { label: "Cancel", value: "cancel", kind: "quiet" },
        ],
        cancelValue: "cancel",
      });
      return v === "ok";
    },
    async confirmDiscard(what) {
      const snap = get().snapshot;
      if (!snap?.dirty) return true;
      const v = await get().ask({
        title: "Unsaved changes",
        message: `“${fileNameOf(snap.path)}” has changes that will be lost by ${what}.`,
        buttons: [
          { label: "Save", value: "save", kind: "primary" },
          { label: "Discard", value: "discard", kind: "danger" },
          { label: "Cancel", value: "cancel", kind: "quiet" },
        ],
        cancelValue: "cancel",
      });
      if (v === "save") return get().saveProject(false);
      return v === "discard";
    },
    async requestClose() {
      const b = await getBridge();
      const snap = get().snapshot;
      if (!snap?.dirty) return b.quit();
      const v = await get().ask({
        title: "Save before closing?",
        message: `“${fileNameOf(snap.path)}” has unsaved changes.`,
        buttons: [
          { label: "Save", value: "save", kind: "primary" },
          { label: "Don’t save", value: "discard", kind: "danger" },
          { label: "Cancel", value: "cancel", kind: "quiet" },
        ],
        cancelValue: "cancel",
      });
      if (v === "cancel") return;
      if (v === "save" && !(await get().saveProject(false))) return;
      await b.quit();
    },
    async refreshRecent() {
      const b = await getBridge();
      try {
        set({ recent: await b.recentFiles() });
      } catch {
        /* no backend */
      }
    },
    async clearRecent() {
      const b = await getBridge();
      await b.clearRecent();
      set({ recent: [] });
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

/**
 * The key the selected pattern is tiered against: if the pattern sits in a section with
 * its own key (its first clip), that key; otherwise the project key.
 */
export function effectiveKey(s: State): KeyState {
  const m = s.snapshot?.model;
  if (!m) return { rootPitchClass: null, scale: "major", isLocked: false };
  const p = selectedPattern(s);
  const clip = p ? m.clips.find((c) => c.patternId === p.id) : undefined;
  return clip ? keyAt(m, clip.startBeat) : m.key;
}
