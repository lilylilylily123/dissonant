import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { getBridge } from "../bridge";
import { barTicks, useStore } from "../store";
import { TICKS_PER_BEAT, type AudioClip, type Track } from "../types";

/**
 * Canvas colours. `getContext("2d")` cannot resolve `var(--…)`, so the palette is duplicated
 * here from the token set rather than read off the document.
 */
const C = {
  bg: "#0e0e12",
  lane: "#131318",
  bar: "#23232b",
  beat: "#1a1a21",
  text: "#6b6b76",
  clip: "#1b2430",
  clipSelected: "#22304a",
  wave: "#7fd1ff",
  waveSelected: "#aee3ff",
  border: "#2e3a4a",
  borderSelected: "#6ea8ff",
  fade: "#0e0e12",
  playhead: "#ff5f56",
  missing: "#ff9d2a",
};
const RULER_H = 20;
/** Shortest the lane ever gets; above this it grows to fill the editor. */
const MIN_LANE_H = 120;
/** How finely a clip's envelope is fetched, in pixels per bucket. */
const PX_PER_BUCKET = 2;

type Drag =
  | { kind: "move"; id: string; grabTick: number; start: AudioClip }
  | { kind: "trim-end"; id: string; start: AudioClip; x0: number }
  | { kind: "trim-start"; id: string; start: AudioClip; x0: number }
  | { kind: "fade-in"; id: string; start: AudioClip; x0: number }
  | { kind: "fade-out"; id: string; start: AudioClip; x0: number };

/**
 * The audio track's timeline: a bar ruler and one lane of clips drawn from their envelopes.
 *
 * Audio does not warp, so a clip's width is `lengthSeconds` converted at the project tempo:
 * changing the tempo moves where it starts but never what it sounds like.
 */
