import { useRef, useState } from "react";
import { selectedPattern, useStore } from "../store";
import { chordAt, chordName, diatonicChords, noteName, tier } from "../theory";
import type { ChordEvent } from "../types";

export const GUTTER = 64;
export const BASE_BEAT_W = 96;

/** The chord track, as a lane above the grid. Drag to move, drag the right edge to resize,
 *  click to free-build, right-click to delete. Starters live in the inspector's HARMONY section. */
export function ChordLane() {
  const pattern = useStore(selectedPattern);
  const key = useStore((s) => s.snapshot!.model.key);
  const playhead = useStore((s) => s.playhead);
  const zoom = useStore((s) => s.zoom);
  const dispatch = useStore((s) => s.dispatch);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [preview, setPreview] = useState<ChordEvent[] | null>(null);
  const drag = useRef<{ id: string; kind: "move" | "resize"; startX: number; orig: ChordEvent } | null>(null);

  if (!pattern) return null;
  const beatW = BASE_BEAT_W * zoom;
  const chords = preview ?? pattern.chords.chords;
  const current = chordAt(pattern.chords.chords, playhead);
  const root = key.rootPitchClass ?? 0;

  const commit = (next: ChordEvent[]) => void dispatch({ type: "setChords", patternId: pattern.id, chords: next });

  const onPointerDown = (e: React.PointerEvent, chord: ChordEvent, kind: "move" | "resize") => {
    if (e.button === 2) {
      commit(chords.filter((c) => c.id !== chord.id));
      return;
    }
    if (e.button !== 0) return;
    e.stopPropagation();
    drag.current = { id: chord.id, kind, startX: e.clientX, orig: chord };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dBeats = Math.round((e.clientX - d.startX) / beatW);
    if (dBeats === 0 && preview === null) return;
    setPreview(
      pattern.chords.chords.map((c) => {
        if (c.id !== d.id) return c;
        if (d.kind === "move") {
          return { ...c, startBeat: Math.min(Math.max(0, d.orig.startBeat + dBeats), pattern.lengthBeats - d.orig.lengthBeats) };
        }
        return { ...c, lengthBeats: Math.min(Math.max(1, d.orig.lengthBeats + dBeats), pattern.lengthBeats - d.orig.startBeat) };
      }),
    );
  };

  const onPointerUp = (e: React.PointerEvent, chord: ChordEvent) => {
    const d = drag.current;
    drag.current = null;
    if (!d) return;
    if (preview) {
      commit(preview);
      setPreview(null);
    } else if (d.kind === "move" && Math.abs(e.clientX - d.startX) < 3) {
      setEditingId(editingId === chord.id ? null : chord.id);
    }
  };

  return (
    <div className="chordlane">
      <div className="gut">
        <span className="cap">chords</span>
      </div>
      <div className="inner" style={{ width: pattern.lengthBeats * beatW }}>
        {chords.length === 0 && <span className="hint">no chords yet: pick a progression under HARMONY, or ＋ chord</span>}
        {chords.map((c) => (
          <div
            key={c.id}
            className={`chordblock${current?.id === c.id ? " current" : ""}`}
            style={{ left: c.startBeat * beatW, width: Math.max(8, c.lengthBeats * beatW - 3) }}
            onPointerDown={(e) => onPointerDown(e, c, "move")}
            onPointerMove={onPointerMove}
            onPointerUp={(e) => onPointerUp(e, c)}
            title="click: edit · drag: move · right edge: resize · right-click: delete"
          >
            {c.name ?? chordName(c.pitchClasses)}
            <div className="handle" onPointerDown={(e) => onPointerDown(e, c, "resize")} />
            {editingId === c.id && (
              <ChordEditor
                chord={c}
                root={root}
                scale={key.scale}
                onChange={(pcs) => commit(chords.map((x) => (x.id === c.id ? { ...x, pitchClasses: pcs, name: chordName(pcs) } : x)))}
                onDelete={() => (setEditingId(null), commit(chords.filter((x) => x.id !== c.id)))}
                onClose={() => setEditingId(null)}
              />
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

function ChordEditor({
  chord,
  root,
  scale,
  onChange,
  onDelete,
  onClose,
}: {
  chord: ChordEvent;
  root: number;
  scale: "major" | "minor";
  onChange: (pcs: number[]) => void;
  onDelete: () => void;
  onClose: () => void;
}) {
  const pcs = chord.pitchClasses;
  const toggle = (pc: number) => onChange(pcs.includes(pc) ? pcs.filter((p) => p !== pc) : [...pcs, pc].sort((a, b) => a - b));
  const nextDiatonic = () => {
    const diatonic = diatonicChords(root, scale);
    const idx = diatonic.findIndex((d) => d.name === (chord.name ?? chordName(pcs)));
    onChange([...diatonic[(idx + 1 + diatonic.length) % diatonic.length].pitchClasses]);
  };

  return (
    <div className="popover" onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()}>
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="big">{chordName(pcs)}</span>
        <span className="cap">free build</span>
      </div>
      <div className="flabel" style={{ marginTop: 4 }}>tap notes · color = how it fits what's stacked</div>
      <div className="pcgrid">
        {Array.from({ length: 12 }, (_, pc) => {
          const inChord = pcs.includes(pc);
          const fit = inChord ? "chordTone" : pcs.length === 0 ? null : tier(pc, pcs, null);
          return (
            <button key={pc} className={inChord ? "in" : fit === "tension" ? "tension" : fit === "dissonance" ? "dissonance" : ""} onClick={() => toggle(pc)}>
              {noteName(pc)}
              {!inChord && fit === "dissonance" && <span className="flag">!</span>}
              {!inChord && fit === "tension" && <span className="flag">·</span>}
            </button>
          );
        })}
      </div>
      <div className="row" style={{ gap: 4 }}>
        <button onClick={nextDiatonic} style={{ textTransform: "none" }}>next ▸ diatonic</button>
        <button className="quiet" onClick={() => onChange([])}>clear</button>
        <button className="quiet danger" onClick={onDelete}>delete</button>
        <span className="spacer" />
        <button className="solid" onClick={onClose}>done</button>
      </div>
    </div>
  );
}
