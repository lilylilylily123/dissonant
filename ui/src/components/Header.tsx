import { useEffect, useState } from "react";
import { selectedPattern, useStore } from "../store";
import { detectKey, keyName } from "../theory";
import type { PlayMode } from "../types";

export function Header() {
  const model = useStore((s) => s.snapshot!.model);
  const canUndo = useStore((s) => s.snapshot!.canUndo);
  const canRedo = useStore((s) => s.snapshot!.canRedo);
  const dirty = useStore((s) => s.snapshot!.dirty);
  const path = useStore((s) => s.snapshot!.path);
  const mode = useStore((s) => s.mode);
  const playing = useStore((s) => s.playing);
  const showLandscape = useStore((s) => s.showLandscape);
  const hearChords = useStore((s) => s.hearChords);
  const audio = useStore((s) => s.audio);
  const pattern = useStore(selectedPattern);
  const selectedTrackId = useStore((s) => s.selectedTrackId);
  const s = useStore();

  const [bpmText, setBpmText] = useState(String(Math.round(model.tempo)));
  useEffect(() => setBpmText(String(Math.round(model.tempo))), [model.tempo]);

  const setBpm = (bpm: number) => void s.dispatch({ type: "setTempo", bpm: Math.min(300, Math.max(20, Math.round(bpm))) });
  const commitBpm = () => {
    const v = parseInt(bpmText, 10);
    if (Number.isFinite(v)) setBpm(v);
    else setBpmText(String(Math.round(model.tempo)));
  };

  // Key inference from the melodic notes in the selected pattern (drums excluded).
  const drumIds = new Set(model.tracks.filter((t) => t.isDrum).map((t) => t.id));
  const pcs = pattern
    ? Object.entries(pattern.notesByTrack)
        .filter(([tid]) => !drumIds.has(tid))
        .flatMap(([, notes]) => notes.map((n) => n.pitch))
    : [];
  const detected = detectKey(pcs);

  const modeChip = (m: PlayMode) => (
    <button className={`chip${mode === m ? " on" : ""}`} onClick={() => s.setMode(m)} style={{ borderRadius: 0 }}>
      {m}
    </button>
  );

  const fileName = path ? path.split(/[\\/]/).pop() : "untitled";

  return (
    <div className="row">
      <span className="wordmark">dissonant</span>

      <div className="row tight">
        <button className="faded" onClick={() => setBpm(model.tempo - 1)}>−</button>
        <input
          type="text"
          value={bpmText}
          onChange={(e) => setBpmText(e.target.value)}
          onBlur={commitBpm}
          onKeyDown={(e) => e.key === "Enter" && (e.target as HTMLInputElement).blur()}
          style={{ width: 44, textAlign: "center" }}
          aria-label="tempo"
        />
        <span className="label">bpm</span>
        <button className="faded" onClick={() => setBpm(model.tempo + 1)}>+</button>
      </div>

      <div className="row" style={{ gap: 0, borderRadius: 5, overflow: "hidden" }}>
        {modeChip("pattern")}
        {modeChip("song")}
      </div>

      {model.key.isLocked && model.key.rootPitchClass !== null ? (
        <span className="keychip">
          <span className="lock">key {keyName(model.key.rootPitchClass, model.key.scale)}</span>
          <button className="ghost faded" onClick={() => s.dispatch({ type: "setKey", key: { rootPitchClass: null, scale: "major", isLocked: false } })} title="clear key">
            ×
          </button>
        </span>
      ) : detected.isConfident && detected.candidates[0] ? (
        <button
          className="chip"
          onClick={() =>
            s.dispatch({
              type: "setKey",
              key: { rootPitchClass: detected.candidates[0].rootPitchClass, scale: detected.candidates[0].scale, isLocked: true },
            })
          }
        >
          looks like {keyName(detected.candidates[0].rootPitchClass, detected.candidates[0].scale)} — lock?
        </button>
      ) : null}

      <span className="spacer" />

      <span className="label" title={path ?? "unsaved"}>
        {fileName}
        {dirty ? " •" : ""}
      </span>
      <button className="faded" onClick={() => s.newProject()} title="new (⌘N)">new</button>
      <button className="faded" onClick={() => s.openProject()} title="open (⌘O)">open</button>
      <button className="faded" onClick={() => s.saveProject(false)} title="save (⌘S)">save</button>
      <button className="faded" onClick={() => s.undo()} disabled={!canUndo} title="undo (⌘Z)">↶</button>
      <button className="faded" onClick={() => s.redo()} disabled={!canRedo} title="redo (⇧⌘Z)">↷</button>
      <span className="divider" />

      <button className="faded" onClick={() => s.rewind()} title="rewind (R)">⏮</button>
      <button className="primary" onClick={() => s.togglePlay()} title="play / stop (space)">
        {playing ? "⏹ stop" : "▶ play"}
      </button>
      <button className={showLandscape ? "on" : "faded"} onClick={() => s.toggleLandscape()} title="harmonic map">◆ map</button>
      <button className={hearChords ? "on" : "faded"} onClick={() => s.toggleHearChords()} title="hear the chord bed">♪ chords</button>
      {mode === "pattern" && pattern && selectedTrackId && (
        <button
          className="faded"
          onClick={() => {
            if (confirm("Clear all notes on this track in this pattern?")) {
              void s.dispatch({ type: "setNotes", patternId: pattern.id, trackId: selectedTrackId, notes: [] });
            }
          }}
        >
          clear
        </button>
      )}
      <button className="faded" onClick={() => s.exportWav()} title="export WAV (⌘E)">⤓ export</button>
      <span
        className={`dot ${audio?.running ? "ok" : "bad"}`}
        title={audio?.running ? `audio @ ${audio.sampleRate} Hz` : `audio off: ${audio?.error ?? "…"} (click to retry)`}
        onClick={() => s.restartAudio()}
        style={{ cursor: "pointer" }}
      />
    </div>
  );
}
