import { describe, expect, it } from "vitest";
import { groupParams } from "./components/PluginWindow";
import type { ParamInfo } from "./types";

const p = (name: string, module = ""): ParamInfo => ({
  id: name.length * 31 + name.charCodeAt(0),
  name,
  module,
  min: 0,
  max: 1,
  default: 0,
  value: 0,
  isStepped: false,
});

describe("grouping a plugin's parameters", () => {
  it("uses the plugin's own modules when it publishes them", () => {
    const groups = groupParams([p("cutoff", "filter"), p("res", "filter"), p("attack", "env")]);
    expect(groups.map((g) => g.name)).toEqual(["filter", "env"]);
    // A module is already a heading, so the names below it are not prefixed with it.
    expect(groups.every((g) => !g.strip)).toBe(true);
  });

  /** Dexed is the case this exists for: 156 parameters, every module empty. */
  it("falls back to the shared first word when every module is empty", () => {
    const params = [
      p("Cutoff"),
      p("Resonance"),
      ...["RATE 1", "RATE 2", "RATE 3"].map((n) => p(`OP1 EG ${n}`)),
      ...["SPEED", "DELAY", "WAVE"].map((n) => p(`LFO ${n}`)),
    ];
    const groups = groupParams(params);
    expect(groups.map((g) => g.name)).toEqual(["general", "OP1", "LFO"]);
    // The heading carries the prefix, so the rows under it drop it and stay readable.
    const op1 = groups.find((g) => g.name === "OP1")!;
    expect(op1.strip).toBe(true);
    expect(op1.params.map((x) => x.name.slice("OP1".length).trim())).toEqual(["EG RATE 1", "EG RATE 2", "EG RATE 3"]);
  });

  it("keeps a word only one or two parameters share out of the headings", () => {
    // "MASTER TUNE ADJ" and "OSC KEY SYNC" are one-offs: a heading per parameter is noise.
    const groups = groupParams([p("MASTER TUNE ADJ"), p("OSC KEY SYNC"), p("FEEDBACK")]);
    expect(groups.map((g) => g.name)).toEqual(["general"]);
    expect(groups[0].params).toHaveLength(3);
  });

  it("puts the catch-all first however late its members appear", () => {
    const groups = groupParams([...["A", "B", "C"].map((n) => p(`OP1 ${n}`)), p("Output")]);
    expect(groups[0].name).toBe("general");
  });

  it("loses no parameter and never repeats one", () => {
    const params = [...Array(40)].map((_, i) => p(`OP${i % 6} THING ${i}`));
    const groups = groupParams(params);
    const ids = groups.flatMap((g) => g.params.map((x) => x.id));
    expect(ids).toHaveLength(params.length);
    expect(new Set(ids).size).toBe(new Set(params.map((x) => x.id)).size);
  });

  it("has nothing to group when the plugin publishes no parameters", () => {
    expect(groupParams([])).toEqual([]);
  });
});
