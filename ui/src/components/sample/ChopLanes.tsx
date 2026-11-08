import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { getBridge } from "../../bridge";
import { barTicks, beatsPerBar, gridLabel, useStore } from "../../store";
import { chopOfPitch, chopPitch, chopSeconds, hitSounds, laneChops, laneRowHeight, moveHits, patternTickAt, songTickOf, spanPeaks, type HitSound } from "../../chopLanes";
import {
  copyNotes,
  duplicateBySpan,
  duplicateNotes,
  glueNotes,
  moveHitsByRow,
  noteValueLabel,
  notesInRect,
  nudgeNoteProp,
  pasteNotes,
  placeNote,
  quantizeNoteProp,
  rampNoteProp,
  removeNotes,
  resizeEdge,
  selectionBounds,
  setNoteProp,
  snapFloor,
  snapRound,
  splitNotes,
  stepSelection,
  tileGhost,
  toggleMute,
  type NoteEdge,
} from "../../noteEditing";
import { shiftedStart } from "../../ropes";
import { sourceSeconds } from "../../sampler";
import {
  DEFAULT_SWING_GRID_TICKS,
  mixHex,
  noteProp,
  NOTE_PROPS,
  swingWarp,
  tempoMap,
  TICKS_PER_BEAT,
  ticksToBeats,
  trackColor,
  type NoteEvent,
  type NoteProp,
  type SongPattern,
  type Track,
} from "../../types";
import { useMediaQuery, usePixelRatio } from "../../uiScale";
import { BASE_BEAT_W } from "../ChordLane";
import { CHOP_HUES } from "./SampleWave";
import "./sample.css";

const RULER_H = 28;
/** Row header width: hue, number, name and a mini waveform; compact keeps the number and the wave. */
export const LANE_HEAD_W = 156;
const LANE_HEAD_W_COMPACT = 64;
const WAVE_W = 52;
const WAVE_W_COMPACT = 28;
/** Room past the pattern's end, so a hit that ends there still shows its last pixel. */
const TAIL_PX = 8;
/** How far a gate note's edge handle reaches into it, and past its ends. */
const EDGE_PX = 7;
const EDGE_OUT = 5;
/** Travel before a press on a hit turns into a move or resize, so a click never nudges one. */
const DEAD_PX = 3;
/** Grid used while ⌥ bypasses snap. */
const FINE_TICKS = TICKS_PER_BEAT / 64;
/** Property lane height; shorter on a short page, like the roll's. */
const VEL_H_TALL = 110;
const VEL_H_SHORT = 64;
const SHORT_PAGE = "(max-height: 600px)";
/**
 * What the lane under the lanes can edit on a sampler: what the scheduler and the sampler's
 * voices read. Pan, fine pitch, release and the mod axes mean nothing to a chop.
 */
const LANE_PROPS: NoteProp[] = ["velocity", "shiftTicks", "repeats"];
/** Buckets one peaks fetch asks for: enough that a chop a sixty-fourth of the source still fills a header. */
const PEAK_BUCKETS = 4096;

const LANE_TOP = 10;
const laneBot = (h: number) => h - 4;
const laneFloor = (p: NoteProp) => (NOTE_PROPS[p].bipolar ? NOTE_PROPS[p].min : 0);
const laneY = (p: NoteProp, v: number, h: number) =>
  laneBot(h) - Math.round(((v - laneFloor(p)) / (NOTE_PROPS[p].max - laneFloor(p))) * (laneBot(h) - LANE_TOP));
const laneValue = (p: NoteProp, y: number, h: number) =>
  laneFloor(p) + ((laneBot(h) - y) / (laneBot(h) - LANE_TOP)) * (NOTE_PROPS[p].max - laneFloor(p));
const NUDGE_STEP: Partial<Record<NoteProp, number>> = { velocity: 10, shiftTicks: TICKS_PER_BEAT / 64, repeats: 1 };

const C = {
  editor: "#101013",
  ruler: "#121215",
  line1: "#26262d",
  rowA: "#17171b",
  rowB: "#141418",
  rowSep: "#0b0b0d",
  gridBar: "#3a3a44",
  gridBeat: "#26262e",
  gridSub: "rgba(255,255,255,.035)",
  head: "#121215",
  headHot: "#1b1b21",
  text1: "#e6e6ea",
  text2: "#b8b8c0",
  text4: "#6a6a74",
  accent: "#b48cff",
};
const hueOf = (chop: number) => CHOP_HUES[chop % CHOP_HUES.length];
const mono = (w: number, px: number) => `${w} ${px}px "JetBrains Mono", ui-monospace, monospace`;

/** Whole-source peaks per source id. A source never changes under its id, so one fetch serves every header. */
const peakCache = new Map<string, Promise<[number, number][]>>();
function sourcePeaks(sourceId: string): Promise<[number, number][]> {
  let p = peakCache.get(sourceId);
  if (!p) {
    p = getBridge().then((b) => b.audioPeaks(sourceId, PEAK_BUCKETS));
    peakCache.set(sourceId, p);
    // A failed fetch is forgotten, so the next mount asks again.
    p.catch(() => peakCache.delete(sourceId));
  }
  return p;
}

type Drag =
  | { kind: "move"; ids: Set<string>; anchor: NoteEvent; startX: number; startY: number; startRow: number; base: NoteEvent[]; lastPitch: number; origin?: "place" | "duplicate"; axis?: "x" | "y" | null; hitId: string; live: boolean }
  | { kind: "resize"; ids: Set<string>; anchor: NoteEvent; edge: NoteEdge; startX: number; startY: number; base: NoteEvent[]; hitId: string; live: boolean }
  | { kind: "paint"; base: NoteEvent[] }
  | { kind: "erase"; base: NoteEvent[] }
  | { kind: "marquee"; x0: number; y0: number; x1: number; y1: number; baseSelection: Set<string> }
  | { kind: "lane"; ids: Set<string>; prop: NoteProp; anchorId: string; base: NoteEvent[] }
  | { kind: "ramp"; ids: Set<string>; prop: NoteProp; t0: number; v0: number; base: NoteEvent[] };

interface Box {
  x: number;
  y: number;
  w: number;
  h: number;
}

/**
 * Chop lanes: a sampler pattern's editor. One row per chop, in chop order with chop 1 at the
 * bottom, headed by the chop's colour, number, name and waveform. Each hit draws what is heard:
 * a one-shot's solid head and then its chop fading out at the song's tempo, cut where the next
 * hit cuts it (any hit on a mono sampler; the same chop or choke group on a poly one); a gate
 * note's own length and then its release.
 *
 *  - click an empty row = place a hit at the snap (keep dragging to move it) · brush = paint
 *  - drag a hit = move in time; up or down = hand it to another chop, heard as it lands
 *    (⇧ one axis, ⌥-drag duplicates) · a gate hit's edges resize it
 *  - ⇧/⌘-drag = marquee · ⇧-click = toggle · right-click/drag = erase · ruler = seek, its top
 *    strip = loop brace (on SAMPLE, which plays the song: the ruler seeks to where the song
 *    plays that tick, and there is no brace) · click a header = hear the chop, double-click =
 *    select its hits
 *  - keys as in the roll: ⌫ ⌘A ⌘C ⌘X ⌘V ⌘D ⌘B ⌘J ⇧⌘E 0 Tab, arrows (↑↓ change chop), [ ]
 *
 * Takes its track and pattern as props so the SAMPLE page can host it too; `compact` drops the
 * names and the property lane for a short strip.
 */
