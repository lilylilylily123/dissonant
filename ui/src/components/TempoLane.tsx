import { useRef, useState } from "react";
import { useStore } from "../store";
import { tempoMap, ticksToBeats, TICKS_PER_BEAT, type Breakpoint, type TempoPoint } from "../types";
import { keys } from "../platform";

/**
 * A line of draggable breakpoints over the song's timeline, shared by the tempo map and the
 * per-track automation lanes. Double-click adds a point at the value in force there; drag a
 * point up/down for its value (⇧ fine) and sideways to move it (snapped to beats); ⌥-click
 * toggles ramp (a glide from the previous point); right-click removes. A drag is one transient
 * run the caller closes in `onCommit`.
 */
export function BreakpointLane({
  headW,
  timelineW,
  pxPerBeat,
  bpb,
  height = 40,
  className,
  points,
  range,
  valueAt,
  format,
  unit,
  quantize,
  caption,
  status,
  flat,
  headExtra,
  onAdd,
  onUpdate,
  onRemove,
  onCommit,
}: {
  headW: number;
  timelineW: number;
  pxPerBeat: number;
  bpb: number;
  height?: number;
  /** An extra class on the root, next to `tempolane`. */
  className?: string;
  points: Breakpoint[];
  /** The vertical scale, over the points as shown so a drag can widen it. */
  range: (shown: Breakpoint[]) => [number, number];
  /** The curve to draw, over the points as shown (the drag preview included). */
  valueAt: (tick: number, shown: Breakpoint[]) => number;
  /** The head and hover readouts; the point tooltips add `unit`. */
  format: (value: number) => string;
  unit?: string;
  /** Where a drag lands: its rounding and the editable range. `fine` is ⇧. */
  quantize: (value: number, fine: boolean) => number;
  caption: string;
  status: string;
  /** Nothing in the lane bends the value yet: draw the curve and the readout dim. */
  flat: boolean;
  headExtra?: React.ReactNode;
  onAdd: (tick: number, value: number) => void;
  /** `curve` is the ⌥-click toggle; `drag` is the transient move `onCommit` closes. */
  onUpdate: (point: Breakpoint, kind: "drag" | "curve") => void;
  onRemove: (id: string) => void;
  onCommit: () => void;
}) {
  const H = height;
  const playhead = useStore((s) => s.playhead);
  const [preview, setPreview] = useState<Breakpoint | null>(null);
  const drag = useRef<{ point: Breakpoint; x0: number; y0: number; moved: boolean } | null>(null);
  const [hover, setHover] = useState<number | null>(null);

  const shown = points.map((p) => (preview && p.id === preview.id ? preview : p));
  const [lo, hi] = range(shown);
  const at = (tick: number) => valueAt(tick, shown);
  const span = Math.max(1e-9, hi - lo);
  const yFor = (value: number) => 4 + (H - 8) * (1 - (value - lo) / span);
  const valueForY = (y: number) => lo + (1 - (y - 4) / (H - 8)) * span;
  const pxPerTick = pxPerBeat / TICKS_PER_BEAT;
  const barStepTicks = bpb * TICKS_PER_BEAT;
  /** One polyline sample per beat of the visible width. */
  const samples = Math.ceil(timelineW / pxPerBeat);

  // Sampled per beat (ramps are straight lines, so beat-wise sampling is exact).
  const path = Array.from({ length: samples + 1 }, (_, b) => {
    const tick = b * TICKS_PER_BEAT;
    const prev = b > 0 ? at(tick - 1e-6) : at(0);
    const here = at(tick);
    // A step shows as a vertical edge: emit the pre-jump value first.
    return (Math.abs(prev - here) > 1e-9 ? `L${b * pxPerBeat},${yFor(prev)} ` : "") + `${b === 0 ? "M" : "L"}${b * pxPerBeat},${yFor(here)}`;
  }).join(" ");

  const onDown = (e: React.PointerEvent, p: Breakpoint) => {
    e.stopPropagation();
    if (e.button === 2) {
      onRemove(p.id);
      return;
    }
    if (e.altKey) {
      onUpdate({ ...p, curve: p.curve === "ramp" ? "step" : "ramp" }, "curve");
      return;
    }
    (e.currentTarget as Element).setPointerCapture(e.pointerId);
    drag.current = { point: p, x0: e.clientX, y0: e.clientY, moved: false };
  };
  const onMove = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x0;
    const dy = e.clientY - d.y0;
    if (!d.moved && Math.hypot(dx, dy) < 3) return;
    d.moved = true;
    const value = quantize(valueForY(yFor(d.point.value) + dy), e.shiftKey);
    const tick = Math.max(0, Math.round((d.point.tick + dx / pxPerTick) / TICKS_PER_BEAT) * TICKS_PER_BEAT);
    const next = { ...d.point, value, tick };
    setPreview(next);
    onUpdate(next, "drag");
  };
  const onUp = () => {
    if (drag.current?.moved) onCommit();
    drag.current = null;
    setPreview(null);
  };

  const now = at(playhead);
  return (
    <div className={`tempolane${className ? ` ${className}` : ""}`} style={{ width: headW + timelineW + 12, height }}>
      <div className="head" style={{ width: headW }}>
        <span className="cap">{caption}</span>
        <span className="mono" style={{ color: flat ? "var(--text-5)" : "var(--accent)" }}>{format(now)}</span>
        <span className="k">{status}</span>
        {headExtra}
      </div>
      <svg
        className="lane"
        width={timelineW}
        height={H}
        onDoubleClick={(e) => {
          const rect = e.currentTarget.getBoundingClientRect();
          const tick = Math.max(0, Math.round((e.clientX - rect.left) / pxPerBeat)) * TICKS_PER_BEAT;
          onAdd(tick, at(tick));
        }}
        onPointerMove={(e) => {
          onMove(e);
          const rect = e.currentTarget.getBoundingClientRect();
          setHover((e.clientX - rect.left) / pxPerTick);
        }}
        onPointerLeave={() => setHover(null)}
        onPointerUp={onUp}
        onPointerCancel={onUp}
        onContextMenu={(e) => e.preventDefault()}
      >
        {Array.from({ length: Math.ceil(samples / bpb) + 1 }, (_, i) => (
          <line key={i} x1={i * barStepTicks * pxPerTick} x2={i * barStepTicks * pxPerTick} y1={0} y2={H} stroke={i % 4 === 0 ? "#2a2a32" : "#1a1a1f"} />
        ))}
        <path d={path} fill="none" stroke="var(--accent)" strokeWidth={1.5} opacity={flat ? 0.35 : 0.9} />
        {shown.map((p) => (
          <g key={p.id} onPointerDown={(e) => onDown(e, p)} style={{ cursor: "ns-resize" }}>
            <circle cx={p.tick * pxPerTick} cy={yFor(p.value)} r={8} fill="transparent" />
            <circle cx={p.tick * pxPerTick} cy={yFor(p.value)} r={3.5} fill={p.curve === "ramp" ? "var(--bg-0)" : "var(--accent)"} stroke="var(--accent)" strokeWidth={1.5} />
            <title>{keys(`${format(p.value)}${unit ? ` ${unit}` : ""} at beat ${ticksToBeats(p.tick)}${p.curve === "ramp" ? " · ramp" : " · step"} · drag: move (⇧ fine) · ⌥-click: ${p.curve === "ramp" ? "step" : "ramp"} · right-click: remove`)}</title>
          </g>
        ))}
        {hover !== null && !drag.current && (
          <text x={hover * pxPerTick + 6} y={10} fill="var(--text-4)" fontFamily="JetBrains Mono, monospace" fontSize={9}>
            {format(at(hover))}
          </text>
        )}
        <line x1={playhead * pxPerTick} x2={playhead * pxPerTick} y1={0} y2={H} stroke="rgba(255,255,255,.5)" />
      </svg>
    </div>
  );
}

