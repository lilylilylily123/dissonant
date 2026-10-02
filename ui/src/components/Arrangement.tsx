import { useLayoutEffect, useRef, useState } from "react";
import { dbText, useStore } from "../store";
import { DRUM_KIT, trackColor, type NoteEvent, type SongPattern, type Track } from "../types";
import { VMeter } from "./Meter";

const HEAD_W = 220;

/** SONG mode center: ruler, track headers + lanes with one clip per arrangement block. */
export function Arrangement() {
  const model = useStore((s) => s.snapshot!.model);
  const playhead = useStore((s) => s.playhead);
  const pxPerBar = useStore((s) => s.arrZoom);
  const setArrZoom = useStore((s) => s.setArrZoom);
  const selectedTrackId = useStore((s) => s.selectedTrackId);
  const selectedPatternId = useStore((s) => s.selectedPatternId);
  const trackPeaks = useStore((s) => s.trackPeaks);
  const s = useStore();

  const scrollerRef = useRef<HTMLDivElement>(null);
  const [viewW, setViewW] = useState(1200);
  useLayoutEffect(() => {
    const el = scrollerRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setViewW(el.clientWidth));
    ro.observe(el);
    setViewW(el.clientWidth);
    return () => ro.disconnect();
  }, []);

  const pxPerBeat = pxPerBar / 4;
  const totalBeats = model.arrangement.reduce((sum, id) => sum + (model.patterns.find((p) => p.id === id)?.lengthBeats ?? 0), 0);
  // Fill the viewport, and always leave room past the end of the song.
  const bars = Math.max(Math.ceil((viewW - HEAD_W - 12) / pxPerBar), Math.ceil(totalBeats / 4) + 8);
  const timelineW = bars * pxPerBar;

  // Clip geometry per arrangement slot.
  let offset = 0;
  const slots = model.arrangement.map((pid, index) => {
    const p = model.patterns.find((x) => x.id === pid);
    const start = offset;
    offset += p?.lengthBeats ?? 0;
    return { index, pattern: p, start };
  });

  const [drag, setDrag] = useState<{ index: number; dx: number } | null>(null);
  const dragRef = useRef<{ index: number; x0: number } | null>(null);

  const reorder = (from: number, to: number) => {
    if (from === to) return;
    const a = [...model.arrangement];
    const [item] = a.splice(from, 1);
    a.splice(to, 0, item);
    void s.dispatch({ type: "setArrangement", arrangement: a });
  };

  const onClipPointerDown = (e: React.PointerEvent, index: number) => {
    const pid = model.arrangement[index];
    if (e.button === 2) {
      void s.dispatch({ type: "setArrangement", arrangement: model.arrangement.filter((_, i) => i !== index) });
      return;
    }
    if (e.button !== 0) return;
    s.selectPattern(pid);
    dragRef.current = { index, x0: e.clientX };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  };
  const onClipPointerMove = (e: React.PointerEvent) => {
    const d = dragRef.current;
    if (!d) return;
    const dx = e.clientX - d.x0;
    if (Math.abs(dx) > 3 || drag) setDrag({ index: d.index, dx });
  };
  const onClipPointerUp = () => {
    const d = dragRef.current;
    dragRef.current = null;
    if (!d || !drag) {
      setDrag(null);
      return;
    }
    const slot = slots[d.index];
    const center = (slot.start + (slot.pattern?.lengthBeats ?? 0) / 2) * pxPerBeat + drag.dx;
    let to = slots.findIndex((sl) => center < (sl.start + (sl.pattern?.lengthBeats ?? 0)) * pxPerBeat);
    if (to < 0) to = slots.length - 1;
    setDrag(null);
    reorder(d.index, to);
  };

  const playX = HEAD_W + playhead * pxPerBeat;

  return (
    <div className="arr">
      <div className="scroller" ref={scrollerRef}>
        <div className="ruler" style={{ width: HEAD_W + timelineW + 12 }}>
          <div className="corner">
            <div className="r">
              <span className="cap" style={{ flex: 1, whiteSpace: "nowrap" }}>
                {model.tracks.length} tracks
              </span>
              <button className="chip tiny" onClick={() => s.dispatch({ type: "addTrack", isDrum: false })}>+ inst</button>
              <button className="chip tiny" onClick={() => s.dispatch({ type: "addTrack", isDrum: true })}>+ drum</button>
            </div>
            <div className="r">
              <span className="mono" style={{ color: "var(--text-5)" }}>{bars} bars</span>
              <span className="spacer" />
              <button className="ico" onClick={() => setArrZoom(pxPerBar / 1.25)}>−</button>
              <span className="mono" style={{ color: "var(--text-5)" }}>1:{Math.round(pxPerBar)}</span>
              <button className="ico" onClick={() => setArrZoom(pxPerBar * 1.25)}>+</button>
            </div>
          </div>
          <div
            className="timeline"
            style={{ width: timelineW }}
            onPointerDown={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              s.seek(Math.max(0, (e.clientX - rect.left) / pxPerBeat));
            }}
            title="click to seek"
          >
            {Array.from({ length: bars }, (_, i) => (
              <div key={i} className={`bar${i % 4 === 0 ? " strong" : ""}`} style={{ left: i * pxPerBar }}>
                {pxPerBar >= 22 || i % 4 === 0 ? i + 1 : ""}
              </div>
            ))}
            <div className="phtri" style={{ left: playhead * pxPerBeat }} />
          </div>
        </div>

        {model.tracks.map((track, ti) => (
          <TrackRow
            key={track.id}
            track={track}
            index={ti}
            color={trackColor(track, ti)}
            selected={track.id === selectedTrackId}
            peak={trackPeaks[ti] ?? 0}
            timelineW={timelineW}
            pxPerBar={pxPerBar}
          >
            {slots.map(({ index, pattern, start }) =>
              pattern ? (
                <Clip
                  key={`${pattern.id}-${index}`}
                  pattern={pattern}
                  track={track}
                  color={trackColor(track, ti)}
                  x={start * pxPerBeat + (drag?.index === index ? drag.dx : 0)}
                  w={pattern.lengthBeats * pxPerBeat - 1}
                  selected={pattern.id === selectedPatternId}
                  dragging={drag?.index === index}
                  onPointerDown={(e) => onClipPointerDown(e, index)}
                  onPointerMove={onClipPointerMove}
                  onPointerUp={onClipPointerUp}
                  onDoubleClick={() => (s.selectPattern(pattern.id), s.selectTrack(track.id), s.setMode("pattern"))}
                />
              ) : null,
            )}
          </TrackRow>
        ))}

        <div className="addrow" style={{ width: HEAD_W + timelineW + 12 }}>
          <div>{model.arrangement.length === 0 ? "← press ＋ on a pattern to add it to the song" : "＋ patterns on the left · drag clips to reorder"}</div>
        </div>
        <div className="playline" style={{ left: playX }} />
      </div>
    </div>
  );
}

