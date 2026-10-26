import { useEffect, useMemo, useRef, useState } from "react";
import { selectedPattern, selectedTrack, selectedTrackIndex, useStore } from "../store";
import { DRUM_KIT, NOTE_PROPS, noteProp, TICKS_PER_BEAT, trackColor, type NoteEvent, type NoteProp, uuid } from "../types";
import {
  clearRow,
  euclideanRow,
  fillEverySteps,
  quantizeNoteProp,
  rampNoteProp,
  randomizeRow,
  rotateRow,
  seededRng,
  setNoteProp,
  stepIndexOf,
  tileGhost,
  type StepGrid,
} from "../noteEditing";
import { BASE_BEAT_W } from "./ChordLane";

/** Geometry shared with the CSS: `.drumrow .dname` width, `.drumcells` gap and left padding. */
const GUTTER = 64;
const CELL_GAP = 2;
const CELL_PAD = 1;
/** `.drumgraph` height, and the breathing room the lane keeps above and below its range. */
const GRAPH_H = 96;
const GRAPH_PAD = 10;

const NO_NOTES: NoteEvent[] = [];
const PROPS = Object.keys(NOTE_PROPS) as NoteProp[];

const numStyle: React.CSSProperties = {
  width: 36,
  fontFamily: "var(--font-mono)",
  fontSize: 10,
  padding: "2px 4px",
  background: "var(--bg-control)",
  color: "var(--text-1)",
  border: "1px solid var(--line-1)",
  borderRadius: 3,
};

/**
 * Step sequencer for a drum track, aligned to the chord lane. Left-click/drag adds hits,
 * right-click/drag removes. Hits are ordinary notes (pitch = kit note), which is why the graph
 * editor below the grid edits exactly the properties the piano roll's lane does — in FL a step
 * *is* a note, and here it already was one. The grid cell stays the step's identity; `shift`
 * only leans how it plays, so a dragged-early hat still lives in its own cell. A ghost pattern
 * shows as outlined steps (another drum pattern) or an onset row (a melodic one).
 */
