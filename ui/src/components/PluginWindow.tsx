import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getBridge } from "../bridge";
import type { ParamInfo, Snapshot, Track } from "../types";

/**
 * The plugin editor, as its own window.
 *
 * This runs in a second webview with no transport, no document and no audio of its own — it
 * reads the track's parameters and writes them back, nothing else. That is deliberate: the main
 * window owns the project, and duplicating its subscriptions here would mean two copies of the
 * playhead and the undo stack racing each other.
 *
 * The layout is built for the real shape of the problem. Dexed publishes 156 parameters and
 * reports no module for any of them, so a flat wall of knobs is unreadable and a knob grid is
 * worse: at that count you scan for a *name*, and names only line up in rows. Rows it is, dense
 * and monospaced, grouped by whatever structure the plugin's own naming reveals.
 */

/** Matches one pointer drag on a parameter row, so the gesture can commit exactly once. */
type Drag = { id: number; startX: number; startValue: number; width: number };

/**
 * Parameters in the order the plugin reports them, split into named groups.
 *
 * CLAP lets a plugin put each parameter in a module, and when it does we use that. Most do not:
 * Dexed leaves every module empty but names its parameters `OP1 EG RATE 1`, `LFO SPEED`,
 * `PITCH EG LEVEL 2`. That first word is the structure, so when enough parameters share one we
 * treat it as a group and strip it from the labels — `OP1` becomes a heading and the row below
 * reads `EG RATE 1`. A first word only one or two parameters share is not structure, it is just
 * a word, and those stay together under "general".
 */
const MIN_GROUP = 3;

export function groupParams(params: ParamInfo[]): { name: string; params: ParamInfo[]; strip: boolean }[] {
  const byModule = params.some((p) => p.module.trim() !== "");
  const key = (p: ParamInfo) => (byModule ? p.module.trim() || "general" : p.name.trim().split(/\s+/)[0] ?? "");

  const counts = new Map<string, number>();
  for (const p of params) counts.set(key(p), (counts.get(key(p)) ?? 0) + 1);

  const groups: { name: string; params: ParamInfo[]; strip: boolean }[] = [];
  for (const p of params) {
    const k = key(p);
    const real = byModule || (counts.get(k) ?? 0) >= MIN_GROUP;
    const name = real ? k : "general";
    let g = groups.find((x) => x.name === name);
    if (!g) {
      g = { name, params: [], strip: real && !byModule };
      groups.push(g);
    }
    g.params.push(p);
  }
  // "general" is the catch-all, so it reads first no matter where its members appeared.
  return groups.sort((a, b) => Number(b.name === "general") - Number(a.name === "general"));
}

/** A parameter's position in 0..1, for the fill bar. Flat ranges cannot be normalised. */
function norm(p: ParamInfo, v: number): number {
  return p.max > p.min ? Math.min(1, Math.max(0, (v - p.min) / (p.max - p.min))) : 0;
}

/**
 * What to print beside the bar. A plugin that reports a plain 0..1 range has no unit worth
 * inventing, so those read as a percentage; everything else keeps its own numbers.
 */
function valueText(p: ParamInfo, v: number): string {
  if (p.isStepped) return String(Math.round(v));
  if (p.min === 0 && p.max === 1) return `${Math.round(v * 100)}%`;
  if (Math.abs(v) >= 1000) return v.toFixed(0);
  return v.toFixed(2);
}

/** Drag across the row to set the value; double-click restores the plugin's own default. */
function ParamRow({
  param,
  label,
  onDrag,
  onCommit,
}: {
  param: ParamInfo;
  label: string;
  onDrag: (p: ParamInfo, value: number) => void;
  onCommit: (p: ParamInfo, value: number) => void;
}) {
  const drag = useRef<Drag | null>(null);

  const valueAt = (d: Drag, clientX: number) => {
    const span = param.max - param.min;
    const moved = (clientX - d.startX) / Math.max(1, d.width);
    const raw = d.startValue + moved * span;
    const clamped = Math.min(param.max, Math.max(param.min, raw));
    return param.isStepped ? Math.round(clamped) : clamped;
  };

  return (
    <div
      className="prow"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        const box = e.currentTarget.getBoundingClientRect();
        e.currentTarget.setPointerCapture(e.pointerId);
        drag.current = { id: param.id, startX: e.clientX, startValue: param.value, width: box.width };
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        onDrag(param, valueAt(d, e.clientX));
      }}
      onPointerUp={(e) => {
        const d = drag.current;
        drag.current = null;
        if (!d) return;
        onCommit(param, valueAt(d, e.clientX));
      }}
      onPointerCancel={() => {
        drag.current = null;
      }}
      onDoubleClick={() => onCommit(param, param.default)}
      title={`${param.name} · ${param.min}…${param.max} · double-click for the plugin's default`}
    >
      <span className="pname">{label}</span>
      <span className="pbar">
        <span className="fill" style={{ width: `${norm(param, param.value) * 100}%` }} />
      </span>
      <span className="pval">{valueText(param, param.value)}</span>
    </div>
  );
}

