// Panel sizes: the limits a drag is held to, and what survives a reload.

import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type * as PanelsModule from "./panels";

/** The stubbed window, kept so a test can shrink it the way a user shrinks the real one. */
const win = { innerWidth: 1600, innerHeight: 1000, addEventListener: () => {}, removeEventListener: () => {} };

beforeAll(() => {
  const store = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => store.get(k) ?? null,
    setItem: (k: string, v: string) => void store.set(k, v),
    removeItem: (k: string) => void store.delete(k),
    clear: () => store.clear(),
  });
  vi.stubGlobal("window", win);
});

let P: typeof PanelsModule;

beforeEach(async () => {
  localStorage.clear();
  win.innerWidth = 1600;
  vi.resetModules();
  // The module reads storage as it loads, so "what a reload sees" can only be exercised by
  // loading it again.
  P = await import("./panels");
});

describe("panel sizes", () => {
  it("holds a drag inside the panel's own range", () => {
    P.setPanelSize("tree", 40);
    expect(P.panelSize("tree")).toBe(150);
    P.setPanelSize("tree", 5000);
    expect(P.panelSize("tree")).toBe(560);
    P.setPanelSize("tree", 300);
    expect(P.panelSize("tree")).toBe(300);
  });

  it("leaves the other panes room, without forgetting the size the window can't show", () => {
    P.setPanelSize("tree", 520);
    win.innerWidth = 700;
    expect(P.panelSize("tree")).toBe(340); // 700 less the 360 the editor keeps
    win.innerWidth = 1600;
    expect(P.panelSize("tree")).toBe(520); // the drag is remembered, not trimmed
  });

  it("remembers every panel across a reload and ignores junk", async () => {
    P.setPanelSize("inspector", 320);
    P.setPanelSize("bottom", 180);
    const saved = JSON.parse(localStorage.getItem("dissonant.panels")!) as Record<string, unknown>;
    localStorage.setItem("dissonant.panels", JSON.stringify({ ...saved, songHead: "wide", nope: 9 }));
    vi.resetModules();
    const reloaded = await import("./panels");
    expect(reloaded.panelSize("inspector")).toBe(320);
    expect(reloaded.panelSize("bottom")).toBe(180);
    expect(reloaded.panelSize("songHead")).toBe(reloaded.panelDefault("songHead"));
  });

  it("survives storage being unreadable", async () => {
    localStorage.setItem("dissonant.panels", "{not json");
    vi.resetModules();
    const reloaded = await import("./panels");
    expect(reloaded.panelSize("tree")).toBe(reloaded.panelDefault("tree"));
  });

  it("keeps the same room to shrink at a bigger ui scale", async () => {
    // The scale is a page zoom: CSS pixels get bigger, so a CSS-px floor would shrink the
    // user's room. 150 screen px must stay 150 screen px — 100 CSS px at 150%.
    vi.resetModules();
    vi.doMock("./uiScale", () => ({ pageZoom: () => 1.5, onZoomChange: () => () => {} }));
    const zoomed = await import("./panels");
    zoomed.setPanelSize("tree", 40);
    expect(zoomed.panelSize("tree")).toBe(100);
    // And the room kept for the other panes is a screen amount too: 360 / 1.5 = 240.
    win.innerWidth = 500;
    zoomed.setPanelSize("tree", 400);
    expect(zoomed.panelSize("tree")).toBe(260);
    vi.doUnmock("./uiScale");
  });
});
