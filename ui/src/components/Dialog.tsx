import { useEffect, useRef } from "react";
import { useStore } from "../store";

/**
 * The one modal the app uses instead of `confirm()` / `alert()`: a title, a line of prose,
 * an optional monospace detail (a path), and up to three buttons. Esc / backdrop click answer
 * with the spec's `cancelValue`; Enter presses the primary button.
 */
export function Dialog() {
  const dialog = useStore((s) => s.dialog);
  const answer = useStore((s) => s.answerDialog);
  const text = useStore((s) => s.dialogText);
  const setText = useStore((s) => s.setDialogText);
  const primaryRef = useRef<HTMLButtonElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!dialog) return;
    if (dialog.input) inputRef.current?.focus();
    else primaryRef.current?.focus();
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        answer(dialog.cancelValue ?? "cancel");
      } else if (e.key === "Enter") {
        const primary = dialog.buttons.find((b) => b.kind === "primary") ?? dialog.buttons[0];
        if (primary) {
          e.preventDefault();
          e.stopPropagation();
          answer(primary.value);
        }
      } else {
        // Nothing else reaches the editor while a question is open.
        e.stopPropagation();
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [dialog, answer]);

  if (!dialog) return null;
  return (
    <div className="modal-backdrop" onPointerDown={() => answer(dialog.cancelValue ?? "cancel")}>
      <div className="modal" role="dialog" aria-modal="true" aria-label={dialog.title} onPointerDown={(e) => e.stopPropagation()}>
        <div className="mtitle">{dialog.title}</div>
        {dialog.message && <div className="mbody">{dialog.message}</div>}
        {dialog.detail && <div className="mdetail mono">{dialog.detail}</div>}
        {dialog.input && (
          <input ref={inputRef} type="text" value={text} placeholder={dialog.input.placeholder} onChange={(e) => setText(e.target.value)} style={{ height: 24, fontSize: 11 }} />
        )}
        <div className="mbtns">
          {dialog.buttons.map((b) => (
            <button
              key={b.value}
              ref={b.kind === "primary" ? primaryRef : undefined}
              className={b.kind === "primary" ? "solid" : b.kind === "danger" ? "danger" : "quiet"}
              onClick={() => answer(b.value)}
            >
              {b.label}
            </button>
          ))}
        </div>
      </div>
    </div>
  );
}
