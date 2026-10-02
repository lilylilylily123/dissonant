// Mirrors the Rust model in crates/dissonant-core (serde camelCase). Keep in sync.

export type ScaleType = "major" | "minor";

export interface KeyState {
  rootPitchClass: number | null;
  scale: ScaleType;
  isLocked: boolean;
}

export const NO_KEY: KeyState = { rootPitchClass: null, scale: "major", isLocked: false };

export interface ChordEvent {
  id: string;
  startBeat: number;
  lengthBeats: number;
  pitchClasses: number[];
  name: string | null;
}

export interface NoteEvent {
  id: string;
  startBeat: number;
  lengthBeats: number;
  pitch: number;
  velocity: number;
}

export interface Track {
  id: string;
  name: string;
  voice: string;
  muted: boolean;
  soloed: boolean;
  isDrum: boolean;
  volume: number;
  reverbSend: number;
  tone: number;
  pan: number;
  /** `#rrggbb`, or null to use the palette by position. */
  color: string | null;
}

export interface ChordTrack {
  chords: ChordEvent[];
}

export interface SongPattern {
  id: string;
  name: string;
  lengthBeats: number;
  chords: ChordTrack;
  notesByTrack: Record<string, NoteEvent[]>;
}

export interface MasterSettings {
  gain: number;
  reverbWet: number;
  lowCutHz: number;
  highCutHz: number;
  lowEq: number;
  midEq: number;
  highEq: number;
}

export interface ProjectModel {
  schemaVersion: number;
  tempo: number;
  key: KeyState;
  tracks: Track[];
  patterns: SongPattern[];
  arrangement: string[];
  master: MasterSettings;
}

export interface Snapshot {
  model: ProjectModel;
  canUndo: boolean;
  canRedo: boolean;
  dirty: boolean;
  path: string | null;
}

export type TrackParam = "volume" | "reverbSend" | "tone" | "pan";
export type PlayMode = "pattern" | "song";
export type Tier = "chordTone" | "tension" | "dissonance";

export type Command =
  | { type: "setTempo"; bpm: number }
  | { type: "setKey"; key: KeyState }
  | { type: "addTrack"; isDrum: boolean }
  | { type: "deleteTrack"; id: string }
  | { type: "renameTrack"; id: string; name: string }
  | { type: "setTrackMuted"; id: string; muted: boolean }
  | { type: "setTrackSoloed"; id: string; soloed: boolean }
  | { type: "moveTrack"; id: string; up: boolean }
  | { type: "setTrackVoice"; id: string; voice: string }
  | { type: "setTrackParam"; id: string; param: TrackParam; value: number }
  | { type: "setTrackColor"; id: string; color: string | null }
  | { type: "addPattern" }
  | { type: "duplicatePattern"; id: string }
  | { type: "deletePattern"; id: string }
  | { type: "renamePattern"; id: string; name: string }
  | { type: "setPatternLength"; id: string; beats: number }
  | { type: "setNotes"; patternId: string; trackId: string; notes: NoteEvent[] }
  | { type: "setChords"; patternId: string; chords: ChordEvent[] }
  | { type: "setArrangement"; arrangement: string[] }
  | { type: "setMaster"; master: MasterSettings };

export interface PlayheadEvent {
  beat: number;
  playing: boolean;
  masterPeak: [number, number];
  trackPeaks: number[];
}

export interface AudioStatus {
  running: boolean;
  sampleRate: number | null;
  error: string | null;
}

export const VOICES = ["saw", "square", "triangle", "sine", "pad", "pluck"] as const;

/** The handoff's 12-color track palette (last one is reserved for returns/master). */
export const TRACK_PALETTE = [
  "#ff3b30",
  "#ff6a2a",
  "#ff9d2a",
  "#ffd02a",
  "#e2ec3e",
  "#b8f53a",
  "#3dffb0",
  "#3dc8ff",
  "#5a8cff",
  "#9a7bff",
  "#ff4fd8",
  "#8a8a94",
];

export function trackColor(track: Track, index: number): string {
  return track.color ?? TRACK_PALETTE[index % 11];
}

export const TIER_COLORS: Record<Tier, string> = {
  chordTone: "#3dffb0",
  tension: "#ff9d2a",
  dissonance: "#ff3b30",
};

/** Drum kit rows, top to bottom. Mirrors `dissonant_engine::drums::KIT`. */
export const DRUM_KIT: { name: string; pitch: number }[] = [
  { name: "hat", pitch: 42 },
  { name: "clap", pitch: 39 },
  { name: "snare", pitch: 38 },
  { name: "kick", pitch: 36 },
];

export function uuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  // Fallback for very old webviews.
  return "xxxxxxxx-xxxx-4xxx-yxxx-xxxxxxxxxxxx".replace(/[xy]/g, (c) => {
    const r = (Math.random() * 16) | 0;
    return (c === "x" ? r : (r & 0x3) | 0x8).toString(16);
  });
}
