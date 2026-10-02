import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { selectedPattern, selectedTrack, useStore } from "../store";
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
import type { NoteEvent, Tier } from "../types";
import { uuid } from "../types";

// Geometry
const LOW = 24; // C1
const HIGH = 96; // C7
const ROW_H = 16;
const BASE_BEAT_W = 52;
const GUTTER = 56;
const RULER_H = 18;
const EDGE_PX = 7;
const BLACK = new Set([1, 3, 6, 8, 10]);

const COLORS = {
  surface: "#121214",
  panel: "#1f1f22",
  grid: "#2e2e33",
  ink: "#e6e3db",
  faded: "#8c8a85",
  brand: "#e8a340",
  chordTone: "#61b88c",
  tension: "#e8a340",
  dissonance: "#e34d4d",
};

const tierColor = (t: Tier | null) => (t ? COLORS[t] : COLORS.faded);

type Drag =
  | { kind: "move"; ids: Set<string>; anchor: NoteEvent; startX: number; startY: number; base: NoteEvent[]; lastPitch: number }
  | { kind: "resize"; ids: Set<string>; anchor: NoteEvent; startX: number; base: NoteEvent[] }
  | { kind: "paint"; base: NoteEvent[] }
  | { kind: "erase"; base: NoteEvent[] }
  | { kind: "marquee"; x0: number; y0: number; x1: number; y1: number; baseSelection: Set<string> };

let clipboard: NoteEvent[] = [];

/**
 * The piano roll. Modeless direct manipulation, FL-style:
 *  - click empty = place a note (and keep dragging to move it) · ⌥-drag empty = paint
 *  - drag a note = move (auditions pitch) · drag its right edge = resize · ⌥-drag = duplicate
 *  - shift-drag empty = marquee select · shift-click = toggle in selection
 *  - right-click / right-drag = erase · ruler click = seek
 *  - keys: ⌫ delete · ⌘A/⌘C/⌘X/⌘V/⌘D · arrows nudge (⇧ = octave / bar) · [ ] velocity · esc
 * Tiering is visual only — any note can be placed in any tier.
 */