export function PluginWindow({ trackId }: { trackId: string }) {
  const [params, setParams] = useState<ParamInfo[] | null>(null);
  const [track, setTrack] = useState<Track | null>(null);
  const [query, setQuery] = useState("");
  const [folded, setFolded] = useState<Record<string, boolean>>({});
  const load = useCallback(async () => {
    const b = await getBridge();
    const snap: Snapshot = await b.getState();
    setTrack(snap.model.tracks.find((t) => t.id === trackId) ?? null);
    setParams(await b.pluginParams(trackId));
  }, [trackId]);

  useEffect(() => {
    void load();
    // Undo, redo and a project load all rewrite plugin state behind our back; the document event
    // is the only signal that the values on screen are no longer what the plugin holds.
    let off: (() => void) | undefined;
    void getBridge().then((b) => {
      off = b.onDocument(() => void load());
    });
    return () => off?.();
  }, [load]);

  // Dragging writes straight to the audio thread and keeps the bar under the finger; the commit
  // on release is what records a single undo step.
  const onDrag = useCallback(
    (p: ParamInfo, value: number) => {
      setParams((ps) => (ps ? ps.map((x) => (x.id === p.id ? { ...x, value } : x)) : ps));
      void getBridge().then((b) => b.setPluginParam(trackId, p.id, value, false));
    },
    [trackId],
  );
  const onCommit = useCallback(
    (p: ParamInfo, value: number) => {
      setParams((ps) => (ps ? ps.map((x) => (x.id === p.id ? { ...x, value } : x)) : ps));
      void getBridge().then((b) => b.setPluginParam(trackId, p.id, value, true));
    },
    [trackId],
  );

  const groups = useMemo(() => {
    if (!params) return [];
    const q = query.trim().toLowerCase();
    const matching = q ? params.filter((p) => p.name.toLowerCase().includes(q)) : params;
    return groupParams(matching);
  }, [params, query]);

  if (!params) {
    return (
      <div className="pluginwin">
        <div className="help" style={{ padding: 16 }}>loading…</div>
      </div>
    );
  }

  const plugin = track?.plugin ?? null;
  const total = params.length;
  const shown = groups.reduce((n, g) => n + g.params.length, 0);

  return (
    <div className="pluginwin">
      <header className="pwhead">
        <div className="pwtitle">
          <span className="n">{plugin?.name ?? "plugin"}</span>
          <span className="meta">{track ? `on ${track.name}` : ""}</span>
        </div>
        <span className="spacer" />
        <input
          type="text"
          placeholder="find a control…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ width: 200 }}
        />
        <span className="help">{query ? `${shown} of ${total}` : `${total} controls`}</span>
      </header>

      {total === 0 ? (
        <div className="help" style={{ padding: 16 }}>
          this plugin publishes no controls of its own — it is played, not dialled.
        </div>
      ) : shown === 0 ? (
        <div className="help" style={{ padding: 16 }}>nothing here is called that.</div>
      ) : (
        <div className="pwbody">
          {groups.map((g) => (
            <section key={g.name} className={`pgroup${folded[g.name] ? " folded" : ""}`}>
              <button className="phead" onClick={() => setFolded((f) => ({ ...f, [g.name]: !f[g.name] }))}>
                <span className="cap">{g.name}</span>
                <span className="help">{g.params.length}</span>
              </button>
              {!folded[g.name] && (
                <div className="pgrid">
                  {g.params.map((p) => (
                    <ParamRow
                      key={p.id}
                      param={p}
                      label={g.strip ? p.name.slice(g.name.length).trim() || p.name : p.name}
                      onDrag={onDrag}
                      onCommit={onCommit}
                    />
                  ))}
                </div>
              )}
            </section>
          ))}
        </div>
      )}

      <footer className="pwfoot">
        <span className="help">drag a row to set it · double-click for the plugin's default · changes land in the project's undo history</span>
      </footer>
    </div>
  );
}
