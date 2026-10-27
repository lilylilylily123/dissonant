import { describe, expect, it } from "vitest";
import {
  clampNativeParam,
  nativeEffect,
  nativeValue,
  NATIVE_EFFECTS,
  sanitizeNativeParams,
} from "./effects";

/**
 * The catalogue is a hand-kept copy of `dissonant_core::effects`. These tests hold the copy to
 * the invariants the Rust side states outright — one block of a hundred ids per effect, unique
 * ids, defaults inside their ranges — so a knob added on one side and forgotten on the other
 * shows up here rather than as a parameter that silently does nothing.
 */
describe("the native effect catalogue", () => {
  it("gives every parameter a unique id inside its own effect's block", () => {
    const seen = new Set<number>();
    for (const effect of NATIVE_EFFECTS) {
      for (const p of effect.params) {
        expect(p.id, `${effect.kind} ${p.name}`).toBeGreaterThanOrEqual(effect.base);
        expect(p.id).toBeLessThan(effect.base + 100);
        expect(seen.has(p.id), `duplicate id ${p.id}`).toBe(false);
        seen.add(p.id);
        expect(p.min).toBeLessThan(p.max);
        expect(p.default).toBeGreaterThanOrEqual(p.min);
        expect(p.default).toBeLessThanOrEqual(p.max);
      }
    }
  });

  it("names every effect with the native prefix and nothing else", () => {
    for (const effect of NATIVE_EFFECTS) {
      expect(effect.uid).toBe(`native:${effect.kind}`);
      expect(nativeEffect(effect.uid)).toBe(effect);
    }
    expect(nativeEffect("clap:/CLAP/Pro-Q.clap:proq3")).toBeNull();
    expect(nativeEffect("native:nope")).toBeNull();
  });

  it("ships the six an unplugged machine needs to mix", () => {
    expect(NATIVE_EFFECTS.map((e) => e.kind)).toEqual([
      "eq",
      "compressor",
      "limiter",
      "saturator",
      "delay",
      "reverb",
    ]);
  });
});

describe("native parameter values", () => {
  const comp = nativeEffect("native:compressor")!;
  const ratio = comp.params.find((p) => p.id === 201)!;

  it("falls back to the default and holds a stored value in range", () => {
    expect(nativeValue(comp, undefined, 201)).toBe(4);
    expect(nativeValue(comp, { "201": 900 }, 201)).toBe(ratio.max);
    expect(nativeValue(comp, { "201": Number.NaN }, 201)).toBe(ratio.default);
    expect(nativeValue(comp, {}, 999)).toBe(0);
  });

  it("rounds a stepped value and leaves a continuous one alone", () => {
    const delay = nativeEffect("native:delay")!;
    const division = delay.params.find((p) => p.id === 500)!;
    const mix = delay.params.find((p) => p.id === 502)!;
    expect(division.stepped).toBe(true);
    expect(clampNativeParam(division, 7.6)).toBe(8);
    expect(clampNativeParam(division, 99)).toBe(32);
    expect(clampNativeParam(mix, 0.37)).toBe(0.37);
  });

  it("drops ids the effect does not own and carries nothing when nothing is left", () => {
    expect(sanitizeNativeParams(comp, { "201": 7.5, "500": 4 })).toEqual({ "201": 7.5 });
    expect(sanitizeNativeParams(comp, { "500": 4 })).toBeUndefined();
    expect(sanitizeNativeParams(comp, undefined)).toBeUndefined();
  });
});
