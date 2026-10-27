import { useRef, type KeyboardEvent, type PointerEvent } from "react";
import { clampPanel, panelDefault, panelSize, setPanelSize, usePanelSize, type PanelId } from "../panels";

/**
 * The draggable seam next to a panel. It is a real flex item between the two panes rather than
 * a strip laid over the panel's edge, so it never steals a click from the row actions that sit
 * against that edge.
 *
 * `grows` says which way the panel gets bigger: `"after"` when the panel is before the seam
 * (the track tree), `"before"` when it is after it (the inspector, the bottom panel).
 */
export function Splitter({
  panel,
  axis = "x",
  grows = "after",
  label,
}: {
  panel: PanelId;
  axis?: "x" | "y";
  grows?: "before" | "after";
  label: string;
}) {
  const size = usePanelSize(panel);
  const drag = useRef<{ from: number; base: number } | null>(null);

  const onPointerDown = (e: PointerEvent<HTMLDivElement>) => {
    if (e.button !== 0) return;
    e.preventDefault();
    drag.current = { from: axis === "x" ? e.clientX : e.clientY, base: panelSize(panel) };
    // Captured on the window: the pointer leaves this 5px strip on the first frame of any real
    // drag, and a capture on the element itself would end the moment the panel re-lays out.
    const move = (ev: globalThis.PointerEvent) => {
      const d = (axis === "x" ? ev.clientX : ev.clientY) - drag.current!.from;
      setPanelSize(panel, drag.current!.base + (grows === "after" ? d : -d));
    };
    const up = () => {
      drag.current = null;
      document.body.classList.remove("splitting", "row");
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
    };
    document.body.classList.add("splitting", ...(axis === "y" ? ["row"] : []));
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const less = axis === "x" ? "ArrowLeft" : "ArrowUp";
    const more = axis === "x" ? "ArrowRight" : "ArrowDown";
    if (e.key !== less && e.key !== more) return;
    e.preventDefault();
    const step = (e.shiftKey ? 32 : 8) * (e.key === more ? 1 : -1) * (grows === "after" ? 1 : -1);
    setPanelSize(panel, size + step);
  };

  return (
    <div
      className={`splitter ${axis}`}
      role="separator"
      aria-orientation={axis === "x" ? "vertical" : "horizontal"}
      aria-label={`resize ${label}`}
      aria-valuenow={size}
      aria-valuemin={clampPanel(panel, 0)}
      aria-valuemax={clampPanel(panel, Infinity)}
      tabIndex={0}
      title={`drag to resize ${label} · double-click to reset · arrows nudge (⇧ faster)`}
      onPointerDown={onPointerDown}
      onDoubleClick={() => setPanelSize(panel, panelDefault(panel))}
      onKeyDown={onKeyDown}
    />
  );
}
