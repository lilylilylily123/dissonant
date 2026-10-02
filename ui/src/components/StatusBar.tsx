import { gridLabel, selectedPattern, selectedTrack, useStore } from "../store";

export function StatusBar() {
  const snapshot = useStore((s) => s.snapshot!);
  const mode = useStore((s) => s.mode);
  const audio = useStore((s) => s.audio);
  const noteLength = useStore((s) => s.noteLength);
  const selectedNoteIds = useStore((s) => s.selectedNoteIds);
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);

  const noteCount = pattern && track ? (pattern.notesByTrack[track.id]?.length ?? 0) : 0;
  const totalBeats = snapshot.model.arrangement.reduce(
    (sum, id) => sum + (snapshot.model.patterns.find((p) => p.id === id)?.lengthBeats ?? 0),
    0,
  );

  const context =
    mode === "pattern"
      ? `${pattern?.name ?? "—"} › ${track?.name ?? "—"} · ${noteCount} notes${selectedNoteIds.length ? ` · ${selectedNoteIds.length} selected` : ""} · snap ${gridLabel(noteLength)}`
      : `song · ${snapshot.model.arrangement.length} blocks · ${totalBeats / 4} bars · ${snapshot.model.tracks.length} tracks`;

  return (
    <div className="statusbar">
      <span style={{ color: audio?.running ? "var(--ok)" : "var(--rec-text)" }}>● {audio?.running ? "READY" : "NO AUDIO"}</span>
      <span>{context}</span>
      <span className="spacer" />
      <span>UNDO {snapshot.canUndo ? "●" : "○"}</span>
      <span>REDO {snapshot.canRedo ? "●" : "○"}</span>
      <span>{audio?.sampleRate ? `${audio.sampleRate / 1000} kHz` : "—"}</span>
      <span style={{ color: "var(--text-3)" }}>v0.2.0</span>
    </div>
  );
}
