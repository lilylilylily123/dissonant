// Panel sizes the user drags (the track tree, the inspector, the bottom panel, …).
//
// They are layout, not project data and not a setting anyone would look for in a settings tab,
// so they live here: one module-level record, persisted per machine in `localStorage`, read
// through `useSyncExternalStore` so a drag on the splitter re-renders both the panel and
// whatever else measures against it (the arrangement's head column is a width every row uses).

import { useSyncExternalStore } from "react";

export type PanelId = "tree" | "inspector" | "bottom" | "songHead" | "guitar" | "soundBrowser";

interface Spec {
  /** The size before anyone drags it. Double-clicking a splitter comes back here. */
  size: number;
  min: number;
  max: number;
  /** Which window dimension the panel eats into, for the "leave room for the rest" clamp. */
  axis: "x" | "y";
  /** Pixels of the other panes that must survive at any window size. */
  keep: number;
}

const SPECS: Record<PanelId, Spec> = {
  tree: { size: 248, min: 150, max: 560, axis: "x", keep: 360 },
  inspector: { size: 264, min: 180, max: 600, axis: "x", keep: 360 },
  bottom: { size: 300, min: 120, max: 760, axis: "y", keep: 220 },
  songHead: { size: 220, min: 120, max: 460, axis: "x", keep: 320 },
  guitar: { size: 300, min: 200, max: 620, axis: "x", keep: 320 },
  soundBrowser: { size: 228, min: 160, max: 520, axis: "x", keep: 420 },
};

const KEY = "dissonant.panels";

const IDS = Object.keys(SPECS) as PanelId[];
const sizes = Object.fromEntries(IDS.map((id) => [id, SPECS[id].size])) as Record<PanelId, number>;
const listeners = new Set<() => void>();

/** The default size of a panel, for a splitter's double-click reset. */
export function panelDefault(id: PanelId): number {
  return SPECS[id].size;
}

/**
 * A size made usable: inside the panel's own range, and never so large that the rest of the
 * window has less than `keep` left. The window clamp is applied on every read rather than on
 * the stored value, so shrinking the window and growing it again gives the size back.
 */
export function clampPanel(id: PanelId, px: number): number {
  const spec = SPECS[id];
  const window_ = typeof window === "undefined" ? null : window;
  const room = window_ ? (spec.axis === "x" ? window_.innerWidth : window_.innerHeight) - spec.keep : Infinity;
  const max = Math.max(spec.min, Math.min(spec.max, room));
  return Math.round(Math.max(spec.min, Math.min(max, px)));
}

export function panelSize(id: PanelId): number {
  return clampPanel(id, sizes[id]);
}

export function setPanelSize(id: PanelId, px: number) {
  const next = clampPanel(id, px);
  if (next === sizes[id]) return;
  sizes[id] = next;
  listeners.forEach((l) => l());
  try {
    localStorage.setItem(KEY, JSON.stringify(sizes));
  } catch {
    /* storage off: the size just won't outlive the session */
  }
}

/** Load the remembered sizes. Unknown ids and nonsense values are ignored, not thrown over. */
function load() {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return;
    const saved = JSON.parse(raw) as Partial<Record<PanelId, unknown>>;
    for (const id of IDS) {
      const v = saved[id];
      if (typeof v === "number" && Number.isFinite(v)) sizes[id] = clampPanel(id, v);
    }
  } catch {
    /* unreadable: the defaults stand */
  }
}
load();

function subscribe(fn: () => void): () => void {
  listeners.add(fn);
  // A narrower window can push a panel past its "leave room" clamp; re-reading on resize keeps
  // the rendered size honest without touching what was stored.
  if (listeners.size === 1 && typeof window !== "undefined") window.addEventListener("resize", notifyResize);
  return () => {
    listeners.delete(fn);
    if (listeners.size === 0 && typeof window !== "undefined") window.removeEventListener("resize", notifyResize);
  };
}

function notifyResize() {
  listeners.forEach((l) => l());
}

/** The panel's current size in px, re-rendering the caller while it is dragged. */
export function usePanelSize(id: PanelId): number {
  return useSyncExternalStore(
    subscribe,
    () => panelSize(id),
    () => SPECS[id].size,
  );
}
