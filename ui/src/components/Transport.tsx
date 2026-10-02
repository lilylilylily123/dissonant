import { useEffect, useState } from "react";
import { GRID_OPTIONS, peakDb, selectedPattern, useStore } from "../store";
import { detectKey, keyName } from "../theory";
import { HMeter } from "./Meter";

function pad(n: number, w: number) {
  return String(n).padStart(w, "0");
}

export function Transport() {
  const s = useStore();
  const model = s.snapshot!.model;
  const pattern = useStore(selectedPattern);
  const [editingBpm, setEditingBpm] = useState(false);
  const [bpmText, setBpmText] = useState(model.tempo.toFixed(3));
  useEffect(() => setBpmText(model.tempo.toFixed(3)), [model.tempo]);

  // Position: BBB.B.SS (bar . beat . sixteenth) and MM:SS.mmm
  const beat = s.playhead;
  const bar = Math.floor(beat / 4) + 1;
  const bib = Math.floor(beat % 4) + 1;
  const six = Math.floor((beat % 1) * 4) + 1;
  const secs = (beat * 60) / model.tempo;
  const mm = Math.floor(secs / 60);
  const ss = secs - mm * 60;

  // Key: locked value, or a suggestion from the melodic notes of the selected pattern.
  const drumIds = new Set(model.tracks.filter((t) => t.isDrum).map((t) => t.id));
  const pcs = pattern
    ? Object.entries(pattern.notesByTrack)
        .filter(([tid]) => !drumIds.has(tid))
        .flatMap(([, notes]) => notes.map((n) => n.pitch))
    : [];
  const detected = detectKey(pcs);
  const suggestion = !model.key.isLocked && detected.isConfident ? detected.candidates[0] : null;

  const commitBpm = () => {
    const v = parseFloat(bpmText);
    if (Number.isFinite(v)) void s.dispatch({ type: "setTempo", bpm: Math.min(300, Math.max(20, v)) });
    setEditingBpm(false);
  };

  const [pl, pr] = s.masterPeak;
  const peak = Math.max(pl, pr);

  return (
    <div className="transport">
      <div className="tgroup">
        <span className="cap">mode</span>
        <div className="seg">
          <div className={s.mode === "pattern" ? "on" : ""} onClick={() => s.setMode("pattern")}>pat</div>
          <div className={s.mode === "song" ? "on" : ""} onClick={() => s.setMode("song")}>song</div>
        </div>
      </div>
      <div className="tdiv" />
      <div className="tbtns">
        <button className="tbtn" onClick={() => s.rewind()} title="return to start (Enter)">
          <svg width="14" height="12" viewBox="0 0 14 12">
            <rect x="0" y="0" width="2" height="12" fill="#9a9aa4" />
            <polygon points="13,0 13,12 3,6" fill="#9a9aa4" />
          </svg>
        </button>
        <button className="tbtn" onClick={() => s.stop()} title="stop (space)">
          <div style={{ width: 11, height: 11, background: "#9a9aa4" }} />
        </button>
        <button className={`tbtn play${s.playing ? "" : " off"}`} onClick={() => s.play()} title="play (space)">
          <svg width="12" height="14" viewBox="0 0 12 14">
            <polygon points="0,0 12,7 0,14" fill={s.playing ? "#0b0b0d" : "#9a9aa4"} />
          </svg>
        </button>
      </div>
      <div className="lcd">
        <div className="tgroup">
          <span className="big">
            {pad(bar, 3)}.{bib}.{pad(six, 2)}
          </span>
          <span className="micro">bar · beat · 16th</span>
        </div>
        <div style={{ width: 1, height: 28, background: "var(--line-0)" }} />
        <div className="tgroup">
          <span className="time">
            {pad(mm, 2)}:{ss.toFixed(3).padStart(6, "0")}
          </span>
          <span className="micro">min:sec.ms</span>
        </div>
      </div>
      <div className="lcd" style={{ padding: "0 10px", gap: 10 }}>
        <div className="tgroup">
          {editingBpm ? (
            <input
              type="text"
              className="mono"
              autoFocus
              value={bpmText}
              onChange={(e) => setBpmText(e.target.value)}
              onBlur={commitBpm}
              onKeyDown={(e) => {
                if (e.key === "Enter") commitBpm();
                if (e.key === "Escape") setEditingBpm(false);
              }}
            />
          ) : (
            <span
              className="bpm"
              onClick={() => (setBpmText(model.tempo.toFixed(3)), setEditingBpm(true))}
              onWheel={(e) => {
                e.preventDefault();
                void s.dispatch({ type: "setTempo", bpm: Math.round(model.tempo) + (e.deltaY < 0 ? 1 : -1) });
              }}
              title="click to type · scroll to nudge"
            >
              {model.tempo.toFixed(3)}
            </span>
          )}
          <span className="micro">bpm</span>
        </div>
        <button className="chip tiny" onClick={() => s.tapTempo()} title="tap tempo">tap</button>
      </div>
      <div className="grid22">
        <span className="k">sig</span>
        <span className="v">4 / 4</span>
        <span className="k">key</span>
        <span className="v">
          {model.key.isLocked && model.key.rootPitchClass !== null ? (
            <>
              <span>{keyName(model.key.rootPitchClass, model.key.scale)}</span>
              <button className="ico" title="unlock key" onClick={() => s.dispatch({ type: "setKey", key: { rootPitchClass: null, scale: "major", isLocked: false } })}>×</button>
            </>
          ) : suggestion ? (
            <button
              className="chip tiny on"
              title="the notes you placed look like this key — lock it?"
              onClick={() =>
                s.dispatch({ type: "setKey", key: { rootPitchClass: suggestion.rootPitchClass, scale: suggestion.scale, isLocked: true } })
              }
            >
              {keyName(suggestion.rootPitchClass, suggestion.scale)}? lock
            </button>
          ) : (
            <span style={{ color: "var(--text-5)" }}>—</span>
          )}
        </span>
      </div>
      <div className="grid22">
        <span className="k">grid</span>
        <span className="v">
          <select value={s.noteLength} onChange={(e) => s.setNoteLength(parseFloat(e.target.value))} style={{ height: 18, padding: "0 14px 0 4px" }}>
            {GRID_OPTIONS.map(([l, v]) => (
              <option key={l} value={v}>{l}</option>
            ))}
          </select>
        </span>
        <span className="k">zoom</span>
        <span className="v">
          <button className="ico" onClick={() => s.setZoom(s.zoom / 1.25)}>−</button>
          <span>{Math.round(s.zoom * 100)}%</span>
          <button className="ico" onClick={() => s.setZoom(s.zoom * 1.25)}>+</button>
        </span>
      </div>
      <span className="spacer" />
      <div className="row" style={{ gap: 3 }}>
        <button className={`chip${s.highlightRows ? " on" : ""}`} onClick={() => s.toggleHighlight()} title="tint piano-roll rows by how each note fits the chord under the playhead">tiers</button>
        <button className={`chip${s.showLandscape ? " on" : ""}`} onClick={() => s.toggleLandscape()} title="harmonic map: color every beat of the roll by its fit against the chord there">map</button>
        <button className={`chip${s.hearChords ? " on" : ""}`} onClick={() => s.toggleHearChords()} title="hear the chord track as a pad bed">chords</button>
      </div>
      <div className="mastermeter">
        <div className="hdr">
          <span>master</span>
          <span className="mono" style={{ color: peak > 0.99 ? "var(--rec)" : "var(--text-6)" }}>
            {peak > 0.99 ? "CLIP " : ""}
            {peakDb(peak).toFixed(1)}
          </span>
        </div>
        <HMeter peak={pl} />
        <HMeter peak={pr} />
        <div className="scale">
          <span>-48</span>
          <span>-24</span>
          <span>-12</span>
          <span>-6</span>
          <span>0</span>
        </div>
      </div>
    </div>
  );
}
