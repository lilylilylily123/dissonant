import { create } from "zustand";
import { getBridge } from "./bridge";
import type {
  AmpRig,
  AudioImportProgress,
  AudioStatus,
  CatalogEntry,
  Command,
  ExportProgress,
  ExportRequest,
  InputDevice,
  InsertTarget,
  InstallProgress,
  KeyState,
  MidiStatus,
  NoteEvent,
  NoteProp,
  OutputDevice,
  ParamInfo,
  PlayMode,
  PluginDescriptor,
  RecoveryCandidate,
  Settings,
  Snapshot,
  SongPattern,
  TemplateInfo,
  Track,
} from "./types";
import {
  barTicks as barTicksOf,
  beatsPerBar as bpbOf,
  DEFAULT_SETTINGS,
  fileNameOf,
  insertChain,
  keyAt,
  TIER_COLORS,
  ticksToBeats,
} from "./types";

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
  /** A text field; the typed value is read back through `dialogText`. */
  input?: { placeholder?: string; value?: string };
}

export type BottomPanel = "devices" | "mixer";
/**
 * Which top-level screen is showing. Separate from `mode`: `PlayMode` is a Rust enum that
 * decides what the engine sequences, and the guitar screen plays the song like any other
 * view — a third variant there would be a lie.
 */
export type Screen = "pattern" | "song" | "guitar";
export type LiveKeyboard = "off" | "tier" | "chromatic";

interface State {
  snapshot: Snapshot | null;
  mode: PlayMode;
  selectedTrackId: string | null;
  selectedPatternId: string | null;
  selectedNoteIds: string[];
  /** Grid / default note length, in ticks. */
  noteLength: number;
  zoom: number;
  arrZoom: number; // px per bar in the arrangement
  showLandscape: boolean;
  highlightRows: boolean;
  hearChords: boolean;
  bottomPanel: BottomPanel;
  /** Transport position in ticks (fractional between blocks). */
  playhead: number;
  playing: boolean;
  /** Beats of count-in left (0 = none). */
  countIn: number;
  masterPeak: [number, number];
  trackPeaks: number[];
  audio: AudioStatus | null;
  toast: Toast | null;
  dialog: DialogSpec | null;
  recent: string[];
  settings: Settings;
  settingsOpen: boolean;
  outputDevices: OutputDevice[];
  dialogText: string;
  exportOpen: boolean;
  exporting: ExportProgress | null;
  templates: TemplateInfo[];
  templatesOpen: boolean;
  looping: boolean;
  /** Loop start/end in ticks. */
  loopRegion: [number, number] | null;
  armed: boolean;
  midi: MidiStatus | null;
  midiActivityAt: number;
  liveKeyboard: LiveKeyboard;
  magnet: boolean;
  stamp: boolean;
  /** Brush: left-drag on empty space paints notes. */
  brush: boolean;
  /** Keep the playhead in view while playing. */
  follow: boolean;
  /** Fold: only rows with notes (or chord tones nearby) are shown. */
  fold: boolean;
  /** Another track whose notes are drawn as ghosts behind the roll. */
  ghostTrackId: string | null;
  /** Which per-note property the lane under the roll (and the drum graph editor) edits. */
  laneProp: NoteProp;
  /** Something outside the roll (inspector, menu) asked for this selection. */
  selectionRequest: { ids: string[]; nonce: number } | null;
  /** Plugins found on this computer, in scan order; empty until `refreshPlugins`. */
  plugins: PluginDescriptor[];
  /** The curated free-plugin catalog; empty until `refreshCatalog`. */
  catalog: CatalogEntry[];
  /** The latest install event per catalog entry, so a row can show its own progress. */
  installing: Record<string, InstallProgress>;
  /** Which top-level screen is showing. */
  screen: Screen;
  /** Peak of the live input (0 when nothing is captured). */
  inputPeak: number;
  /** True while a take is being captured. */
  recordingAudio: boolean;
  inputDevices: InputDevice[];
  /** The audio track that hears the live input. */
  armedAudioTrackId: string | null;
  /** Hear the input through the rig, with or without the transport running. */
  monitor: boolean;
  selectedAudioClipId: string | null;
  /** Non-null while a file decodes. */
  importing: AudioImportProgress | null;

