import { useRef, useState } from "react";
import { selectedPattern, useStore } from "../store";
import { chordAt, chordName, diatonicChords, noteName, progression, STARTERS, tier } from "../theory";
import type { ChordEvent } from "../types";
import { uuid } from "../types";

const GUTTER = 56;
const BASE_BEAT_W = 52;

/** The guided chord lane: starters (in the project key), drag to move, drag the right edge to
 *  resize, click to open the free-build editor, right-click to delete. */
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
  const scale = key.scale;

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
    const next = pattern.chords.chords.map((c) => {
      if (c.id !== d.id) return c;
      if (d.kind === "move") {
        const start = Math.min(Math.max(0, d.orig.startBeat + dBeats), pattern.lengthBeats - d.orig.lengthBeats);
        return { ...c, startBeat: start };
      }
      const len = Math.min(Math.max(1, d.orig.lengthBeats + dBeats), pattern.lengthBeats - d.orig.startBeat);
      return { ...c, lengthBeats: len };
    });
    setPreview(next);
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

  const addChord = () => {
    const end = chords.reduce((m, c) => Math.max(m, c.startBeat + c.lengthBeats), 0);
    if (end >= pattern.lengthBeats) return;
    const tonic = diatonicChords(root, scale)[0];
    commit([
      ...chords,
      { id: uuid(), startBeat: end, lengthBeats: Math.min(4, pattern.lengthBeats - end), pitchClasses: [...tonic.pitchClasses], name: tonic.name },
    ]);
  };

  return (
    <div className="chordlane">
      <div className="lane">
        <div className="lane-inner" style={{ width: GUTTER + pattern.lengthBeats * beatW }}>
          {chords.map((c) => (
            <div
              key={c.id}
              className={`chordblock${current?.id === c.id ? " current" : ""}`}
              style={{ left: GUTTER + c.startBeat * beatW, width: Math.max(8, c.lengthBeats * beatW - 3) }}
              onPointerDown={(e) => onPointerDown(e, c, "move")}
              onPointerMove={onPointerMove}
              onPointerUp={(e) => onPointerUp(e, c)}
              title="click: edit · drag: move · right-click: delete"
            >
              {c.name ?? chordName(c.pitchClasses)}
              <div className="handle" onPointerDown={(e) => onPointerDown(e, c, "resize")} />
              {editingId === c.id && (
                <ChordEditor
                  chord={c}
                  root={root}
                  scale={scale}
                  onChange={(pcs) => commit(chords.map((x) => (x.id === c.id ? { ...x, pitchClasses: pcs, name: chordName(pcs) } : x)))}
                  onDelete={() => (setEditingId(null), commit(chords.filter((x) => x.id !== c.id)))}
                  onClose={() => setEditingId(null)}
                />
              )}
            </div>
          ))}
        </div>
      </div>
      <div className="row tight">
        <span className="label">starters</span>
        {STARTERS.map((st) => (
          <button key={st.name} className="chip small" onClick={() => commit(progression(st.degrees, root, scale, pattern.lengthBeats))}>
            {st.name}
          </button>
        ))}
        <button className="chip small ghost" onClick={addChord}>+ chord</button>
        <button className="chip small ghost faded" onClick={() => commit([])}>clear</button>
        <span className="label">· in {noteName(root)} {scale} · click a chord to edit</span>
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
    const next = diatonic[(idx + 1 + diatonic.length) % diatonic.length];
    onChange([...next.pitchClasses]);
  };

  return (
    <div className="popover" onPointerDown={(e) => e.stopPropagation()} onPointerUp={(e) => e.stopPropagation()} style={{ left: 0 }}>
      <div style={{ color: "var(--brand)", fontSize: 18, fontWeight: 700 }}>{chordName(pcs)}</div>
      <div className="label">tap notes to build · color = how it fits what's stacked</div>
      <div className="pcgrid">
        {Array.from({ length: 12 }, (_, pc) => {
          const inChord = pcs.includes(pc);
          const fit = inChord ? "chordTone" : pcs.length === 0 ? null : tier(pc, pcs, null);
          const bg = inChord ? undefined : fit === "tension" ? "rgba(232,163,64,.38)" : fit === "dissonance" ? "rgba(227,77,77,.38)" : undefined;
          return (
            <button key={pc} className={inChord ? "in" : ""} style={{ background: bg }} onClick={() => toggle(pc)}>
              {noteName(pc)}
              {!inChord && fit === "dissonance" && <span className="flag">!</span>}
            </button>
          );
        })}
      </div>
      <div className="row tight">
        <button className="chip small" onClick={nextDiatonic}>next ▸ diatonic</button>
        <button className="chip small faded" onClick={() => onChange([])}>clear</button>
        <button className="chip small faded" onClick={onDelete}>delete</button>
        <span className="spacer" />
        <button className="chip small faded" onClick={onClose}>done</button>
      </div>
    </div>
  );
}
