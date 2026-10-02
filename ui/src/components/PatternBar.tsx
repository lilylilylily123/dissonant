import { useState } from "react";
import { selectedPattern, useStore } from "../store";

export function PatternBar() {
  const patterns = useStore((s) => s.snapshot!.model.patterns);
  const pattern = useStore(selectedPattern);
  const selectPattern = useStore((s) => s.selectPattern);
  const dispatch = useStore((s) => s.dispatch);
  const [renaming, setRenaming] = useState(false);
  const [draft, setDraft] = useState("");

  if (!pattern) return null;

  const commitRename = () => {
    if (draft.trim()) void dispatch({ type: "renamePattern", id: pattern.id, name: draft.trim() });
    setRenaming(false);
  };

  return (
    <div className="row tight">
      <span className="label">pattern</span>
      {patterns.map((p) => (
        <button key={p.id} className={`chip${p.id === pattern.id ? " on" : ""}`} onClick={() => selectPattern(p.id)}>
          {p.name}
        </button>
      ))}
      <span className="divider" />
      <button className="ghost" onClick={() => dispatch({ type: "addPattern" })}>+ new</button>
      <button className="ghost" onClick={() => dispatch({ type: "duplicatePattern", id: pattern.id })}>⧉ dup</button>
      <button className="ghost" onClick={() => (setDraft(pattern.name), setRenaming(true))}>✎ rename</button>
      {patterns.length > 1 && (
        <button className="ghost" onClick={() => confirm(`Delete “${pattern.name}”?`) && dispatch({ type: "deletePattern", id: pattern.id })}>
          × del
        </button>
      )}
      <span className="divider" />
      <span className="label">bars</span>
      {[1, 2, 4, 8].map((bars) => (
        <button
          key={bars}
          className={`chip small${pattern.lengthBeats === bars * 4 ? " on" : ""}`}
          onClick={() => dispatch({ type: "setPatternLength", id: pattern.id, beats: bars * 4 })}
        >
          {bars}
        </button>
      ))}
      {renaming && (
        <>
          <input
            type="text"
            autoFocus
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename();
              if (e.key === "Escape") setRenaming(false);
            }}
            onBlur={commitRename}
            style={{ width: 130 }}
          />
          <button className="ghost" onMouseDown={(e) => e.preventDefault()} onClick={commitRename}>✓</button>
        </>
      )}
    </div>
  );
}
