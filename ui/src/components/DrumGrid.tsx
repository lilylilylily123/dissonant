import { useRef, useState } from "react";
import { selectedPattern, selectedTrack, useStore } from "../store";
import { DRUM_KIT, type NoteEvent, uuid } from "../types";

/** Step sequencer for a drum track. Left-click/drag adds hits, right-click/drag removes.
 *  Hits are ordinary notes (pitch = kit note) so they play and arrange like everything else. */
export function DrumGrid() {
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const playhead = useStore((s) => s.playhead);
  const noteLength = useStore((s) => s.noteLength);
  const dispatch = useStore((s) => s.dispatch);
  const audition = useStore((s) => s.audition);
  const dragMode = useRef<"add" | "remove" | null>(null);
  const [working, setWorking] = useState<NoteEvent[] | null>(null);

  if (!pattern || !track) return null;
  const committed = pattern.notesByTrack[track.id] ?? [];
  const notes = working ?? committed;
  const stepBeats = Math.min(0.5, noteLength);
  const steps = Math.max(1, Math.round(pattern.lengthBeats / stepBeats));
  const currentStep = Math.floor(playhead / stepBeats);

  const find = (list: NoteEvent[], pitch: number, step: number) =>
    list.find((n) => n.pitch === pitch && Math.abs(n.startBeat - step * stepBeats) < 1e-6);

  const commit = () => {
    if (working) {
      void dispatch({ type: "setNotes", patternId: pattern.id, trackId: track.id, notes: working });
      setWorking(null);
    }
    dragMode.current = null;
  };

  const paint = (pitch: number, step: number) => {
    const mode = dragMode.current;
    if (!mode) return;
    setWorking((prev) => {
      const cur = prev ?? committed;
      const existing = find(cur, pitch, step);
      if (mode === "add" && !existing) {
        audition(pitch);
        return [...cur, { id: uuid(), startBeat: step * stepBeats, lengthBeats: stepBeats, pitch, velocity: 100 }];
      }
      if (mode === "remove" && existing) return cur.filter((n) => n !== existing);
      return cur;
    });
  };

  return (
    <div className="drumgrid" onPointerUp={commit} onPointerLeave={commit}>
      {DRUM_KIT.map((drum) => (
        <div className="drumrow" key={drum.pitch}>
          <span className="name">{drum.name}</span>
          {Array.from({ length: steps }, (_, step) => {
            const on = !!find(notes, drum.pitch, step);
            const onBeat = Math.abs((step * stepBeats) % 1) < 1e-6;
            return (
              <div
                key={step}
                className={`step${on ? " on" : ""}${onBeat ? " beat" : ""}${step === currentStep ? " now" : ""}`}
                onPointerDown={(e) => {
                  e.preventDefault();
                  dragMode.current = e.button === 2 ? "remove" : "add";
                  paint(drum.pitch, step);
                }}
                onPointerEnter={() => paint(drum.pitch, step)}
              />
            );
          })}
        </div>
      ))}
      <div className="roll-help">
        left-click / drag: add hits · right-click / drag: remove · step = {stepBeats === 0.5 ? "1/8" : "1/16"} (set via len)
      </div>
    </div>
  );
}