export function ChopLanes({ track, pattern, compact = false }: { track: Track; pattern: SongPattern; compact?: boolean }) {
  const sampler = track.sampler ?? null;
  const notes = pattern.notes;
  const trackIdx = useStore((s) => s.snapshot?.model.tracks.findIndex((t) => t.id === track.id) ?? 0);
  const duration = useStore((s) => {
    const src = sampler ? s.snapshot?.model.audioSources?.find((a) => a.id === sampler.sourceId) : undefined;
    return src ? sourceSeconds(src) : 0;
  });
  const songPlayhead = useStore((s) => s.playhead);
  const clips = useStore((s) => s.snapshot?.model.clips);
  const noteLength = useStore((s) => s.noteLength);
  const zoom = useStore((s) => s.zoom);
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);
  const previewChop = useStore((s) => s.previewChop);
  const seek = useStore((s) => s.seek);
  const setZoom = useStore((s) => s.setZoom);
  const setSelectedNoteIds = useStore((s) => s.setSelectedNoteIds);
  const clipboard = useStore((s) => s.noteClipboard);
  const setClipboard = useStore((s) => s.setNoteClipboard);
  const bpb = useStore(beatsPerBar);
  const ticksPerBar = useStore(barTicks);
  const loopRegion = useStore((s) => s.loopRegion);
  const looping = useStore((s) => s.looping);
  const setLoopRegion = useStore((s) => s.setLoopRegion);
  const brush = useStore((s) => s.brush);
  const follow = useStore((s) => s.follow);
  const playing = useStore((s) => s.playing);
  const selectionRequest = useStore((s) => s.selectionRequest);
  const storeLaneProp = useStore((s) => s.laneProp);
  const setLaneProp = useStore((s) => s.setLaneProp);
  const editing = useStore((s) => s.settings.editing);
  const appearance = useStore((s) => s.settings.appearance);
  const swing = useStore((s) => s.snapshot?.model.swing ?? 50);
  const swingGridTicks = useStore((s) => s.snapshot?.model.swingGridTicks ?? DEFAULT_SWING_GRID_TICKS);
  const tempo = useStore((s) => s.snapshot?.model.tempo ?? 120);
  const tempoPoints = useStore((s) => s.snapshot?.model.tempoPoints);
  // PATTERN loops the pattern at the project tempo; every other screen plays the song, along
  // its tempo map, from where this pattern was opened (`chordOffsetTicks`).
  const songMode = useStore((s) => s.screen !== "pattern");
  const offset = useStore((s) => s.chordOffsetTicks);
  const showToast = useStore((s) => s.showToast);
  /**
   * The playhead in this pattern's ticks. PATTERN's playhead already is; SAMPLE's is a song
   * tick, which means something here only inside a clip of this pattern — elsewhere the
   * pattern is not playing and there is no playhead to draw (`null`), as recording files
   * nothing there either.
   */
  const playhead = songMode ? patternTickAt(clips ?? [], pattern, songPlayhead) : songPlayhead;
  const ghostPatternId = useStore((s) => s.ghostPatternId);
  const ghostPattern = useStore((s) => s.snapshot?.model.patterns.find((p) => p.id === ghostPatternId) ?? null);
  const ghostColor = useStore((s) => {
    const ts = s.snapshot?.model.tracks ?? [];
    const i = ghostPattern ? ts.findIndex((t) => t.id === ghostPattern.trackId) : -1;
    return i >= 0 ? trackColor(ts[i], i) : C.text4;
  });
  const pixelRatio = usePixelRatio();
  const VEL_H = useMediaQuery(SHORT_PAGE) ? VEL_H_SHORT : VEL_H_TALL;
  const HEAD_W = compact ? LANE_HEAD_W_COMPACT : LANE_HEAD_W;
  const laneProp = LANE_PROPS.includes(storeLaneProp) ? storeLaneProp : "velocity";

  const gridRef = useRef<HTMLCanvasElement>(null);
  const rulerRef = useRef<HTMLCanvasElement>(null);
  const headRef = useRef<HTMLCanvasElement>(null);
  const velRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<Drag | null>(null);
  const loopDrag = useRef<{ anchor: number } | null>(null);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<NoteEvent[] | null>(null);
  const [usedOnly, setUsedOnly] = useState(false);
  /** The rows as they were when a drag began, so a row emptied mid-drag (used only) stays put. */
  const [frozenRows, setFrozenRows] = useState<number[] | null>(null);
  const [view, setView] = useState({ w: 800, h: 400, left: 0, top: 0 });
  const [hotRow, setHotRow] = useState<number | null>(null);
  const [edgeHot, setEdgeHot] = useState<{ ids: string[]; edge: NoteEdge } | null>(null);
  const [readout, setReadout] = useState<{ x: number; y: number; text: string } | null>(null);
  const [peaks, setPeaks] = useState<{ id: string; data: [number, number][] } | null>(null);

  const shown = preview ?? notes;
  const totalTicks = pattern.lengthTicks;
  const beatW = BASE_BEAT_W * zoom;
  const pxPerTick = beatW / TICKS_PER_BEAT;
  const gridW = totalTicks * pxPerTick;
  /** The finest grid line drawn, which is what a dragged edge lands on. */
  const gridTicks = Math.min(noteLength, TICKS_PER_BEAT);

  const allRows = useMemo(() => (sampler ? laneChops(sampler, [], false) : []), [sampler]);
  const usedRows = useMemo(() => (sampler && usedOnly ? laneChops(sampler, shown, true) : null), [sampler, usedOnly, shown]);
  // Used only with nothing used yet would leave nowhere to place the first hit.
  const rows = frozenRows ?? (usedRows?.length ? usedRows : allRows);
  const rowOf = useMemo(() => new Map(rows.map((c, i) => [c, i])), [rows]);
  /** The shown rows' keys, bottom row first: the ladder a vertical drag climbs. */
  const rowPitches = useMemo(() => (sampler ? [...rows].reverse().map((c) => chopPitch(sampler, c)) : []), [rows, sampler]);
  const ROW_H = laneRowHeight(view.h - RULER_H, rows.length, compact ? { min: 10, max: 28 } : { min: 14, max: 44 });
  const gridH = rows.length * ROW_H;

  const secondsAt = useMemo(() => {
    const map = tempoMap(tempo, songMode ? tempoPoints : undefined);
    const base = songMode ? offset : 0;
    return (tick: number) => map.secondsAt(base + tick);
  }, [tempo, tempoPoints, songMode, offset]);
  const sounds = useMemo(() => (sampler ? hitSounds(sampler, shown, secondsAt) : []), [sampler, shown, secondsAt]);
  const soundOf = useMemo(() => new Map<string, HitSound>(sounds.map((s) => [s.id, s])), [sounds]);
  const ghostHits = useMemo(
    () => (ghostPattern && ghostPattern.id !== pattern.id ? tileGhost(ghostPattern.notes, ghostPattern.lengthTicks, totalTicks) : []),
    [ghostPattern, pattern.id, totalTicks],
  );

  const chopAtY = (y: number) => rows[Math.floor((y - RULER_H) / ROW_H)] ?? -1;
  /** Bottom-up row index under `y`, clamped to the rows so a drag past either end holds there. */
  const rowUpAt = (y: number) => rows.length - 1 - Math.min(rows.length - 1, Math.max(0, Math.floor((y - RULER_H) / ROW_H)));
  const pitchAtY = (y: number) => {
    const c = chopAtY(y);
    return sampler && c >= 0 ? chopPitch(sampler, c) : NaN;
  };
  const isGate = (n: NoteEvent) => {
    const c = sampler ? chopOfPitch(sampler, n.pitch) : -1;
    return c >= 0 && sampler!.chops[c].mode === "gate";
  };
  /**
   * A hit's body as drawn: from where it sounds to `bodyEnd` (a gate note's length; a
   * one-shot's length as far as the chop lasts). Muted hits make no sound and keep their length.
   */
  const boxOf = (n: NoteEvent): Box | null => {
    const c = sampler ? chopOfPitch(sampler, n.pitch) : -1;
    const row = rowOf.get(c);
    if (row === undefined) return null;
    const start = shiftedStart(n);
    const end = soundOf.get(n.id)?.bodyEnd ?? start + n.lengthTicks;
    return { x: start * pxPerTick, y: RULER_H + row * ROW_H, w: Math.max(3, (end - start) * pxPerTick - 1), h: ROW_H - 1 };
  };

  // ─── Edits ────────────────────────────────────────────────────────────────────────────────
  const commit = (next: NoteEvent[], label: string) => void dispatch({ type: "setNotes", patternId: pattern.id, notes: next }, false, label);
  /**
   * A drag sends each step transient, so the engine plays the hits where they are going and the
   * other editors follow, and its release folds them into one undo step. Sends are chained so
   * the gesture is closed only after its last step has landed.
   */
  const gesture = useRef<{ chain: Promise<void>; sent: NoteEvent[] | null }>({ chain: Promise.resolve(), sent: null });
  const send = (next: NoteEvent[], label: string) => {
    setPreview(next);
    const g = gesture.current;
    if (next === (g.sent ?? notes)) return;
    g.sent = next;
    const patternId = pattern.id;
    g.chain = g.chain.then(() => dispatch({ type: "setNotes", patternId, notes: next }, true, label));
  };
  const finish = () => {
    const g = gesture.current;
    const sent = g.sent;
    g.sent = null;
    g.chain = g.chain
      .then(() => (sent ? commitGesture() : undefined))
      .then(() => {
        // A new drag may have begun meanwhile; its preview is its own.
        if (!dragRef.current) setPreview(null);
      });
  };
  const hear = (pitch: number, lengthTicks?: number) => {
    if (!sampler) return;
    const c = chopOfPitch(sampler, pitch);
    if (c < 0) return;
    // A gate chop is held for the hit's length; a one-shot plays out whatever it is given.
    const held = lengthTicks !== undefined ? secondsAt(lengthTicks) - secondsAt(0) : chopSeconds(sampler.chops[c]);
    previewChop(track.id, pitch, c, Math.min(chopSeconds(sampler.chops[c]), held), false);
  };

  // Keep the selection to hits that exist, and mirror it to the store (the inspector reads it).
  useEffect(() => {
    setSelection((sel) => {
      const ids = new Set(shown.map((n) => n.id));
      const next = new Set([...sel].filter((id) => ids.has(id)));
      return next.size === sel.size ? sel : next;
    });
  }, [shown]);
  useEffect(() => setSelectedNoteIds([...selection]), [selection, setSelectedNoteIds]);
  useEffect(() => {
    if (selectionRequest) setSelection(new Set(selectionRequest.ids));
  }, [selectionRequest]);

  useEffect(() => {
    if (!sampler) return;
    let live = true;
    void sourcePeaks(sampler.sourceId).then((data) => live && setPeaks({ id: sampler.sourceId, data }));
    return () => {
      live = false;
    };
  }, [sampler?.sourceId]);

  // Viewport size and scroll, for the row height and the canvases drawn outside the scroller.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => setView((v) => ({ ...v, w: el.clientWidth, h: el.clientHeight }));
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, []);
  /**
   * The lanes open scrolled to the bottom, on chop 1, and stay there while the rows settle and
   * the panel resizes, until the user scrolls away. Another sampler opens at the bottom again.
   */
  const pinned = useRef<{ track: string | null; bottom: boolean }>({ track: null, bottom: true });

  // Follow: page the view when the playhead leaves it.
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || !follow || !playing || playhead === null) return;
    const px = playhead * pxPerTick;
    if (px < el.scrollLeft || px > el.scrollLeft + el.clientWidth - 24) el.scrollLeft = Math.max(0, px - 16);
  }, [playhead, follow, playing, pxPerTick]);

  // ─── Grid canvas ──────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = gridRef.current;
    if (!canvas) return;
    const dpr = pixelRatio;
    const W = gridW + TAIL_PX;
    const H = RULER_H + gridH;
    fit(canvas, W, H, dpr);
    const el = scrollRef.current;
    if (el) {
      if (pinned.current.track !== track.id) pinned.current = { track: track.id, bottom: true };
      if (pinned.current.bottom) el.scrollTop = el.scrollHeight;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = C.editor;
    ctx.fillRect(0, 0, W, H);

    // Rows: alternating, faintly washed in the chop's hue so a row reads as its chop.
    rows.forEach((c, i) => {
      const y = RULER_H + i * ROW_H;
      const plain = i % 2 ? C.rowB : C.rowA;
      ctx.fillStyle = mixHex(hueOf(c), plain, 0.95);
      ctx.fillRect(0, y, gridW, ROW_H);
      ctx.fillStyle = C.rowSep;
      ctx.fillRect(0, y + ROW_H - 1, gridW, 1);
    });

    // Vertical grid, the roll's: subdivision, beat, bar, and the swung sub-lines where they sound.
    for (let t = 0; t <= totalTicks + 1e-9; t += gridTicks) {
      const x = t * pxPerTick;
      const isBar = t % ticksPerBar === 0;
      const isBeat = t % TICKS_PER_BEAT === 0;
      ctx.fillStyle = isBar ? C.gridBar : isBeat ? C.gridBeat : C.gridSub;
      ctx.fillRect(Math.round(x), RULER_H, 1, gridH);
      if (swing > 50 && !isBeat) {
        const w = swingWarp(t, swing, swingGridTicks);
        if (Math.abs(w - t) > 1e-6) {
          ctx.fillStyle = "rgba(180,140,255,.22)";
          ctx.fillRect(Math.round(w * pxPerTick), RULER_H, 1, gridH);
        }
      }
    }

    // A ghost pattern as its rhythm: one faint line per hit, across every row.
    if (ghostHits.length) {
      ctx.fillStyle = ghostColor;
      ctx.globalAlpha = 0.16;
      for (const g of ghostHits) ctx.fillRect(Math.round(g.startTick * pxPerTick), RULER_H, 1, gridH);
      ctx.globalAlpha = 1;
    }

    // Tails under the bodies: the chop fading out, and a hard edge where a later hit cuts it.
    for (const s of sounds) {
      const row = rowOf.get(s.chop);
      if (row === undefined) continue;
      const y = RULER_H + row * ROW_H;
      const x0 = s.bodyEnd * pxPerTick;
      const x1 = Math.min(totalTicks, s.end) * pxPerTick;
      const hue = hueOf(s.chop);
      if (x1 > x0 + 0.5) {
        // Faded by how far the unbroken tail would have run, so a cut tail stops mid-fade.
        const fadeEnd = Math.max(x1, Math.min(totalTicks, s.natural) * pxPerTick);
        const grad = ctx.createLinearGradient(x0, 0, Math.max(x0 + 1, fadeEnd), 0);
        grad.addColorStop(0, hexA(hue, 0.42));
        grad.addColorStop(1, hexA(hue, 0.05));
        ctx.fillStyle = grad;
        ctx.fillRect(x0, y + 2, x1 - x0, ROW_H - 5);
        ctx.fillStyle = hexA(hue, 0.5);
        ctx.fillRect(x0, y + ROW_H / 2 - 0.5, x1 - x0, 1);
      }
      if (s.cutAt !== null && s.cutAt < totalTicks) {
        const cx = Math.round(s.cutAt * pxPerTick);
        ctx.fillStyle = hue;
        ctx.fillRect(cx - 1, y + 2, 2, ROW_H - 5);
      }
    }

    // Bodies
    for (const n of shown) {
      const b = boxOf(n);
      if (!b) continue;
      const c = chopOfPitch(sampler!, n.pitch);
      const hue = hueOf(c);
      const sel = selection.has(n.id);
      ctx.fillStyle = sel ? mixHex(hue, "#ffffff", 0.7) : hue;
      ctx.globalAlpha = n.muted ? 0.14 : sel ? 1 : 0.45 + 0.55 * (n.velocity / 127);
      roundRect(ctx, b.x, b.y, b.w, b.h, 2);
      ctx.fill();
      ctx.globalAlpha = 1;
      if (n.muted) {
        ctx.setLineDash([3, 2]);
        ctx.strokeStyle = sel ? "#ffffff" : hue;
        roundRect(ctx, b.x + 0.5, b.y + 0.5, b.w - 1, b.h - 1, 2);
        ctx.stroke();
        ctx.setLineDash([]);
        continue;
      }
      // Repeats: one divider per extra strike across the note's own length.
      const repeats = n.repeats ?? 0;
      const span = n.lengthTicks * pxPerTick;
      if (repeats > 0 && span / (repeats + 1) >= 3) {
        ctx.fillStyle = "rgba(0,0,0,.45)";
        for (let i = 1; i <= repeats; i++) {
          const rx = Math.round(b.x + (i * span) / (repeats + 1));
          if (rx < b.x + b.w) ctx.fillRect(rx, b.y, 1, b.h);
        }
      }
      if (n.velocity >= 118) {
        ctx.fillStyle = "#ff9d2a";
        ctx.fillRect(b.x, b.y, 3, b.h);
      }
      if (b.w > 14 && b.h >= 10) {
        ctx.fillStyle = "#0b0b0d";
        ctx.font = mono(700, Math.min(9, b.h - 3));
        ctx.fillText(String(c + 1), b.x + (n.velocity >= 118 ? 6 : 4), b.y + b.h / 2);
      }
      ctx.strokeStyle = sel ? "#ffffff" : mixHex(hue, "#000000", 0.38);
      ctx.lineWidth = 1;
      roundRect(ctx, b.x + 0.5, b.y + 0.5, b.w - 1, b.h - 1, 2);
      ctx.stroke();
    }

    if (edgeHot) {
      ctx.fillStyle = C.accent;
      for (const id of edgeHot.ids) {
        const n = shown.find((m) => m.id === id);
        const b = n && boxOf(n);
        if (b) ctx.fillRect(Math.round(edgeHot.edge === "start" ? b.x : b.x + b.w - 2), b.y, 2, b.h);
      }
    }

    const d = dragRef.current;
    if (d && d.kind === "marquee") {
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = "rgba(255,255,255,.4)";
      ctx.lineWidth = 1;
      ctx.strokeRect(Math.min(d.x0, d.x1), Math.min(d.y0, d.y1), Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
      ctx.setLineDash([]);
    }

    if (playhead !== null) {
      const px = Math.round(playhead * pxPerTick);
      if (!appearance.reducedMotion) {
        ctx.shadowColor = "rgba(255,255,255,.5)";
        ctx.shadowBlur = 6;
      }
      ctx.fillStyle = "#fff";
      ctx.fillRect(px, RULER_H, 1, gridH);
      ctx.shadowBlur = 0;
    }

    if (readout) {
      ctx.font = mono(500, 9);
      const tw = ctx.measureText(readout.text).width + 12;
      const rx = Math.max(2, Math.min(readout.x + 12, W - tw - 4));
      const ry = Math.max(view.top + RULER_H + 2, readout.y - 24);
      ctx.fillStyle = "rgba(22,22,26,.94)";
      ctx.fillRect(rx, ry, tw, 16);
      ctx.fillStyle = C.text1;
      ctx.fillText(readout.text, rx + 6, ry + 8);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shown, sounds, rows, rowOf, ROW_H, gridH, gridW, pxPerTick, totalTicks, gridTicks, ticksPerBar, swing, swingGridTicks, ghostHits, ghostColor, selection, edgeHot, readout, playhead, appearance, pixelRatio, view.top, sampler, track.id]);

  // ─── Ruler ────────────────────────────────────────────────────────────────────────────────
  const color = trackColor(track, trackIdx);
  useEffect(() => {
    const canvas = rulerRef.current;
    if (!canvas) return;
    const dpr = pixelRatio;
    const W = gridW + TAIL_PX;
    fit(canvas, W, RULER_H, dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = C.ruler;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = C.line1;
    ctx.fillRect(0, RULER_H - 1, W, 1);
    // The loop brace is PATTERN's: on SAMPLE the song's own brace is what loops, so the strip
    // draws none rather than one that would do nothing.
    if (!songMode) {
      ctx.globalAlpha = looping ? 0.8 : 0.25;
      ctx.fillStyle = color;
      ctx.fillRect(0, 2, gridW, 6);
      if (loopRegion) {
        ctx.fillStyle = C.accent;
        ctx.fillRect(loopRegion[0] * pxPerTick, 2, (loopRegion[1] - loopRegion[0]) * pxPerTick, 6);
      }
      ctx.globalAlpha = 1;
    }
    for (let tk = 0; tk < totalTicks; tk += TICKS_PER_BEAT) {
      const x = tk * pxPerTick;
      const beat = ticksToBeats(tk);
      const barIdx = Math.floor(beat / bpb);
      const isBar = tk % ticksPerBar === 0;
      if (!isBar && beatW < 40) continue;
      ctx.fillStyle = isBar ? "#5a5a62" : "#2e2e36";
      ctx.fillRect(Math.round(x), 10, 1, 18);
      ctx.fillStyle = isBar ? C.text1 : C.text4;
      ctx.font = isBar ? mono(600, 11) : mono(400, 9);
      ctx.fillText(isBar ? `${barIdx + 1}` : `${barIdx + 1}.${Math.floor(beat % bpb) + 1}`, x + 4, 19);
    }
    if (playhead !== null) {
      const px = Math.round(playhead * pxPerTick);
      ctx.fillStyle = "#fff";
      ctx.beginPath();
      ctx.moveTo(px - 5, 8);
      ctx.lineTo(px + 5, 8);
      ctx.lineTo(px, 15);
      ctx.closePath();
      ctx.fill();
    }
  }, [gridW, pxPerTick, totalTicks, beatW, bpb, ticksPerBar, loopRegion, looping, color, playhead, pixelRatio, songMode]);

  // ─── Row headers ──────────────────────────────────────────────────────────────────────────
  // Their own canvas beside the scroller, so they stay put as the lanes scroll sideways; drawn
  // at the scroller's vertical offset.
  const headH = Math.max(0, view.h - RULER_H);
  useEffect(() => {
    const canvas = headRef.current;
    if (!canvas || !sampler) return;
    const dpr = pixelRatio;
    fit(canvas, HEAD_W, headH, dpr);
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = C.head;
    ctx.fillRect(0, 0, HEAD_W, headH);
    const waveW = compact ? WAVE_W_COMPACT : WAVE_W;
    const wave = peaks?.id === sampler.sourceId ? peaks.data : null;
    rows.forEach((c, i) => {
      const y = i * ROW_H - view.top;
      if (y + ROW_H < 0 || y > headH) return;
      const chop = sampler.chops[c];
      const hue = hueOf(c);
      ctx.fillStyle = c === hotRow ? C.headHot : i % 2 ? "#111114" : C.head;
      ctx.fillRect(0, y, HEAD_W, ROW_H);
      ctx.fillStyle = hue;
      ctx.fillRect(0, y, 3, ROW_H - 1);
      const mid = y + ROW_H / 2;
      ctx.font = mono(700, Math.min(10, ROW_H - 3));
      const num = String(c + 1);
      ctx.fillText(num, 8, mid);
      const numW = ctx.measureText(num).width;
      const waveX = HEAD_W - waveW - 6;
      if (!compact && ROW_H >= 12) {
        // The name, else the mode, cut to fit before the waveform.
        ctx.font = mono(400, Math.min(9, ROW_H - 4));
        ctx.fillStyle = chop.name ? C.text2 : C.text4;
        const room = waveX - (14 + numW) - 6;
        ctx.fillText(clipText(ctx, chop.name ?? (chop.mode === "gate" ? "gate" : "one-shot"), room), 14 + numW, mid);
      }
      if (wave && ROW_H >= 8) {
        const cols = spanPeaks(wave, duration, chop.startSeconds, chop.endSeconds, waveW, chop.reverse);
        const half = (ROW_H - 5) / 2;
        ctx.fillStyle = hue;
        ctx.globalAlpha = 0.75;
        cols.forEach(([lo, hi], x) => {
          const top = mid - hi * half;
          ctx.fillRect(waveX + x, top, 1, Math.max(1, mid - lo * half - top));
        });
        ctx.globalAlpha = 1;
      }
      ctx.fillStyle = C.rowSep;
      ctx.fillRect(0, y + ROW_H - 1, HEAD_W, 1);
    });
    ctx.fillStyle = C.line1;
    ctx.fillRect(HEAD_W - 1, 0, 1, headH);
  }, [rows, ROW_H, view.top, headH, HEAD_W, compact, sampler, peaks, duration, hotRow, pixelRatio]);

  // ─── Property lane ────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = velRef.current;
    if (!canvas || !sampler) return;
    const dpr = pixelRatio;
    const W = HEAD_W + view.w;
    const H = VEL_H;
    fit(canvas, W, H, dpr);
    const spec = NOTE_PROPS[laneProp];
    const floor = laneFloor(laneProp);
    const middle = (floor + spec.max) / 2;
    const ruleY = spec.bipolar ? laneY(laneProp, middle, H) : H / 2;
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = "#0c0c0f";
    ctx.fillRect(0, 0, W, H);
    ctx.save();
    ctx.translate(HEAD_W - view.left, 0);
    for (let tk = 0; tk <= totalTicks; tk += TICKS_PER_BEAT) {
      ctx.fillStyle = tk % ticksPerBar === 0 ? "#2a2a32" : "#18181d";
      ctx.fillRect(Math.round(tk * pxPerTick), 0, 1, H);
    }
    ctx.fillStyle = spec.bipolar ? "#32323c" : "#1c1c22";
    ctx.fillRect(0, ruleY, gridW, 1);
    for (const n of shown) {
      const c = chopOfPitch(sampler, n.pitch);
      if (c < 0) continue;
      const x = n.startTick * pxPerTick + 2;
      const yv = laneY(laneProp, noteProp(n, laneProp), H);
      const root = spec.bipolar ? ruleY : laneBot(H);
      ctx.fillStyle = selection.has(n.id) ? "#f0f0f4" : hueOf(c);
      ctx.fillRect(x, Math.min(yv, root), 3, Math.max(1, Math.abs(root - yv)));
      ctx.fillRect(x - 2, yv - 2, 7, 4);
    }
    if (playhead !== null) {
      ctx.fillStyle = "rgba(255,255,255,.6)";
      ctx.fillRect(Math.round(playhead * pxPerTick), 0, 1, H);
    }
    ctx.restore();
    ctx.fillStyle = "#101013";
    ctx.fillRect(0, 0, HEAD_W, H);
    ctx.fillStyle = C.line1;
    ctx.fillRect(HEAD_W - 1, 0, 1, H);
    ctx.fillStyle = "#4a4a52";
    ctx.font = mono(400, 8.5);
    for (const [text, ty] of [
      [spec.format(spec.max), 8],
      [spec.format(middle), ruleY],
      [spec.format(floor), H - 8],
    ] as const) {
      ctx.fillText(text, HEAD_W - 8 - ctx.measureText(text).width, ty);
    }
  }, [shown, sampler, selection, laneProp, view.w, view.left, HEAD_W, VEL_H, totalTicks, ticksPerBar, pxPerTick, gridW, playhead, pixelRatio]);

  // ─── Interaction ──────────────────────────────────────────────────────────────────────────
  const local = (e: { clientX: number; clientY: number }, el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const inRuler = (y: number) => y - view.top < RULER_H;

  /** The hit under the pointer, and which edge handle when it is a gate hit's. */
  const hitTest = (x: number, y: number, among: NoteEvent[] = shown): { note: NoteEvent; edge: NoteEdge | null } | null => {
    if (inRuler(y)) return null;
    const c = chopAtY(y);
    if (c < 0) return null;
    let near: { note: NoteEvent; edge: NoteEdge; dist: number } | null = null;
    for (let i = among.length - 1; i >= 0; i--) {
      const n = among[i];
      if (chopOfPitch(sampler!, n.pitch) !== c) continue;
      const b = boxOf(n);
      if (!b) continue;
      const gate = isGate(n);
      const right = b.x + b.w;
      if (x >= b.x && x < right) {
        const inner = Math.min(EDGE_PX, b.w / 4);
        return { note: n, edge: gate ? (x < b.x + inner ? "start" : x >= right - inner ? "end" : null) : null };
      }
      if (!gate) continue;
      const edge: NoteEdge = x < b.x ? "start" : "end";
      const dist = edge === "start" ? b.x - x : x - right;
      if (dist <= EDGE_OUT && (!near || dist < near.dist)) near = { note: n, edge, dist };
    }
    return near && { note: near.note, edge: near.edge };
  };

  const positionLabel = (tick: number) =>
    `${Math.floor(tick / ticksPerBar) + 1}.${Math.floor((tick % ticksPerBar) / TICKS_PER_BEAT) + 1}.${Math.floor((tick % TICKS_PER_BEAT) / (TICKS_PER_BEAT / 4)) + 1}`;
  const chopLabel = (pitch: number) => {
    const c = chopOfPitch(sampler!, pitch);
    const name = sampler!.chops[c]?.name;
    return `chop ${c + 1}${name ? ` ${name}` : ""}`;
  };
  const beginDrag = (d: Drag) => {
    dragRef.current = d;
    setFrozenRows(rows);
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (!sampler) return;
    const canvas = gridRef.current!;
    scrollRef.current?.focus();
    const { x, y } = local(e, canvas);
    if (inRuler(y)) {
      // On SAMPLE the brace is the song's (see the ruler), so the whole ruler seeks there.
      if (e.button === 2) {
        if (!songMode) setLoopRegion(null);
        return;
      }
      if (!songMode && y - view.top < 10) {
        canvas.setPointerCapture(e.pointerId);
        loopDrag.current = { anchor: Math.max(0, Math.min(totalTicks, Math.round(x / pxPerTick / TICKS_PER_BEAT) * TICKS_PER_BEAT)) };
        return;
      }
      const at = Math.max(0, Math.min(totalTicks, x / pxPerTick));
      if (!songMode) {
        seek(at);
        return;
      }
      // The song plays here, so the click goes to where the song plays this tick of the pattern.
      const songTick = songTickOf(clips ?? [], pattern, at, songPlayhead);
      if (songTick === null) showToast(`${pattern.name} is not in the song there — place it on SONG to hear it`, true);
      else seek(songTick);
      return;
    }
    const tick = x / pxPerTick;
    const pitch = pitchAtY(y);
    const hit = e.button === 0 ? hitTest(x, y) : null;
    if (!hit && (tick < 0 || tick >= totalTicks || Number.isNaN(pitch))) return;
    canvas.setPointerCapture(e.pointerId);

    if (e.button === 2) {
      const n = hitTest(x, y)?.note;
      const base = n ? removeNotes(shown, new Set([n.id])) : shown;
      beginDrag({ kind: "erase", base });
      if (n) send(base, "erase hits");
      return;
    }
    if (e.button !== 0) return;

    if (hit) {
      if (e.shiftKey) {
        setSelection((sel) => {
          const next = new Set(sel);
          if (next.has(hit.note.id)) next.delete(hit.note.id);
          else next.add(hit.note.id);
          return next;
        });
        return;
      }
      let ids = selection.has(hit.note.id) ? new Set(selection) : new Set([hit.note.id]);
      let base = shown;
      let anchor = hit.note;
      let origin: "duplicate" | undefined;
      if (e.altKey && !hit.edge) {
        // The copies only appear (and take the selection) once the drag goes live.
        origin = "duplicate";
        const originals = shown.filter((n) => ids.has(n.id));
        const copies = copyNotes(originals);
        base = [...shown, ...copies];
        ids = new Set(copies.map((c) => c.id));
        anchor = copies[originals.indexOf(hit.note)] ?? copies[0];
      } else {
        setSelection(ids);
      }
      if (hit.edge) {
        // Only gate hits have a length worth dragging; one-shots in the selection keep theirs.
        const gates = new Set([...ids].filter((id) => {
          const n = shown.find((m) => m.id === id);
          return n ? isGate(n) : false;
        }));
        beginDrag({ kind: "resize", ids: gates, anchor, edge: hit.edge, startX: x, startY: y, base, hitId: hit.note.id, live: false });
        setEdgeHot({ ids: [...gates], edge: hit.edge });
        canvas.style.cursor = "ew-resize";
      } else {
        beginDrag({ kind: "move", ids, anchor, startX: x, startY: y, startRow: rowUpAt(y), base, lastPitch: anchor.pitch, origin, hitId: hit.note.id, live: false });
        setEdgeHot(null);
        canvas.style.cursor = origin === "duplicate" ? "copy" : "grabbing";
      }
      return;
    }

    if (e.shiftKey || e.metaKey || e.ctrlKey) {
      beginDrag({ kind: "marquee", x0: x, y0: y, x1: x, y1: y, baseSelection: new Set(e.shiftKey ? selection : []) });
      setPreview([...shown]);
      return;
    }
    if (brush || (e.altKey && editing.altKey === "paint")) {
      const placed = placeNote(shown, tick, pitch, noteLength, noteLength, editing.defaultVelocity);
      const base = placed ? [...shown, placed] : shown;
      beginDrag({ kind: "paint", base });
      if (placed) {
        if (editing.auditionOnPlace) hear(pitch, placed.lengthTicks);
        send(base, "paint hits");
      }
      return;
    }
    const snapGrid = e.altKey && editing.altKey === "noSnap" ? FINE_TICKS : noteLength;
    const placed = placeNote(shown, tick, pitch, noteLength, snapGrid, editing.defaultVelocity);
    if (!placed) {
      setSelection(new Set());
      return;
    }
    if (editing.auditionOnPlace) hear(pitch, placed.lengthTicks);
    const base = [...shown, placed];
    const ids = new Set([placed.id]);
    setSelection(ids);
    beginDrag({ kind: "move", ids, anchor: placed, startX: x, startY: y, startRow: rowUpAt(y), base, lastPitch: pitch, origin: "place", hitId: placed.id, live: false });
    canvas.style.cursor = "grabbing";
    send(base, "place hit");
  };

  const refreshHover = (x: number, y: number, alt: boolean) => {
    const canvas = gridRef.current;
    if (!canvas) return;
    const hit = hitTest(x, y);
    const edge = hit?.edge ? { ids: [hit.note.id], edge: hit.edge } : null;
    setEdgeHot((prev) => (prev?.edge === edge?.edge && prev?.ids[0] === edge?.ids[0] ? prev : edge));
    const row = inRuler(y) ? -1 : chopAtY(y);
    setHotRow(row >= 0 ? row : null);
    canvas.style.cursor = edge ? "ew-resize" : hit ? (alt ? "copy" : "grab") : "";
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const canvas = gridRef.current!;
    const { x, y } = local(e, canvas);
    if (loopDrag.current) {
      const tk = Math.max(0, Math.min(totalTicks, Math.round(x / pxPerTick / TICKS_PER_BEAT) * TICKS_PER_BEAT));
      const a = loopDrag.current.anchor;
      if (tk !== a) setLoopRegion([Math.min(a, tk), Math.max(a, tk)]);
      return;
    }
    const d = dragRef.current;
    if (!d) {
      refreshHover(x, y, e.altKey);
      return;
    }
    if ((d.kind === "move" || d.kind === "resize") && !d.live) {
      if (Math.hypot(x - d.startX, y - d.startY) < DEAD_PX) return;
      d.live = true;
      if (d.kind === "move" && d.origin === "duplicate") setSelection(d.ids);
    }
    const el = scrollRef.current;
    if (el && d.kind !== "lane" && d.kind !== "ramp") {
      const rect = el.getBoundingClientRect();
      if (e.clientX > rect.right - 24) el.scrollLeft += 12;
      else if (e.clientX < rect.left + 12) el.scrollLeft = Math.max(0, el.scrollLeft - 12);
      if (e.clientY > rect.bottom - 24) el.scrollTop += 10;
      else if (e.clientY < rect.top + RULER_H + 12) el.scrollTop = Math.max(0, el.scrollTop - 10);
    }
    const snapGrid = e.altKey && editing.altKey === "noSnap" ? FINE_TICKS : noteLength;
    switch (d.kind) {
      case "move": {
        const newStart = snapRound(d.anchor.startTick + (x - d.startX) / pxPerTick, snapGrid);
        let dTicks = newStart - d.anchor.startTick;
        let dRows = rowUpAt(y) - d.startRow;
        if (e.shiftKey) {
          d.axis ??= Math.abs(x - d.startX) >= Math.abs(y - d.startY) ? "x" : "y";
          if (d.axis === "x") dRows = 0;
          else dTicks = 0;
        } else {
          d.axis = null;
        }
        const next = moveHits(d.base, d.ids, dTicks, dRows, rowPitches, totalTicks);
        const moved = next.find((n) => n.id === d.anchor.id);
        if (moved && moved.pitch !== d.lastPitch) {
          // Landing on another chop plays it, so the swap is heard before it is let go.
          d.lastPitch = moved.pitch;
          hear(moved.pitch, moved.lengthTicks);
        }
        if (moved) setReadout({ x, y, text: `${chopLabel(moved.pitch)} · ${positionLabel(moved.startTick)}` });
        send(next, d.origin === "place" ? "place hit" : d.origin === "duplicate" ? "duplicate hits" : "move hits");
        break;
      }
      case "resize": {
        if (!d.ids.size) break;
        const grid = e.altKey && editing.altKey === "noSnap" ? FINE_TICKS : gridTicks;
        const at = d.edge === "end" ? d.anchor.startTick + d.anchor.lengthTicks : d.anchor.startTick;
        const to = snapRound(at + (x - d.startX) / pxPerTick, grid);
        const next = resizeEdge(d.base, d.ids, d.edge, to - at, grid);
        const sized = next.find((n) => n.id === d.anchor.id);
        if (sized) setReadout({ x, y, text: noteValueLabel(sized.lengthTicks) });
        send(next, "resize hits");
        break;
      }
      case "paint": {
        const tick = x / pxPerTick;
        const pitch = pitchAtY(y);
        if (tick < 0 || tick >= totalTicks || Number.isNaN(pitch)) return;
        const placed = placeNote(d.base, tick, pitch, noteLength, noteLength, editing.defaultVelocity);
        if (placed) {
          d.base = [...d.base, placed];
          if (editing.auditionOnPlace) hear(pitch, placed.lengthTicks);
          send(d.base, "paint hits");
        }
        break;
      }
      case "erase": {
        const n = hitTest(x, y, d.base)?.note;
        if (n) {
          d.base = removeNotes(d.base, new Set([n.id]));
          send(d.base, "erase hits");
        }
        break;
      }
      case "marquee": {
        d.x1 = x;
        d.y1 = y;
        const p0 = pitchAtY(Math.max(RULER_H, Math.min(RULER_H + gridH - 1, d.y0)));
        const p1 = pitchAtY(Math.max(RULER_H, Math.min(RULER_H + gridH - 1, d.y1)));
        const ids = notesInRect(notes, d.x0 / pxPerTick, d.x1 / pxPerTick, p0, p1);
        setSelection(new Set([...d.baseSelection, ...ids]));
        setPreview([...shown]);
        break;
      }
      case "lane":
      case "ramp":
        break;
    }
  };

  const endDrag = () => {
    if (loopDrag.current) {
      loopDrag.current = null;
      return;
    }
    const d = dragRef.current;
    dragRef.current = null;
    setFrozenRows(null);
    if (!d) return;
    setReadout(null);
    setEdgeHot(null);
    // A press on a hit that never went live was a click: it selects just that hit.
    if ((d.kind === "move" || d.kind === "resize") && !d.live && !(d.kind === "move" && d.origin === "place")) {
      setSelection(new Set([d.hitId]));
    }
    finish();
  };

  const onDoubleClickHead = (e: React.MouseEvent) => {
    const { y } = local(e, headRef.current!);
    const c = rows[Math.floor((y + view.top) / ROW_H)];
    if (c === undefined || !sampler) return;
    const pitch = chopPitch(sampler, c);
    setSelection(new Set(notes.filter((n) => n.pitch === pitch).map((n) => n.id)));
  };
  const onHeadDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || !sampler) return;
    const { y } = local(e, headRef.current!);
    const c = rows[Math.floor((y + view.top) / ROW_H)];
    if (c !== undefined) hear(chopPitch(sampler, c));
  };

  // ⌥-wheel moves the selection a chop up or down; ⌘-wheel zooms at the cursor; ⇧-wheel scrolls sideways.
  const wheelCommit = useRef<ReturnType<typeof setTimeout> | null>(null);
  const onWheel = (e: WheelEvent) => {
    const el = scrollRef.current;
    if (!el) return;
    if (e.altKey && selection.size) {
      e.preventDefault();
      const next = moveHitsByRow(shown, selection, rowPitches, e.deltaY < 0 ? 1 : -1);
      if (next !== shown) {
        send(next, "change chop");
        clearTimeout(wheelCommit.current ?? undefined);
        wheelCommit.current = setTimeout(finish, 400);
      }
      return;
    }
    if (e.ctrlKey || e.metaKey) {
      e.preventDefault();
      const rect = el.getBoundingClientRect();
      const cursorX = e.clientX - rect.left;
      const tickUnderCursor = (el.scrollLeft + cursorX) / pxPerTick;
      const nextZoom = Math.min(3, Math.max(0.25, zoom * (e.deltaY < 0 ? 1.1 : 0.9)));
      setZoom(nextZoom);
      const nextPxPerTick = (BASE_BEAT_W * nextZoom) / TICKS_PER_BEAT;
      requestAnimationFrame(() => {
        el.scrollLeft = Math.max(0, tickUnderCursor * nextPxPerTick - cursorX);
      });
      return;
    }
    if (e.shiftKey && Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
      e.preventDefault();
      el.scrollLeft += e.deltaY;
    }
  };
  const onWheelRef = useRef(onWheel);
  onWheelRef.current = onWheel;
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const handler = (e: WheelEvent) => onWheelRef.current(e);
    el.addEventListener("wheel", handler, { passive: false });
    return () => el.removeEventListener("wheel", handler);
  }, []);

  // Property lane: drag a stem (and every selected stem with it); ⌥-drag draws a ramp.
  const laneTick = (x: number) => (x - HEAD_W + view.left) / pxPerTick;
  const onLaneDown = (e: React.PointerEvent) => {
    const canvas = velRef.current!;
    const { x, y } = local(e, canvas);
    if (x < HEAD_W || e.button !== 0) return;
    const tick = laneTick(x);
    if (e.altKey) {
      canvas.setPointerCapture(e.pointerId);
      dragRef.current = { kind: "ramp", ids: new Set(selection), prop: laneProp, t0: tick, v0: quantizeNoteProp(laneProp, laneValue(laneProp, y, VEL_H)), base: shown };
      return;
    }
    let best: NoteEvent | null = null;
    let bestDx = Infinity;
    for (const n of shown) {
      const dx = Math.abs(n.startTick - tick) * pxPerTick;
      if (dx < bestDx && dx <= 8) {
        best = n;
        bestDx = dx;
      }
    }
    if (!best) return;
    canvas.setPointerCapture(e.pointerId);
    const ids = selection.has(best.id) ? new Set(selection) : new Set([best.id]);
    if (!selection.has(best.id)) setSelection(ids);
    dragRef.current = { kind: "lane", ids, prop: laneProp, anchorId: best.id, base: shown };
    applyLaneAt(y);
  };
  const applyLaneAt = (y: number) => {
    const d = dragRef.current;
    if (!d || d.kind !== "lane") return;
    const anchor = d.base.find((n) => n.id === d.anchorId);
    if (!anchor) return;
    const pinned = setNoteProp(d.base, new Set([d.anchorId]), d.prop, laneValue(d.prop, y, VEL_H));
    const delta = noteProp(pinned.find((n) => n.id === d.anchorId)!, d.prop) - noteProp(anchor, d.prop);
    const others = new Set([...d.ids].filter((id) => id !== d.anchorId));
    send(others.size ? nudgeNoteProp(pinned, others, d.prop, delta) : pinned, `set hit ${NOTE_PROPS[d.prop].label}`);
  };
  const onLaneMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    const { x, y } = local(e, velRef.current!);
    if (d.kind === "ramp") {
      send(rampAcross(d, laneTick(x), quantizeNoteProp(d.prop, laneValue(d.prop, y, VEL_H))), `ramp hit ${NOTE_PROPS[d.prop].label}`);
      return;
    }
    if (d.kind === "lane") applyLaneAt(y);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    const k = e.key;
    if (e.altKey && !k.startsWith("Arrow")) return;
    const bounds = selectionBounds(notes, selection);
    const grid = noteLength;
    if (k === "Backspace" || k === "Delete") {
      if (selection.size) commit(removeNotes(notes, selection), "delete hits");
    } else if (k === "Tab") {
      const id = stepSelection(notes, selection, e.shiftKey ? -1 : 1);
      if (id) {
        setSelection(new Set([id]));
        const n = notes.find((m) => m.id === id);
        if (n && editing.auditionOnPlace) hear(n.pitch, n.lengthTicks);
      }
    } else if (k === "0" && !mod) {
      if (selection.size) commit(toggleMute(notes, selection), "mute hits");
    } else if (mod && e.shiftKey && k.toLowerCase() === "e") {
      if (playhead === null) return;
      const next = splitNotes(notes, selection, playhead);
      if (next !== notes && next.length !== notes.length) commit(next, "split hits");
    } else if (mod && k.toLowerCase() === "j") {
      if (selection.size) commit(glueNotes(notes, selection), "glue hits");
    } else if (mod && k.toLowerCase() === "b") {
      if (!selection.size) return;
      const { notes: next, newIds } = duplicateBySpan(notes, selection, totalTicks);
      commit(next, "duplicate to next loop");
      setSelection(newIds);
    } else if (mod && k.toLowerCase() === "a") {
      setSelection(new Set(notes.map((n) => n.id)));
    } else if (mod && k.toLowerCase() === "c") {
      setClipboard(notes.filter((n) => selection.has(n.id)));
    } else if (mod && k.toLowerCase() === "x") {
      setClipboard(notes.filter((n) => selection.has(n.id)));
      if (selection.size) commit(removeNotes(notes, selection), "cut hits");
    } else if (mod && k.toLowerCase() === "v") {
      if (!clipboard.length) return;
      const at = bounds ? snapFloor(bounds.end, grid) : snapFloor(playhead ?? 0, grid);
      const { notes: next, newIds } = pasteNotes(notes, clipboard, at);
      commit(next, "paste hits");
      setSelection(newIds);
    } else if (mod && k.toLowerCase() === "d") {
      if (!selection.size) return;
      const { notes: next, newIds } = duplicateNotes(notes, selection, grid);
      commit(next, "duplicate hits");
      setSelection(newIds);
    } else if (mod && (k === "=" || k === "+")) {
      setZoom(zoom * 1.25);
    } else if (mod && k === "-") {
      setZoom(zoom / 1.25);
    } else if (k === "Escape") {
      setSelection(new Set());
    } else if (k.startsWith("Arrow") && selection.size) {
      if (k === "ArrowUp" || k === "ArrowDown") {
        const next = moveHitsByRow(notes, selection, rowPitches, k === "ArrowUp" ? 1 : -1);
        if (next !== notes) commit(next, "change chop");
      } else {
        const step = e.altKey ? FINE_TICKS : e.shiftKey ? ticksPerBar : grid;
        commit(moveHits(notes, selection, k === "ArrowLeft" ? -step : step, 0, rowPitches, totalTicks), "nudge hits");
      }
    } else if ((k === "[" || k === "]") && selection.size) {
      const step = NUDGE_STEP[laneProp] ?? 1;
      commit(nudgeNoteProp(notes, selection, laneProp, k === "]" ? step : -step), `change ${NOTE_PROPS[laneProp].label}`);
    } else {
      return;
    }
    e.preventDefault();
    e.stopPropagation();
  };

  if (!sampler) return null;

  const laneSpec = NOTE_PROPS[laneProp];
  const laneVals = (selection.size ? notes.filter((n) => selection.has(n.id)) : notes).map((n) => noteProp(n, laneProp));
  const laneAvg = laneVals.length ? laneSpec.format(laneVals.reduce((a, b) => a + b, 0) / laneVals.length) : "";
  const laneSummary = selection.size
    ? laneVals.length
      ? `sel ${laneSpec.format(Math.min(...laneVals))}–${laneSpec.format(Math.max(...laneVals))} · avg ${laneAvg}`
      : "—"
    : notes.length
      ? `${notes.length} hits · avg ${laneAvg}`
      : "—";

  return (
    <div className={`rollwrap choplanes${compact ? " compact" : ""}`}>
      <div className="cl-main">
        <div className="cl-heads" style={{ width: HEAD_W }}>
          <div className="cl-corner" style={{ height: RULER_H }}>
            <span className="cl-grid" title="the grid hits snap to">{gridLabel(noteLength)}</span>
            <button
              className={`chip tiny${usedOnly ? " on" : ""}`}
              onClick={() => setUsedOnly((v) => !v)}
              aria-pressed={usedOnly}
              title="show only the chops this pattern plays (all of them while it plays none)"
            >
              used
            </button>
          </div>
          <div className="cl-headrows">
            <canvas
              ref={headRef}
              onPointerDown={onHeadDown}
              onDoubleClick={onDoubleClickHead}
              onPointerLeave={() => setHotRow(null)}
              onWheel={(e) => {
                if (scrollRef.current) scrollRef.current.scrollTop += e.deltaY;
              }}
              title="click: hear the chop · double-click: select its hits"
            />
          </div>
        </div>
        <div
          className="rollscroll chopscroll"
          ref={scrollRef}
          tabIndex={0}
          onKeyDown={onKeyDown}
          onScroll={(e) => {
            const el = e.currentTarget;
            pinned.current.bottom = el.scrollTop + el.clientHeight >= el.scrollHeight - 2;
            setView((v) => (v.left === el.scrollLeft && v.top === el.scrollTop ? v : { ...v, left: el.scrollLeft, top: el.scrollTop }));
          }}
        >
          <canvas className="rollruler" ref={rulerRef} style={{ marginBottom: -RULER_H }} />
          <canvas
            ref={gridRef}
            onPointerDown={onPointerDown}
            onPointerMove={onPointerMove}
            onPointerUp={(e) => {
              endDrag();
              const { x, y } = local(e, e.currentTarget);
              refreshHover(x, y, e.altKey);
            }}
            onPointerCancel={endDrag}
            onPointerLeave={() => {
              if (dragRef.current) return;
              setHotRow(null);
              setEdgeHot(null);
            }}
            onContextMenu={(e) => e.preventDefault()}
          />
        </div>
      </div>
      {sampler.chops.length === 0 ? (
        <div className="emptyhint" aria-hidden="true">
          no chops yet: cut some on SAMPLE
        </div>
      ) : (
        notes.length === 0 &&
        !compact && (
          <div className="emptyhint" aria-hidden="true">
            click a chop's row to place a hit
            <br />
            drag a hit up or down to hand it to another chop
          </div>
        )
      )}
      {!compact && (
        <>
          <div className="lanehdr">
            <select className="lanepick" value={laneProp} onChange={(e) => setLaneProp(e.target.value as NoteProp)} aria-label="lane property">
              {LANE_PROPS.map((p) => (
                <option key={p} value={p}>
                  {NOTE_PROPS[p].label}
                </option>
              ))}
            </select>
            <span className="sum">{laneSummary}</span>
          </div>
          <div className="vellane" style={{ height: VEL_H }}>
            <canvas ref={velRef} onPointerDown={onLaneDown} onPointerMove={onLaneMove} onPointerUp={endDrag} onPointerCancel={endDrag} onContextMenu={(e) => e.preventDefault()} />
          </div>
        </>
      )}
    </div>
  );
}

