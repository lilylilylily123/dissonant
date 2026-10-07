import { useLayoutEffect, useRef, useState } from "react";
import { beatsPerBar, dbText, useStore } from "../store";
import { DRUM_KIT, TRACK_PALETTE, trackColor, type Clip, type NoteEvent, type ScaleType, type Section, type SongPattern, type Track } from "../types";
import { NOTE_NAMES, noteName } from "../theory";
import { VMeter } from "./Meter";
import { TempoLane } from "./TempoLane";
import { keys } from "../platform";

const HEAD_W = 220;
const SECTION_COLORS = ["#9a9aa4", "#ff9d2a", "#ff3b30", "#3dc8ff", "#b48cff", "#3dffb0"];

type Drag =
  | { kind: "move"; clip: Clip; x0: number; dup: boolean }
  | { kind: "resize"; clip: Clip; x0: number }
  | { kind: "section"; section: Section; x0: number };

/**
 * SONG mode center: ruler with section markers, track headers, and lanes showing one block
 * per clip with that track's notes previewed. Clips: drag to move, right edge to trim or
 * loop-extend, ⌥-drag to copy, right-click to remove, double-click to edit. Sections:
 * double-click the marker strip to add, drag to move, click to edit, right-click to remove.
 */
export function Arrangement() {
  const model = useStore((s) => s.snapshot!.model);
  const playhead = useStore((s) => s.playhead);
  const pxPerBar = useStore((s) => s.arrZoom);
  const setArrZoom = useStore((s) => s.setArrZoom);
  const selectedTrackId = useStore((s) => s.selectedTrackId);
  const selectedPatternId = useStore((s) => s.selectedPatternId);
  const trackPeaks = useStore((s) => s.trackPeaks);
  const bpb = useStore(beatsPerBar);
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

  const pxPerBeat = pxPerBar / bpb;
  const songEnd = model.clips.reduce((max, c) => Math.max(max, c.startBeat + c.lengthBeats), 0);
  const bars = Math.max(Math.ceil((viewW - HEAD_W - 12) / pxPerBar), Math.ceil(songEnd / bpb) + 8);
  const timelineW = bars * pxPerBar;
  const snap = (beat: number) => Math.max(0, Math.round(beat / bpb) * bpb);
  const snapFine = (beat: number) => Math.max(0, Math.round(beat));

  const [drag, setDrag] = useState<Drag | null>(null);
  const [preview, setPreview] = useState<Clip | null>(null);
  const [sectionPreview, setSectionPreview] = useState<Section | null>(null);
  const [editingSection, setEditingSection] = useState<string | null>(null);

  const dispatch = s.dispatch;

  // ── Clip interaction ────────────────────────────────────────────────────────────────────
  const onClipDown = (e: React.PointerEvent, clip: Clip, edge: boolean) => {
    e.stopPropagation();
    if (e.button === 2) {
      void dispatch({ type: "removeClip", id: clip.id });
      return;
    }
    if (e.button !== 0) return;
    s.selectPattern(clip.patternId);
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setDrag(edge ? { kind: "resize", clip, x0: e.clientX } : { kind: "move", clip, x0: e.clientX, dup: e.altKey });
  };
  const onClipMove = (e: React.PointerEvent) => {
    if (!drag || drag.kind === "section") return;
    const dBeats = (e.clientX - drag.x0) / pxPerBeat;
    if (drag.kind === "move") {
      setPreview({ ...drag.clip, startBeat: snap(drag.clip.startBeat + dBeats) });
    } else {
      const len = Math.max(1, (e.shiftKey ? snapFine : snap)(drag.clip.lengthBeats + dBeats));
      setPreview({ ...drag.clip, lengthBeats: len });
    }
  };
  const onClipUp = () => {
    if (!drag || drag.kind === "section") return;
    const d = drag;
    setDrag(null);
    const result = preview;
    setPreview(null);
    if (!result) return;
    if (d.kind === "move" && d.dup) {
      void dispatch({ type: "addClip", patternId: result.patternId, startBeat: result.startBeat, lengthBeats: result.lengthBeats });
    } else if (result.startBeat !== d.clip.startBeat || result.lengthBeats !== d.clip.lengthBeats) {
      void dispatch({ type: "updateClip", clip: result });
    }
  };

  // ── Section interaction ─────────────────────────────────────────────────────────────────
  const onSectionDown = (e: React.PointerEvent, section: Section) => {
    e.stopPropagation();
    if (e.button === 2) {
      void dispatch({ type: "removeSection", id: section.id });
      return;
    }
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    setDrag({ kind: "section", section, x0: e.clientX });
  };
  const onSectionMove = (e: React.PointerEvent) => {
    if (!drag || drag.kind !== "section") return;
    const dBeats = (e.clientX - drag.x0) / pxPerBeat;
    if (Math.abs(e.clientX - drag.x0) > 3) setSectionPreview({ ...drag.section, startBeat: snap(drag.section.startBeat + dBeats) });
  };
  const onSectionUp = (e: React.PointerEvent) => {
    if (!drag || drag.kind !== "section") return;
    const d = drag;
    setDrag(null);
    const moved = sectionPreview;
    setSectionPreview(null);
    if (moved && moved.startBeat !== d.section.startBeat) void dispatch({ type: "updateSection", section: moved });
    else if (Math.abs(e.clientX - d.x0) <= 3) setEditingSection(editingSection === d.section.id ? null : d.section.id);
  };

  const clipsShown = model.clips.map((c) => (preview && c.id === preview.id ? preview : c));
  const sectionsShown = model.sections.map((x) => (sectionPreview && x.id === sectionPreview.id ? sectionPreview : x));
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
              <button className="chip tiny" onClick={() => dispatch({ type: "addTrack", isDrum: false })}>+ inst</button>
              <button className="chip tiny" onClick={() => dispatch({ type: "addTrack", isDrum: true })}>+ drum</button>
            </div>
            <div className="r">
              <span className="mono" style={{ color: "var(--text-5)" }}>{Math.ceil(songEnd / bpb)} bars</span>
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
              if (e.clientY - rect.top < 16) return; // marker strip
              s.seek(Math.max(0, (e.clientX - rect.left) / pxPerBeat));
            }}
            onDoubleClick={(e) => {
              const rect = e.currentTarget.getBoundingClientRect();
              if (e.clientY - rect.top >= 16) return;
              void dispatch({ type: "addSection", name: "", startBeat: snap((e.clientX - rect.left) / pxPerBeat) });
            }}
            title="click to seek · double-click the top strip to add a section"
          >
            {sectionsShown.map((sec, i) => {
              const next = sectionsShown[i + 1];
              const end = next ? next.startBeat : Math.max(songEnd, sec.startBeat + bpb * 4);
              const color = sec.color ?? SECTION_COLORS[i % SECTION_COLORS.length];
              return (
                <div
                  key={sec.id}
                  className="marker"
                  style={{ left: sec.startBeat * pxPerBeat, width: Math.max(24, (end - sec.startBeat) * pxPerBeat - 2), borderLeftColor: color, background: `${color}1a`, color }}
                  onPointerDown={(e) => onSectionDown(e, sec)}
                  onPointerMove={onSectionMove}
                  onPointerUp={onSectionUp}
                  onDoubleClick={(e) => e.stopPropagation()}
                  title={`${sec.name}${sec.key?.rootPitchClass != null ? ` · ${noteName(sec.key.rootPitchClass)} ${sec.key.scale}` : ""} · click: edit · drag: move · right-click: remove`}
                >
                  {sec.name}
                  {sec.key?.rootPitchClass != null && (
                    <span className="mono" style={{ marginLeft: 6, opacity: 0.8, textTransform: "none" }}>
                      {noteName(sec.key.rootPitchClass)} {sec.key.scale === "major" ? "maj" : "min"}
                    </span>
                  )}
                  {editingSection === sec.id && <SectionEditor section={sec} color={color} onClose={() => setEditingSection(null)} />}
                </div>
              );
            })}
            {Array.from({ length: bars }, (_, i) => (
              <div key={i} className={`bar${i % 4 === 0 ? " strong" : ""}`} style={{ left: i * pxPerBar }}>
                {pxPerBar >= 22 || i % 4 === 0 ? i + 1 : ""}
              </div>
            ))}
            <div className="phtri" style={{ left: playhead * pxPerBeat }} />
          </div>
        </div>

        <TempoLane headW={HEAD_W} timelineW={timelineW} pxPerBeat={pxPerBeat} bpb={bpb} />

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
            bpb={bpb}
          >
            {clipsShown.map((clip) => {
              const pattern = model.patterns.find((p) => p.id === clip.patternId);
              if (!pattern) return null;
              return (
                <ClipView
                  key={clip.id}
                  clip={clip}
                  pattern={pattern}
                  track={track}
                  color={trackColor(track, ti)}
                  pxPerBeat={pxPerBeat}
                  selected={pattern.id === selectedPatternId}
                  dragging={drag?.kind !== "section" && drag?.clip.id === clip.id}
                  onDown={onClipDown}
                  onMove={onClipMove}
                  onUp={onClipUp}
                  onDoubleClick={() => (s.selectPattern(pattern.id), s.selectTrack(track.id), s.setMode("pattern"))}
                />
              );
            })}
          </TrackRow>
        ))}

        <div className="addrow" style={{ width: HEAD_W + timelineW + 12 }}>
          <div>{model.clips.length === 0 ? "← press ＋ on a pattern to add it to the song" : keys("＋ patterns on the left · drag clips, trim their right edge, ⌥-drag to copy")}</div>
        </div>
        <div className="playline" style={{ left: playX }} />
      </div>
    </div>
  );
}

