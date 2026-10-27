import { useEffect, useRef, useState } from "react";
import { selectedTrack, useStore } from "../store";
import { hostPlatforms, VOICES, type CatalogAsset, type CatalogEntry, type InstallProgress, type ParamInfo, type Platform } from "../types";
import { nativeEffect } from "../effects";
import { hz, Knob, lin, log, pct } from "./Knob";

/**
 * Everything plugin-shaped the user touches: the instrument picker and parameter list that the
 * inspector and the devices panel both show, and the catalog browser that installs new ones.
 */

// ─── Instrument picker ─────────────────────────────────────────────────────────────────────

/**
 * The selected track's voice: the built-in ones first, then whatever instrument plugins are
 * installed. `columns` lets the narrow inspector and the wide devices panel lay the chips out
 * differently without either owning the list.
 */
export function InstrumentPicker({ columns }: { columns: number }) {
  const track = useStore(selectedTrack);
  const plugins = useStore((s) => s.plugins);
  const dispatch = useStore((s) => s.dispatch);
  const setTrackPlugin = useStore((s) => s.setTrackPlugin);
  const openPluginBrowser = useStore((s) => s.openPluginBrowser);
  if (!track) return null;

  const instruments = plugins.filter((p) => p.kind === "instrument");
  const current = track.plugin;
  // A project can name a plugin this computer has never had: the uid is saved, the file is not.
  const missing = current !== null && !plugins.some((p) => p.uid === current.uid);
  const grid = { display: "grid", gridTemplateColumns: `repeat(${columns}, 1fr)` } as const;

  const pickBuiltIn = async (voice: string | null) => {
    if (current) await setTrackPlugin(track.id, null);
    if (voice) await dispatch({ type: "setTrackVoice", id: track.id, voice });
  };

  return (
    <>
      <div className="chipsrow" style={grid}>
        {track.isDrum ? (
          <button className={`chip${current ? "" : " on"}`} onClick={() => void pickBuiltIn(null)}>
            drum kit
          </button>
        ) : (
          VOICES.map((v) => (
            <button key={v} className={`chip${!current && track.voice === v ? " on" : ""}`} onClick={() => void pickBuiltIn(v)}>
              {v}
            </button>
          ))
        )}
      </div>
      <div className="pickdivide">
        <span>plugins</span>
      </div>
      {instruments.length === 0 ? (
        <div className="help">
          nothing installed yet —{" "}
          <button className="quiet" onClick={() => openPluginBrowser(true)}>
            find a free one
          </button>
        </div>
      ) : (
        <div className="chipsrow" style={grid}>
          {instruments.map((p) => (
            <button
              key={p.uid}
              className={`chip${current?.uid === p.uid ? " on" : ""}`}
              title={`${p.vendor} · ${p.version}`}
              onClick={() => void setTrackPlugin(track.id, p.uid)}
            >
              {p.name}
            </button>
          ))}
          {/* Wanting a sound you do not have yet is the normal case, so the way to go looking
              sits with the instruments rather than only in a menu. */}
          <button className="chip quiet" onClick={() => openPluginBrowser(true)} title="browse free plugins">
            + find more
          </button>
        </div>
      )}
      {missing && (
        <div className="pluginmissing">
          <div className="devchip">
            <span className="led" />
            <span className="n">{current.name}</span>
            <span className="spacer" />
            <span className="meta">not installed on this computer</span>
          </div>
          <div className="help">this track asks for a plugin that is not here. install it, or go back to a built-in voice.</div>
          <div className="row" style={{ gap: 4 }}>
            <button onClick={() => openPluginBrowser(true)}>browse plugins</button>
            <button className="quiet" onClick={() => void pickBuiltIn(track.isDrum ? null : track.voice)}>
              use the built-in voice
            </button>
          </div>
        </div>
      )}
    </>
  );
}

// ─── Plugin parameters ─────────────────────────────────────────────────────────────────────

/** A knob maps 0..1, so each parameter needs a curve. A range spanning two decades above zero
 *  reads far better logarithmically — that is how the built-in tone and cut knobs already work. */