function TrackRow({
  track,
  index,
  color,
  selected,
  peak,
  timelineW,
  pxPerBar,
  children,
}: {
  track: Track;
  index: number;
  color: string;
  selected: boolean;
  peak: number;
  timelineW: number;
  pxPerBar: number;
  children: React.ReactNode;
}) {
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);
  const selectTrack = useStore((s) => s.selectTrack);
  const sliderRef = useRef<HTMLDivElement>(null);
  const volPct = Math.min(100, (track.volume / 1.5) * 100);

  const setVolumeFromEvent = (e: React.PointerEvent) => {
    const rect = sliderRef.current!.getBoundingClientRect();
    const p = Math.min(1, Math.max(0, (e.clientX - rect.left) / rect.width));
    void dispatch({ type: "setTrackParam", id: track.id, param: "volume", value: p * 1.5 }, true);
  };

  const gridBg = {
    backgroundImage:
      "linear-gradient(90deg,#2a2a31 1px,transparent 1px),linear-gradient(90deg,#1e1e24 1px,transparent 1px),linear-gradient(90deg,#17171b 1px,transparent 1px)",
    backgroundSize: `${pxPerBar * 4}px 100%, ${pxPerBar}px 100%, ${pxPerBar / 4}px 100%`,
  };

  return (
    <div className="trow" style={{ width: HEAD_W + timelineW + 12 }}>
      <div className={`thead${selected ? " on" : ""}`} onClick={() => selectTrack(track.id)}>
        <div className="strip" style={{ background: color }} />
        <div className="body">
          <div className="l1">
            <span className="num">{String(index + 1).padStart(2, "0")}</span>
            <span className={`name${track.muted ? " muted" : ""}`}>{track.name}</span>
            <span className="type">{track.isDrum ? "drum" : "midi"}</span>
          </div>
          <div className="l2" onClick={(e) => e.stopPropagation()}>
            <button className={`msr m${track.muted ? " on" : ""}`} onClick={() => dispatch({ type: "setTrackMuted", id: track.id, muted: !track.muted })}>M</button>
            <button className={`msr s${track.soloed ? " on" : ""}`} onClick={() => dispatch({ type: "setTrackSoloed", id: track.id, soloed: !track.soloed })}>S</button>
            <div
              className="minislider"
              ref={sliderRef}
              onPointerDown={(e) => {
                (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
                setVolumeFromEvent(e);
              }}
              onPointerMove={(e) => e.buttons === 1 && setVolumeFromEvent(e)}
              onPointerUp={() => void commitGesture()}
              onDoubleClick={() => void dispatch({ type: "setTrackParam", id: track.id, param: "volume", value: 1 })}
              title="volume · double-click resets"
            >
              <div className="fill" style={{ width: `${volPct}%` }} />
              <div className="handle" style={{ left: `${volPct}%` }} />
            </div>
            <span className="db">{dbText(track.volume)}</span>
          </div>
        </div>
        <div className="meters">
          <VMeter peak={peak} height={38} />
          <VMeter peak={peak} height={38} />
        </div>
      </div>
      <div className={`lane${selected ? " on" : ""}`} style={{ width: timelineW, ...gridBg }}>
        {children}
      </div>
    </div>
  );
}

