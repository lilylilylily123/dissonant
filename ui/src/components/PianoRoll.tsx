import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { selectedPattern, selectedTrack, selectedTrackIndex, useStore } from "../store";
import { midiName, tierMap } from "../theory";
import {
  adjustVelocity,
  duplicateNotes,
  moveNotes,
  noteAt,
  notesInRect,
  pasteNotes,
  placeNote,
  removeNotes,
  resizeNotes,
  selectionBounds,
  snapFloor,
  snapRound,
} from "../noteEditing";
import { TIER_COLORS, trackColor, uuid, type NoteEvent, type Tier } from "../types";
import { BASE_BEAT_W, GUTTER } from "./ChordLane";

// Geometry (from the handoff: 18px rows, 96px beats at 1×, 64px keyboard, 28px ruler)
const LOW = 24; // C1
const HIGH = 96; // C7
const ROW_H = 18;
const RULER_H = 28;
const EDGE_PX = 7;
const VEL_H = 110;
const BLACK = new Set([1, 3, 6, 8, 10]);

const C = {
  editor: "#101013",
  ruler: "#121215",
  line1: "#26262d",
  rowWhite: "#17171b",
  rowBlack: "#121215",
  rowSep: "#0b0b0d",
  octave: "#2e2e36",
  gridBar: "#3a3a44",
  gridBeat: "#26262e",
  gridSub: "rgba(255,255,255,.035)",
  keyWhite: "#d4d4d9",
  keyBlack: "#0e0e10",
  keyBlackBase: "#c9c9ce",
  keySep: "#8a8a92",
  text1: "#e6e6ea",
  text4: "#6a6a74",
  text5: "#5f5f68",
  accent: "#b48cff",
  rootRow: "#261d3a",
  rootRowBlack: "#1f1830",
};

const NOTE_BORDER: Record<Tier, string> = { chordTone: "#1a9e6c", tension: "#b26a12", dissonance: "#a3261f" };
const NOTE_SEL: Record<Tier, string> = { chordTone: "#c9ffe9", tension: "#ffe0b8", dissonance: "#ffc9c5" };
const tierColor = (t: Tier | null) => (t ? TIER_COLORS[t] : "#9a9aa4");

type Drag =
  | { kind: "move"; ids: Set<string>; anchor: NoteEvent; startX: number; startY: number; base: NoteEvent[]; lastPitch: number }
  | { kind: "resize"; ids: Set<string>; anchor: NoteEvent; startX: number; base: NoteEvent[] }
  | { kind: "paint"; base: NoteEvent[] }
  | { kind: "erase"; base: NoteEvent[] }
  | { kind: "marquee"; x0: number; y0: number; x1: number; y1: number; baseSelection: Set<string> }
  | { kind: "velocity"; ids: Set<string>; anchorId: string; startY: number; base: NoteEvent[] };

let clipboard: NoteEvent[] = [];

/**
 * The piano roll. Modeless direct manipulation, FL-style:
 *  - click empty = place a note (and keep dragging to move it) · ⌥-drag empty = paint
 *  - drag a note = move (auditions pitch) · drag its right edge = resize · ⌥-drag = duplicate
 *  - shift-drag empty = marquee select · shift-click = toggle in selection
 *  - right-click / right-drag = erase · ruler click = seek · velocity lane: drag stems
 *  - keys: ⌫ delete · ⌘A/⌘C/⌘X/⌘V/⌘D · arrows nudge (⇧ = octave / bar) · [ ] velocity · esc
 * Tiering is visual only — any note can be placed in any tier.
 */