/** Size a canvas for `w`×`h` CSS pixels at `dpr`, touching it only when that changes. */
function fit(canvas: HTMLCanvasElement, w: number, h: number, dpr: number) {
  if (canvas.width === Math.round(w * dpr) && canvas.height === Math.round(h * dpr)) return;
  canvas.width = Math.round(w * dpr);
  canvas.height = Math.round(h * dpr);
  canvas.style.width = `${w}px`;
  canvas.style.height = `${h}px`;
}

/** `#rrggbb` at an alpha, for the tails' gradients. */
function hexA(hex: string, a: number): string {
  const v = parseInt(hex.slice(1), 16);
  return `rgba(${(v >> 16) & 255},${(v >> 8) & 255},${v & 255},${a})`;
}

/** `text` cut with an ellipsis to fit `room` pixels in the context's current font. */
function clipText(ctx: CanvasRenderingContext2D, text: string, room: number): string {
  if (room <= 0) return "";
  if (ctx.measureText(text).width <= room) return text;
  let s = text;
  while (s.length > 1 && ctx.measureText(`${s}…`).width > room) s = s.slice(0, -1);
  return `${s}…`;
}

/** The ⌥-ramp across the lane: the roll's, from (`t0`, `v0`) to (`t1`, `v1`). */
function rampAcross(d: Extract<Drag, { kind: "ramp" }>, t1: number, v1: number): NoteEvent[] {
  const [from, to] = d.t0 <= t1 ? [d.t0, t1] : [t1, d.t0];
  const [vf, vt] = d.t0 <= t1 ? [d.v0, v1] : [v1, d.v0];
  const pool = d.ids.size ? d.base.filter((n) => d.ids.has(n.id)) : d.base;
  const touched = pool.filter((n) => n.startTick >= from - 1e-9 && n.startTick <= to + 1e-9);
  if (!touched.length) return d.base;
  const slope = (vt - vf) / Math.max(1e-9, to - from);
  const starts = touched.map((n) => n.startTick);
  const lo = Math.min(...starts);
  const hi = Math.max(...starts);
  return rampNoteProp(d.base, new Set(touched.map((n) => n.id)), d.prop, vf + slope * (lo - from), vf + slope * (hi - from));
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  const rr = Math.min(r, w / 2, h / 2);
  ctx.beginPath();
  ctx.moveTo(x + rr, y);
  ctx.lineTo(x + w - rr, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + rr);
  ctx.lineTo(x + w, y + h - rr);
  ctx.quadraticCurveTo(x + w, y + h, x + w - rr, y + h);
  ctx.lineTo(x + rr, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - rr);
  ctx.lineTo(x, y + rr);
  ctx.quadraticCurveTo(x, y, x + rr, y);
  ctx.closePath();
}
