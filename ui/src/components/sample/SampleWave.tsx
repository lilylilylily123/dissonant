import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { getBridge } from "../../bridge";
import { usePixelRatio } from "../../uiScale";
import { midiName } from "../../theory";
import type { AudioSource, Sampler } from "../../types";
import { chopAt, clampView, cutAt, DEFAULT_CHOPPER, deleteChop, dragEdge, dragRegionEdge, edgeAt, pxPerSecond, rechopped, regionEdgeAt, regionOf, secondsToX, sweptRegion, xToSeconds, zoomAround, type Edge, type WaveView } from "../../sampleChops";
import { MAX_CHOPS, sourceSeconds } from "../../sampler";

/**
 * Canvas colours. `getContext("2d")` cannot resolve `var(--…)`, so the palette is duplicated
 * here from the token set rather than read off the document (as in `WaveformLane`).
 */
const C = {
  bg: "#0b0b0d",
  lane: "#101013",
  ruler: "#121215",
  tick: "#26262d",
  tickText: "#5f5f68",
  wave: "#5f5f68",
  onset: "rgba(230,230,234,0.16)",
  edge: "rgba(230,230,234,0.35)",
  selected: "#f2f2f4",
  label: "#0b0b0d",
  playhead: "#ff5f56",
  overview: "#16161a",
  viewport: "rgba(180,140,255,0.35)",
  missing: "#ff9d2a",
  /** The region: a band in the ruler, flags at its edges, the source outside it shaded. */
  region: "rgba(242,242,244,0.16)",
  regionEdge: "#f2f2f4",
  outside: "rgba(6,6,8,0.62)",
};
/**
 * One hue per chop, cycling. The chop number drawn on every region is the second channel, so
 * neighbours never have to be told apart by colour alone.
 */
export const CHOP_HUES = ["#b48cff", "#3dc8ff", "#3dffb0", "#ffb02a", "#ff6a9a", "#7fd1ff", "#c8ff3d", "#ff8a3d"];
const RULER_H = 18;
const OVERVIEW_H = 14;
/** How near (px) the pointer has to be to grab a chop edge. */
const EDGE_GRAB_PX = 5;
/** How near (px) the pointer has to be to grab a region flag: wider, the ruler being only a strip. */
const REGION_GRAB_PX = 7;
/** How near (px) an edge snaps to an onset while ⇧ is held. */
const SNAP_PX = 10;
/** Most buckets one peaks request asks for (the Rust side caps there too). */
const MAX_BUCKETS = 8192;

/** A chop being auditioned, for the moving playhead: when it started and how long it plays. */
export interface Playing {
  index: number;
  startedAt: number;
  seconds: number;
}

/**
 * `edge`: a chop edge. `region`: a region edge, re-cutting as it goes. `sweep`: a new region
 * swept along the ruler from `anchor`. `start` is the sampler as the gesture found it.
 */
type Drag =
  | { kind: "edge"; index: number; edge: Edge; start: Sampler }
  | { kind: "region"; edge: Edge; start: Sampler }
  | { kind: "sweep"; anchor: number; start: Sampler }
  | { kind: "overview" };

/** What the pointer would grab, for the cursor. */
type Hover = "edge" | "region" | "ruler" | null;

/**
 * The sampler's source, big: chop regions coloured and numbered over the waveform, the
 * analysis's onsets as faint ticks along the bottom, an overview strip to pan with, and the
 * sampler's region — where the chop tools cut — as a band on the ruler with the source outside
 * it shaded.
 *
 * Drag an edge to move it (a shared cut moves both chops; ⌥ moves only this one, ⇧ snaps to
 * an onset), double-click to cut, right-click a chop to delete it, click one to select and hear
 * it. ⌘/ctrl-wheel zooms at the pointer, the wheel pans. On the ruler: drag a region flag to
 * move that edge, drag anywhere else to sweep a new region (⇧ snaps either to a hit); both
 * re-cut the region with the last tool used as they go, one undo step per drag. The ruler is
 * the region's own strip because every other surface already has a drag: the lane moves chop
 * edges and the overview pans.
 */
