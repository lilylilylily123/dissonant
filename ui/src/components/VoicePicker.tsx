import { LENGTH_OPTIONS, selectedTrack, useStore } from "../store";
import { VOICES } from "../types";

export function VoicePicker() {
  const track = useStore(selectedTrack);
  const noteLength = useStore((s) => s.noteLength);
  const zoom = useStore((s) => s.zoom);
  const s = useStore();
  if (!track) return null;

  return (
    <div className="row tight">
      <span className="label">voice</span>
      {VOICES.map((v) => (
        <button key={v} className={`chip${track.voice === v ? " on" : ""}`} onClick={() => s.dispatch({ type: "setTrackVoice", id: track.id, voice: v })}>
          {v}
        </button>
      ))}
      <span className="spacer" />
      <span className="label">zoom</span>
      <button className="chip small faded" onClick={() => s.setZoom(zoom / 1.25)}>−</button>
      <button className="chip small faded" onClick={() => s.setZoom(zoom * 1.25)}>+</button>
      <span className="label">len</span>
      {LENGTH_OPTIONS.map(([label, len]) => (
        <button key={label} className={`chip small${noteLength === len ? " on" : ""}`} onClick={() => s.setNoteLength(len)}>
          {label}
        </button>
      ))}
    </div>
  );
}
