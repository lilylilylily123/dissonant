import { GRID_OPTIONS, selectedPattern, selectedTrack, selectedTrackIndex, useStore } from "../store";
import { trackColor } from "../types";
import { ChordLane } from "./ChordLane";
import { DrumGrid } from "./DrumGrid";
import { PianoRoll } from "./PianoRoll";

/** PAT mode editor column: toolbar · chord lane · piano roll (or drum grid) · velocity lane. */
export function Editor() {
  const s = useStore();
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const idx = useStore(selectedTrackIndex);
  if (!pattern || !track) return null;
  const color = trackColor(track, idx);
  const notes = pattern.notesByTrack[track.id] ?? [];

  return (
    <div className="editor">
      <div className="toolbar">
        <div className="title">
          <span className="sq" style={{ background: color }} />
          <b>{track.isDrum ? "step sequencer" : "piano roll"}</b>
          <span className="meta">
            {String(idx + 1).padStart(2, "0")} {track.name} / {pattern.name}
          </span>
        </div>
        <div className="vdiv" />
        <div className="opt">
          <span className="k">snap</span>
          <select value={s.noteLength} onChange={(e) => s.setNoteLength(parseFloat(e.target.value))}>
            {GRID_OPTIONS.map(([l, v]) => (
              <option key={l} value={v}>{l}</option>
            ))}
          </select>
        </div>
        <button className={`chip${s.highlightRows ? " on" : ""}`} onClick={() => s.toggleHighlight()} title="tint rows by tier">tiers</button>
        <button className={`chip${s.showLandscape ? " on" : ""}`} onClick={() => s.toggleLandscape()} title="harmonic map">map</button>
        <button className={`chip${s.hearChords ? " on" : ""}`} onClick={() => s.toggleHearChords()} title="hear the chord bed">chords</button>
        <span className="spacer" />
        <span className="mono" style={{ fontSize: 9.5, color: "var(--text-4)" }}>
          {notes.length} notes{s.selectedNoteIds.length ? ` · ${s.selectedNoteIds.length} sel` : ""}
        </span>
        <div className="zoom">
          <span className="flabel">zoom</span>
          <input type="range" min={0.25} max={3} step={0.05} value={s.zoom} onChange={(e) => s.setZoom(parseFloat(e.target.value))} />
        </div>
        <span
          className="chip"
          style={{ cursor: "help" }}
          title={
            "click: place a note (keep dragging to move it)\ndrag: move · right edge: resize · ⌥-drag: paint / duplicate\n⇧-drag: marquee · ⇧-click: toggle · right-click: erase\n⌫ delete · ⌘A ⌘C ⌘X ⌘V ⌘D · arrows nudge (⇧ = octave / bar)\n[ ] velocity · ⌘± zoom · ruler: seek · velocity lane: drag"
          }
        >
          ?
        </span>
      </div>
      <ChordLane />
      {track.isDrum ? <DrumGrid /> : <PianoRoll />}
    </div>
  );
}
