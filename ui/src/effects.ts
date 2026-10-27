/**
 * The stock effects, as data. Mirrors `dissonant_core::effects` field for field: the same uids,
 * the same parameter ids, the same ranges and defaults, in the same order.
 *
 * It is a copy rather than something fetched because the browser mock in `bridge.ts` is a real
 * reimplementation of the Rust rules — it has to clamp and default exactly as the backend does
 * with no backend present — and because the picker wants the list before any effect exists.
 * The parity test in `effects.test.ts` is what keeps the two honest.
 */

/** Which stock effect, in the order the picker offers them. Mirrors `NativeKind`. */
export type NativeKind =
  | "eq"
  | "compressor"
  | "limiter"
  | "saturator"
  | "delay"
  | "reverb";

/** The prefix that tells a native effect's uid from a plugin's. */
export const NATIVE_PREFIX = "native:";

/** One knob on one native effect. The same shape as {@link ParamInfo} minus the live value, so
 *  the knob column draws a native effect and a hosted plugin with one component. */
export interface NativeParam {
  id: number;
  /** Includes the unit (`"attack ms"`), the only unit information a knob ever gets. */
  name: string;
  /** The group heading the knob sits under; groups draw in first-appearance order. */
  module: string;
  min: number;
  max: number;
  default: number;
  /** Integral values only: a choice, not a continuum. */
  stepped: boolean;
}

/** One stock effect: what it is called, what it is for, and what it can be set to. */
export interface NativeEffectInfo {
  kind: NativeKind;
  uid: string;
  name: string;
  blurb: string;
  /** First id of this effect's block of a hundred. Mirrors `NativeKind::base_id`. */
  base: number;
  params: NativeParam[];
}

const p = (
  id: number,
  name: string,
  module: string,
  min: number,
  max: number,
  def: number,
  stepped = false,
): NativeParam => ({ id, name, module, min, max, default: def, stepped });

/**
 * Every stock effect, in picker order.
 *
 * Parameter ids are stable forever — a project file and an automation lane store one — and each
 * effect owns a block of a hundred so an id names its own effect on sight. Append within a
 * block; never renumber, never reuse.
 */
export const NATIVE_EFFECTS: NativeEffectInfo[] = [
  {
    kind: "eq",
    uid: "native:eq",
    name: "EQ",
    blurb: "cut, shelve and sweep",
    base: 100,
    params: [
      p(100, "high-pass hz", "cut", 20, 2_000, 20),
      p(114, "low-pass hz", "cut", 500, 20_000, 20_000),
      p(101, "low shelf hz", "low", 40, 1_000, 120),
      p(102, "low shelf dB", "low", -18, 18, 0),
      p(103, "band 1 hz", "band 1", 40, 18_000, 220),
      p(104, "band 1 dB", "band 1", -18, 18, 0),
      p(105, "band 1 q", "band 1", 0.3, 6, 1),
      p(106, "band 2 hz", "band 2", 40, 18_000, 1_200),
      p(107, "band 2 dB", "band 2", -18, 18, 0),
      p(108, "band 2 q", "band 2", 0.3, 6, 1),
      p(109, "band 3 hz", "band 3", 40, 18_000, 5_000),
      p(110, "band 3 dB", "band 3", -18, 18, 0),
      p(111, "band 3 q", "band 3", 0.3, 6, 1),
      p(112, "high shelf hz", "high", 1_000, 18_000, 6_000),
      p(113, "high shelf dB", "high", -18, 18, 0),
    ],
  },
  {
    kind: "compressor",
    uid: "native:compressor",
    name: "compressor",
    blurb: "hold the level down",
    base: 200,
    params: [
      p(200, "threshold dB", "gain", -60, 0, -18),
      p(201, "ratio", "gain", 1, 20, 4),
      p(204, "knee dB", "gain", 0, 18, 6),
      p(205, "makeup dB", "gain", -12, 24, 0),
      p(202, "attack ms", "time", 0.1, 50, 10),
      p(203, "release ms", "time", 20, 800, 150),
    ],
  },
  {
    kind: "limiter",
    uid: "native:limiter",
    name: "limiter",
    blurb: "a hard ceiling, lookahead",
    base: 300,
    params: [
      p(301, "drive dB", "limit", 0, 24, 0),
      p(300, "ceiling dB", "limit", -24, 0, -0.3),
      p(302, "release ms", "limit", 10, 500, 60),
    ],
  },
  {
    kind: "saturator",
    uid: "native:saturator",
    name: "saturator",
    blurb: "tube warmth and grit",
    base: 400,
    params: [
      p(400, "drive", "tube", 1, 20, 3),
      p(401, "bias", "tube", -1, 1, -0.2),
      p(402, "emphasis hz", "tube", 0, 4_000, 0),
      p(403, "mix", "out", 0, 1, 1),
      p(404, "output dB", "out", -24, 12, 0),
    ],
  },
  {
    kind: "delay",
    uid: "native:delay",
    name: "delay",
    blurb: "repeats, in time",
    base: 500,
    params: [
      p(500, "delay 1/x", "time", 1, 32, 8, true),
      p(501, "feedback", "repeats", 0, 0.95, 0.35),
      p(502, "mix", "repeats", 0, 1, 0.3),
    ],
  },
  {
    kind: "reverb",
    uid: "native:reverb",
    name: "reverb",
    blurb: "space around it",
    base: 600,
    params: [
      p(600, "mix", "space", 0, 1, 0.25),
      p(601, "decay", "space", 0, 1, 0.5),
      p(602, "pre-delay ms", "space", 0, 200, 20),
      p(603, "low-pass hz", "tone", 1_000, 20_000, 12_000),
    ],
  },
];

/** The effect a uid names, or `null` for a plugin uid. Mirrors `NativeKind::from_uid`. */
export function nativeEffect(uid: string): NativeEffectInfo | null {
  return NATIVE_EFFECTS.find((e) => e.uid === uid) ?? null;
}

/** Hold a value inside the knob's range, rounding a stepped one. A value that is not a number
 *  is not a position on the knob, so it falls back to the default. Mirrors `NativeParam::clamp`. */
export function clampNativeParam(param: NativeParam, value: number): number {
  if (!Number.isFinite(value)) return param.default;
  const v = Math.min(param.max, Math.max(param.min, value));
  return param.stepped ? Math.round(v) : v;
}

/** What `id` is set to given a slot's stored values. Mirrors `NativeKind::value_of`. */
export function nativeValue(
  effect: NativeEffectInfo,
  params: Record<string, number> | undefined,
  id: number,
): number {
  const param = effect.params.find((x) => x.id === id);
  if (!param) return 0;
  const stored = params?.[String(id)];
  return stored === undefined ? param.default : clampNativeParam(param, stored);
}

/** Drop values no parameter of this effect owns and hold the rest in range. Mirrors
 *  `effects::sanitize_params`. Returns `undefined` when nothing is left, so a slot at its
 *  defaults carries no map at all — exactly what the Rust `skip_serializing_if` does. */
export function sanitizeNativeParams(
  effect: NativeEffectInfo,
  params: Record<string, number> | undefined,
): Record<string, number> | undefined {
  if (!params) return undefined;
  const out: Record<string, number> = {};
  let any = false;
  for (const [key, value] of Object.entries(params)) {
    const param = effect.params.find((x) => x.id === Number(key));
    if (!param || !Number.isFinite(value)) continue;
    out[key] = clampNativeParam(param, value);
    any = true;
  }
  return any ? out : undefined;
}