function curveOf(p: ParamInfo) {
  return p.min > 0 && p.max / p.min >= 100 ? log(p.min, p.max) : lin(p.min, p.max);
}

function paramText(p: ParamInfo, v: number): string {
  if (p.isStepped) return String(Math.round(v));
  if (p.min === 0 && p.max === 1) return pct(v);
  if (p.max >= 1000) return hz(v);
  return v.toFixed(2);
}

/** How many controls still read well as knobs in a side column. Past this the list stops being
 *  scannable and the editor window is the only honest place for it — Dexed publishes 156. */
const INLINE_PARAM_LIMIT = 8;

/**
 * Whose controls these are: a track's instrument, or one effect in an insert chain. Exactly
 * one of the two ids is given; the pair of backend commands behind them is the only
 * difference between the two cases.
 */
export type ParamTarget = { trackId: string; insertId?: undefined } | { insertId: string; trackId?: undefined };

/**
 * The effect's own controls, hosted plugin or stock. Values come from the backend on every
 * change of effect or saved state — including undo — so the knobs always show what is actually
 * loaded. Dragging writes straight through; releasing commits, which is what makes one undo
 * step.
 *
 * A plugin with more controls than fit here is summarised instead, with its own window one
 * click away; cramming a hundred knobs into a 300px column is not showing them. A native
 * effect is exempt: its parameter list was written for this column, grouped and ordered for it,
 * and it has no window of its own to send anyone to.
 */
export function PluginParams(target: ParamTarget & { uid: string; state: string | null }) {
  const { uid, state } = target;
  const native = nativeEffect(uid) !== null;
  const pluginParams = useStore((s) => s.pluginParams);
  const setPluginParam = useStore((s) => s.setPluginParam);
  const insertParams = useStore((s) => s.insertParams);
  const setInsertParam = useStore((s) => s.setInsertParam);
  const openPluginWindow = useStore((s) => s.openPluginWindow);
  const installed = useStore((s) => native || s.plugins.some((p) => p.uid === uid));
  const [params, setParams] = useState<ParamInfo[]>([]);
  // The knob's commit callback carries no value, so the last dragged one is kept beside it.
  const dragged = useRef(new Map<number, number>());
  // The one thing the two cases disagree about, and what a change of owner is recognised by.
  const owner = target.insertId ?? target.trackId;
  const read = () => (target.insertId !== undefined ? insertParams(target.insertId) : pluginParams(target.trackId));
  const write = (paramId: number, value: number, commit: boolean) =>
    target.insertId !== undefined
      ? setInsertParam(target.insertId, paramId, value, commit)
      : setPluginParam(target.trackId, paramId, value, commit);

  useEffect(() => {
    if (!installed) return;
    let live = true;
    void read().then((ps) => {
      if (!live) return;
      setParams(ps);
      dragged.current = new Map(ps.map((p) => [p.id, p.value]));
    });
    return () => {
      live = false;
    };
  }, [pluginParams, insertParams, owner, uid, state, installed]);

  // A plugin named by the project but absent from this computer has nothing loaded behind it,
  // so there are no controls to draw — the picker says why instead.
  if (!installed) return null;

  // An insert has no window of its own: the chain row is where it lives.
  const trackId = target.trackId;
  const openEditor =
    trackId === undefined ? null : (
      <button className="solid" onClick={() => void openPluginWindow(trackId)}>
        open editor
      </button>
    );
  if (params.length === 0) {
    return <div className="help">this plugin publishes no controls of its own — it is played, not dialled.</div>;
  }
  if (!native && params.length > INLINE_PARAM_LIMIT) {
    return (
      <div className="pluginparams">
        <div className="help">{params.length} controls — too many for this column</div>
        {openEditor && <div className="row" style={{ gap: 4 }}>{openEditor}</div>}
      </div>
    );
  }

  const drag = (p: ParamInfo, norm: number) => {
    const raw = curveOf(p).from(norm);
    const v = p.isStepped ? Math.round(raw) : raw;
    dragged.current.set(p.id, v);
    setParams((ps) => ps.map((x) => (x.id === p.id ? { ...x, value: v } : x)));
    void write(p.id, v, false);
  };

  const modules = [...new Set(params.map((p) => p.module))];
  return (
    <div className="pluginparams">
      {modules.map((m) => (
        <div key={m} className="pgroup">
          {m && <span className="flabel">{m}</span>}
          <div className="knobrow">
            {params
              .filter((p) => p.module === m)
              .map((p) => (
                <Knob
                  key={p.id}
                  label={p.name}
                  value={curveOf(p).to(p.value)}
                  display={paramText(p, p.value)}
                  defaultValue={curveOf(p).to(p.default)}
                  onChange={(norm) => drag(p, norm)}
                  onCommit={() => void write(p.id, dragged.current.get(p.id) ?? p.value, true)}
                />
              ))}
          </div>
        </div>
      ))}
      {openEditor && <div className="row" style={{ gap: 4 }}>{openEditor}</div>}
    </div>
  );
}