export function PianoRoll() {
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const key = useStore((s) => s.snapshot!.model.key);
  const playhead = useStore((s) => s.playhead);
  const showLandscape = useStore((s) => s.showLandscape);
  const noteLength = useStore((s) => s.noteLength);
  const zoom = useStore((s) => s.zoom);
  const dispatch = useStore((s) => s.dispatch);
  const audition = useStore((s) => s.audition);
  const seek = useStore((s) => s.seek);
  const setZoom = useStore((s) => s.setZoom);

  const canvasRef = useRef<HTMLCanvasElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const [selection, setSelection] = useState<Set<string>>(new Set());
  const dragRef = useRef<Drag | null>(null);
  const [preview, setPreview] = useState<NoteEvent[] | null>(null);
  const [hover, setHover] = useState<{ beat: number; pitch: number } | null>(null);

  const notes = useMemo(() => (pattern && track ? (pattern.notesByTrack[track.id] ?? []) : []), [pattern, track]);
  const chords = pattern?.chords.chords ?? [];
  const beats = pattern?.lengthBeats ?? 16;
  const beatW = BASE_BEAT_W * zoom;
  const rows = HIGH - LOW + 1;
  const gridW = beats * beatW;
  const gridH = rows * ROW_H;
  const shown = preview ?? notes;

  const yForPitch = (p: number) => RULER_H + (HIGH - p) * ROW_H;
  const pitchForY = (y: number) => HIGH - Math.floor((y - RULER_H) / ROW_H);
  const beatForX = (x: number) => (x - GUTTER) / beatW;

  const commit = useCallback(
    (next: NoteEvent[]) => {
      if (pattern && track) void dispatch({ type: "setNotes", patternId: pattern.id, trackId: track.id, notes: next });
    },
    [dispatch, pattern, track],
  );

  // Drop selection entries that no longer exist (undo, pattern switch…).
  useEffect(() => {
    setSelection((sel) => {
      const ids = new Set(notes.map((n) => n.id));
      const next = new Set([...sel].filter((id) => ids.has(id)));
      return next.size === sel.size ? sel : next;
    });
  }, [notes]);

  // Center on C4 initially.
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (el) el.scrollTop = Math.max(0, yForPitch(60) - el.clientHeight / 2);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // ─── Drawing ──────────────────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const W = GUTTER + gridW;
    const H = RULER_H + gridH;
    if (canvas.width !== W * dpr || canvas.height !== H * dpr) {
      canvas.width = W * dpr;
      canvas.height = H * dpr;
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.font = "9px ui-monospace, Menlo, monospace";
    ctx.textBaseline = "middle";

    ctx.fillStyle = COLORS.surface;
    ctx.fillRect(0, 0, W, H);

    // Rows (black-key shading) + live tier tint
    const live = tierMap(playhead, chords, key);
    for (let p = LOW; p <= HIGH; p++) {
      const y = yForPitch(p);
      ctx.fillStyle = BLACK.has(p % 12) ? COLORS.surface : "rgba(31,31,34,.55)";
      ctx.fillRect(GUTTER, y, gridW, ROW_H);
      if (!showLandscape) {
        const t = live[p % 12];
        if (t) {
          ctx.fillStyle = tierColor(t);
          ctx.globalAlpha = 0.16;
          ctx.fillRect(GUTTER, y, gridW, ROW_H);
          ctx.globalAlpha = 1;
        }
      }
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
          ctx.globalAlpha = t === "dissonance" ? 0.4 : t === "tension" ? 0.26 : 0.16;
          ctx.fillStyle = tierColor(t);
          ctx.fillRect(x, y, beatW, ROW_H);
          ctx.globalAlpha = 1;
          if (t === "dissonance") {
            ctx.strokeStyle = "rgba(227,77,77,.6)";
            ctx.lineWidth = 0.9;
            ctx.beginPath();
            ctx.moveTo(x, y + ROW_H);
            ctx.lineTo(x + beatW, y);
            ctx.stroke();
          }
        }
      }
    }

    // Grid lines
    if (noteLength < 1) {
      ctx.strokeStyle = "rgba(46,46,51,.5)";
      ctx.lineWidth = 0.5;
      for (let t = noteLength; t < beats; t += noteLength) {
        if (Math.abs(t % 1) < 1e-9) continue;
        const x = GUTTER + t * beatW;
        ctx.beginPath();
        ctx.moveTo(x, RULER_H);
        ctx.lineTo(x, H);
        ctx.stroke();
      }
    }
    for (let b = 0; b <= beats; b++) {
      const x = GUTTER + b * beatW;
      ctx.strokeStyle = b % 4 === 0 ? COLORS.grid : "rgba(46,46,51,.5)";
      ctx.lineWidth = b % 4 === 0 ? 1.2 : 0.6;
      ctx.beginPath();
      ctx.moveTo(x, RULER_H);
      ctx.lineTo(x, H);
      ctx.stroke();
    }

    // Ruler
    ctx.fillStyle = COLORS.panel;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = COLORS.faded;
    for (let b = 0; b < beats; b += 1) {
      if (b % 4 === 0) ctx.fillText(`${b / 4 + 1}`, GUTTER + b * beatW + 4, RULER_H / 2);
    }
    // Chord names in the ruler
    ctx.fillStyle = COLORS.ink;
    for (const c of chords) {
      ctx.fillText(c.name ?? "", GUTTER + c.startBeat * beatW + 22, RULER_H / 2);
    }

    // Notes
    const velocityAlpha = (v: number) => 0.45 + 0.55 * (v / 127);
    for (const n of shown) {
      if (n.pitch < LOW || n.pitch > HIGH) continue;
      const t = tierMap(n.startBeat, chords, key)[n.pitch % 12];
      const x = GUTTER + n.startBeat * beatW + 1;
      const y = yForPitch(n.pitch) + 1;
      const w = Math.max(3, n.lengthBeats * beatW - 2);
      const h = ROW_H - 2;
      ctx.globalAlpha = velocityAlpha(n.velocity);
      ctx.fillStyle = tierColor(t);
      roundRect(ctx, x, y, w, h, 2);
      ctx.fill();
      ctx.globalAlpha = 1;
      if (t === "tension") {
        ctx.fillStyle = "rgba(18,18,20,.85)";
        ctx.beginPath();
        ctx.arc(x + 6, y + h / 2, 2, 0, Math.PI * 2);
        ctx.fill();
      } else if (t === "dissonance") {
        ctx.save();
        ctx.beginPath();
        ctx.rect(x, y, w, h);
        ctx.clip();
        ctx.strokeStyle = "rgba(0,0,0,.55)";
        ctx.lineWidth = 1.2;
        for (let sx = x - h; sx < x + w; sx += 5) {
          ctx.beginPath();
          ctx.moveTo(sx, y);
          ctx.lineTo(sx + h, y + h);
          ctx.stroke();
        }
        ctx.restore();
        ctx.fillStyle = COLORS.surface;
        ctx.font = "bold 9px ui-monospace, Menlo, monospace";
        ctx.fillText("!", x + w / 2 - 2, y + h / 2);
        ctx.font = "9px ui-monospace, Menlo, monospace";
      }
      ctx.strokeStyle = selection.has(n.id) ? COLORS.ink : "rgba(18,18,20,.6)";
      ctx.lineWidth = selection.has(n.id) ? 1.5 : 0.5;
      roundRect(ctx, x, y, w, h, 2);
      ctx.stroke();
    }

    // Marquee
    const d = dragRef.current;
    if (d && d.kind === "marquee") {
      ctx.strokeStyle = COLORS.brand;
      ctx.fillStyle = "rgba(232,163,64,.08)";
      ctx.lineWidth = 1;
      const x = Math.min(d.x0, d.x1);
      const y = Math.min(d.y0, d.y1);
      ctx.fillRect(x, y, Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
      ctx.strokeRect(x, y, Math.abs(d.x1 - d.x0), Math.abs(d.y1 - d.y0));
    }

    // Keyboard gutter
    ctx.fillStyle = COLORS.panel;
    ctx.fillRect(0, RULER_H, GUTTER, gridH);
    for (let p = LOW; p <= HIGH; p++) {
      const y = yForPitch(p);
      ctx.fillStyle = BLACK.has(p % 12) ? COLORS.surface : COLORS.panel;
      ctx.fillRect(0, y, GUTTER, ROW_H);
      ctx.fillStyle = COLORS.ink;
      ctx.fillText(midiName(p), 5, y + ROW_H / 2);
      const t = live[p % 12];
      ctx.fillStyle = tierColor(t);
      ctx.globalAlpha = t ? 1 : 0.15;
      ctx.beginPath();
      ctx.arc(GUTTER - 8, y + ROW_H / 2, 2.5, 0, Math.PI * 2);
      ctx.fill();
      ctx.globalAlpha = 1;
    }
    ctx.fillStyle = COLORS.panel;
    ctx.fillRect(0, 0, GUTTER, RULER_H);

    // Playhead
    const px = GUTTER + playhead * beatW;
    ctx.fillStyle = COLORS.brand;
    ctx.fillRect(px - 0.75, 0, 1.5, H);

    // Hover readout
    if (hover) {
      const t = tierMap(hover.beat, chords, key)[hover.pitch % 12];
      const label = `${midiName(hover.pitch)}${t ? ` · ${t === "chordTone" ? "chord tone" : t}` : ""}`;
      ctx.fillStyle = "rgba(31,31,34,.9)";
      const tw = ctx.measureText(label).width + 10;
      ctx.fillRect(GUTTER + 4, RULER_H + 4, tw, 14);
      ctx.fillStyle = COLORS.ink;
      ctx.fillText(label, GUTTER + 9, RULER_H + 11);
    }
  }, [shown, chords, key, playhead, showLandscape, noteLength, beatW, beats, gridW, gridH, selection, hover, preview]);

  // ─── Interaction ──────────────────────────────────────────────────────────────────────────
  const local = (e: React.PointerEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  const hitTest = (x: number, y: number): { note: NoteEvent; edge: boolean } | null => {
    const beat = beatForX(x);
    const pitch = pitchForY(y);
    const n = noteAt(shown, beat, pitch);
    if (!n) return null;
    const rightX = GUTTER + (n.startBeat + n.lengthBeats) * beatW;
    const edgeZone = Math.min(EDGE_PX, (n.lengthBeats * beatW) / 3);
    return { note: n, edge: x >= rightX - edgeZone };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    const canvas = canvasRef.current!;
    canvas.focus();
    const { x, y } = local(e);
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
        // Duplicate-drag: copies start exactly over the originals and follow the cursor.
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
    // Plain click on empty space: place a note and let the same drag move it.
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
    const { x, y } = local(e);
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
        setPreview([...notes]); // trigger redraw
        break;
      }
    }
  };

  const onPointerUp = () => {
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
      return; // let global shortcuts (space, undo…) through
    }
    e.preventDefault();
    e.stopPropagation();
  };

  if (!pattern || !track) return null;

  return (
    <>
      <div className="roll" ref={scrollRef}>
        <canvas
          ref={canvasRef}
          tabIndex={0}
          onPointerDown={onPointerDown}
          onPointerMove={onPointerMove}
          onPointerUp={onPointerUp}
          onPointerCancel={onPointerUp}
          onPointerLeave={() => setHover(null)}
          onKeyDown={onKeyDown}
          onContextMenu={(e) => e.preventDefault()}
          onWheel={(e) => {
            if (e.ctrlKey || e.metaKey) {
              e.preventDefault();
              setZoom(zoom * (e.deltaY < 0 ? 1.1 : 0.9));
            }
          }}
          style={{ outline: "none" }}
        />
      </div>
      <div className="roll-help">
        click: place · drag: move · right edge: resize · ⌥-drag: paint / duplicate · ⇧-drag: select · right-click: erase · ⌫ ⌘A ⌘C ⌘V ⌘D · arrows nudge · [ ] velocity · ruler: seek
      </div>
    </>
  );
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.moveTo(x + r, y);
  ctx.lineTo(x + w - r, y);
  ctx.quadraticCurveTo(x + w, y, x + w, y + r);
  ctx.lineTo(x + w, y + h - r);
  ctx.quadraticCurveTo(x + w, y + h, x + w - r, y + h);
  ctx.lineTo(x + r, y + h);
  ctx.quadraticCurveTo(x, y + h, x, y + h - r);
  ctx.lineTo(x, y + r);
  ctx.quadraticCurveTo(x, y, x + r, y);
  ctx.closePath();
}