  init(): Promise<void>;
  /** `label` names the edit in the Edit menu ("move notes"); defaults to the command's name. */
  dispatch(
    command: Command,
    transient?: boolean,
    label?: string,
  ): Promise<void>;
  commitGesture(): Promise<void>;
  undo(): Promise<void>;
  redo(): Promise<void>;
  /** Switch screens. The only writer of `screen` and `mode`. */
  setScreen(screen: Screen): void;
  selectTrack(id: string): void;
  selectPattern(id: string): void;
  setSelectedNoteIds(ids: string[]): void;
  setNoteLength(ticks: number): void;
  setZoom(zoom: number): void;
  setArrZoom(px: number): void;
  toggleLandscape(): void;
  toggleHighlight(): void;
  toggleHearChords(): void;
  setBottomPanel(p: BottomPanel): void;
  play(): void;
  stop(): void;
  toggleMetronome(): void;
  togglePlay(): void;
  rewind(): void;
  seek(tick: number): void;
  tapTempo(): void;
  /** Preview a pitch. `lengthTicks` defaults to the current grid, so a preview never outlasts the note. */
  audition(pitch: number, velocity?: number, lengthTicks?: number): void;
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
  toggleBrush(): void;
  toggleFollow(): void;
  toggleFold(): void;
  setGhostTrack(id: string | null): void;
  setLaneProp(prop: NoteProp): void;
  requestSelection(ids: string[]): void;
  /** Apply a pure edit to the selected track's notes in the selected pattern (ids = current selection). */
  transformSelection(
    fn: (notes: NoteEvent[], ids: Set<string>) => NoteEvent[],
    label: string,
  ): void;
  newProject(): Promise<void>;
  openProject(): Promise<void>;
  /** Open a known path (recent file, dropped file), asking about unsaved work first. */
  openPath(path: string): Promise<void>;
  /** Save; resolves true when the document ended up saved (false on cancel / error). */
  saveProject(saveAs?: boolean): Promise<boolean>;
  /** Open the export dialog (⌘E). */
  exportWav(): Promise<void>;
  openExport(open?: boolean): void;
  /** Pick a file and run the export described by `request` (path is filled in here). */
  runExport(request: Omit<ExportRequest, "path">): Promise<void>;
  exportMidi(): Promise<void>;
  importMidi(): Promise<void>;
  setDialogText(text: string): void;
  /** A dialog with one text field; resolves with the text, or null on cancel. */
  askInput(
    title: string,
    message: string,
    placeholder?: string,
    okLabel?: string,
    initial?: string,
  ): Promise<string | null>;
  openTemplates(open?: boolean): void;
  refreshTemplates(): Promise<void>;
  saveAsTemplate(): Promise<void>;
  deleteTemplate(name: string): Promise<void>;
  newFromTemplate(name: string | "starter" | "empty"): Promise<void>;
  restartAudio(): Promise<void>;
  showToast(text: string, error?: boolean): void;
  /** Show an in-app dialog and resolve with the chosen button's value. */
  ask(spec: DialogSpec): Promise<string>;
  /** Yes/no convenience over `ask`. */
  confirm(
    title: string,
    message?: string,
    okLabel?: string,
    danger?: boolean,
  ): Promise<boolean>;
  /** Called by a dialog button; resolves the pending `ask`. */
  answerDialog(value: string): void;
  /** Unsaved changes stand in the way of `what`; returns true when it is OK to proceed. */
  confirmDiscard(what: string): Promise<boolean>;
  /** The OS close button: save / discard / cancel, then quit. */
  requestClose(): Promise<void>;
  refreshRecent(): Promise<void>;
  clearRecent(): Promise<void>;
  openSettings(open?: boolean): void;
  /** Merge a partial patch into one settings section, persist, and apply it. */
  updateSettings<K extends keyof Settings>(
    section: K,
    patch: Partial<Settings[K]>,
  ): Promise<void>;
  refreshOutputDevices(): Promise<void>;
  testTone(): void;
  refreshAudio(): Promise<void>;
  /** Plugins found on this computer; refreshed after an install. */
  refreshPlugins(rescan?: boolean): Promise<void>;
  /** The curated free-plugin catalog, loaded when the browser is first shown. */
  refreshCatalog(): Promise<void>;
  /** Put a scanned plugin on a track, or `null` to go back to the built-in voice. */
  setTrackPlugin(trackId: string, uid: string | null): Promise<void>;
  /** A track plugin's parameters with the values it currently holds. */
  pluginParams(trackId: string): Promise<ParamInfo[]>;
  /** Write one plugin parameter: `commit` false while dragging, true on release — only the
   *  committed one becomes an undo step. */
  setPluginParam(
    trackId: string,
    paramId: number,
    value: number,
    commit?: boolean,
  ): Promise<void>;
  /** Install one catalog entry and rescan; progress shows up in `installing`. */
  installPlugin(entryId: string): Promise<void>;
  /** Hand an https link to the user's own browser; anything else is ignored. */
  openUrl(url: string): Promise<void>;
  /** Open (or raise) the separate window holding one track's plugin controls. */
  openPluginWindow(trackId: string): Promise<void>;