// ─── Catalog browser ───────────────────────────────────────────────────────────────────────

/** The download this computer would use. A plain archive beats an installer: it is the one the
 *  app can unpack itself. */
function assetFor(entry: CatalogEntry, platforms: Platform[]): CatalogAsset | null {
  const mine = entry.assets.filter((a) => platforms.includes(a.platform));
  return mine.find((a) => a.kind !== "installer") ?? mine[0] ?? null;
}

function sizeText(bytes: number): string {
  return bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.round(bytes / 1e6)} MB`;
}

/**
 * One bar for the whole install. `done`/`total` restart at each phase — bytes while
 * downloading, steps after — so they are folded into a share of the phases instead, which is
 * the only way the bar never runs backwards.
 */
function installPct(p: InstallProgress): number {
  const within = p.total > 0 ? Math.min(1, p.done / p.total) : 0;
  if (p.phase === "downloading") return within * 80;
  if (p.phase === "verifying") return 80 + within * 10;
  if (p.phase === "unpacking") return 90 + within * 10;
  return 100;
}

/**
 * The free-plugin browser. It is written for someone who does not know what a CLAP is: each row
 * leads with a plain sentence about what the plugin sounds like and the words you would search
 * for, and keeps vendor, licence and size to the quiet line underneath.
 */

/**
 * What to do with a plugin once it is installed. Finding a sound is only half the job: without
 * this the user is left to work out that the thing they just downloaded now has to be picked
 * again from a list in another panel. Effects are named honestly — they can be hosted, but
 * nothing can route to them until tracks grow a device chain.
 */
function UseOnTrack({ entry }: { entry: CatalogEntry }) {
    const track = useStore(selectedTrack);
    const plugins = useStore((s) => s.plugins);
    const setTrackPlugin = useStore((s) => s.setTrackPlugin);

    if (entry.kind === "effect") {
      return <span className="help">installed — effects cannot be put on a track yet</span>;
    }
    const match = plugins.find((p) => p.kind === "instrument" && p.name.toLowerCase() === entry.name.toLowerCase());
    if (!track || !match) return <span className="help">ready to use</span>;
    if (track.plugin?.uid === match.uid) return <span className="help">playing on {track.name}</span>;
    return (
      <>
        <button className="solid" onClick={() => void setTrackPlugin(track.id, match.uid)}>
          use on {track.name}
        </button>
        <span className="help">installed</span>
      </>
    );
}
/**
 * The free-plugin browser. It is written for someone who does not know what a CLAP is: each row
 * leads with a plain sentence about what the plugin sounds like and the words you would search
 * for, and keeps vendor, licence and size to the quiet line underneath.
 */
export function PluginBrowser() {
  const catalog = useStore((s) => s.catalog);
  const plugins = useStore((s) => s.plugins);
  const installing = useStore((s) => s.installing);
  const installPlugin = useStore((s) => s.installPlugin);
  const refreshCatalog = useStore((s) => s.refreshCatalog);
  const openUrl = useStore((s) => s.openUrl);
  const [query, setQuery] = useState("");
  const [tag, setTag] = useState<string | null>(null);

  useEffect(() => {
    void refreshCatalog();
  }, [refreshCatalog]);

  const platforms = hostPlatforms();
  // The scan knows file names, the catalog knows product names; the name is the only thing both
  // sides agree on before an entry has ever been installed from here.
  const installedNames = new Set(plugins.map((p) => p.name.toLowerCase()));
  const tags = [...new Set(catalog.flatMap((e) => e.tags))].sort();
  const q = query.trim().toLowerCase();
  const shown = catalog.filter((e) => {
    if (tag && !e.tags.includes(tag)) return false;
    return !q || `${e.name} ${e.vendor} ${e.summary} ${e.tags.join(" ")}`.toLowerCase().includes(q);
  });

  return (
    <div className="browser">
      <div className="bfilter">
        <input
          type="text"
          placeholder="what are you after? pad, bass, drums…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ width: 240 }}
        />
        <div className="chipsrow">
          {tags.map((t) => (
            <button key={t} className={`chip tiny${tag === t ? " on" : ""}`} onClick={() => setTag(tag === t ? null : t)}>
              {t}
            </button>
          ))}
        </div>
        <span className="spacer" />
        <span className="help">
          {shown.length} of {catalog.length} · free to use
        </span>
      </div>
      <div className="brows">
        {shown.length === 0 && <div className="help" style={{ padding: 10 }}>nothing matches that — try a shorter word, or clear the tags.</div>}
        {shown.map((e) => {
          const asset = assetFor(e, platforms);
          const installed = installedNames.has(e.name.toLowerCase());
          const progress: InstallProgress | undefined = installing[e.id];
          const busy = progress !== undefined && progress.phase !== "installed" && progress.phase !== "failed";
          return (
            <div key={e.id} className={`brow${asset ? "" : " off"}`}>
              <div className="bmain">
                <div className="btitle">
                  <span className="n">{e.name}</span>
                  {installed && <span className="badge">installed</span>}
                  {e.kind === "effect" && <span className="chip tiny">effect</span>}
                </div>
                <div className="bsummary">{e.summary}</div>
                <div className="chipsrow">
                  {e.tags.map((t) => (
                    <button key={t} className={`chip tiny${tag === t ? " on" : ""}`} onClick={() => setTag(tag === t ? null : t)}>
                      {t}
                    </button>
                  ))}
                </div>
                <div className="help">
                  {e.vendor} · {e.license}
                  {asset ? ` · ${sizeText(asset.bytes)} download` : " · not available for your computer"}
                </div>
              </div>
              <div className="bact">
                {!asset ? null : asset.kind === "installer" ? (
                  <>
                    {e.homepage.startsWith("https://") && (
                      <button onClick={() => void openUrl(e.homepage)}>open page</button>
                    )}
                    <span className="help">this one comes as an installer you run yourself</span>
                  </>
                ) : installed ? (
                  <UseOnTrack entry={e} />
                ) : (
                  <>
                    <button className="solid" disabled={busy} onClick={() => void installPlugin(e.id)}>
                      {busy ? "installing…" : "install"}
                    </button>
                    {progress && (
                      <>
                        <div className="bbar">
                          <div style={{ width: `${installPct(progress)}%` }} />
                        </div>
                        <span className="help">{progress.message}</span>
                      </>
                    )}
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
/**
 * The browser as an overlay, reachable from anywhere.
 *
 * It used to live only in the bottom panel, which exists in song mode alone — so from the piano
 * roll, where you are when you notice you need a different sound, there was no plugin browser
 * at all. Finding a sound is not a reason to change what the window is showing.
 */
export function PluginBrowserWindow() {
  const open = useStore((s) => s.pluginBrowserOpen);
  const setOpen = useStore((s) => s.openPluginBrowser);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.stopPropagation();
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, setOpen]);

  if (!open) return null;
  return (
    <div className="modal-backdrop" onPointerDown={() => setOpen(false)}>
      <div className="modal plugins" role="dialog" aria-modal="true" aria-label="Free plugins" onPointerDown={(e) => e.stopPropagation()}>
        <div className="shead">
          <span className="mtitle">Free plugins</span>
          <span className="spacer" />
          <button className="ico" onClick={() => setOpen(false)} title="close (Esc)">
            ×
          </button>
        </div>
        <PluginBrowser />
      </div>
    </div>
  );
}
