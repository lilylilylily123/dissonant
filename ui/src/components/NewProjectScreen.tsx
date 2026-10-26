import { useEffect } from "react";
import { useStore } from "../store";
import { keyName } from "../theory";
import { fileNameOf } from "../types";

/**
 * The new-project screen, shown at launch (Settings → editing → show on launch) and from
 * File → New project…: start from a vibe, the starter, empty or a saved template, or open a
 * recent file. The footer introduces the three tiers by how they look in the roll.
 */
export function NewProjectScreen() {
  const s = useStore();
  const open = s.newProjectOpen;

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !useStore.getState().dialog) useStore.getState().openNewProject(false);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  if (!open) return null;
  const close = () => s.openNewProject(false);

  return (
    <div className="modal-backdrop" onPointerDown={close}>
      <div className="modal settings newproject" role="dialog" aria-modal="true" aria-label="New project" onPointerDown={(e) => e.stopPropagation()}>
        <div className="shead">
          <span className="mtitle">start something</span>
          <span className="spacer" />
          <button className="ico" onClick={close} title="close (Esc)">×</button>
        </div>
        <div className="npbody">
          <div className="npcol">
            <div className="scap">start from a vibe</div>
            {s.vibes.map((v) => (
              <div key={v.id} className="tplrow vibe" onClick={() => void s.newFromVibe(v.id)}>
                <span className="name">{v.name}</span>
                <span className="meta" title={v.blurb}>{v.blurb}</span>
                <span className="facts">
                  {v.tempo} bpm · {v.key.rootPitchClass !== null ? keyName(v.key.rootPitchClass, v.key.scale) : "—"}
                </span>
              </div>
            ))}
            <div className="sdiv" />
            <div className="tplrow" onClick={() => void s.newFromTemplate("starter")}>
              <span className="name">starter</span>
              <span className="meta">I–IV–V–vi in C with a melody, drums and the chord bed</span>
            </div>
            <div className="tplrow" onClick={() => void s.newFromTemplate("empty")}>
              <span className="name">empty</span>
              <span className="meta">one track, one blank pattern, no key yet</span>
            </div>
            {s.templates.length > 0 && (
              <>
                <div className="sdiv" />
                <div className="scap">your templates</div>
                {s.templates.map((t) => (
                  <div key={t.name} className="tplrow" onClick={() => void s.newFromTemplate(t.name)}>
                    <span className="name">{t.name}</span>
                    <span className="meta" title={t.path}>template</span>
                    <button
                      className="ico"
                      title="delete template"
                      onClick={(e) => {
                        e.stopPropagation();
                        void s.deleteTemplate(t.name);
                      }}
                    >
                      ×
                    </button>
                  </div>
                ))}
              </>
            )}
          </div>
          <div className="npcol side">
            <div className="scap">recent</div>
            {s.recent.length === 0 && <div className="npempty">nothing yet: what you save shows up here</div>}
            {s.recent.slice(0, 8).map((p) => (
              <div
                key={p}
                className="tplrow"
                title={p}
                onClick={() => {
                  close();
                  void s.openPath(p);
                }}
              >
                <span className="name">{fileNameOf(p)}</span>
              </div>
            ))}
            <div className="row" style={{ gap: 6, marginTop: 6 }}>
              <button
                onClick={() => {
                  close();
                  void s.openProject();
                }}
              >
                open…
              </button>
              <button className="quiet" onClick={() => void s.saveAsTemplate()} title="save the open project as a template">
                save as template…
              </button>
            </div>
          </div>
        </div>
        <div className="npfoot">
          <div className="legend" aria-label="how notes are marked">
            <span className="lg">
              <i className="sw solid" /> chord tone: rock solid
            </span>
            <span className="lg">
              <i className="sw tension" /> tension: spicy, still good
            </span>
            <span className="lg">
              <i className="sw dissonance">!</i> flagged: clashes. could be exactly what you want
            </span>
          </div>
          <label className="npcheck">
            <input type="checkbox" checked={s.settings.editing.showWelcome} onChange={(e) => void s.updateSettings("editing", { showWelcome: e.target.checked })} />
            show this when dissonant starts
          </label>
        </div>
      </div>
    </div>
  );
}