export function SampleWave({
  sampler,
  source,
  selected,
  playing,
  view,
  setView,
  onSelect,
  onAudition,
  onEdit,
  onCommit,
  onRefuse,
  beatsPerBar,
}: {
  sampler: Sampler;
  source: AudioSource;
  selected: number;
  playing: Playing | null;
  view: WaveView;
  setView: (v: WaveView) => void;
  onSelect: (index: number) => void;
  onAudition: (index: number) => void;
  /** `transient` while a drag is under way; the drag's release calls `onCommit`. */
  onEdit: (next: Sampler, transient: boolean, label: string) => void;
  onCommit: () => void;
  onRefuse: (why: string) => void;
  /** The song's bar, for the grid tool when a region edit re-cuts. */
  beatsPerBar: number;
}) {
  const duration = sourceSeconds(source);
  const boxRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const dpr = usePixelRatio();
  const [size, setSize] = useState({ w: 800, h: 240 });
  const [peaks, setPeaks] = useState<{ id: string; data: [number, number][] } | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  /** The sampler as the drag has it, so the canvas follows the pointer without a round trip. */
  const [preview, setPreview] = useState<Sampler | null>(null);
  const [hover, setHover] = useState<Hover>(null);
  const [now, setNow] = useState(0);
  const shown = preview ?? sampler;
  const laneH = Math.max(40, size.h - RULER_H - OVERVIEW_H);

  useLayoutEffect(() => {
    const el = boxRef.current;
    if (!el) return;
    const measure = () => setSize({ w: Math.max(100, el.clientWidth), h: Math.max(120, el.clientHeight) });
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    measure();
    return () => ro.disconnect();
  }, []);

  // Enough buckets for one per pixel at this zoom, in powers of two so a zoom step only
  // refetches when the detail would really change.
  const buckets = Math.min(MAX_BUCKETS, Math.max(512, 2 ** Math.ceil(Math.log2(size.w * view.zoom))));
  const [peakBuckets, setPeakBuckets] = useState(0);
  useEffect(() => {
    if (peaks?.id === source.id && peakBuckets >= buckets) return;
    let cancelled = false;
    void getBridge()
      .then((b) => b.audioPeaks(source.id, buckets))
      .then((data) => {
        if (cancelled) return;
        setPeaks({ id: source.id, data });
        setPeakBuckets(buckets);
      });
    return () => {
      cancelled = true;
    };
  }, [source.id, buckets]);

  // The auditioned chop's playhead runs on its own clock; it stops when the chop ends.
  useEffect(() => {
    if (!playing) return;
    let raf = 0;
    const tick = () => {
      const t = performance.now();
      setNow(t);
      if (t - playing.startedAt < playing.seconds * 1000) raf = requestAnimationFrame(tick);
    };
    raf = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(raf);
  }, [playing]);

  // Wheel: a non-passive listener, so zooming does not also scroll the page.
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const x = e.clientX - el.getBoundingClientRect().left;
      if (e.metaKey || e.ctrlKey) {
        setView(zoomAround(view, view.zoom * (e.deltaY < 0 ? 1.15 : 1 / 1.15), x, duration, size.w));
      } else {
        const pps = pxPerSecond(view, duration, size.w);
        const delta = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
        if (pps > 0) setView(clampView({ zoom: view.zoom, offset: view.offset + delta / pps }, duration));
      }
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [view, duration, size.w, setView]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const W = size.w;
    const H = size.h;
    if (canvas.width !== Math.round(W * dpr) || canvas.height !== Math.round(H * dpr)) {
      canvas.width = Math.round(W * dpr);
      canvas.height = Math.round(H * dpr);
      canvas.style.width = `${W}px`;
      canvas.style.height = `${H}px`;
    }
    const ctx = canvas.getContext("2d")!;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.textBaseline = "middle";
    ctx.fillStyle = C.bg;
    ctx.fillRect(0, 0, W, H);
    ctx.fillStyle = C.lane;
    ctx.fillRect(0, RULER_H, W, laneH);
    const X = (s: number) => secondsToX(s, view, duration, W);
    const chops = shown.chops;
    const region = regionOf(shown, duration);
    const rx0 = X(region.startSeconds);
    const rx1 = X(region.endSeconds);

    // Ruler: the region as a band, then a tick every "nice" step that leaves ~70 px between labels.
    ctx.fillStyle = C.ruler;
    ctx.fillRect(0, 0, W, RULER_H);
    ctx.fillStyle = C.region;
    ctx.fillRect(rx0, 0, rx1 - rx0, RULER_H);
    const pps = pxPerSecond(view, duration, W);
    const step = [0.01, 0.02, 0.05, 0.1, 0.2, 0.5, 1, 2, 5, 10, 15, 30, 60].find((s) => s * pps >= 70) ?? 60;
    ctx.font = `500 9px "JetBrains Mono", ui-monospace, monospace`;
    for (let t = Math.ceil(view.offset / step) * step; X(t) < W; t += step) {
      const x = Math.round(X(t)) + 0.5;
      ctx.fillStyle = C.tick;
      ctx.fillRect(x, RULER_H - 5, 1, 5);
      ctx.fillStyle = C.tickText;
      ctx.fillText(step < 1 ? `${t.toFixed(step < 0.1 ? 2 : 1)}s` : `${Math.round(t)}s`, x + 3, RULER_H / 2);
    }

    // Chop regions under the waveform.
    chops.forEach((c, i) => {
      const x0 = X(c.startSeconds);
      const x1 = X(c.endSeconds);
      if (x1 < 0 || x0 > W) return;
      ctx.globalAlpha = i === selected ? 0.2 : 0.08;
      ctx.fillStyle = CHOP_HUES[i % CHOP_HUES.length];
      ctx.fillRect(x0, RULER_H, x1 - x0, laneH);
      ctx.globalAlpha = 1;
    });

    // Waveform: each pixel column is the envelope over the source seconds it spans, tinted
    // with the chop it falls in.
    const env = peaks?.id === source.id ? peaks.data : null;
    const mid = RULER_H + laneH / 2;
    const half = laneH / 2 - 6;
    if (env && env.length && duration > 0) {
      const n = env.length;
      for (let x = 0; x < W; x++) {
        const t0 = xToSeconds(x, view, duration, W);
        const t1 = xToSeconds(x + 1, view, duration, W);
        if (t1 <= 0 || t0 >= duration) continue;
        const i0 = Math.max(0, Math.floor((t0 / duration) * n));
        const i1 = Math.min(n - 1, Math.max(i0, Math.ceil((t1 / duration) * n) - 1));
        let lo = 0;
        let hi = 0;
        for (let i = i0; i <= i1; i++) {
          lo = Math.min(lo, env[i][0]);
          hi = Math.max(hi, env[i][1]);
        }
        const at = chopAt(chops, (t0 + t1) / 2);
        ctx.fillStyle = at >= 0 ? CHOP_HUES[at % CHOP_HUES.length] : C.wave;
        ctx.globalAlpha = at >= 0 ? (at === selected ? 1 : 0.75) : 0.5;
        const top = mid - hi * half;
        ctx.fillRect(x, top, 1, Math.max(1, mid - lo * half - top));
      }
      ctx.globalAlpha = 1;
    } else {
      ctx.fillStyle = C.missing;
      ctx.fillText("loading…", 10, mid);
    }

    // Onsets: faint ticks up from the bottom, as tall as they are strong.
    ctx.fillStyle = C.onset;
    for (const o of shown.analysis?.onsets ?? []) {
      const x = Math.round(X(o.seconds));
      if (x < 0 || x > W) continue;
      const h = 4 + o.strength * 18;
      ctx.fillRect(x, RULER_H + laneH - h, 1, h);
    }

    // Edges and labels on top; the selected chop last, so a sliver of a chop stays findable.
    ctx.font = `700 9px "JetBrains Mono", ui-monospace, monospace`;
    const order = chops.map((_, i) => i).filter((i) => i !== selected);
    if (chops[selected]) order.push(selected);
    order.forEach((i) => {
      const c = chops[i];
      const x0 = X(c.startSeconds);
      const x1 = X(c.endSeconds);
      if (x1 < 0 || x0 > W) return;
      const hue = CHOP_HUES[i % CHOP_HUES.length];
      const sel = i === selected;
      ctx.fillStyle = sel ? C.selected : hue;
      ctx.fillRect(Math.round(x0), RULER_H, sel ? 2 : 1, laneH);
      ctx.fillStyle = sel ? C.selected : C.edge;
      ctx.fillRect(Math.round(x1) - 1, RULER_H, 1, laneH);
      // The number tag: always the chop number; the key and the name when there is room.
      const label = `${i + 1}`;
      const key = midiName(shown.basePitch + i);
      const room = x1 - x0;
      const full = c.name ? `${label} ${c.name}` : `${label} ${key}`;
      const text = room > ctx.measureText(full).width + 10 ? full : label;
      const tw = ctx.measureText(text).width + 6;
      if (room < tw && !sel) return;
      const lx = Math.max(0, x0) + (sel ? 2 : 1);
      ctx.fillStyle = hue;
      ctx.globalAlpha = sel ? 1 : 0.85;
      ctx.fillRect(lx, RULER_H + 2, tw, 13);
      ctx.globalAlpha = 1;
      ctx.fillStyle = C.label;
      ctx.fillText(text, lx + 3, RULER_H + 9);
      if (sel) {
        ctx.strokeStyle = C.selected;
        ctx.lineWidth = 1;
        ctx.strokeRect(Math.round(x0) + 0.5, RULER_H + 0.5, Math.round(x1 - x0) - 1, laneH - 1);
      }
    });

    // Outside the region: shaded, so what the tools will cut stands out; its edges are flags
    // in the ruler (grab them there) with a hairline down through the lane.
    ctx.fillStyle = C.outside;
    if (rx0 > 0) ctx.fillRect(0, RULER_H, Math.min(W, rx0), laneH);
    if (rx1 < W) ctx.fillRect(Math.max(0, rx1), RULER_H, W - Math.max(0, rx1), laneH);
    ctx.fillStyle = C.regionEdge;
    for (const [x, dir] of [[rx0, 1], [rx1, -1]] as const) {
      if (x < -8 || x > W + 8) continue;
      const px = Math.round(x);
      ctx.globalAlpha = 0.55;
      ctx.fillRect(px - (dir < 0 ? 1 : 0), RULER_H, 1, laneH);
      ctx.globalAlpha = 1;
      ctx.fillRect(px - (dir < 0 ? 2 : 0), 0, 2, RULER_H);
      // A pennant pointing into the region.
      ctx.beginPath();
      ctx.moveTo(px, 1);
      ctx.lineTo(px + dir * 7, 5);
      ctx.lineTo(px, 9);
      ctx.closePath();
      ctx.fill();
    }

    // The auditioned chop's playhead.
    if (playing) {
      const c = chops[playing.index];
      const elapsed = (now - playing.startedAt) / 1000;
      if (c && elapsed >= 0 && elapsed < playing.seconds) {
        const t = c.reverse ? c.endSeconds - elapsed : c.startSeconds + elapsed;
        const x = Math.round(X(t)) + 0.5;
        ctx.fillStyle = C.playhead;
        ctx.fillRect(x, RULER_H, 1, laneH);
      }
    }

    // Overview strip: the whole source, the chops as bands, the visible window as a box.
    const oy = RULER_H + laneH;
    ctx.fillStyle = C.overview;
    ctx.fillRect(0, oy, W, OVERVIEW_H);
    if (duration > 0) {
      chops.forEach((c, i) => {
        ctx.fillStyle = CHOP_HUES[i % CHOP_HUES.length];
        ctx.globalAlpha = i === selected ? 0.9 : 0.4;
        ctx.fillRect((c.startSeconds / duration) * W, oy + 4, Math.max(1, ((c.endSeconds - c.startSeconds) / duration) * W - 1), OVERVIEW_H - 8);
      });
      ctx.globalAlpha = 1;
      const ox0 = (region.startSeconds / duration) * W;
      const ox1 = (region.endSeconds / duration) * W;
      ctx.fillStyle = C.outside;
      ctx.fillRect(0, oy, ox0, OVERVIEW_H);
      ctx.fillRect(ox1, oy, W - ox1, OVERVIEW_H);
      ctx.fillStyle = C.regionEdge;
      ctx.fillRect(Math.round(ox0), oy + 2, 1, OVERVIEW_H - 4);
      ctx.fillRect(Math.max(0, Math.round(ox1) - 1), oy + 2, 1, OVERVIEW_H - 4);
      ctx.strokeStyle = C.viewport;
      ctx.strokeRect((view.offset / duration) * W + 0.5, oy + 1.5, Math.max(4, W / view.zoom - 1), OVERVIEW_H - 3);
    }
  }, [shown, peaks, view, size, laneH, selected, playing, now, dpr, duration, source.id]);

  const local = (e: React.PointerEvent | React.MouseEvent) => {
    const r = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - r.left, y: e.clientY - r.top };
  };
  const inOverview = (y: number) => y >= RULER_H + laneH;
  const panTo = (x: number) => setView(clampView({ zoom: view.zoom, offset: (x / size.w) * duration - duration / view.zoom / 2 }, duration));
  /** The source second under `x`, pulled onto the nearest onset in reach when `snap` (⇧). */
  const secondsAt = (x: number, snap: boolean) => {
    const t = xToSeconds(x, view, duration, size.w);
    if (!snap) return t;
    const pps = pxPerSecond(view, duration, size.w);
    const near = (sampler.analysis?.onsets ?? []).reduce<number | null>((best, o) => (Math.abs(o.seconds - t) * pps <= SNAP_PX && (best === null || Math.abs(o.seconds - t) < Math.abs(best - t)) ? o.seconds : best), null);
    return near ?? t;
  };

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    const { x, y } = local(e);
    if (inOverview(y)) {
      e.currentTarget.setPointerCapture(e.pointerId);
      setDrag({ kind: "overview" });
      panTo(x);
      return;
    }
    if (y < RULER_H) {
      e.currentTarget.setPointerCapture(e.pointerId);
      const edge = regionEdgeAt(regionOf(sampler, duration), x, view, duration, size.w, REGION_GRAB_PX);
      setDrag(edge ? { kind: "region", edge, start: sampler } : { kind: "sweep", anchor: secondsAt(x, e.shiftKey), start: sampler });
      return;
    }
    const hit = edgeAt(sampler.chops, x, view, duration, size.w, EDGE_GRAB_PX);
    if (hit) {
      e.currentTarget.setPointerCapture(e.pointerId);
      onSelect(hit.index);
      setDrag({ kind: "edge", index: hit.index, edge: hit.edge, start: sampler });
      setPreview(sampler);
      return;
    }
    // A double-click cuts; its first click must not also start a sound.
    if (e.detail > 1) return;
    const at = chopAt(sampler.chops, xToSeconds(x, view, duration, size.w));
    if (at >= 0) {
      onSelect(at);
      onAudition(at);
    }
  };

  const onPointerMove = (e: React.PointerEvent) => {
    const { x, y } = local(e);
    if (!drag) {
      if (y < RULER_H) setHover(regionEdgeAt(regionOf(sampler, duration), x, view, duration, size.w, REGION_GRAB_PX) ? "region" : "ruler");
      else setHover(!inOverview(y) && edgeAt(sampler.chops, x, view, duration, size.w, EDGE_GRAB_PX) !== null ? "edge" : null);
      return;
    }
    if (drag.kind === "overview") {
      panTo(x);
      return;
    }
    const t = secondsAt(x, e.shiftKey);
    if (drag.kind === "edge") {
      const next = dragEdge(drag.start, drag.index, drag.edge, t, duration, !e.altKey);
      setPreview(next);
      onEdit(next, true, "move chop edge");
      return;
    }
    // A region edit re-cuts with the tool used last, carrying each chop's name and shaping
    // over from the chops the drag began with.
    const region = drag.kind === "region" ? dragRegionEdge(regionOf(drag.start, duration), drag.edge, t, duration) : sweptRegion(drag.anchor, t, duration);
    if (!region) return;
    const next = rechopped(drag.start, drag.start.chopper ?? DEFAULT_CHOPPER, region, beatsPerBar, drag.start.chops);
    if (!next) return;
    setPreview(next);
    onEdit(next, true, drag.kind === "region" ? "move sample region" : "new sample region");
  };

  const onPointerUp = () => {
    // A region click that never swept sent nothing, so there is no gesture to close.
    if (drag?.kind === "edge" || ((drag?.kind === "region" || drag?.kind === "sweep") && preview)) onCommit();
    setDrag(null);
    setPreview(null);
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    const { x, y } = local(e);
    if (y < RULER_H || inOverview(y)) return;
    const cut = cutAt(sampler, xToSeconds(x, view, duration, size.w), duration);
    if (!cut) {
      onRefuse(sampler.chops.length >= MAX_CHOPS ? `a sampler holds up to ${MAX_CHOPS} chops` : "too close to a chop edge to cut there");
      return;
    }
    onEdit(cut.sampler, false, "cut chop");
    onSelect(cut.index);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const { x, y } = local(e);
    if (y < RULER_H || inOverview(y)) return;
    const at = chopAt(sampler.chops, xToSeconds(x, view, duration, size.w));
    if (at < 0) return;
    onEdit(deleteChop(sampler, at), false, "delete chop");
    onSelect(Math.max(0, Math.min(at, sampler.chops.length - 2)));
  };

  return (
    <div className="smp-wave" ref={boxRef}>
      <canvas
        ref={canvasRef}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={onPointerUp}
        onPointerLeave={() => !drag && setHover(null)}
        onDoubleClick={onDoubleClick}
        onContextMenu={onContextMenu}
        style={{ cursor: drag?.kind === "edge" || drag?.kind === "region" || hover === "edge" || hover === "region" ? "ew-resize" : drag?.kind === "sweep" || hover === "ruler" ? "crosshair" : "pointer" }}
        aria-label="the sample's waveform and its chops: drag an edge to move it, double-click to cut, right-click a chop to delete it; on the ruler, drag the region's flags or sweep a new region"
      />
    </div>
  );
}
