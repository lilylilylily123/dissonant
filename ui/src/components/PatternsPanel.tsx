import { useState } from "react";
import { beatsPerBar, selectedPattern, useStore } from "../store";
import { songLength } from "../types";

/** Left column in SONG mode: the pattern library. Click selects, ＋ appends to the song. */
export function PatternsPanel() {
  const model = useStore((s) => s.snapshot!.model);
  const pattern = useStore(selectedPattern);
  const selectPattern = useStore((s) => s.selectPattern);
  const setMode = useStore((s) => s.setMode);
  const dispatch = useStore((s) => s.dispatch);
  const bpb = useStore(beatsPerBar);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");

  const noteCount = (pid: string) =>
    Object.values(model.patterns.find((p) => p.id === pid)?.notesByTrack ?? {}).reduce((n, arr) => n + arr.length, 0);

  const finishRename = (id: string) => {
    if (draft.trim()) void dispatch({ type: "renamePattern", id, name: draft.trim() });
    setRenaming(null);
  };

  return (
    <aside className="side browser">
      <div className="tabs">
        <div className="on">patterns</div>
      </div>
      <div className="section" style={{ gap: 6 }}>
        <div className="row" style={{ gap: 4 }}>
          <button onClick={() => dispatch({ type: "addPattern" })}>+ new</button>
          <button disabled={!pattern} onClick={() => pattern && dispatch({ type: "duplicatePattern", id: pattern.id })}>dup</button>
          <button
            disabled={!pattern || model.patterns.length <= 1}
            onClick={() => pattern && confirm(`Delete “${pattern.name}”?`) && dispatch({ type: "deletePattern", id: pattern.id })}
          >
            del
          </button>
          <span className="spacer" />
          <span className="mono" style={{ fontSize: 9, color: "var(--text-5)" }}>{model.patterns.length}</span>
        </div>
      </div>
      <div className="section grow" style={{ padding: "6px 6px", gap: 0 }}>
        <div className="list">
          {model.patterns.map((p, i) => {
            const on = p.id === pattern?.id;
            return (
              <div
                key={p.id}
                className={`lrow${on ? " on" : ""}`}
                style={{ paddingLeft: 6 }}
                onClick={() => selectPattern(p.id)}
                onDoubleClick={() => (selectPattern(p.id), setMode("pattern"))}
                title="click: select · double-click: open in the piano roll · ＋: add to song"
              >
                <span className="num">{String(i + 1).padStart(2, "0")}</span>
                {renaming === p.id ? (
                  <input
                    type="text"
                    autoFocus
                    value={draft}
                    onChange={(e) => setDraft(e.target.value)}
                    onBlur={() => finishRename(p.id)}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") finishRename(p.id);
                      if (e.key === "Escape") setRenaming(null);
                    }}
                    onClick={(e) => e.stopPropagation()}
                  />
                ) : (
                  <span className="name">{p.name}</span>
                )}
                <span className="meta">
                  {+(p.lengthBeats / bpb).toFixed(2)}b · {noteCount(p.id)}n
                </span>
                <button className="ico" title="rename" onClick={(e) => (e.stopPropagation(), setDraft(p.name), setRenaming(p.id))}>✎</button>
                <button
                  className="ico"
                  title="append to song"
                  style={{ color: "var(--accent)" }}
                  onClick={(e) => (e.stopPropagation(), dispatch({ type: "addClip", patternId: p.id, startBeat: songLength(model), lengthBeats: null }))}
                >
                  ＋
                </button>
              </div>
            );
          })}
        </div>
      </div>
      {pattern && (
        <div className="section" style={{ borderBottom: 0, borderTop: "1px solid var(--line-1)" }}>
          <div className="head">
            <span className="cap">length</span>
            <span className="mono" style={{ fontSize: 9, color: "var(--text-5)" }}>{+(pattern.lengthBeats / bpb).toFixed(2)} bars</span>
          </div>
          <div className="chipsrow">
            {[1, 2, 4, 8].map((bars) => (
              <button
                key={bars}
                className={`chip${pattern.lengthBeats === bars * bpb ? " on" : ""}`}
                onClick={() => dispatch({ type: "setPatternLength", id: pattern.id, beats: bars * bpb })}
              >
                {bars}
              </button>
            ))}
          </div>
          <div className="help" style={{ padding: 0 }}>
            ＋ adds the pattern to the end of the song · timeline: drag a clip to move, its right edge to trim or loop-extend, ⌥-drag to copy, right-click to remove, double-click to edit · double-click the marker strip to add a section
          </div>
        </div>
      )}
    </aside>
  );
}
