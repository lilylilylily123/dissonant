/**
 * The rig's knob mapping, tested without a DOM.
 *
 * `vite.config.ts` pins `environment: "node"`, so nothing here renders a component — which is
 * exactly why `pedalKnobs` is a pure function taking a pedal and returning what to draw.
 */

import { describe, expect, it } from "vitest";
import { AMP_PRESETS, matchingPreset, pedalKnobs, PEDAL_LABELS } from "./components/AmpRack";
import type { AmpPreset } from "./components/AmpRack";
import type { Pedal } from "./types";
import { defaultAmpRig, newPedal, PEDAL_KINDS } from "./types";

/** The rig a model menu entry builds, which is all `matchingPreset` is asked about. */
const rigOf = (p: AmpPreset) => ({ ...defaultAmpRig(), amp: p.amp, eq: p.eq, cab: p.cab });

describe("pedalKnobs", () => {
  it("gives every pedal kind a label and at least two knobs", () => {
    for (const kind of PEDAL_KINDS) {
      const knobs = pedalKnobs(newPedal(kind));
      expect(PEDAL_LABELS[kind]).toBeTruthy();
      expect(knobs.length).toBeGreaterThanOrEqual(2);
      for (const k of knobs) {
        expect(k.label).toBeTruthy();
        expect(k.display).toBeTruthy();
        expect(k.norm).toBeGreaterThanOrEqual(0);
        expect(k.norm).toBeLessThanOrEqual(1);
      }
    }
  });

  it("round-trips a knob's position without losing the pedal's other fields", () => {
    for (const kind of PEDAL_KINDS) {
      const before = newPedal(kind);
      for (const k of pedalKnobs(before)) {
        const after = k.set(k.norm);
        // The same position means the same pedal: nothing drifts just from being read back.
        expect(after.kind).toBe(before.kind);
        expect(after.on).toBe(before.on);
        for (const field of Object.keys(before) as (keyof Pedal)[]) {
          const a = after[field as keyof typeof after];
          const b = before[field as keyof typeof before];
          if (typeof a === "number" && typeof b === "number") expect(a).toBeCloseTo(b, 6);
          else expect(a).toBe(b);
        }
      }
    }
  });

  it("moves exactly one field per knob", () => {
    for (const kind of PEDAL_KINDS) {
      const before = newPedal(kind);
      const knobs = pedalKnobs(before);
      for (const k of knobs) {
        const moved = k.set(Math.min(1, k.norm + 0.3));
        const changed = (Object.keys(before) as string[]).filter((f) => {
          const a = (moved as Record<string, unknown>)[f];
          const b = (before as Record<string, unknown>)[f];
          return typeof a === "number" && typeof b === "number" ? Math.abs(a - b) > 1e-9 : a !== b;
        });
        expect(changed).toHaveLength(1);
      }
    }
  });

  it("spans each knob's full range between its ends", () => {
    for (const kind of PEDAL_KINDS) {
      const p = newPedal(kind);
      for (const k of pedalKnobs(p)) {
        const low = k.set(0) as unknown as Record<string, unknown>;
        const high = k.set(1) as unknown as Record<string, unknown>;
        const field = Object.keys(low).find((f) => typeof low[f] === "number" && low[f] !== high[f])!;
        expect(low[field] as number).toBeLessThan(high[field] as number);
        expect(pedalKnobs(k.set(0)).find((x) => x.label === k.label)!.norm).toBeCloseTo(0, 6);
        expect(pedalKnobs(k.set(1)).find((x) => x.label === k.label)!.norm).toBeCloseTo(1, 6);
      }
    }
  });

  it("prints a unit on the knobs that have one", () => {
    const delay = pedalKnobs({ kind: "delay", on: true, timeMs: 350, feedback: 3, mix: 3 });
    expect(delay.find((k) => k.label === "time")!.display).toBe("350ms");
    const chorus = pedalKnobs({ kind: "chorus", on: true, rateHz: 1.2, depth: 5, mix: 5 });
    expect(chorus.find((k) => k.label === "rate")!.display).toBe("1.20Hz");
    // A plain 0…10 knob reads as a number, like the markings on a real pedal.
    expect(delay.find((k) => k.label === "mix")!.display).toBe("3.0");
  });
});

describe("matchingPreset", () => {
  it("recognises each model by its own component set, and only its own", () => {
    for (const p of AMP_PRESETS) {
      expect(matchingPreset(rigOf(p))?.name).toBe(p.name);
    }
  });

  it("ignores the knobs, and lets go once a component is changed by hand", () => {
    const p = AMP_PRESETS[0];
    const dialled = { ...rigOf(p), amp: { ...p.amp, gain: 10, master: 1 } };
    // Turning up the gain does not make it another amp — the model menu holds.
    expect(matchingPreset(dialled)?.name).toBe(p.name);

    const rewired = { ...rigOf(p), eq: "boutique" as const };
    expect(matchingPreset(rewired)).toBeNull();
  });
});
