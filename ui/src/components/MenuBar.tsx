import { useEffect, useState } from "react";
import { selectedPattern, selectedTrack, useStore } from "../store";
import { fileNameOf } from "../types";

type Item = { label: string; sc?: string; sub?: string; action?: () => void; disabled?: boolean; sep?: false } | { sep: true };

export function MenuBar() {
  const s = useStore();
  const snapshot = s.snapshot!;
  const pattern = useStore(selectedPattern);
  const track = useStore(selectedTrack);
  const [open, setOpen] = useState<string | null>(null);

  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(null);
    window.addEventListener("pointerdown", close);
    return () => window.removeEventListener("pointerdown", close);
  }, [open]);

  const mod = navigator.platform.toLowerCase().includes("mac") ? "⌘" : "Ctrl+";

  const recentItems: Item[] = s.recent.length
    ? [
        { sep: true },
        ...s.recent.slice(0, 8).map((p) => ({ label: fileNameOf(p), sub: p === fileNameOf(p) ? undefined : p, action: () => s.openPath(p) })),
        { label: "Clear recent", action: () => s.clearRecent() },
      ]
    : [];

  const menus: Record<string, Item[]> = {
    file: [
      { label: "New project", sc: `${mod}N`, action: () => s.newProject() },
      { label: "Open…", sc: `${mod}O`, action: () => s.openProject() },
      ...recentItems,
      { sep: true },
      { label: "Save", sc: `${mod}S`, action: () => s.saveProject(false) },
      { label: "Save as…", sc: `⇧${mod}S`, action: () => s.saveProject(true) },
      { sep: true },
      { label: "Export WAV…", sc: `${mod}E`, action: () => s.exportWav() },
      { sep: true },
      { label: "Quit", sc: `${mod}Q`, action: () => s.requestClose() },
    ],
    edit: [
      { label: snapshot.undoLabel ? `Undo ${snapshot.undoLabel}` : "Undo", sc: `${mod}Z`, action: () => s.undo(), disabled: !snapshot.canUndo },
      { label: snapshot.redoLabel ? `Redo ${snapshot.redoLabel}` : "Redo", sc: `⇧${mod}Z`, action: () => s.redo(), disabled: !snapshot.canRedo },
      { sep: true },
      {
        label: "Clear notes on this track",
        disabled: !pattern || !track,
        action: async () => {
          if (!pattern || !track) return;
          if (await s.confirm("Clear notes?", `Every note of “${track.name}” in “${pattern.name}” will be removed.`, "Clear", true)) {
            void s.dispatch({ type: "setNotes", patternId: pattern.id, trackId: track.id, notes: [] }, false, "clear notes");
          }
        },
      },
    ],
    view: [
      { label: `${s.mode === "pattern" ? "Song (arrangement)" : "Pattern (piano roll)"}`, sc: "Tab", action: () => s.setMode(s.mode === "pattern" ? "song" : "pattern") },
      { sep: true },
      { label: `${s.highlightRows ? "✓ " : ""}Highlight rows by tier`, action: () => s.toggleHighlight() },
      { label: `${s.showLandscape ? "✓ " : ""}Harmonic map`, action: () => s.toggleLandscape() },
      { label: `${s.hearChords ? "✓ " : ""}Hear chord bed`, action: () => s.toggleHearChords() },
      { sep: true },
      { label: "Zoom in", sc: `${mod}=`, action: () => s.setZoom(s.zoom * 1.25) },
      { label: "Zoom out", sc: `${mod}-`, action: () => s.setZoom(s.zoom / 1.25) },
    ],
  };

  const fileName = fileNameOf(snapshot.path);

  return (
    <div className="menubar" onPointerDown={(e) => e.stopPropagation()}>
      <div className="logo">
        <i />
        <span>dissonant</span>
      </div>
      {Object.entries(menus).map(([name, items]) => (
        <div key={name} className={`menu${open === name ? " open" : ""}`}>
          <div
            className="mi"
            onClick={() => {
              if (name === "file" && open !== "file") void s.refreshRecent();
              setOpen(open === name ? null : name);
            }}
            onPointerEnter={() => open && setOpen(name)}
          >
            {name[0].toUpperCase() + name.slice(1)}
          </div>
          {open === name && (
            <div className="dropdown">
              {items.map((it, i) =>
                "sep" in it && it.sep ? (
                  <div key={i} className="sep" />
                ) : (
                  <div
                    key={i}
                    className={`item${(it as { disabled?: boolean }).disabled ? " disabled" : ""}`}
                    onClick={() => {
                      const item = it as { action?: () => void; disabled?: boolean };
                      if (item.disabled) return;
                      setOpen(null);
                      item.action?.();
                    }}
                  >
                    <span>
                      {(it as { label: string }).label}
                      {(it as { sub?: string }).sub && <span className="sub" title={(it as { sub?: string }).sub}>{(it as { sub?: string }).sub}</span>}
                    </span>
                    <span className="sc">{(it as { sc?: string }).sc ?? ""}</span>
                  </div>
                ),
              )}
            </div>
          )}
        </div>
      ))}
      <span className="spacer" />
      <div className="meta">
        <span style={{ color: "var(--text-2)" }}>
          {fileName}
          {snapshot.dirty ? " •" : ""}
        </span>
        <span>{s.audio?.sampleRate ? `${(s.audio.sampleRate / 1000).toFixed(s.audio.sampleRate % 1000 ? 1 : 0)} kHz · 32-bit float` : "no device"}</span>
        <span
          style={{ color: s.audio?.running ? "var(--ok)" : "var(--rec-text)", cursor: "pointer" }}
          title={s.audio?.running ? `audio engine running on ${s.audio.deviceName ?? "the default output"}` : `audio engine off: ${s.audio?.error ?? "…"} — click to retry`}
          onClick={() => s.restartAudio()}
        >
          ● {s.audio?.running ? "ENGINE" : "NO AUDIO"}
        </span>
      </div>
    </div>
  );
}
