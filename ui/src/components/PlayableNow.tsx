import { selectedPattern, useStore } from "../store";
import { chordAt, noteName, tierMap } from "../theory";

/** Live "notes you could play right now" readout — same tier map as the roll (R13). */
export function PlayableNow() {
  const pattern = useStore(selectedPattern);
  const key = useStore((s) => s.snapshot!.model.key);
  const playhead = useStore((s) => s.playhead);
  if (!pattern) return null;

  const chords = pattern.chords.chords;
  const chord = chordAt(chords, playhead);
  const map = tierMap(playhead, chords, key);
  const order = [0, 2, 4, 5, 7, 9, 11, 1, 3, 6, 8, 10];

  return (
    <div className="playable">
      <span className="label">now</span>
      <span style={{ fontWeight: 700, minWidth: 40 }}>{chord?.name ?? "—"}</span>
      {map.every((t) => t === null) ? (
        <span className="label">place some notes or pick a progression — no guidance yet</span>
      ) : (
        order.map((pc) => (
          <span key={pc} className={`pc ${map[pc] ?? ""}`} title={map[pc] ?? "no guidance"}>
            {noteName(pc)}
            {map[pc] === "dissonance" ? "!" : map[pc] === "tension" ? "·" : ""}
          </span>
        ))
      )}
    </div>
  );
}