  // ── Effect inserts ──────────────────────────────────────────────────────────────────────
  /** Put a scanned effect in a chain, appended unless `index` says otherwise. */
  addInsert(
    target: InsertTarget,
    uid: string,
    index?: number | null,
  ): Promise<void>;
  removeInsert(target: InsertTarget, insertId: string): Promise<void>;
  /** Move an insert within its chain; out-of-range indices land on the nearest end. */
  moveInsert(target: InsertTarget, insertId: string, toIndex: number): Promise<void>;
  setInsertEnabled(
    target: InsertTarget,
    insertId: string,
    on: boolean,
  ): Promise<void>;
  /** One insert's parameters with the values its effect currently holds. */
  insertParams(insertId: string): Promise<ParamInfo[]>;
  /** Write one insert parameter: `commit` false while dragging, true on release — only the
   *  committed one becomes an undo step. */
  setInsertParam(
    insertId: string,
    paramId: number,
    value: number,
    commit?: boolean,
  ): Promise<void>;
  /** The free-plugin browser is an overlay, like settings and export: finding a sound is
   *  something you do from wherever you are, not a reason to change what the window shows. */
  pluginBrowserOpen: boolean;
  openPluginBrowser(open?: boolean): void;

  // ── Guitar ──────────────────────────────────────────────────────────────────────────────
  addAudioTrack(): Promise<void>;
  /** Point the live input at one audio track (null disarms). */
  setArm(trackId: string | null): Promise<void>;
  toggleMonitor(): Promise<void>;
  /** Arm the recorder, then start the transport — so a count-in is never recorded. */
  startRecording(): Promise<void>;
  stopRecording(): Promise<void>;
  /** Pick a file and import it onto `trackId` (or a new audio track). */
  importAudioFile(trackId?: string | null): Promise<void>;
  /** Import a known path (dropped file). */
  importAudioPath(path: string, trackId?: string | null): Promise<void>;
  selectAudioClip(id: string | null): void;
  /** Merge a patch into the selected audio track's rig. `transient` while a knob is dragged. */
  setAmp(
    trackId: string,
    patch: Partial<AmpRig>,
    transient?: boolean,
  ): Promise<void>;
  refreshInputDevices(): Promise<void>;
}