function SectionEditor({ section, color, onClose }: { section: Section; color: string; onClose: () => void }) {
  const dispatch = useStore((s) => s.dispatch);
  const [name, setName] = useState(section.name);
  const update = (patch: Partial<Section>) => void dispatch({ type: "updateSection", section: { ...section, ...patch } });
  const root = section.key?.rootPitchClass ?? null;
  const scale: ScaleType = section.key?.scale ?? "major";
  return (
    <div
      className="popover"
      style={{ top: 16, left: 0, width: 260 }}
      onPointerDown={(e) => e.stopPropagation()}
      onPointerUp={(e) => e.stopPropagation()}
      onDoubleClick={(e) => e.stopPropagation()}
    >
      <div className="row" style={{ justifyContent: "space-between" }}>
        <span className="cap">section</span>
        <span className="mono" style={{ fontSize: 9, color: "var(--text-5)" }}>from beat {section.startBeat}</span>
      </div>
      <input
        type="text"
        autoFocus
        value={name}
        onChange={(e) => setName(e.target.value)}
        onBlur={() => name.trim() && name !== section.name && update({ name: name.trim() })}
        onKeyDown={(e) => {
          if (e.key === "Enter") (e.target as HTMLInputElement).blur();
          if (e.key === "Escape") onClose();
        }}
        style={{ width: "100%", margin: "6px 0", fontFamily: "var(--font-ui)", fontWeight: 600, fontSize: 11 }}
      />
      <div className="field">
        <span className="flabel">key</span>
        <div className="row" style={{ gap: 4 }}>
          <select
            value={root ?? ""}
            onChange={(e) => update({ key: e.target.value === "" ? null : { rootPitchClass: parseInt(e.target.value, 10), scale, isLocked: true } })}
            title="a section key tiers the patterns placed inside it against this key"
          >
            <option value="">inherit</option>
            {NOTE_NAMES.map((n, i) => (
              <option key={n} value={i}>{n}</option>
            ))}
          </select>
          <select value={scale} disabled={root === null} onChange={(e) => update({ key: { rootPitchClass: root, scale: e.target.value as ScaleType, isLocked: true } })}>
            <option value="major">Major</option>
            <option value="minor">Minor</option>
          </select>
        </div>
      </div>
      <div className="field">
        <span className="flabel">color</span>
        <div className="swatches" style={{ width: 150 }}>
          {[...SECTION_COLORS, ...TRACK_PALETTE.slice(0, 6)].map((c) => (
            <div key={c} className={c === color ? "on" : ""} style={{ background: c, height: 10 }} onClick={() => update({ color: c })} />
          ))}
        </div>
      </div>
      <div className="row" style={{ gap: 4, marginTop: 6 }}>
        <button className="quiet danger" onClick={() => (onClose(), dispatch({ type: "removeSection", id: section.id }))}>delete</button>
        <span className="spacer" />
        <button className="solid" onClick={onClose}>done</button>
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
  bpb,
  children,
}: {
  track: Track;
  index: number;
  color: string;
  selected: boolean;
  peak: number;
  timelineW: number;
  pxPerBar: number;
  bpb: number;
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
    backgroundSize: `${pxPerBar * 4}px 100%, ${pxPerBar}px 100%, ${pxPerBar / bpb}px 100%`,
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

function ClipView({
  clip,
  pattern,
  track,
  color,
  pxPerBeat,
  selected,
  dragging,
  onDown,
  onMove,
  onUp,
  onDoubleClick,
}: {
  clip: Clip;
  pattern: SongPattern;
  track: Track;
  color: string;
  pxPerBeat: number;
  selected: boolean;
  dragging: boolean;
  onDown: (e: React.PointerEvent, clip: Clip, edge: boolean) => void;
  onMove: (e: React.PointerEvent) => void;
  onUp: (e: React.PointerEvent) => void;
  onDoubleClick: () => void;
}) {
  const notes = pattern.notesByTrack[track.id] ?? [];
  const w = Math.max(6, clip.lengthBeats * pxPerBeat - 1);
  const repeats = clip.lengthBeats / pattern.lengthBeats;
  return (
    <div
      className="clip"
      style={{
        left: clip.startBeat * pxPerBeat,
        width: w,
        background: `${color}24`,
        boxShadow: `inset 0 0 0 1px ${selected ? "#ffffff" : `${color}66`}`,
        opacity: dragging ? 0.75 : clip.muted ? 0.4 : 1,
        zIndex: dragging ? 3 : 1,
      }}
      onPointerDown={(e) => {
        const rect = e.currentTarget.getBoundingClientRect();
        onDown(e, clip, e.clientX > rect.right - 7);
      }}
      onPointerMove={onMove}
      onPointerUp={onUp}
      onDoubleClick={onDoubleClick}
      title={`${pattern.name} · ${notes.length} notes${repeats !== 1 ? ` · ×${+repeats.toFixed(2)}` : ""} · drag: move · right edge: trim/extend · ${keys("⌥-drag")}: copy · double-click: edit · right-click: remove`}
    >
      <div className="ch" style={{ background: color }}>
        {pattern.name}
        {repeats !== 1 && <span style={{ opacity: 0.7, marginLeft: 4 }}>×{+repeats.toFixed(2)}</span>}
      </div>
      <ClipPreview notes={notes} isDrum={track.isDrum} pattern={pattern} clip={clip} w={w} h={33} color={color} />
      <div style={{ position: "absolute", right: 0, top: 0, width: 7, height: "100%", cursor: "ew-resize" }} />
    </div>
  );
}

function ClipPreview({ notes, isDrum, pattern, clip, w, h, color }: { notes: NoteEvent[]; isDrum: boolean; pattern: SongPattern; clip: Clip; w: number; h: number; color: string }) {
  if (notes.length === 0) return <svg width={w} height={h} />;
  const ppb = w / clip.lengthBeats;
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
  // Draw every repetition of the pattern inside the clip, from the offset.
  for (let base = -(clip.offsetBeats % pattern.lengthBeats); base < clip.lengthBeats; base += pattern.lengthBeats) {
    for (const n of notes) {
      const start = base + n.startBeat;
      if (start < 0 || start >= clip.lengthBeats) continue;
      if (isDrum && !DRUM_KIT.some((k) => k.pitch === n.pitch)) continue;
      const p = isDrum ? 1 - DRUM_KIT.findIndex((k) => k.pitch === n.pitch) / Math.max(1, rows - 1) : (n.pitch - lo) / Math.max(1, rows);
      const x = start * ppb;
      const y = 2 + (1 - p) * (h - 6);
      const ww = Math.max(1, Math.min(n.lengthBeats, clip.lengthBeats - start) * ppb - 1);
      d += `M${x.toFixed(1)} ${y.toFixed(1)}h${ww.toFixed(1)}v3h-${ww.toFixed(1)}z`;
    }
  }
  return (
    <svg width={w} height={h}>
      <path d={d} fill={color} fillOpacity={0.9} />
    </svg>
  );
}