export function WaveformLane({ track }: { track: Track }) {
  const model = useStore((s) => s.snapshot!.model);
  const playhead = useStore((s) => s.playhead);
  const selectedId = useStore((s) => s.selectedAudioClipId);
  const selectClip = useStore((s) => s.selectAudioClip);
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);
  const seek = useStore((s) => s.seek);
  const barTick = useStore(barTicks);

  const scrollRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [width, setWidth] = useState(900);
  const [laneH, setLaneH] = useState(MIN_LANE_H);
  const [drag, setDrag] = useState<Drag | null>(null);
  /** Live clip while dragging, so the lane redraws without a round trip per pointer move. */
  const [preview, setPreview] = useState<AudioClip | null>(null);
  const [peaks, setPeaks] = useState<Record<string, [number, number][]>>({});

  const pxPerBeat = 28;
  const pxPerTick = pxPerBeat / TICKS_PER_BEAT;
  const clips = (model.audioClips ?? []).filter((c) => c.trackId === track.id);
  const shown = clips.map((c) =>
    preview && c.id === preview.id ? preview : c,
  );
  const secondsToTicks = (s: number) => ((s * model.tempo) / 60) * TICKS_PER_BEAT;
  const endTick = shown.reduce(
    (m, c) => Math.max(m, c.startTick + secondsToTicks(c.lengthSeconds)),
    0,
  );
  const timelineW = Math.max(width, (endTick + barTick * 4) * pxPerTick);
  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const measure = () => {
      setWidth(el.clientWidth);
      // The scroller owns whatever height the editor gives it; the canvas follows, so the
      // waveform fills the window instead of leaving a dead band under a fixed-height strip.
      setLaneH(Math.max(MIN_LANE_H, el.clientHeight - RULER_H));
    };
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, []);

  // Envelopes, fetched once per source at a fixed resolution.
  useEffect(() => {
    let cancelled = false;
    const wanted = [...new Set(clips.map((c) => c.sourceId))].filter(
      (id) => !(id in peaks),
    );
    if (!wanted.length) return;
    void (async () => {
      const b = await getBridge();
      for (const id of wanted) {
        const data = await b.audioPeaks(
          id,
          Math.round(timelineW / PX_PER_BUCKET),
        );
        if (cancelled) return;
        setPeaks((p) => ({ ...p, [id]: data }));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [clips.map((c) => c.sourceId).join(","), timelineW]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const dpr = window.devicePixelRatio || 1;
    const W = timelineW;
    const H = RULER_H + laneH;
    if (
      canvas.width !== Math.round(W * dpr) ||
      canvas.height !== Math.round(H * dpr)
    ) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.font = `500 9px "JetBrains Mono", ui-monospace, monospace`;

    ctx.fillStyle = C.bg;
    ctx.fillRect(0, 0, W, H);
    ctx.fillStyle = C.lane;
    ctx.fillRect(0, RULER_H, W, laneH);

    // Ruler: a line per beat, a brighter one and a number per bar.
    const spanTicks = Math.ceil(W / pxPerBeat) * TICKS_PER_BEAT;
    for (let tick = 0; tick <= spanTicks; tick += TICKS_PER_BEAT) {
      const x = Math.round(tick * pxPerTick) + 0.5;
      const isBar = tick % barTick === 0;
      ctx.strokeStyle = isBar ? C.bar : C.beat;
      ctx.beginPath();
      ctx.moveTo(x, isBar ? 0 : RULER_H);
      ctx.lineTo(x, H);
      ctx.stroke();
      if (isBar) {
        ctx.fillStyle = C.text;
        ctx.fillText(String(tick / barTick + 1), x + 4, RULER_H / 2);
      }
    }

    // Clips.
    const midY = RULER_H + laneH / 2;
    const half = laneH / 2 - 10;
    for (const clip of shown) {
      const x = clip.startTick * pxPerTick;
      const w = Math.max(3, secondsToTicks(clip.lengthSeconds) * pxPerTick);
      const selected = clip.id === selectedId;
      const source = (model.audioSources ?? []).find(
        (s) => s.id === clip.sourceId,
      );

      ctx.fillStyle = selected ? C.clipSelected : C.clip;
      ctx.fillRect(x, RULER_H + 4, w, laneH - 8);

      const env = peaks[clip.sourceId];
      if (env && env.length) {
        ctx.fillStyle = selected ? C.waveSelected : C.wave;
        ctx.globalAlpha = clip.muted ? 0.3 : 1;
        // The clip shows a window of the source: `offsetSeconds` in, `lengthSeconds` long.
        const total =
          (source?.frames ?? 0) / Math.max(1, source?.sampleRate ?? 1);
        const from = total > 0 ? clip.offsetSeconds / total : 0;
        const span = total > 0 ? clip.lengthSeconds / total : 1;
        for (let px = 0; px < w; px++) {
          const t = from + (px / w) * span;
          const i = Math.min(
            env.length - 1,
            Math.max(0, Math.round(t * env.length)),
          );
          const [lo, hi] = env[i];
          const top = midY - hi * half * clip.gain;
          const bottom = midY - lo * half * clip.gain;
          ctx.fillRect(x + px, top, 1, Math.max(1, bottom - top));
        }
        ctx.globalAlpha = 1;
      } else {
        ctx.fillStyle = C.missing;
        ctx.fillText(source ? "loading…" : "missing audio", x + 6, midY);
      }

      // Fades, drawn as the part of the clip they darken.
      const fadeIn = secondsToTicks(clip.fadeInSeconds) * pxPerTick;
      const fadeOut = secondsToTicks(clip.fadeOutSeconds) * pxPerTick;
      ctx.fillStyle = C.fade;
      ctx.globalAlpha = 0.55;
      if (fadeIn > 0) {
        ctx.beginPath();
        ctx.moveTo(x, RULER_H + 4);
        ctx.lineTo(x + fadeIn, RULER_H + 4);
        ctx.lineTo(x, RULER_H + laneH - 4);
        ctx.closePath();
        ctx.fill();
      }
      if (fadeOut > 0) {
        ctx.beginPath();
        ctx.moveTo(x + w, RULER_H + 4);
        ctx.lineTo(x + w - fadeOut, RULER_H + 4);
        ctx.lineTo(x + w, RULER_H + laneH - 4);
        ctx.closePath();
        ctx.fill();
      }
      ctx.globalAlpha = 1;

      ctx.strokeStyle = selected ? C.borderSelected : C.border;
      ctx.strokeRect(
        Math.round(x) + 0.5,
        RULER_H + 4.5,
        Math.round(w) - 1,
        laneH - 9,
      );

      ctx.fillStyle = C.text;
      ctx.fillText(
        `${source?.name ?? "?"}${clip.muted ? " (muted)" : ""}`,
        x + 6,
        RULER_H + 12,
      );
    }

    // Playhead.
    const px = Math.round(playhead * pxPerTick) + 0.5;
    ctx.strokeStyle = C.playhead;
    ctx.beginPath();
    ctx.moveTo(px, 0);
    ctx.lineTo(px, H);
    ctx.stroke();
  }, [
    shown,
    peaks,
    playhead,
    selectedId,
    timelineW,
    laneH,
    barTick,
    model.tempo,
    model.audioSources,
  ]);

  /** Which handle is under the pointer, for the cursor and for what a drag means. */
  const hitTest = (x: number, y: number) => {
    const tick = x / pxPerTick;
    for (const clip of [...shown].reverse()) {
      const left = clip.startTick * pxPerTick;
      const right = left + secondsToTicks(clip.lengthSeconds) * pxPerTick;
      if (x < left - 4 || x > right + 4) continue;
      const nearStart = x < left + 6;
      const nearEnd = x > right - 6;
      const topHalf = y < RULER_H + 24;
      if (topHalf && nearStart) return { clip, kind: "fade-in" as const, tick };
      if (topHalf && nearEnd) return { clip, kind: "fade-out" as const, tick };
      if (nearStart) return { clip, kind: "trim-start" as const, tick };
      if (nearEnd) return { clip, kind: "trim-end" as const, tick };
      return { clip, kind: "move" as const, tick };
    }
    return null;
  };

  const localPoint = (e: React.PointerEvent | React.MouseEvent) => {
    const rect = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - rect.left, y: e.clientY - rect.top };
  };

  const onPointerDown = (e: React.PointerEvent) => {
    const { x, y } = localPoint(e);
    if (y < RULER_H) {
      seek(Math.max(0, x / pxPerTick));
      return;
    }
    const hit = hitTest(x, y);
    if (!hit) {
      selectClip(null);
      return;
    }
    selectClip(hit.clip.id);
    if (e.button !== 0) return;
    e.currentTarget.setPointerCapture(e.pointerId);
    setPreview(hit.clip);
    switch (hit.kind) {
      case "move":
        setDrag({
          kind: "move",
          id: hit.clip.id,
          grabTick: hit.tick - hit.clip.startTick,
          start: hit.clip,
        });
        break;
      default:
        setDrag({ kind: hit.kind, id: hit.clip.id, start: hit.clip, x0: x });
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const { x } = localPoint(e);
    if (!drag) return;
    const tick = x / pxPerTick;
    const snap = e.shiftKey ? TICKS_PER_BEAT : barTick;
    const ticksToSeconds = (t: number) => ((t / TICKS_PER_BEAT) * 60) / model.tempo;
    let next: AudioClip = { ...drag.start };
    switch (drag.kind) {
      case "move":
        next.startTick = Math.max(
          0,
          Math.round((tick - drag.grabTick) / snap) * snap,
        );
        break;
      case "trim-end": {
        const delta = ticksToSeconds((x - drag.x0) / pxPerTick);
        next.lengthSeconds = Math.max(0.05, drag.start.lengthSeconds + delta);
        break;
      }
      case "trim-start": {
        const delta = ticksToSeconds((x - drag.x0) / pxPerTick);
        // Trimming the front moves the start and the offset together, so the audio under the
        // remaining part does not slide.
        const capped = Math.max(
          -drag.start.offsetSeconds,
          Math.min(delta, drag.start.lengthSeconds - 0.05),
        );
        next.offsetSeconds = drag.start.offsetSeconds + capped;
        next.lengthSeconds = drag.start.lengthSeconds - capped;
        next.startTick = Math.max(
          0,
          Math.round(drag.start.startTick + secondsToTicks(capped)),
        );
        break;
      }
      case "fade-in":
        next.fadeInSeconds = Math.max(
          0,
          ticksToSeconds((x - drag.start.startTick * pxPerTick) / pxPerTick),
        );
        break;
      case "fade-out": {
        const right =
          (drag.start.startTick + secondsToTicks(drag.start.lengthSeconds)) *
          pxPerTick;
        next.fadeOutSeconds = Math.max(
          0,
          ticksToSeconds((right - x) / pxPerTick),
        );
        break;
      }
    }
    setPreview(next);
    void dispatch({ type: "updateAudioClip", clip: next }, true);
  };

  const onPointerUp = () => {
    if (!drag) return;
    setDrag(null);
    setPreview(null);
    void commitGesture();
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const { x, y } = localPoint(e);
    const hit = hitTest(x, y);
    if (!hit) return;
    void dispatch({ type: "removeAudioClip", id: hit.clip.id });
    if (hit.clip.id === selectedId) selectClip(null);
  };

  return (
    <div className="wavescroll" ref={scrollRef}>
      <canvas
        ref={canvasRef}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onContextMenu={onContextMenu}
        style={{ display: "block", touchAction: "none" }}
      />
    </div>
  );
}