export function PianoRoll() {
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const trackIdx = useStore(selectedTrackIndex);
  const key = useStore((s) => s.snapshot!.model.key);
  const playhead = useStore((s) => s.playhead);
  const showLandscape = useStore((s) => s.showLandscape);
  const highlightRows = useStore((s) => s.highlightRows);
  const noteLength = useStore((s) => s.noteLength);
  const zoom = useStore((s) => s.zoom);
  const dispatch = useStore((s) => s.dispatch);
  const audition = useStore((s) => s.audition);
  const seek = useStore((s) => s.seek);
  const setZoom = useStore((s) => s.setZoom);
  const setSelectedNoteIds = useStore((s) => s.setSelectedNoteIds);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const velRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const dragRef = useRef<Drag | null>(null);
  const [preview, setPreview] = useState<NoteEvent[] | null>(null);
  const [hover, setHover] = useState<{ beat: number; pitch: number } | null>(null);
  const [scrollLeft, setScrollLeft] = useState(0);
  const [viewW, setViewW] = useState(800);

  const notes = useMemo(() => (pattern && track ? (pattern.notesByTrack[track.id] ?? []) : []), [pattern, track]);
  const chords = pattern?.chords.chords ?? [];
  const beats = pattern?.lengthBeats ?? 16;
  const beatW = BASE_BEAT_W * zoom;
  const rows = HIGH - LOW + 1;
  const gridW = beats * beatW;
  const gridH = rows * ROW_H;
  const shown = preview ?? notes;
  const color = track ? trackColor(track, trackIdx) : C.accent;
  const rootPc = key.rootPitchClass;

  const yForPitch = (p: number) => RULER_H + (HIGH - p) * ROW_H;
  const pitchForY = (y: number) => HIGH - Math.floor((y - RULER_H) / ROW_H);
  const beatForX = (x: number) => (x - GUTTER) / beatW;

  const commit = useCallback(
    (next: NoteEvent[]) => {
      if (pattern && track) void dispatch({ type: "setNotes", patternId: pattern.id, trackId: track.id, notes: next });
    },
    [dispatch, pattern, track],
  );

  // Keep the selection valid and mirror it to the store (inspector shows ranges).
  useEffect(() => {
    setSelection((sel) => {
      const ids = new Set(notes.map((n) => n.id));
      const next = new Set([...sel].filter((id) => ids.has(id)));
      return next.size === sel.size ? sel : next;
    });
  }, [notes]);
  useEffect(() => {
    setSelectedNoteIds([...selection]);
  }, [selection, setSelectedNoteIds]);

  // Center on C4 initially; track viewport width for the velocity lane.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = Math.max(0, yForPitch(60) - el.clientHeight / 2);
    const ro = new ResizeObserver(() => setViewW(el.clientWidth));
    ro.observe(el);
    setViewW(el.clientWidth);
    return () => ro.disconnect();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ─── Main canvas ──────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const W = GUTTER + gridW;
    const H = RULER_H + gridH;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    const mono = (w: number, px: number) => `${w} ${px}px "JetBrains Mono", ui-monospace, monospace`;

    ctx.fillStyle = C.editor;
    ctx.fillRect(0, 0, W, H);

    // Rows
    const live = tierMap(playhead, chords, key);
    for (let p = LOW; p <= HIGH; p++) {
      const y = yForPitch(p);
      const blk = BLACK.has(p % 12);
      let bg = blk ? C.rowBlack : C.rowWhite;
      if (highlightRows && !showLandscape) {
        const t = live[p % 12];
        if (rootPc !== null && p % 12 === rootPc && t !== "dissonance") bg = blk ? C.rootRowBlack : C.rootRow;
        else if (t === "chordTone") bg = blk ? "#111a17" : "#15221d";
        else if (t === "tension") bg = blk ? "#151411" : "#1a1814";
        else if (t === "dissonance") bg = blk ? "#0e0e10" : "#121214";
      }
      ctx.fillStyle = bg;
      ctx.fillRect(GUTTER, y, gridW, ROW_H);
      ctx.fillStyle = p % 12 === 0 ? C.octave : C.rowSep;
      ctx.fillRect(GUTTER, y + ROW_H - 1, gridW, 1);
    }

    // Harmonic landscape: every cell tinted by its fit against the chord at that beat
    if (showLandscape) {
      for (let b = 0; b < beats; b++) {
        const map = tierMap(b + 0.5, chords, key);
        for (let p = LOW; p <= HIGH; p++) {
          const t = map[p % 12];
          if (!t) continue;
          const x = GUTTER + b * beatW;
          const y = yForPitch(p);
          ctx.globalAlpha = t === "dissonance" ? 0.28 : t === "tension" ? 0.2 : 0.14;
          ctx.fillStyle = tierColor(t);
          ctx.fillRect(x, y, beatW, ROW_H - 1);
          ctx.globalAlpha = 1;
          if (t === "dissonance") {
            ctx.strokeStyle = "rgba(255,59,48,.45)";
            ctx.lineWidth = 0.8;
            ctx.beginPath();
            ctx.moveTo(x, y + ROW_H);
            ctx.lineTo(x + beatW, y);
            ctx.stroke();
          }
        }
      }
    }

    // Vertical grid: subdivision, beat, bar
    const sub = Math.min(noteLength, 1);
    for (let t = 0; t <= beats + 1e-9; t += sub) {
      const x = GUTTER + t * beatW;
      const isBar = Math.abs(t % 4) < 1e-9;
      const isBeat = Math.abs(t % 1) < 1e-9;
      ctx.fillStyle = isBar ? C.gridBar : isBeat ? C.gridBeat : C.gridSub;
      ctx.fillRect(Math.round(x), RULER_H, 1, gridH);
    }

    // Ruler: loop bar (the pattern loops), bar + beat labels
    ctx.fillStyle = C.ruler;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = C.line1;
    ctx.fillRect(0, RULER_H - 1, W, 1);
    ctx.globalAlpha = 0.8;
    ctx.fillStyle = color;
    ctx.fillRect(GUTTER, 2, gridW, 6);
    ctx.globalAlpha = 1;
    for (let b = 0; b < beats; b++) {
      const x = GUTTER + b * beatW;
      const isBar = b % 4 === 0;
      if (!isBar && beatW < 40) continue;
      ctx.fillStyle = isBar ? "#5a5a62" : "#2e2e36";
      ctx.fillRect(Math.round(x), 10, 1, 18);
      ctx.fillStyle = isBar ? C.text1 : C.text4;
      ctx.font = isBar ? mono(600, 11) : mono(400, 9);
      ctx.fillText(isBar ? `${b / 4 + 1}` : `${Math.floor(b / 4) + 1}.${(b % 4) + 1}`, x + 4, 19);
    }
    // Chord names in the ruler
    ctx.font = mono(500, 9);
    ctx.fillStyle = C.accent;
    for (const c of chords) {
      const x = GUTTER + c.startBeat * beatW;
      const label = c.name ?? "";
      const tw = ctx.measureText(label).width;
      ctx.fillText(label, x + c.lengthBeats * beatW - tw - 4, 19);
    }

    // Notes
    const velocityAlpha = (v: number) => 0.45 + 0.55 * (v / 127);
    for (const n of shown) {
      if (n.pitch < LOW || n.pitch > HIGH) continue;
      const t = tierMap(n.startBeat, chords, key)[n.pitch % 12];
      const sel = selection.has(n.id);
      const x = GUTTER + n.startBeat * beatW;
      const y = yForPitch(n.pitch);
      const w = Math.max(3, n.lengthBeats * beatW - 1);
      const h = ROW_H - 1;
      ctx.fillStyle = sel ? (t ? NOTE_SEL[t] : "#f0f0f4") : tierColor(t);
      ctx.globalAlpha = sel ? 1 : velocityAlpha(n.velocity);
      roundRect(ctx, x, y, w, h, 2);
      ctx.fill();
      ctx.globalAlpha = 1;
      // Non-color tier cues
      if (t === "tension") {
        ctx.fillStyle = "rgba(4,20,13,.85)";
        ctx.beginPath();
        ctx.arc(x + w - 6, y + h / 2, 2, 0, Math.PI * 2);
        ctx.fill();
      } else if (t === "dissonance") {
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, y, w, h);
        ctx.clip();
        ctx.strokeStyle = "rgba(0,0,0,.5)";
        ctx.lineWidth = 1.2;
        for (let sx = x - h; sx < x + w; sx += 5) {
          ctx.beginPath();
          ctx.moveTo(sx, y);
          ctx.lineTo(sx + h, y + h);
          ctx.stroke();
        }
        ctx.restore();
      }
      // Accent marker (vel ≥ 118): 3px bar on the left edge
      if (n.velocity >= 118) {
        ctx.fillStyle = "#ff9d2a";
        ctx.fillRect(x, y, 3, h);
      }
      // Label
      if (w > 22) {
        ctx.fillStyle = "#04140d";
        ctx.font = mono(600, 8);
        ctx.fillText(t === "dissonance" ? `${midiName(n.pitch)} !` : midiName(n.pitch), x + (n.velocity >= 118 ? 6 : 4), y + h / 2);
      }
      // Border
      ctx.strokeStyle = sel ? "#ffffff" : t ? NOTE_BORDER[t] : "#5f5f68";
      ctx.lineWidth = 1;
      roundRect(ctx, x + 0.5, y + 0.5, w - 1, h - 1, 2);
      ctx.stroke();
      if (sel) {
        ctx.strokeStyle = "rgba(255,255,255,.4)";
        roundRect(ctx, x - 0.5, y - 0.5, w + 1, h + 1, 3);
        ctx.stroke();
      }
    }

    // Marquee
    const d = dragRef.current;
    if (d && d.kind === "marquee") {
      ctx.setLineDash([3, 3]);
      ctx.strokeStyle = "rgba(255,255,255,.4)";
      ctx.lineWidth = 1;
      ctx.strokeRect(Math.min(d.x0, d.x1), Math.min(d.y0, d.y1), Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
      ctx.setLineDash([]);
    }

    // Keyboard
    for (let p = LOW; p <= HIGH; p++) {
      const y = yForPitch(p);
      const blk = BLACK.has(p % 12);
      ctx.fillStyle = blk ? C.keyBlackBase : C.keyWhite;
      ctx.fillRect(0, y, GUTTER, ROW_H);
      if (blk) {
        ctx.fillStyle = C.keyBlack;
        ctx.fillRect(0, y, 40, ROW_H);
      }
      ctx.fillStyle = C.keySep;
      ctx.fillRect(0, y + ROW_H - 1, GUTTER, 1);
      const isC = p % 12 === 0;
      const isRoot = rootPc !== null && p % 12 === rootPc;
      if (isC || isRoot) {
        ctx.font = mono(600, 8.5);
        ctx.fillStyle = isC ? "#2a2a31" : C.accent;
        const label = isC ? midiName(p) : midiName(p).replace(/\d+$/, "");
        ctx.fillText(label, GUTTER - 4 - ctx.measureText(label).width - 6, y + ROW_H / 2);
      }
      // Tier dot at the key edge (guidance cue, mirrors the rows)
      const t = live[p % 12];
      if (t) {
        ctx.fillStyle = tierColor(t);
        ctx.beginPath();
        ctx.arc(GUTTER - 5, y + ROW_H / 2, 2, 0, Math.PI * 2);
        ctx.fill();
      }
    }
    ctx.fillStyle = C.line1;
    ctx.fillRect(GUTTER - 1, RULER_H, 1, gridH);
    // Ruler corner
    ctx.fillStyle = C.ruler;
    ctx.fillRect(0, 0, GUTTER, RULER_H);
    ctx.fillStyle = C.line1;
    ctx.fillRect(GUTTER - 1, 0, 1, RULER_H);
    ctx.fillRect(0, RULER_H - 1, GUTTER, 1);
    ctx.fillStyle = C.text5;
    ctx.font = mono(400, 9);
    const gl = gridLabelFor(noteLength);
    ctx.fillText(gl, (GUTTER - ctx.measureText(gl).width) / 2, RULER_H / 2);

    // Playhead
    const px = Math.round(GUTTER + playhead * beatW);
    ctx.shadowColor = "rgba(255,255,255,.5)";
    ctx.shadowBlur = 6;
    ctx.fillStyle = "#fff";
    ctx.fillRect(px, RULER_H, 1, gridH);
    ctx.shadowBlur = 0;
    ctx.beginPath();
    ctx.moveTo(px - 5, 8);
    ctx.lineTo(px + 5, 8);
    ctx.lineTo(px, 15);
    ctx.closePath();
    ctx.fill();

    // Hover readout
    if (hover && !dragRef.current) {
      const t = tierMap(hover.beat, chords, key)[hover.pitch % 12];
      const label = `${midiName(hover.pitch)}${t ? ` · ${t === "chordTone" ? "chord tone" : t}` : ""}`;
      ctx.font = mono(500, 9);
      const tw = ctx.measureText(label).width + 10;
      const hx = GUTTER + hover.beat * beatW + 8;
      const hy = yForPitch(hover.pitch) - 16;
      ctx.fillStyle = "rgba(22,22,26,.92)";
      ctx.fillRect(hx, hy, tw, 14);
      ctx.fillStyle = t ? tierColor(t) : C.text1;
      ctx.fillText(label, hx + 5, hy + 7);
    }
  }, [shown, chords, key, playhead, showLandscape, highlightRows, noteLength, beatW, beats, gridW, gridH, selection, hover, preview, color, rootPc]);

  // ─── Velocity lane canvas ─────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = velRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const W = viewW;
    const H = VEL_H;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = "#0c0c0f";
    ctx.fillRect(0, 0, W, H);
    ctx.save();
    ctx.translate(-scrollLeft, 0);
    // grid
    for (let b = 0; b <= beats; b++) {
      ctx.fillStyle = b % 4 === 0 ? "#2a2a32" : "#18181d";
      ctx.fillRect(Math.round(GUTTER + b * beatW), 0, 1, H);
    }
    ctx.fillStyle = "#1c1c22";
    ctx.fillRect(GUTTER, H / 2, gridW, 1);
    // stems
    for (const n of shown) {
      const t = tierMap(n.startBeat, chords, key)[n.pitch % 12];
      const sel = selection.has(n.id);
      const x = GUTTER + n.startBeat * beatW + 2;
      const h = Math.round((n.velocity / 127) * (H - 14));
      ctx.fillStyle = sel ? "#e6fff4" : tierColor(t);
      ctx.fillRect(x, H - 4 - h, 3, h);
      ctx.fillRect(x - 2, H - 4 - h - 2, 7, 4);
    }
    // playhead
    ctx.fillStyle = "rgba(255,255,255,.6)";
    ctx.fillRect(Math.round(GUTTER + playhead * beatW), 0, 1, H);
    ctx.restore();
    // scale column (fixed)
    ctx.fillStyle = "#101013";
    ctx.fillRect(0, 0, GUTTER, H);
    ctx.fillStyle = C.line1;
    ctx.fillRect(GUTTER - 1, 0, 1, H);
    ctx.fillStyle = "#4a4a52";
    ctx.font = '400 8.5px "JetBrains Mono", ui-monospace, monospace';
    ctx.fillText("127", GUTTER - 8 - ctx.measureText("127").width, 8);
    ctx.fillText("64", GUTTER - 8 - ctx.measureText("64").width, H / 2);
    ctx.fillText("0", GUTTER - 8 - ctx.measureText("0").width, H - 8);
  }, [shown, chords, key, playhead, beatW, beats, gridW, selection, scrollLeft, viewW]);

  // ─── Interaction ──────────────────────────────────────────────────────────────────────────
  const local = (e: React.PointerEvent, el: HTMLElement) => {
    const rect = el.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  const hitTest = (x: number, y: number): { note: NoteEvent; edge: boolean } | null => {
    const n = noteAt(shown, beatForX(x), pitchForY(y));
    if (!n) return null;
    const rightX = GUTTER + (n.startBeat + n.lengthBeats) * beatW;
    const edgeZone = Math.min(EDGE_PX, (n.lengthBeats * beatW) / 3);
    return { note: n, edge: x >= rightX - edgeZone };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    const canvas = canvasRef.current!;
    scrollRef.current?.focus();
    const { x, y } = local(e, canvas);
    if (y < RULER_H) {
      if (x >= GUTTER) seek(Math.max(0, Math.min(beats, beatForX(x))));
      return;
    }
    if (x < GUTTER) {
      const pitch = pitchForY(y);
      if (pitch >= LOW && pitch <= HIGH) audition(pitch);
      return;
    }
    const beat = beatForX(x);
    const pitch = pitchForY(y);
    if (beat < 0 || beat >= beats || pitch < LOW || pitch > HIGH) return;
    canvas.setPointerCapture(e.pointerId);

    if (e.button === 2) {
      const n = noteAt(notes, beat, pitch);
      const base = n ? removeNotes(notes, new Set([n.id])) : notes;
      dragRef.current = { kind: "erase", base };
      setPreview(base);
      return;
    }
    if (e.button !== 0) return;

    const hit = hitTest(x, y);
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
      let base = notes;
      let anchor = hit.note;
      if (e.altKey && !hit.edge) {
        const originals = notes.filter((n) => ids.has(n.id));
        const copies = originals.map((o) => ({ ...o, id: uuid() }));
        base = [...notes, ...copies];
        ids = new Set(copies.map((c) => c.id));
        anchor = copies[originals.indexOf(hit.note)] ?? copies[0];
      }
      setSelection(ids);
      dragRef.current = hit.edge
        ? { kind: "resize", ids, anchor, startX: x, base }
        : { kind: "move", ids, anchor, startX: x, startY: y, base, lastPitch: anchor.pitch };
      setPreview(base);
      return;
    }

    if (e.shiftKey) {
      dragRef.current = { kind: "marquee", x0: x, y0: y, x1: x, y1: y, baseSelection: new Set(selection) };
      setPreview(notes);
      return;
    }
    if (e.altKey) {
      const placed = placeNote(notes, beat, pitch, noteLength, noteLength);
      const base = placed ? [...notes, placed] : notes;
      if (placed) audition(pitch, placed.velocity);
      dragRef.current = { kind: "paint", base };
      setPreview(base);
      return;
    }
    const placed = placeNote(notes, beat, pitch, noteLength, noteLength);
    if (!placed) {
      setSelection(new Set());
      return;
    }
    audition(pitch, placed.velocity);
    const base = [...notes, placed];
    const ids = new Set([placed.id]);
    setSelection(ids);
    dragRef.current = { kind: "move", ids, anchor: placed, startX: x, startY: y, base, lastPitch: pitch };
    setPreview(base);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const canvas = canvasRef.current!;
    const { x, y } = local(e, canvas);
    const d = dragRef.current;
    if (!d) {
      const beat = beatForX(x);
      const pitch = pitchForY(y);
      setHover(x >= GUTTER && y >= RULER_H && beat < beats && pitch >= LOW && pitch <= HIGH ? { beat, pitch } : null);
      return;
    }
    switch (d.kind) {
      case "move": {
        const rawDelta = (x - d.startX) / beatW;
        const newStart = snapRound(d.anchor.startBeat + rawDelta, noteLength);
        const dBeats = newStart - d.anchor.startBeat;
        const dPitch = pitchForY(y) - pitchForY(d.startY);
        const next = moveNotes(d.base, d.ids, dBeats, dPitch, { min: LOW, max: HIGH }, beats);
        const moved = next.find((n) => n.id === d.anchor.id);
        if (moved && moved.pitch !== d.lastPitch) {
          d.lastPitch = moved.pitch;
          audition(moved.pitch, moved.velocity);
        }
        setPreview(next);
        break;
      }
      case "resize": {
        const rawDelta = (x - d.startX) / beatW;
        const newLen = Math.max(noteLength, snapRound(d.anchor.lengthBeats + rawDelta, noteLength));
        setPreview(resizeNotes(d.base, d.ids, newLen - d.anchor.lengthBeats, noteLength));
        break;
      }
      case "paint": {
        const beat = beatForX(x);
        const pitch = pitchForY(y);
        if (beat < 0 || beat >= beats || pitch < LOW || pitch > HIGH) return;
        const placed = placeNote(d.base, beat, pitch, noteLength, noteLength);
        if (placed) {
          d.base = [...d.base, placed];
          audition(pitch, placed.velocity);
          setPreview(d.base);
        }
        break;
      }
      case "erase": {
        const n = noteAt(d.base, beatForX(x), pitchForY(y));
        if (n) {
          d.base = removeNotes(d.base, new Set([n.id]));
          setPreview(d.base);
        }
        break;
      }
      case "marquee": {
        d.x1 = x;
        d.y1 = y;
        const ids = notesInRect(notes, beatForX(d.x0), beatForX(d.x1), pitchForY(d.y0), pitchForY(d.y1));
        setSelection(new Set([...d.baseSelection, ...ids]));
        setPreview([...notes]);
        break;
      }
      case "velocity":
        break;
    }
  };

  const endDrag = () => {
    const d = dragRef.current;
    dragRef.current = null;
    if (!d) return;
    if (d.kind === "marquee") {
      setPreview(null);
      return;
    }
    const result = preview;
    setPreview(null);
    if (result && result !== notes) commit(result);
  };

  // Velocity lane: drag a stem (or all selected stems) vertically.
  const onVelDown = (e: React.PointerEvent) => {
    const canvas = velRef.current!;
    const { x, y } = local(e, canvas);
    if (x < GUTTER || e.button !== 0) return;
    const beat = (x + scrollLeft - GUTTER) / beatW;
    let best: NoteEvent | null = null;
    let bestDx = Infinity;
    for (const n of notes) {
      const dx = Math.abs(n.startBeat - beat) * beatW;
      if (dx < bestDx && dx <= 8) {
        best = n;
        bestDx = dx;
      }
    }
    if (!best) return;
    canvas.setPointerCapture(e.pointerId);
    const ids = selection.has(best.id) ? new Set(selection) : new Set([best.id]);
    if (!selection.has(best.id)) setSelection(ids);
    dragRef.current = { kind: "velocity", ids, anchorId: best.id, startY: y, base: notes };
    applyVelocityAt(y);
  };
  const applyVelocityAt = (y: number) => {
    const d = dragRef.current;
    if (!d || d.kind !== "velocity") return;
    const v = Math.round(Math.min(127, Math.max(1, ((VEL_H - 4 - y) / (VEL_H - 14)) * 127)));
    const anchor = d.base.find((n) => n.id === d.anchorId);
    if (!anchor) return;
    const delta = v - anchor.velocity;
    setPreview(d.base.map((n) => (d.ids.has(n.id) ? { ...n, velocity: n.id === d.anchorId ? v : Math.min(127, Math.max(1, n.velocity + delta)) } : n)));
  };
  const onVelMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d || d.kind !== "velocity") return;
    applyVelocityAt(local(e, velRef.current!).y);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    const mod = e.metaKey || e.ctrlKey;
    const k = e.key;
    const bounds = selectionBounds(notes, selection);
    const grid = noteLength;

    if (k === "Backspace" || k === "Delete") {
      if (selection.size) commit(removeNotes(notes, selection));
    } else if (mod && k.toLowerCase() === "a") {
      setSelection(new Set(notes.map((n) => n.id)));
    } else if (mod && k.toLowerCase() === "c") {
      clipboard = notes.filter((n) => selection.has(n.id));
    } else if (mod && k.toLowerCase() === "x") {
      clipboard = notes.filter((n) => selection.has(n.id));
      if (selection.size) commit(removeNotes(notes, selection));
    } else if (mod && k.toLowerCase() === "v") {
      if (!clipboard.length) return;
      const at = bounds ? snapFloor(bounds.end, grid) : snapFloor(playhead, grid);
      const { notes: next, newIds } = pasteNotes(notes, clipboard, at);
      commit(next);
      setSelection(newIds);
    } else if (mod && k.toLowerCase() === "d") {
      if (!selection.size) return;
      const { notes: next, newIds } = duplicateNotes(notes, selection, grid);
      commit(next);
      setSelection(newIds);
    } else if (mod && (k === "=" || k === "+")) {
      setZoom(zoom * 1.25);
    } else if (mod && k === "-") {
      setZoom(zoom / 1.25);
    } else if (k === "Escape") {
      setSelection(new Set());
    } else if (k.startsWith("Arrow") && selection.size) {
      const dBeats = k === "ArrowLeft" ? -(e.shiftKey ? 4 : grid) : k === "ArrowRight" ? (e.shiftKey ? 4 : grid) : 0;
      const dPitch = k === "ArrowUp" ? (e.shiftKey ? 12 : 1) : k === "ArrowDown" ? -(e.shiftKey ? 12 : 1) : 0;
      commit(moveNotes(notes, selection, dBeats, dPitch, { min: LOW, max: HIGH }, beats));
    } else if ((k === "[" || k === "]") && selection.size) {
      commit(adjustVelocity(notes, selection, k === "]" ? 10 : -10));
    } else {
      return; // let global shortcuts through
    }
    e.preventDefault();
    e.stopPropagation();
  };

  if (!pattern || !track) return null;

  const selVel = notes.filter((n) => selection.has(n.id)).map((n) => n.velocity);
  const velSummary = selVel.length
    ? `sel ${Math.min(...selVel)}–${Math.max(...selVel)} · avg ${Math.round(selVel.reduce((a, b) => a + b, 0) / selVel.length)}`
    : notes.length
      ? `${notes.length} notes · avg ${Math.round(notes.reduce((a, n) => a + n.velocity, 0) / notes.length)}`
      : "—";

  return (
    <div className="rollwrap">
      <div
        className="rollscroll"
        ref={scrollRef}
        tabIndex={0}
        onKeyDown={onKeyDown}
        onScroll={(e) => setScrollLeft((e.target as HTMLDivElement).scrollLeft)}
        onWheel={(e) => {
          if (e.ctrlKey || e.metaKey) {
            e.preventDefault();
            setZoom(zoom * (e.deltaY < 0 ? 1.1 : 0.9));
          }
        }}
      >
        <canvas
          ref={canvasRef}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={endDrag}
          onPointerCancel={endDrag}
          onPointerLeave={() => setHover(null)}
          onContextMenu={(e) => e.preventDefault()}
        />
      </div>
      <div className="lanehdr">
        <span className="on">velocity</span>
        <span className="sum">{velSummary}</span>
      </div>
      <div className="vellane">
        <canvas ref={velRef} onPointerDown={onVelDown} onPointerMove={onVelMove} onPointerUp={endDrag} onPointerCancel={endDrag} onContextMenu={(e) => e.preventDefault()} />
      </div>
    </div>
  );
}

function gridLabelFor(len: number): string {
  const map: Record<string, string> = { "0.125": "1/32", "0.25": "1/16", "0.5": "1/8", "1": "1/4", "2": "1/2", "4": "1" };
  return map[String(len)] ?? `${len}`;
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
