import { describe, expect, it } from "vitest";
import { chordAt, chordName, detectKey, diatonicChords, midiName, progression, tier, tierMap } from "./theory";
import { NO_KEY } from "./types";

const C_MAJOR = [0, 2, 4, 5, 7, 9, 11];

describe("tiers (must agree with dissonant-core)", () => {
  it("classifies against a C major triad", () => {
    for (const pc of [0, 4, 7]) expect(tier(pc, [0, 4, 7], null)).toBe("chordTone");
    expect(tier(2, [0, 4, 7], null)).toBe("tension");
    expect(tier(5, [0, 4, 7], null)).toBe("dissonance");
    expect(tier(10, [0, 4, 7], null)).toBe("tension");
    expect(tier(10, [0, 4, 7], C_MAJOR)).toBe("dissonance");
    expect(tier(13, [0, 4, 7], null)).toBe(tier(1, [0, 4, 7], null));
  });

  it("degrades through three states", () => {
    const chords = progression([0, 3], 0, "major", 8, 4);
    expect(tierMap(1, chords, NO_KEY).every((t) => t !== null)).toBe(true);
    expect(tierMap(1, [], NO_KEY).every((t) => t === null)).toBe(true);
    const scaleOnly = tierMap(1, [], { rootPitchClass: 0, scale: "major", isLocked: true });
    expect(scaleOnly[0]).toBe("chordTone");
    expect(scaleOnly[1]).toBe("dissonance");
    expect(chordAt(chords, 4)?.name).toBe("F");
    expect(chordAt(chords, 8)).toBeNull();
  });
});

describe("harmony", () => {
  it("names diatonic chords and progressions in the key", () => {
    expect(diatonicChords(0, "major").map((c) => c.name)).toEqual(["C", "Dm", "Em", "F", "G", "Am", "Bdim"]);
    expect(diatonicChords(9, "minor").map((c) => c.romanNumeral)).toEqual(["i", "ii°", "III", "iv", "v", "VI", "VII"]);
    expect(progression([0, 3, 4, 5], 7, "major", 16).map((c) => c.name)).toEqual(["G", "C", "D", "Em"]);
    expect(progression([0, 3, 4, 5], 0, "major", 32)).toHaveLength(8);
  });

  it("names arbitrary chords", () => {
    expect(chordName([4, 7, 0])).toBe("C");
    expect(chordName([9, 0, 4])).toBe("Am");
    expect(chordName([0, 1, 2])).toBe("C·C#·D");
    expect(chordName([])).toBe("—");
    expect(midiName(60)).toBe("C4");
  });
});

describe("key detection", () => {
  it("is confident on a full scale, not on three notes", () => {
    const r = detectKey([0, 2, 4, 5, 7, 9, 11, 0, 4, 7, 0, 7]);
    expect(r.candidates[0]).toMatchObject({ rootPitchClass: 0, scale: "major" });
    expect(r.isConfident).toBe(true);
    expect(detectKey([0, 4, 7]).isConfident).toBe(false);
    expect(detectKey([]).candidates).toHaveLength(0);
  });
});