export function DrumGrid() {
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const idx = useStore(selectedTrackIndex);
  const playhead = useStore((s) => s.playhead);
  const noteLength = useStore((s) => s.noteLength);
  const zoom = useStore((s) => s.zoom);
  const dispatch = useStore((s) => s.dispatch);
  const audition = useStore((s) => s.audition);
  const laneProp = useStore((s) => s.laneProp);
  const setLaneProp = useStore((s) => s.setLaneProp);
  const defaultVelocity = useStore((s) => s.settings.editing.defaultVelocity);
  const ghostPatternId = useStore((s) => s.ghostPatternId);
  const ghostPattern = useStore((s) => s.snapshot?.model.patterns.find((p) => p.id === ghostPatternId) ?? null);
  const ghostTrack = useStore((s) => (ghostPattern ? (s.snapshot?.model.tracks.find((t) => t.id === ghostPattern.trackId) ?? null) : null));
  const ghostIdx = useStore((s) => (ghostTrack ? (s.snapshot?.model.tracks.indexOf(ghostTrack) ?? 0) : 0));
  const dragMode = useRef<"add" | "remove" | null>(null);
  const [working, setWorking] = useState<NoteEvent[] | null>(null);
  /** Which kit row the graph editor and the step tools act on. */
  const [rowPitch, setRowPitch] = useState(DRUM_KIT[0].pitch);
  const [euclid, setEuclid] = useState({ onsets: 5, rotation: 0 });
  const graphRef = useRef<HTMLCanvasElement>(null);
  const graphDrag = useRef<{ base: NoteEvent[]; ramp: boolean; anchor: { step: number; value: number } } | null>(null);
  /** Names the pending gesture in the Edit menu; one gesture, one undo step. */
  const commitLabel = useRef("edit drum steps");

  const color = track ? trackColor(track, idx) : "#8a8a93";
  const committed = (pattern ? pattern.notes : undefined) ?? NO_NOTES;
  const notes = working ?? committed;
  const pxPerTick = (BASE_BEAT_W * zoom) / TICKS_PER_BEAT;
  const stepTicks = Math.min(TICKS_PER_BEAT / 2, Math.max(TICKS_PER_BEAT / 8, noteLength));
  /** The grid spans the whole pattern: one step per `stepTicks`. */
  const totalTicks = pattern?.lengthTicks ?? 4 * TICKS_PER_BEAT;
  const steps = Math.max(1, Math.round(totalTicks / stepTicks));
  const cellW = stepTicks * pxPerTick - 2;
  const pitchW = cellW + CELL_GAP;
  const currentStep = Math.floor((playhead % totalTicks) / stepTicks);
  const graphW = GUTTER + CELL_PAD + steps * pitchW;
  const spec = NOTE_PROPS[laneProp];

  const grid = useMemo<StepGrid>(() => ({ pitch: rowPitch, stepTicks, steps }), [rowPitch, stepTicks, steps]);

  /** The selected row's note in each step slot, or null where the slot is empty. */
  const cellsOf = (list: NoteEvent[]): (NoteEvent | null)[] => {
    const out: (NoteEvent | null)[] = Array.from({ length: steps }, () => null);
    for (const n of list) {
      const i = stepIndexOf(n, grid);
      if (i !== null && i >= 0 && i < steps) out[i] = n;
    }
    return out;
  };
  const rowCells = useMemo(() => cellsOf(notes), [notes, grid, steps]);

  // ─── Graph editor canvas ────────────────────────────────────────────────────────────────
  useEffect(() => {
    const canvas = graphRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    if (canvas.width !== Math.round(graphW * dpr) || canvas.height !== Math.round(GRAPH_H * dpr)) {
      canvas.width = Math.round(graphW * dpr);
      canvas.height = Math.round(GRAPH_H * dpr);
      canvas.style.width = `${graphW}px`;
      canvas.style.height = `${GRAPH_H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.font = '400 8.5px "JetBrains Mono", ui-monospace, monospace';
    ctx.fillStyle = "#0c0c0f";
    ctx.fillRect(0, 0, graphW, GRAPH_H);

    const yFor = (v: number) =>
      GRAPH_H - GRAPH_PAD - ((v - spec.min) / (spec.max - spec.min)) * (GRAPH_H - 2 * GRAPH_PAD);
    // Bipolar properties grow from the lane centre, unipolar ones from the floor.
    const floor = Math.round(yFor(spec.bipolar ? spec.neutral : spec.min));
    ctx.fillStyle = "#1c1c22";
    ctx.fillRect(GUTTER, floor, graphW - GUTTER, 1);

    for (let s = 0; s < steps; s++) {
      const x = GUTTER + CELL_PAD + s * pitchW;
      if (s === currentStep) {
        ctx.fillStyle = "rgba(255,255,255,.05)";
        ctx.fillRect(x, 0, cellW, GRAPH_H);
      }
      const n = rowCells[s];
      if (!n) {
        // Nothing to edit here: a dead tick, so the empty slot reads as empty rather than zero.
        ctx.fillStyle = "#17171c";
        ctx.fillRect(x, floor - 1, cellW, 3);
        continue;
      }
      const y = Math.round(yFor(noteProp(n, laneProp)));
      ctx.fillStyle = color;
      ctx.globalAlpha = 0.7;
      ctx.fillRect(x, Math.min(y, floor), cellW, Math.max(1, Math.abs(y - floor)));
      ctx.globalAlpha = 1;
      ctx.fillRect(x, y - 1, cellW, 2);
    }

    // Scale column: the property's own range, read against the bars beside it.
    ctx.fillStyle = "#101013";
    ctx.fillRect(0, 0, GUTTER, GRAPH_H);
    ctx.fillStyle = "#23232a";
    ctx.fillRect(GUTTER - 1, 0, 1, GRAPH_H);
    ctx.fillStyle = "#4a4a52";
    const right = (text: string, y: number) => ctx.fillText(text, GUTTER - 8 - ctx.measureText(text).width, y);
    right(spec.format(spec.max), GRAPH_PAD);
    if (spec.bipolar) right(spec.format(spec.neutral), floor);
    right(spec.format(spec.min), GRAPH_H - GRAPH_PAD);
  }, [rowCells, laneProp, spec, steps, cellW, pitchW, graphW, currentStep, color]);

  const ghostNotes = useMemo(
    () => (ghostPattern && ghostPattern.id !== pattern?.id ? tileGhost(ghostPattern.notes, ghostPattern.lengthTicks, totalTicks) : []),
    [ghostPattern, pattern?.id, totalTicks],
  );

  if (!pattern || !track) return null;
  const ghostColor = ghostTrack ? trackColor(ghostTrack, ghostIdx) : "var(--text-4)";
  // Ghost lookups per step: kit hits by pitch (a drum ghost), or onsets (a melodic ghost).
  const ghostIsDrum = !!ghostTrack?.isDrum;
  const stepOf = (tick: number) => Math.floor(tick / stepTicks);
  const ghostHits = new Set(ghostIsDrum ? ghostNotes.map((n) => `${n.pitch}:${stepOf(n.startTick)}`) : []);
  const ghostOnsets = new Map<number, number>();
  if (!ghostIsDrum) for (const n of ghostNotes) ghostOnsets.set(stepOf(n.startTick), (ghostOnsets.get(stepOf(n.startTick)) ?? 0) + 1);

  const find = (list: NoteEvent[], pitch: number, step: number) =>
    list.find((n) => n.pitch === pitch && n.startTick === step * stepTicks);

  const commit = () => {
    if (working) {
      void dispatch({ type: "setNotes", patternId: pattern.id, notes: working }, false, commitLabel.current);
      setWorking(null);
    }
    dragMode.current = null;
    graphDrag.current = null;
  };

  /** One-shot step tool: a whole new row in a single command, so it is a single undo step. */
  const run = (next: NoteEvent[], label: string) => {
    if (next === committed) return;
    void dispatch({ type: "setNotes", patternId: pattern.id, notes: next }, false, label);
  };

  const paint = (pitch: number, step: number) => {
    const mode = dragMode.current;
    if (!mode) return;
    setWorking((prev) => {
      const cur = prev ?? committed;
      const existing = find(cur, pitch, step);
      if (mode === "add" && !existing) {
        audition(pitch);
        return [...cur, { id: uuid(), startTick: step * stepTicks, lengthTicks: stepTicks, pitch, velocity: defaultVelocity }];
      }
      if (mode === "remove" && existing) return cur.filter((n) => n !== existing);
      return cur;
    });
  };

  // ─── Graph interaction ──────────────────────────────────────────────────────────────────
  /** Which step the pointer is over, and the property value its height stands for. */
  const graphAt = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    const step = Math.floor((e.clientX - r.left - GUTTER - CELL_PAD) / pitchW);
    const t = Math.min(1, Math.max(0, (GRAPH_H - GRAPH_PAD - (e.clientY - r.top)) / (GRAPH_H - 2 * GRAPH_PAD)));
    return { step, value: spec.min + t * (spec.max - spec.min) };
  };

  const graphApply = (at: { step: number; value: number }) => {
    const d = graphDrag.current;
    if (!d || at.step < 0 || at.step >= steps) return;
    if (d.ramp) {
      // ⌥: a straight line from where the drag started to where it is now, across the steps between.
      const cells = cellsOf(d.base);
      const lo = Math.min(d.anchor.step, at.step);
      const hi = Math.max(d.anchor.step, at.step);
      const ids = new Set<string>();
      for (let s = lo; s <= hi; s++) {
        const n = cells[s];
        if (n) ids.add(n.id);
      }
      if (!ids.size) return;
      // Quantise the ends first, so the interior is the straight line between the values the
      // end steps really take (the module owns the clamp-and-round rule, not this component).
      const first = quantizeNoteProp(laneProp, d.anchor.step <= at.step ? d.anchor.value : at.value);
      const last = quantizeNoteProp(laneProp, d.anchor.step <= at.step ? at.value : d.anchor.value);
      setWorking(rampNoteProp(d.base, ids, laneProp, first, last));
      return;
    }
    setWorking((prev) => {
      const cur = prev ?? d.base;
      const n = cellsOf(cur)[at.step];
      // Only steps that exist are editable: the lane never conjures a hit.
      return n ? setNoteProp(cur, new Set([n.id]), laneProp, at.value) : cur;
    });
  };

  const onGraphDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    const at = graphAt(e);
    if (at.step < 0 || at.step >= steps) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    commitLabel.current = `set step ${spec.label}`;
    graphDrag.current = { base: notes, ramp: e.altKey, anchor: at };
    graphApply(at);
  };

  const selectedDrum = DRUM_KIT.find((d) => d.pitch === rowPitch) ?? DRUM_KIT[0];

  return (
    <div className="drumgrid" onPointerUp={commit} onPointerLeave={commit}>
      {ghostPattern && !ghostIsDrum && ghostNotes.length > 0 && (
        <div className="drumrow ghostrow" title={`${ghostPattern.name}: where its notes start · the ghost`}>
          <span className="dname">ghost</span>
          <div className="drumcells">
            {Array.from({ length: steps }, (_, step) => {
              const n = ghostOnsets.get(step) ?? 0;
              return <div key={step} className={`gstep${n ? " on" : ""}`} style={{ width: cellW, background: n ? ghostColor : undefined, opacity: n ? Math.min(0.75, 0.3 + n * 0.15) : undefined }} />;
            })}
          </div>
        </div>
      )}
      {DRUM_KIT.map((drum) => {
        const picked = drum.pitch === rowPitch;
        return (
          <div className="drumrow" key={drum.pitch} style={picked ? { background: "rgba(255,255,255,.045)" } : undefined}>
            <span
              className="dname"
              title="the row the graph editor and the step tools act on"
              style={{
                cursor: "pointer",
                color: picked ? "var(--text-1)" : undefined,
                background: picked ? "var(--bg-control)" : undefined,
                boxShadow: picked ? `inset 3px 0 0 ${color}` : undefined,
              }}
              onPointerDown={(e) => {
                e.preventDefault();
                setRowPitch(drum.pitch);
              }}
            >
              {drum.name}
            </span>
            <div className="drumcells">
              {Array.from({ length: steps }, (_, step) => {
                const hit = find(notes, drum.pitch, step);
                const ghost = !hit && ghostHits.has(`${drum.pitch}:${step}`);
                const onBeat = (step * stepTicks) % TICKS_PER_BEAT === 0;
                // A hit shows how hard it plays, and leans by its shift — at most one cell, so the
                // slot it sits in still reads as the step it is.
                const lean = hit ? Math.max(-1, Math.min(1, noteProp(hit, "shiftTicks") / stepTicks)) : 0;
                return (
                  <div
                    key={step}
                    className={`step${hit ? " on" : ""}${onBeat ? " beat" : ""}${step === currentStep ? " now" : ""}${ghost ? " ghost" : ""}`}
                    style={{ width: cellW, ...(ghost ? { borderColor: ghostColor } : {}) }}
                    onPointerDown={(e) => {
                      e.preventDefault();
                      commitLabel.current = "edit drum steps";
                      dragMode.current = e.button === 2 ? "remove" : "add";
                      paint(drum.pitch, step);
                    }}
                    onPointerEnter={() => paint(drum.pitch, step)}
                  >
                    {hit && (
                      <span
                        className="lean"
                        style={{
                          background: color,
                          opacity: 0.35 + 0.65 * Math.min(1, Math.max(0, (noteProp(hit, "velocity") - 1) / 126)),
                          transform: lean ? `translateX(${(lean * 100).toFixed(1)}%)` : undefined,
                        }}
                      />
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        );
      })}

      <div className="steptools" style={{ position: "sticky", left: 0 }}>
        <span className="lbl" style={{ color }}>{selectedDrum.name}</span>
        <select
          className="lanepick"
          value={laneProp}
          title="which per-note property the graph editor below draws (shared with the piano roll's lane)"
          onChange={(e) => setLaneProp(e.target.value as NoteProp)}
        >
          {PROPS.map((p) => (
            <option key={p} value={p}>
              {NOTE_PROPS[p].label}
            </option>
          ))}
        </select>
        <button className="chip tiny" title="a hit every 2 steps" onClick={() => run(fillEverySteps(committed, grid, 2, 0, defaultVelocity), "fill every 2 steps")}>
          fill /2
        </button>
        <button className="chip tiny" title="a hit every 4 steps" onClick={() => run(fillEverySteps(committed, grid, 4, 0, defaultVelocity), "fill every 4 steps")}>
          fill /4
        </button>
        <button className="chip tiny" title="a hit every 8 steps" onClick={() => run(fillEverySteps(committed, grid, 8, 0, defaultVelocity), "fill every 8 steps")}>
          fill /8
        </button>
        <button className="chip tiny" title="empty this row" onClick={() => run(clearRow(committed, grid), `clear ${selectedDrum.name} steps`)}>
          clear
        </button>
        <button className="chip tiny" title="rotate the row one step earlier" onClick={() => run(rotateRow(committed, grid, -1), "rotate steps left")}>
          ◀
        </button>
        <button className="chip tiny" title="rotate the row one step later" onClick={() => run(rotateRow(committed, grid, 1), "rotate steps right")}>
          ▶
        </button>
        <button
          className="chip tiny"
          title="coin-flip each step"
          onClick={() => run(randomizeRow(committed, grid, 0.5, defaultVelocity, seededRng(Date.now())), "randomize steps")}
        >
          rand
        </button>
        <button
          className="chip tiny"
          title="spread the onsets as evenly as the steps allow"
          onClick={() => run(euclideanRow(committed, grid, euclid.onsets, euclid.rotation, defaultVelocity), "euclidean steps")}
        >
          euclid
        </button>
        <input
          type="number"
          style={numStyle}
          min={0}
          max={steps}
          value={euclid.onsets}
          title="onsets"
          onChange={(e) => setEuclid((p) => ({ ...p, onsets: Math.max(0, Math.min(steps, Math.round(+e.target.value || 0))) }))}
        />
        <input
          type="number"
          style={numStyle}
          min={0}
          max={Math.max(0, steps - 1)}
          value={euclid.rotation}
          title="rotation"
          onChange={(e) => setEuclid((p) => ({ ...p, rotation: Math.max(0, Math.min(steps - 1, Math.round(+e.target.value || 0))) }))}
        />
      </div>

      <div className="drumgraph" style={{ width: graphW }}>
        <canvas
          ref={graphRef}
          onPointerDown={onGraphDown}
          onPointerMove={(e) => {
            if (graphDrag.current) graphApply(graphAt(e));
          }}
          onContextMenu={(e) => e.preventDefault()}
        />
      </div>

      {notes.length === 0 && <div className="help emptyline">no hits yet: click a step to add one. kick on 1 and 3, snare on 2 and 4 is a fine place to start.</div>}
      <div className="help">
        left-click / drag: add hits · right-click / drag: remove · step {stepTicks === TICKS_PER_BEAT / 2 ? "1/8" : stepTicks === TICKS_PER_BEAT / 4 ? "1/16" : "1/32"} follows snap
        {` · graph: drag to set ${spec.label} on ${selectedDrum.name}, ⌥-drag to ramp`}
      </div>
    </div>
  );
}
