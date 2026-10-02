import { beatsPerBar, gridLabel, selectedPattern, selectedTrack, useStore } from "../store";

export function StatusBar() {
  const snapshot = useStore((s) => s.snapshot!);
  const mode = useStore((s) => s.mode);
  const audio = useStore((s) => s.audio);
  const noteLength = useStore((s) => s.noteLength);
  const selectedNoteIds = useStore((s) => s.selectedNoteIds);
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const bpb = useStore(beatsPerBar);

  const noteCount = pattern && track ? (pattern.notesByTrack[track.id]?.length ?? 0) : 0;
  const totalBeats = snapshot.model.clips.reduce((max, c) => Math.max(max, c.startBeat + c.lengthBeats), 0);

  const context =
    mode === "pattern"
      ? `${pattern?.name ?? "—"} › ${track?.name ?? "—"} · ${noteCount} notes${selectedNoteIds.length ? ` · ${selectedNoteIds.length} selected` : ""} · snap ${gridLabel(noteLength)}`
      : `song · ${snapshot.model.clips.length} clips · ${snapshot.model.sections.length} sections · ${+(totalBeats / bpb).toFixed(2)} bars · ${snapshot.model.tracks.length} tracks`;

  return (
    <div className="statusbar">
      <span style={{ color: audio?.running ? "var(--ok)" : "var(--rec-text)" }}>● {audio?.running ? "READY" : "NO AUDIO"}</span>
      <span>{context}</span>
      {useStore.getState().armed && <span style={{ color: "var(--rec-text)" }}>● REC</span>}
      {useStore.getState().liveKeyboard !== "off" && <span style={{ color: "var(--accent)" }}>keys: {useStore.getState().liveKeyboard}</span>}
      <span className="spacer" />
      <span>UNDO {snapshot.canUndo ? "●" : "○"}</span>
      <span>REDO {snapshot.canRedo ? "●" : "○"}</span>
      <span>{audio?.sampleRate ? `${audio.sampleRate / 1000} kHz` : "—"}</span>
      <span style={{ color: "var(--text-3)" }}>v0.2.0</span>
    </div>
  );
}
