// Tier mode's key: the section key at the song position under the playhead. In PATTERN that
// position is `chordOffsetTicks` + the pattern-relative playhead; in SETUP / SONG it is the playhead.

import { beforeAll, describe, expect, it, vi } from "vitest";
import type { KeyState, ProjectModel } from "./types";
import { TICKS_PER_BEAT as B } from "./types";

beforeAll(() => {
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("document", { title: "" });
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
  });
});

const C_MAJOR: KeyState = { rootPitchClass: 0, scale: "major", isLocked: true };
const D_MAJOR: KeyState = { rootPitchClass: 2, scale: "major", isLocked: true };

/** The slice of the store `pitchForKey` reads. */
interface KeyboardState {
  screen: "pattern" | "song";
  playhead: number;
  chordOffsetTicks: number;
  selectedPatternId: string;
  selectedTrackId: string;
  snapshot: { model: ProjectModel };
}

// No chords, so the home row is the key's scale tones from C3 (48): C major starts on C (48),
// D major on C♯ (49). The second section (D major) starts at song beat 32.
function state(screen: "pattern" | "song", playheadBeats: number, chordOffsetBeats: number): KeyboardState {
  const model = {
    key: C_MAJOR,
    chords: [],
    sections: [
      { id: "a", name: "a", startTick: 0, key: null, color: null },
      { id: "b", name: "b", startTick: 32 * B, key: D_MAJOR, color: null },
    ],
    patterns: [{ id: "p", trackId: "t", name: "p", lengthTicks: 16 * B, notes: [] }],
    tracks: [{ id: "t" }],
  } as unknown as ProjectModel;
  return {
    screen,
    playhead: playheadBeats * B,
    chordOffsetTicks: chordOffsetBeats * B,
    selectedPatternId: "p",
    selectedTrackId: "t",
    snapshot: { model },
  };
}

type PitchForKey = (key: string, mode: "tier" | "chromatic", octave: number, state?: never) => number | null;
let pitchForKey: PitchForKey;
beforeAll(async () => {
  // Loaded here, not at the top: importing it pulls in the store and the mock bridge, which
  // read `localStorage` and `document` as they initialise — both stubbed only just above.
  ({ pitchForKey } = (await import("./liveKeyboard")) as unknown as { pitchForKey: PitchForKey });
});
const home = (s: KeyboardState) => pitchForKey("a", "tier", 0, s as unknown as never);

describe("pitchForKey in tier mode", () => {
  it("song: follows the section at the playhead", () => {
    expect(home(state("song", 4, 0))).toBe(48);
    expect(home(state("song", 33, 0))).toBe(49);
  });

  it("pattern: follows the section at chordOffsetTicks + playhead", () => {
    expect(home(state("pattern", 1, 0))).toBe(48);
    expect(home(state("pattern", 1, 32))).toBe(49);
    // A pattern sitting across the section change: 28 + 6 = song beat 34, in D.
    expect(home(state("pattern", 6, 28))).toBe(49);
  });
});
