import { useEffect, useRef, useState } from "react";
import { selectedTrack, useStore } from "../store";
import { hostPlatforms, VOICES, type CatalogAsset, type CatalogEntry, type InstallProgress, type ParamInfo, type Platform } from "../types";
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
          <button className="quiet" onClick={openPluginBrowser}>
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
            <button onClick={openPluginBrowser}>browse plugins</button>
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

/**
 * The hosted plugin's own controls. Values come from the backend on every change of plugin or
 * saved state — including undo — so the knobs always show what the plugin actually holds.
 * Dragging writes straight through; releasing commits, which is what makes one undo step.
 */
export function PluginParams({ trackId, uid, state }: { trackId: string; uid: string; state: string | null }) {
  const pluginParams = useStore((s) => s.pluginParams);
  const setPluginParam = useStore((s) => s.setPluginParam);
  const installed = useStore((s) => s.plugins.some((p) => p.uid === uid));
  const [params, setParams] = useState<ParamInfo[]>([]);
  // The knob's commit callback carries no value, so the last dragged one is kept beside it.
  const dragged = useRef(new Map<number, number>());

  useEffect(() => {
    if (!installed) return;
    let live = true;
    void pluginParams(trackId).then((ps) => {
      if (!live) return;
      setParams(ps);
      dragged.current = new Map(ps.map((p) => [p.id, p.value]));
    });
    return () => {
      live = false;
    };
  }, [pluginParams, trackId, uid, state, installed]);

  // A plugin named by the project but absent from this computer has nothing loaded behind it,
  // so there are no controls to draw — the picker says why instead.
  if (!installed) return null;
  if (params.length === 0) return <div className="help">this plugin publishes no parameters</div>;

  const drag = (p: ParamInfo, norm: number) => {
    const raw = curveOf(p).from(norm);
    const v = p.isStepped ? Math.round(raw) : raw;
    dragged.current.set(p.id, v);
    setParams((ps) => ps.map((x) => (x.id === p.id ? { ...x, value: v } : x)));
    void setPluginParam(trackId, p.id, v, false);
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
                  onCommit={() => void setPluginParam(trackId, p.id, dragged.current.get(p.id) ?? p.value, true)}
                />
              ))}
          </div>
        </div>
      ))}
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
                  <span className="help">ready to use</span>
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
