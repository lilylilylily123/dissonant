import { useState } from "react";
import { useStore } from "../store";
import type { Track } from "../types";

export function TrackSidebar() {
  const tracks = useStore((s) => s.snapshot!.model.tracks);
  const selectedId = useStore((s) => s.selectedTrackId);
  const peaks = useStore((s) => s.trackPeaks);
  const dispatch = useStore((s) => s.dispatch);
  const selectTrack = useStore((s) => s.selectTrack);

  return (
    <aside className="sidebar">
      <div style={{ fontWeight: 700 }}>tracks</div>
      <div className="row tight">
        <button className="ghost" onClick={() => dispatch({ type: "addTrack", isDrum: false })}>+ inst</button>
        <button className="ghost" onClick={() => dispatch({ type: "addTrack", isDrum: true })}>+ drum</button>
      </div>
      {tracks.map((t, i) => (
        <TrackRow
          key={t.id}
          track={t}
          selected={t.id === selectedId}
          canDelete={tracks.length > 1}
          peak={peaks[i] ?? 0}
          onSelect={() => selectTrack(t.id)}
        />
      ))}
    </aside>
  );
}

function TrackRow({
  track,
  selected,
  canDelete,
  peak,
  onSelect,
}: {
  track: Track;
  selected: boolean;
  canDelete: boolean;
  peak: number;
  onSelect: () => void;
}) {
  const dispatch = useStore((s) => s.dispatch);
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(track.name);

  const finish = () => {
    if (draft.trim() && draft !== track.name) void dispatch({ type: "renameTrack", id: track.id, name: draft.trim() });
    setEditing(false);
  };

  return (
    <div className={`track${selected ? " selected" : ""}`} onClick={onSelect}>
      <div className="row tight" style={{ flexWrap: "nowrap" }}>
        {editing ? (
          <input
            type="text"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={finish}
            onKeyDown={(e) => {
              if (e.key === "Enter") finish();
              if (e.key === "Escape") setEditing(false);
            }}
            onClick={(e) => e.stopPropagation()}
            style={{ flex: 1, minWidth: 0 }}
          />
        ) : (
          <span className="name" onDoubleClick={() => (setDraft(track.name), setEditing(true))} title="double-click to rename">
            {track.name || "untitled"}
          </span>
        )}
        <button className="mini" onClick={(e) => (e.stopPropagation(), dispatch({ type: "moveTrack", id: track.id, up: true }))} title="move up">▲</button>
        <button className="mini" onClick={(e) => (e.stopPropagation(), dispatch({ type: "moveTrack", id: track.id, up: false }))} title="move down">▼</button>
        <button className="mini" onClick={(e) => (e.stopPropagation(), setDraft(track.name), setEditing(true))} title="rename">✎</button>
        {canDelete && (
          <button
            className="mini"
            onClick={(e) => {
              e.stopPropagation();
              if (confirm(`Delete track “${track.name}” and its notes?`)) void dispatch({ type: "deleteTrack", id: track.id });
            }}
            title="delete"
          >
            ×
          </button>
        )}
      </div>
      <div className="row tight" style={{ flexWrap: "nowrap" }}>
        <span className="label">{track.isDrum ? "drums" : track.voice}</span>
        <span className="spacer" />
        <button
          className={`toggle mute${track.muted ? " on" : ""}`}
          onClick={(e) => (e.stopPropagation(), dispatch({ type: "setTrackMuted", id: track.id, muted: !track.muted }))}
          title="mute"
        >
          M
        </button>
        <button
          className={`toggle solo${track.soloed ? " on" : ""}`}
          onClick={(e) => (e.stopPropagation(), dispatch({ type: "setTrackSoloed", id: track.id, soloed: !track.soloed }))}
          title="solo"
        >
          S
        </button>
      </div>
      <div className="meter">
        <div style={{ width: `${Math.min(100, peak * 100)}%` }} />
      </div>
    </div>
  );
}
