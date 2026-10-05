import { useStore } from "../store";

/** File → New from template…: starter, empty, and every saved template. */
export function TemplatePicker() {
  const open = useStore((s) => s.templatesOpen);
  const close = useStore((s) => s.openTemplates);
  const templates = useStore((s) => s.templates);
  const pick = useStore((s) => s.newFromTemplate);
  const del = useStore((s) => s.deleteTemplate);
  const saveAs = useStore((s) => s.saveAsTemplate);
  if (!open) return null;
  return (
    <div className="modal-backdrop" onPointerDown={() => close(false)}>
      <div className="modal settings" style={{ width: 440 }} role="dialog" aria-modal="true" aria-label="New project" onPointerDown={(e) => e.stopPropagation()}>
        <div className="shead">
          <span className="mtitle">New project</span>
          <span className="spacer" />
          <button className="ico" onClick={() => close(false)} title="close (Esc)">×</button>
        </div>
        <div className="spane" style={{ padding: 10, gap: 4, minHeight: 0 }}>
          <div className="tplrow" onClick={() => pick("starter")}>
            <span className="name">starter</span>
            <span className="meta">I–IV–V–vi in C, a melody and a drum track</span>
          </div>
          <div className="tplrow" onClick={() => pick("empty")}>
            <span className="name">empty</span>
            <span className="meta">one track, one blank pattern</span>
          </div>
          {templates.length > 0 && <div className="sdiv" />}
          {templates.map((t) => (
            <div key={t.name} className="tplrow" onClick={() => pick(t.name)}>
              <span className="name">{t.name}</span>
              <span className="meta" title={t.path}>template</span>
              <button
                className="ico"
                title="delete template"
                onClick={(e) => {
                  e.stopPropagation();
                  void del(t.name);
                }}
              >
                ×
              </button>
            </div>
          ))}
          <div className="sdiv" />
          <div className="row">
            <span className="k" style={{ fontSize: 10, color: "var(--text-5)" }}>templates live in the app data folder</span>
            <span className="spacer" />
            <button className="chip tiny" onClick={() => void saveAs()}>save current as template…</button>
          </div>
        </div>
      </div>
    </div>
  );
}