function Clip({
  pattern,
  track,
  color,
  x,
  w,
  selected,
  dragging,
  onPointerDown,
  onPointerMove,
  onPointerUp,
  onDoubleClick,
}: {
  pattern: SongPattern;
  track: Track;
  color: string;
  x: number;
  w: number;
  selected: boolean;
  dragging: boolean;
  onPointerDown: (e: React.PointerEvent) => void;
  onPointerMove: (e: React.PointerEvent) => void;
  onPointerUp: (e: React.PointerEvent) => void;
  onDoubleClick: () => void;
}) {
  const notes = pattern.notesByTrack[track.id] ?? [];
  return (
    <div
      className="clip"
      style={{
        left: x,
        width: Math.max(6, w),
        background: `${color}24`,
        boxShadow: `inset 0 0 0 1px ${selected ? "#ffffff" : `${color}66`}`,
        opacity: dragging ? 0.75 : 1,
        zIndex: dragging ? 3 : 1,
      }}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onDoubleClick={onDoubleClick}
      title={`${pattern.name} · ${notes.length} notes · double-click to edit · right-click to remove`}
    >
      <div className="ch" style={{ background: color }}>
        {pattern.name}
      </div>
      <MidiPreview notes={notes} isDrum={track.isDrum} lengthBeats={pattern.lengthBeats} w={Math.max(6, w)} h={33} color={color} />
    </div>
  );
}

function MidiPreview({ notes, isDrum, lengthBeats, w, h, color }: { notes: NoteEvent[]; isDrum: boolean; lengthBeats: number; w: number; h: number; color: string }) {
  if (notes.length === 0) return <svg width={w} height={h} />;
  const ppb = w / lengthBeats;
  let lo = 36;
  let hi = 84;
  if (!isDrum) {
    lo = Math.min(...notes.map((n) => n.pitch));
    hi = Math.max(...notes.map((n) => n.pitch));
    if (hi - lo < 12) {
      const mid = (hi + lo) / 2;
      lo = mid - 6;
      hi = mid + 6;
    }
  }
  const rows = isDrum ? DRUM_KIT.length : hi - lo;
  let d = "";
  for (const n of notes) {
    const p = isDrum ? 1 - DRUM_KIT.findIndex((k) => k.pitch === n.pitch) / Math.max(1, rows - 1) : (n.pitch - lo) / Math.max(1, rows);
    if (isDrum && !DRUM_KIT.some((k) => k.pitch === n.pitch)) continue;
    const x = n.startBeat * ppb;
    const y = 2 + (1 - p) * (h - 6);
    const ww = Math.max(1, n.lengthBeats * ppb - 1);
    d += `M${x.toFixed(1)} ${y.toFixed(1)}h${ww.toFixed(1)}v3h-${ww.toFixed(1)}z`;
  }
  return (
    <svg width={w} height={h}>
      <path d={d} fill={color} fillOpacity={0.9} />
    </svg>
  );
}