/** Push the appearance section into CSS variables, the live tier palette and the page zoom. */
export function applyAppearance(a: Settings["appearance"]) {
  const root = document.documentElement;
  TIER_COLORS.chordTone = a.tierColors.chordTone;
  TIER_COLORS.tension = a.tierColors.tension;
  TIER_COLORS.dissonance = a.tierColors.dissonance;
  root.style.setProperty("--tier-solid", a.tierColors.chordTone);
  root.style.setProperty("--tier-tension", a.tierColors.tension);
  root.style.setProperty("--tier-dissonance", a.tierColors.dissonance);
  root.style.setProperty("--ok", a.tierColors.chordTone);
  root.style.setProperty("--accent", a.accent);
  const [r, g, b] = [1, 3, 5].map((i) =>
    parseInt(a.accent.slice(i, i + 2), 16),
  );
  root.style.setProperty("--accent-rgb", `${r}, ${g}, ${b}`);
  root.style.setProperty("--accent-bg", `rgba(${r}, ${g}, ${b}, 0.16)`);
  root.style.setProperty("--accent-border", `rgba(${r}, ${g}, ${b}, 0.45)`);
  (root.style as CSSStyleDeclaration & { zoom?: string }).zoom = String(
    a.uiScale,
  );
  root.classList.toggle("reduced-motion", a.reducedMotion);
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

  /** What the user is seeing this insert called, for an undo label. Falls back to the plain
   *  word when the slot has already gone — an undo label is never worth an error. */
  const insertName = (target: InsertTarget, insertId: string) => {
    const m = get().snapshot?.model;
    const found = m && insertChain(m, target).find((i) => i.id === insertId);
    return found ? found.name : "insert";
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
      selectedPatternId: patternOk
        ? selectedPatternId
        : (patterns[0]?.id ?? null),
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
    noteLength: 240,
    zoom: 1,
    arrZoom: 30,
    showLandscape: false,
    highlightRows: true,
    hearChords: false,
    bottomPanel: "devices",
    playhead: 0,
    playing: false,
    countIn: 0,
    masterPeak: [0, 0],
    trackPeaks: [],
    audio: null,
    toast: null,
    dialog: null,
    recent: [],
    settings: DEFAULT_SETTINGS,
    settingsOpen: false,
    pluginBrowserOpen: false,
    outputDevices: [],
    dialogText: "",
    exportOpen: false,
    exporting: null,
    templates: [],
    templatesOpen: false,
    looping: true,
    loopRegion: null,
    armed: false,
    midi: null,
    midiActivityAt: 0,
    liveKeyboard: "off",
    magnet: false,
    stamp: false,
    brush: false,
    follow: true,
    fold: false,
    ghostTrackId: null,
    laneProp: "velocity",
    selectionRequest: null,
    plugins: [],
    catalog: [],
    installing: {},
    screen: "pattern",
    inputPeak: 0,
    recordingAudio: false,
    inputDevices: [],
    armedAudioTrackId: null,
    monitor: false,
    selectedAudioClipId: null,
    importing: null,

    async init() {
      const b = await getBridge();
      try {
        const settings = await b.getSettings();
        set({
          settings,
          noteLength: settings.editing.defaultGrid,
        });
        applyAppearance(settings.appearance);
      } catch (e) {
        console.warn("settings unavailable", e);
      }
      applySnapshot(await b.getState());
      await syncPlayback();
      await b.setLiveTrack(get().selectedTrackId);
      b.onPlayhead((e) =>
        set({
          playhead: e.tick,
          playing: e.playing,
          masterPeak: e.masterPeak,
          trackPeaks: e.trackPeaks,
          countIn: e.countIn ?? 0,
          inputPeak: e.inputPeak ?? 0,
          recordingAudio: e.recording ?? false,
        }),
      );
      b.onDocument((snap) => applySnapshot(snap));
      b.onMidiActivity(() => set({ midiActivityAt: performance.now() }));
      b.onCloseRequested(() => void get().requestClose());
      b.onFileDropped((path) => void get().openPath(path));
      b.onAudioDropped((path) => void get().importAudioPath(path));
      b.onAudioImport((p) => set({ importing: p.fraction >= 1 ? null : p }));
      b.onAudioStatus((e) => {
        set({ audio: e.status });
        get().showToast(e.message, !e.status.running);
      });
      b.onMidiStatus((midi) => {
        const before = get().midi?.open ?? null;
        set({ midi });
        if (midi.open && midi.open !== before)
          get().showToast(`MIDI in: ${midi.open}`);
        else if (!midi.open && before)
          get().showToast(`MIDI in: ${before} disconnected`, true);
      });
      b.onExportProgress((p) =>
        set({ exporting: p.done >= p.total ? null : p }),
      );
      b.onPluginInstall((p) =>
        set((s) => ({ installing: { ...s.installing, [p.entryId]: p } })),
      );
      // Engine load / xruns in the status bar.
      setInterval(() => void get().refreshAudio(), 2000);
      const audio = await b.audioStatus();
      set({ audio });
      if (!audio.running && b.isTauri)
        get().showToast(`audio: ${audio.error ?? "not running"}`, true);
      void get().refreshMidi();
      void get().refreshRecent();
      void get().refreshPlugins();
      void get().refreshInputDevices();
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

    setScreen(screen) {
      // The guitar screen plays the song timeline, so it shares song mode with the arrangement.
      set({ screen, mode: screen === "pattern" ? "pattern" : "song" });
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
    setNoteLength(ticks) {
      set({ noteLength: ticks });
      if (get().armed) void getBridge().then((b) => b.setRecord(true, ticks));
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
      const { armed, settings, playhead, snapshot } = get();
      const m = settings.metronome;
      void getBridge().then(async (b) => {
        if (armed && m.preRollBars > 0) {
          const bar = barTicksOf(snapshot?.model.timeSignature);
          await b.seek(Math.max(0, playhead - m.preRollBars * bar));
        }
        await b.play(armed ? m.countInBars : 0);
      });
    },
    stop() {
      void getBridge().then((b) => b.stop());
    },
    toggleMetronome() {
      const on = !get().settings.metronome.on;
      void get().updateSettings("metronome", { on });
    },
    togglePlay() {
      if (get().playing) get().stop();
      else get().play();
    },
    rewind() {
      void getBridge().then((b) => b.seek(0));
    },
    seek(tick) {
      void getBridge().then((b) => b.seek(tick));
    },

    tapTempo() {
      const now = performance.now();
      // A pause of 3 s starts a new run; only the last 8 taps count.
      taps = taps.filter((t) => now - t < 3000).slice(-7);
      taps.push(now);
      if (taps.length >= 2) {
        const intervals = taps.slice(1).map((t, i) => t - taps[i]);
        const avg = intervals.reduce((a, b) => a + b, 0) / intervals.length;
        const bpm = Math.round((60000 / avg) * 10) / 10;
        if (bpm >= 20 && bpm <= 300) {
          void get().dispatch({ type: "setTempo", bpm }, false, "tap tempo");
          get().showToast(`tap · ${bpm.toFixed(1)} bpm (${taps.length} taps)`);
        }
      } else {
        get().showToast("tap · keep tapping…");
      }
    },

    audition(pitch, velocity = 100, lengthTicks) {
      const { selectedTrackId: id, noteLength, snapshot } = get();
      if (!id) return;
      const bpm = snapshot?.model.tempo ?? 120;
      const seconds = (ticksToBeats(lengthTicks ?? noteLength) * 60) / bpm;
      void getBridge().then((b) => b.audition(id, pitch, velocity, seconds));
    },

    setLoopRegion(region) {
      set({ loopRegion: region });
      void getBridge().then((b) =>
        region ? b.setLoop(region[0], region[1]) : b.clearLoop(),
      );
    },
    toggleLooping() {
      const on = !get().looping;
      set({ looping: on });
      void getBridge().then((b) => b.setLooping(on));
    },
    toggleRecord() {
      const armed = !get().armed;
      set({ armed });
      void getBridge().then(async (b) =>
        set({ midi: await b.setRecord(armed, get().noteLength) }),
      );
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
    toggleBrush() {
      set((s) => ({ brush: !s.brush }));
    },
    toggleFollow() {
      set((s) => ({ follow: !s.follow }));
    },
    toggleFold() {
      set((s) => ({ fold: !s.fold }));
    },
    setGhostTrack(id) {
      set({ ghostTrackId: id });
    },
    setLaneProp(prop) {
      set({ laneProp: prop });
    },
    requestSelection(ids) {
      set((s) => ({
        selectionRequest: { ids, nonce: (s.selectionRequest?.nonce ?? 0) + 1 },
      }));
    },
    transformSelection(fn, label) {
      const st = get();
      const pattern = selectedPattern(st);
      const track = selectedTrack(st);
      if (!pattern || !track) return;
      const notes = pattern.notesByTrack[track.id] ?? [];
      const next = fn(notes, new Set(st.selectedNoteIds));
      if (next === notes) return;
      void st.dispatch(
        {
          type: "setNotes",
          patternId: pattern.id,
          trackId: track.id,
          notes: next,
        },
        false,
        label,
      );
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
      if (!(await get().confirmDiscard(`opening “${fileNameOf(path)}”`)))
        return;
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
        const picked = await b.pickSavePath(
          fileNameOf(get().snapshot?.path),
          "dissonant",
        );
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
      set({ exportOpen: true });
    },
    openExport(open = true) {
      set({ exportOpen: open });
    },
    async runExport(request) {
      const b = await getBridge();
      const base = fileNameOf(get().snapshot?.path).replace(/\.dissonant$/, "");
      const path = await b.pickSavePath(
        `${request.scope === "pattern" ? `${base} - ${selectedPattern(get())?.name ?? "pattern"}` : base}.wav`,
        "wav",
      );
      if (!path) return;
      set({
        exportOpen: false,
        exporting: {
          done: 0,
          total: request.stems ? 2 : 1,
          current: "rendering…",
        },
      });
      try {
        const out = await b.exportWav({ ...request, path });
        get().showToast(
          out.files.length > 1
            ? `exported ${out.files.length} files next to ${fileNameOf(path)}`
            : `exported ${fileNameOf(path)}`,
        );
      } catch (e) {
        get().showToast(String(e), true);
      }
      set({ exporting: null });
    },
    async exportMidi() {
      const b = await getBridge();
      const st = get();
      const scope =
        st.mode === "song" && st.snapshot?.model.clips.length
          ? "song"
          : "pattern";
      const base = fileNameOf(st.snapshot?.path).replace(/\.dissonant$/, "");
      const path = await b.pickSavePath(
        scope === "pattern"
          ? `${base} - ${selectedPattern(st)?.name ?? "pattern"}.mid`
          : `${base}.mid`,
        "mid",
      );
      if (!path) return;
      try {
        await b.exportMidi(path, scope);
        get().showToast(`exported ${fileNameOf(path)}`);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async importMidi() {
      const b = await getBridge();
      const pattern = selectedPattern(get());
      if (!pattern) return;
      const path = await b.pickOpenPath(["mid", "midi"], "MIDI file");
      if (!path) return;
      try {
        applySnapshot(await b.importMidi(path, pattern.id));
        get().showToast(
          `imported ${fileNameOf(path)} into ${pattern.name} as new tracks`,
        );
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    setDialogText(text) {
      set({ dialogText: text });
    },
    async askInput(title, message, placeholder, okLabel = "OK", initial = "") {
      set({ dialogText: initial });
      const v = await get().ask({
        title,
        message,
        input: { placeholder, value: initial },
        buttons: [
          { label: okLabel, value: "ok", kind: "primary" },
          { label: "Cancel", value: "cancel", kind: "quiet" },
        ],
        cancelValue: "cancel",
      });
      return v === "ok" ? get().dialogText.trim() || null : null;
    },
    openTemplates(open = true) {
      set({ templatesOpen: open });
      if (open) void get().refreshTemplates();
    },
    async refreshTemplates() {
      const b = await getBridge();
      try {
        set({ templates: await b.listTemplates() });
      } catch {
        /* no backend */
      }
    },
    async saveAsTemplate() {
      const b = await getBridge();
      const suggested = fileNameOf(get().snapshot?.path).replace(
        /\.dissonant$/,
        "",
      );
      const name = await get().askInput(
        "Save as template",
        "New projects can start from this one (tracks, chords, notes and mix included).",
        "template name",
        "Save",
        suggested === "untitled" ? "" : suggested,
      );
      if (!name) return;
      try {
        await b.saveTemplate(name);
        get().showToast(`template “${name}” saved`);
        void get().refreshTemplates();
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async deleteTemplate(name) {
      const b = await getBridge();
      if (
        !(await get().confirm(
          "Delete template?",
          `“${name}” will be removed.`,
          "Delete",
          true,
        ))
      )
        return;
      await b.deleteTemplate(name);
      void get().refreshTemplates();
    },
    async newFromTemplate(name) {
      const b = await getBridge();
      if (!(await get().confirmDiscard("creating a new project"))) return;
      set({ templatesOpen: false });
      try {
        if (name === "starter" || name === "empty")
          applySnapshot(await b.newProject(name === "starter"));
        else applySnapshot(await b.newFromTemplate(name));
        await syncPlayback();
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async restartAudio() {
      const b = await getBridge();
      const audio = await b.restartAudio();
      set({ audio });
      get().showToast(
        audio.running
          ? `audio running @ ${audio.sampleRate} Hz`
          : `audio: ${audio.error}`,
        !audio.running,
      );
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
      if (danger && !get().settings.editing.confirmDestructive) return true;
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

    openSettings(open = true) {
      set({ settingsOpen: open });
      if (open) {
        void get().refreshOutputDevices();
        void get().refreshMidi();
      }
    },
    async updateSettings(section, patch) {
      const b = await getBridge();
      const next: Settings = {
        ...get().settings,
        [section]: { ...get().settings[section], ...patch },
      };
      set({ settings: next });
      if (section === "appearance") applyAppearance(next.appearance);
      if (section === "metronome")
        void b.setMetronome(next.metronome.on, next.metronome.volume);
      try {
        const audio = await b.setSettings(next);
        if (b.isTauri) set({ audio });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async refreshOutputDevices() {
      const b = await getBridge();
      try {
        set({ outputDevices: await b.outputDevices() });
      } catch {
        /* no backend */
      }
    },
    testTone() {
      void getBridge().then((b) => b.testTone());
    },
    async refreshAudio() {
      const b = await getBridge();
      if (!b.isTauri) return;
      try {
        set({ audio: await b.audioStatus() });
      } catch {
        /* ignore */
      }
    },

    async refreshPlugins(rescan = false) {
      const b = await getBridge();
      try {
        set({ plugins: await b.scanPlugins(rescan) });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async setTrackPlugin(trackId, uid) {
      const b = await getBridge();
      try {
        await b.setTrackPlugin(trackId, uid);
        applySnapshot(await b.getState());
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async refreshCatalog() {
      const b = await getBridge();
      try {
        set({ catalog: await b.pluginCatalog() });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async pluginParams(trackId) {
      const b = await getBridge();
      try {
        return await b.pluginParams(trackId);
      } catch (e) {
        get().showToast(String(e), true);
        return [];
      }
    },
    async setPluginParam(trackId, paramId, value, commit = false) {
      const b = await getBridge();
      try {
        await b.setPluginParam(trackId, paramId, value, commit);
        // A committed value is a document edit; the dragging ones never touch it.
        if (commit) applySnapshot(await b.getState());
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async installPlugin(entryId) {
      const b = await getBridge();
      try {
        await b.installPlugin(entryId);
        await get().refreshPlugins(true);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async openUrl(url) {
      // Only https: handing an arbitrary string to the OS opener is a shell-adjacent surface,
      // and the backend refuses anything else too.
      if (!url.startsWith("https://")) return;
      const b = await getBridge();
      try {
        await b.openUrl(url);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async openPluginWindow(trackId) {
      const b = await getBridge();
      try {
        await b.openPluginWindow(trackId);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    openPluginBrowser(open = true) {
      set({ pluginBrowserOpen: open });
    },

    // ── Effect inserts ────────────────────────────────────────────────────────────────────
    // Every one of these names the effect in its undo label, so the Edit menu reads
    // "undo bypass Airwindows Consolidated" rather than "undo bypass insert".
    async addInsert(target, uid, index = null) {
      const b = await getBridge();
      const name = get().plugins.find((p) => p.uid === uid)?.name ?? "effect";
      try {
        applySnapshot(await b.addInsert(target, uid, index, `add ${name}`));
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async removeInsert(target, insertId) {
      const b = await getBridge();
      const label = `remove ${insertName(target, insertId)}`;
      try {
        applySnapshot(await b.removeInsert(target, insertId, label));
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async moveInsert(target, insertId, toIndex) {
      const b = await getBridge();
      const label = `reorder ${insertName(target, insertId)}`;
      try {
        applySnapshot(await b.moveInsert(target, insertId, toIndex, label));
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async setInsertEnabled(target, insertId, on) {
      const b = await getBridge();
      const label = `${on ? "enable" : "bypass"} ${insertName(target, insertId)}`;
      try {
        applySnapshot(await b.setInsertEnabled(target, insertId, on, label));
      } catch (e) {
        get().showToast(String(e), true);
      }
    },
    async insertParams(insertId) {
      const b = await getBridge();
      try {
        return await b.insertParams(insertId);
      } catch (e) {
        get().showToast(String(e), true);
        return [];
      }
    },
    async setInsertParam(insertId, paramId, value, commit = false) {
      const b = await getBridge();
      try {
        // A committed value is a document edit and comes back as a snapshot; a dragged one
        // only reaches the effect and answers null.
        const s = await b.setInsertParam(insertId, paramId, value, commit);
        if (s) applySnapshot(s);
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    // ── Guitar ────────────────────────────────────────────────────────────────────────────

    async addAudioTrack() {
      const n =
        (get().snapshot?.model.tracks.filter((t) => t.isAudio).length ?? 0) + 1;
      await get().dispatch({
        type: "addAudioTrack",
        name: n === 1 ? "guitar" : `guitar ${n}`,
      });
      const tracks = get().snapshot?.model.tracks ?? [];
      const added = tracks[tracks.length - 1];
      if (added?.isAudio) {
        set({ selectedTrackId: added.id });
        await get().setArm(added.id);
      }
    },

    async setArm(trackId) {
      const b = await getBridge();
      try {
        const audio = await b.setAudioArm(trackId, get().monitor);
        set({ armedAudioTrackId: trackId, audio });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async toggleMonitor() {
      const monitor = !get().monitor;
      set({ monitor });
      const b = await getBridge();
      try {
        set({ audio: await b.setAudioArm(get().armedAudioTrackId, monitor) });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async startRecording() {
      const b = await getBridge();
      try {
        await b.startAudioRecord();
      } catch (e) {
        get().showToast(String(e), true);
        return;
      }
      // Arming first and playing second is what keeps the count-in out of the take.
      get().play();
    },

    async stopRecording() {
      const b = await getBridge();
      get().stop();
      try {
        const snap = await b.stopAudioRecord();
        applySnapshot(snap);
        const clips = snap.model.audioClips ?? [];
        if (clips.length)
          set({ selectedAudioClipId: clips[clips.length - 1].id });
      } catch (e) {
        get().showToast(String(e), true);
      }
    },

    async importAudioFile(trackId = null) {
      const b = await getBridge();
      const path = await b.pickOpenPath(
        ["wav", "wave", "aif", "aiff", "flac", "mp3", "m4a", "aac"],
        "audio file",
      );
      if (!path) return;
      await get().importAudioPath(path, trackId);
    },

    async importAudioPath(path, trackId = null) {
      const b = await getBridge();
      // Drop onto the selected audio track when there is one, so a drag does the obvious thing.
      const selected = get().snapshot?.model.tracks.find(
        (t) => t.id === get().selectedTrackId,
      );
      const target = trackId ?? (selected?.isAudio ? selected.id : null);
      try {
        applySnapshot(await b.importAudio(path, target, 0));
        set({ screen: "guitar", mode: "song", importing: null });
        await syncPlayback();
        get().showToast(`imported ${fileNameOf(path)}`);
      } catch (e) {
        set({ importing: null });
        get().showToast(String(e), true);
      }
    },

    selectAudioClip(id) {
      set({ selectedAudioClipId: id });
    },

    async setAmp(trackId, patch, transient = false) {
      const track = get().snapshot?.model.tracks.find((t) => t.id === trackId);
      const amp = track?.amp;
      if (!amp) return;
      await get().dispatch(
        { type: "setTrackAmp", id: trackId, amp: { ...amp, ...patch } },
        transient,
      );
    },

    async refreshInputDevices() {
      const b = await getBridge();
      try {
        set({ inputDevices: await b.inputDevices() });
      } catch {
        set({ inputDevices: [] });
      }
    },
  };
});

// ─── Selectors / helpers ───────────────────────────────────────────────────────────────────

export function selectedPattern(s: State): SongPattern | null {
  const m = s.snapshot?.model;
  if (!m) return null;
  return (
    m.patterns.find((p) => p.id === s.selectedPatternId) ??
    m.patterns[0] ??
    null
  );
}

export function selectedTrack(s: State): Track | null {
  const m = s.snapshot?.model;
  if (!m) return null;
  return (
    m.tracks.find((t) => t.id === s.selectedTrackId) ?? m.tracks[0] ?? null
  );
}

/**
 * The audio track the guitar screen is showing: the selected one when it is an audio track,
 * else the first. One rule so the screen and the status bar never disagree about it.
 */
export function selectedAudioTrack(s: State): Track | null {
  const tracks = s.snapshot?.model.tracks.filter((t) => t.isAudio) ?? [];
  return tracks.find((t) => t.id === s.selectedTrackId) ?? tracks[0] ?? null;
}

export function selectedTrackIndex(s: State): number {
  const m = s.snapshot?.model;
  if (!m) return 0;
  const i = m.tracks.findIndex((t) => t.id === s.selectedTrackId);
  return i < 0 ? 0 : i;
}

export const GRID_OPTIONS: [string, number][] = [
  ["1/32", 120],
  ["1/16", 240],
  ["1/8", 480],
  ["1/4", 960],
  ["1/2", 1920],
  ["1", 3840],
];

export function gridLabel(ticks: number): string {
  return GRID_OPTIONS.find(([, v]) => v === ticks)?.[0] ?? `${ticks}`;
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

/** Ticks per bar at the project's time signature. */
export function barTicks(s: State): number {
  return barTicksOf(s.snapshot?.model.timeSignature);
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
  return clip ? keyAt(m, clip.startTick) : m.key;
}