/** The tempo map along the song (SETUP, under the chords): a {@link BreakpointLane} in bpm. */
export function TempoLane({ headW, timelineW, pxPerBeat, bpb, height = 40 }: { headW: number; timelineW: number; pxPerBeat: number; bpb: number; height?: number }) {
  const model = useStore((s) => s.snapshot!.model);
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);
  const points = model.tempoPoints ?? [];

  // The lane samples the curve once per beat off one `shown` array: build its map once too.
  const cache = useRef<{ shown: Breakpoint[]; bpmAt: (tick: number) => number } | null>(null);
  const bpmOf = (shown: Breakpoint[]) => {
    if (cache.current?.shown !== shown) {
      const map = tempoMap(model.tempo, shown.map((p): TempoPoint => ({ id: p.id, tick: p.tick, bpm: p.value, ramp: p.curve === "ramp" })));
      cache.current = { shown, bpmAt: map.bpmAt };
    }
    return cache.current.bpmAt;
  };

  return (
    <BreakpointLane
      headW={headW}
      timelineW={timelineW}
      pxPerBeat={pxPerBeat}
      bpb={bpb}
      height={height}
      points={points.map((p): Breakpoint => ({ id: p.id, tick: p.tick, value: p.bpm, curve: p.ramp ? "ramp" : "step" }))}
      range={(shown) => {
        const all = [model.tempo, ...shown.map((p) => p.value)];
        return [Math.max(20, Math.min(...all) - 10), Math.min(300, Math.max(...all) + 10)];
      }}
      valueAt={(tick, shown) => bpmOf(shown)(tick)}
      format={(v) => v.toFixed(1)}
      unit="bpm"
      quantize={(v, fine) => {
        const step = fine ? 0.1 : 1;
        return Math.min(300, Math.max(20, Math.round(v / step) * step));
      }}
      caption="tempo"
      status={points.length ? `${points.length} change${points.length === 1 ? "" : "s"}` : "double-click to add"}
      flat={tempoMap(model.tempo, points).isConstant}
      onAdd={(tick, value) => void dispatch({ type: "addTempoPoint", tick, bpm: Math.round(value), ramp: false })}
      onUpdate={(p, kind) =>
        void dispatch(
          { type: "updateTempoPoint", point: { id: p.id, tick: p.tick, bpm: p.value, ramp: p.curve === "ramp" } },
          kind === "drag",
          kind === "drag" ? "move tempo change" : p.curve === "ramp" ? "ramp tempo change" : "step tempo change",
        )
      }
      onRemove={(id) => void dispatch({ type: "removeTempoPoint", id })}
      onCommit={() => void commitGesture()}
    />
  );
}
