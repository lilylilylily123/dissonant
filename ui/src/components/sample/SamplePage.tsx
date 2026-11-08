import { useEffect, useRef, useState } from "react";
import "./sample.css";
import { previewSounding, selectedTrack, useStore } from "../../store";
import { usePanelSize } from "../../panels";
import { MAX_CHOKE_GROUP, MAX_CHOP_ATTACK_SECONDS, MAX_CHOP_GAIN, MAX_CHOP_RELEASE_SECONDS, MIN_CHOP_SECONDS, sourceSeconds } from "../../sampler";
import { CHOP_COUNTS, clampView, cleared, MAX_WAVE_ZOOM, PAD_KEYS, pageSlots, PADS_PER_PAGE, rechopped, regionOf, sameChopper, viewShowing, type WaveView } from "../../sampleChops";
import { beatsPerBar as bpbOf, type AudioSource, type Chop, type Chopper as ChopTool, type GridStep, type SampleFetchPhase, type Sampler, type SamplerVoice, type Track } from "../../types";
import { midiName, noteName } from "../../theory";
import { Knob } from "../Knob";
import { Splitter } from "../Splitter";
import { ChopLanes } from "./ChopLanes";
import { CHOP_HUES, SampleWave } from "./SampleWave";

/** What each fetch phase is doing, in words. */
const PHASE_TEXT: Record<SampleFetchPhase, string> = {
  locating: "finding yt-dlp",
  installing: "installing yt-dlp (first time only)",
  downloading: "downloading",
  converting: "converting",
  decoding: "decoding",
  analyzing: "listening for hits, tempo and key",
  done: "done",
  failed: "failed",
  cancelled: "cancelled",
};

const GRID_STEPS: [GridStep, string, string][] = [
  ["bar", "bar", "a chop every bar of the detected tempo, from the region's start"],
  ["half", "½", "a chop every half bar, from the region's start"],
  ["beat", "beat", "a chop every beat, from the region's start"],
];

const VOICES: [SamplerVoice, string][] = [
  ["mono", "mono: every new hit cuts whatever the track is sounding, like one finger on an MPC"],
  ["poly", "poly: hits layer; only the same chop, or one in the same choke group, cuts"],
];

/** Knob curves: square-law, so the useful low end (short attacks, unity gain) gets most of the travel. */
const sq = (max: number) => ({ to: (v: number) => Math.sqrt(Math.max(0, v) / max), from: (p: number) => p * p * max });
const GAIN = sq(MAX_CHOP_GAIN);
const ATTACK = sq(MAX_CHOP_ATTACK_SECONDS);
const RELEASE = sq(MAX_CHOP_RELEASE_SECONDS);
const secs = (s: number) => (s < 1 ? `${Math.round(s * 1000)}ms` : `${s.toFixed(2)}s`);
const gainDb = (g: number) => (g <= 0.0001 ? "-∞" : `${(20 * Math.log10(g)).toFixed(1)}dB`);

/** The last `sampleLinkFocus` the page acted on, so a remount does not steal focus again. */
let focusHandled = 0;

/**
 * SAMPLE: paste a link, and chop what comes back. The selected sampler track's source fills the
 * middle, with its region (where the chop tools cut) and its chops; chop `i` plays on key
 * `basePitch + i`, and the pads, the inspector, the pattern strip and PAT's chop lanes all
 * speak in those keys. Every edit is a `setSampler` (or the pattern's `setNotes`) on the
 * song's own undo.
 */
export function SamplePage() {
  const track = useStore(selectedTrack);
  const model = useStore((s) => s.snapshot!.model);
  const samplers = model.tracks.filter((t) => t.sampler);
  const sampler = track?.sampler ? track : null;
  const source = sampler ? (model.audioSources ?? []).find((x) => x.id === sampler.sampler!.sourceId) : undefined;
  return (
    <div className="smp">
      <LinkBar samplers={samplers} current={sampler} />
      {!sampler ? (
        <div className="smp-empty">
          {samplers.length === 0 ? (
            <>
              <span className="mono">no sampler tracks yet.</span>
              <span className="mono hint">paste a YouTube (or any http) link above: it downloads, gets analysed, and lands on a new track with a few bars already picked out and chopped at their hits.</span>
            </>
          ) : (
            <>
              <span className="mono">{track ? `${track.name} is not a sampler track.` : "no track selected."} pick one to chop:</span>
              <div className="smp-picks">
                {samplers.map((t) => (
                  <button key={t.id} onClick={() => useStore.getState().selectTrack(t.id)} title={`open ${t.name}`}>
                    {t.name} · {t.sampler!.chops.length} chops
                  </button>
                ))}
              </div>
            </>
          )}
        </div>
      ) : !source ? (
        <div className="smp-empty">
          <span className="mono">missing audio: {sampler.name}'s source is not in this project.</span>
          <span className="mono hint">the track plays silence; fetch the link again to replace it.</span>
        </div>
      ) : (
        <Chopper key={sampler.id} track={sampler} sampler={sampler.sampler!} source={source} />
      )}
    </div>
  );
}

/** The link field, the fetch progress and which sampler track the page shows. */
function LinkBar({ samplers, current }: { samplers: Track[]; current: Track | null }) {
  const fetch = useStore((s) => s.sampleFetch);
  const fetchSample = useStore((s) => s.fetchSample);
  const cancel = useStore((s) => s.cancelSampleFetch);
  const focus = useStore((s) => s.sampleLinkFocus);
  const selectTrack = useStore((s) => s.selectTrack);
  const [url, setUrl] = useState("");
  const inputRef = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (focus === focusHandled) return;
    focusHandled = focus;
    inputRef.current?.focus();
    inputRef.current?.select();
  }, [focus]);
  const go = () => {
    if (!url.trim() || fetch) return;
    // Only a fetch that landed clears the field; a failed one keeps the link to fix.
    void fetchSample(url.trim()).then((landed) => landed && setUrl(""));
  };
  const pct = fetch && fetch.fraction >= 0 ? Math.round(fetch.fraction * 100) : null;
  return (
    <div className="smp-head">
      <span className="smp-kind">sample</span>
      <input
        ref={inputRef}
        type="text"
        className="smp-link"
        placeholder="paste a youtube link…"
        value={url}
        disabled={!!fetch}
        onChange={(e) => setUrl(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") go();
          if (e.key === "Escape") (e.target as HTMLInputElement).blur();
        }}
        spellCheck={false}
        aria-label="link to sample"
      />
      {fetch ? (
        <button className="danger" onClick={() => void cancel()} title="stop the download; nothing is added">
          cancel
        </button>
      ) : (
        <button className="solid" onClick={go} disabled={!url.trim()} title="download the audio, analyse it and add it as a new sampler track (one undo step)">
          fetch
        </button>
      )}
      {fetch && (
        <div className="smp-progress" role="status" aria-live="polite">
          <div className={`smp-bar${pct === null ? " busy" : ""}`}>
            <div style={{ width: `${pct ?? 100}%` }} />
          </div>
          <span className="mono">
            {PHASE_TEXT[fetch.phase]}
            {pct !== null ? ` ${pct}%` : "…"}
            {fetch.title ? ` · ${fetch.title}` : ""}
          </span>
        </div>
      )}
      <span className="spacer" />
      {samplers.length > 0 && (
        <>
          <span className="cap">track</span>
          <select value={current?.id ?? ""} onChange={(e) => selectTrack(e.target.value)} aria-label="sampler track to chop">
            {!current && <option value="">—</option>}
            {samplers.map((t) => (
              <option key={t.id} value={t.id}>
                {t.name}
              </option>
            ))}
          </select>
        </>
      )}
    </div>
  );
}

/** The whole chopping surface for one sampler track whose source is present. */
function Chopper({ track, sampler, source }: { track: Track; sampler: Sampler; source: AudioSource }) {
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);
  const previewChop = useStore((s) => s.previewChop);
  const strikePad = useStore((s) => s.strikePad);
  const stopSamplePreview = useStore((s) => s.stopSamplePreview);
  const noteOff = useStore((s) => s.noteOff);
  const showToast = useStore((s) => s.showToast);
  const bpb = useStore((s) => bpbOf(s.snapshot!.model.timeSignature));
  const duration = sourceSeconds(source);
  const [selected, setSelected] = useState(0);
  const [view, setViewRaw] = useState<WaveView>({ offset: 0, zoom: 1 });
  const preview = useStore((s) => s.samplePreview);
  const playing = preview?.trackId === track.id ? preview : null;
  // Re-render when the preview runs out by its own clock, so play/stop reads what is heard.
  const [, expire] = useState(0);
  useEffect(() => {
    if (!playing) return;
    const left = playing.startedAt + playing.seconds * 1000 - performance.now();
    if (left <= 0) return;
    const t = setTimeout(() => expire((n) => n + 1), left + 20);
    return () => clearTimeout(t);
  }, [playing]);
  const sounding = previewSounding(playing) ? playing : null;
  const [held, setHeld] = useState<ReadonlySet<number>>(() => new Set());
  const setView = (v: WaveView) => setViewRaw(clampView(v, duration));
  const chops = sampler.chops;
  const sel = Math.min(selected, chops.length - 1);
  const chop = sel >= 0 ? chops[sel] : null;
  const page = Math.max(0, Math.floor(sel / PADS_PER_PAGE));
  const pages = Math.max(1, Math.ceil(chops.length / PADS_PER_PAGE));

  const edit = (next: Sampler, transient: boolean, label: string) => void dispatch({ type: "setSampler", id: track.id, sampler: next }, transient, label);
  const commit = () => void commitGesture();
  const select = (i: number) => {
    setSelected(i);
    const c = chops[i];
    if (c) setViewRaw((v) => viewShowing(v, c, duration));
  };
  /** How long chop `i` sounds, for the playhead and for how long the audition holds a gate chop. */
  const chopSeconds = (c: Chop) => c.endSeconds - c.startSeconds;
  const play = (i: number) => {
    const c = chops[i];
    if (!c) return;
    // One preview at a time: a one-shot can't be released, so each pad cuts the last instead
    // of stacking chops that may run for minutes. The hold is the chop's length, which a gate
    // chop needs and a one-shot ignores.
    previewChop(track.id, sampler.basePitch + i, i, chopSeconds(c), false);
  };
  /** Pads held down as live notes (armed and playing): their release is a recorded note-off. */
  const livePads = useRef(new Set<number>());
  const padDown = (i: number) => {
    const c = chops[i];
    select(i);
    if (c && strikePad(track.id, sampler.basePitch + i, i, chopSeconds(c), false)) livePads.current.add(i);
  };
  const padUp = (i: number) => {
    if (livePads.current.delete(i)) noteOff(sampler.basePitch + i);
  };

  // The computer keys play this page's pads: held for as long as the key is.
  useEffect(() => {
    const down = new Map<string, number>();
    const isTyping = (t: EventTarget | null) => {
      const el = t as HTMLElement | null;
      return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.isContentEditable);
    };
    const sync = () => setHeld(new Set(down.values()));
    const onDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !isTyping(e.target)) {
        stopSamplePreview();
        return;
      }
      if (e.metaKey || e.ctrlKey || e.altKey || e.repeat || isTyping(e.target)) return;
      const k = e.key.toLowerCase();
      const slot = (PAD_KEYS as readonly string[]).indexOf(k);
      if (slot < 0 || down.has(k)) return;
      const i = page * PADS_PER_PAGE + slot;
      const c = chops[i];
      if (!c) return;
      e.preventDefault();
      down.set(k, i);
      sync();
      setSelected(i);
      strikePad(track.id, sampler.basePitch + i, i, chopSeconds(c), true);
    };
    const onUp = (e: KeyboardEvent) => {
      const k = e.key.toLowerCase();
      const i = down.get(k);
      if (i === undefined) return;
      down.delete(k);
      sync();
      noteOff(sampler.basePitch + i);
    };
    const allOff = () => {
      for (const i of down.values()) noteOff(sampler.basePitch + i);
      down.clear();
      sync();
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    window.addEventListener("blur", allOff);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("blur", allOff);
      allOff();
    };
  }, [page, chops, sampler.basePitch, track.id, strikePad, stopSamplePreview, noteOff]);

  /** A tool chip: cut the region with it and remember it for region edits; chop *i* keeps its name and shaping. */
  const applyTool = (tool: ChopTool, label: string) => {
    const next = rechopped(sampler, tool, regionOf(sampler, duration), bpb);
    if (!next) return;
    edit(next, false, label);
    setSelected(0);
  };
  const toolClass = (tool: ChopTool) => `chip tiny${sameChopper(sampler.chopper, tool) ? " on" : ""}`;
  const analysis = sampler.analysis;
  const onsets = analysis?.onsets ?? [];
  const zoomBy = (f: number) => {
    const span = duration / view.zoom;
    const zoom = Math.min(MAX_WAVE_ZOOM, Math.max(1, view.zoom * f));
    setView({ zoom, offset: view.offset + span / 2 - duration / zoom / 2 });
  };

  return (
    <>
      <SourceBar track={track} sampler={sampler} source={source} />
      <div className="smp-tools">
        <span className="cap" title="cut at the region's start and its strongest hits: auto N makes at most N chops">auto</span>
        <div className="smp-chips">
          {CHOP_COUNTS.map((n) => (
            <button
              key={n}
              className={toolClass({ kind: "auto", count: n })}
              disabled={onsets.length === 0}
              onClick={() => applyTool({ kind: "auto", count: n }, `auto chop ${n}`)}
              title={onsets.length ? `at most ${n} chops: the region's start and the ${n - 1} strongest hits inside it (fewer when the region holds fewer)` : "no onsets were detected in this source"}
            >
              {n}
            </button>
          ))}
        </div>
        <div className="vdiv" />
        <span className="cap" title="N chops of equal length across the region">equal</span>
        <div className="smp-chips">
          {CHOP_COUNTS.map((n) => (
            <button key={n} className={toolClass({ kind: "equal", count: n })} onClick={() => applyTool({ kind: "equal", count: n }, `equal chops ${n}`)} title={`${n} chops of equal length across the region`}>
              {n}
            </button>
          ))}
        </div>
        <div className="vdiv" />
        <span className="cap" title="cut on the detected tempo's grid, from the region's start">grid</span>
        <div className="smp-chips">
          {GRID_STEPS.map(([step, label, hint]) => (
            <button
              key={step}
              className={toolClass({ kind: "grid", step })}
              disabled={!analysis?.bpm}
              onClick={() => applyTool({ kind: "grid", step }, `grid chops: ${label}`)}
              title={analysis?.bpm ? `${hint} (${analysis.bpm.toFixed(1)} bpm)` : "no tempo was detected in this source"}
            >
              {label}
            </button>
          ))}
        </div>
        <div className="vdiv" />
        <button className="chip tiny" onClick={() => (edit(cleared(sampler, duration), false, "clear chops"), setSelected(0))} title="one chop over the whole region, to cut by hand (double-click the waveform)">
          clear
        </button>
        <span className="spacer" />
        <span className="flabel">zoom</span>
        <button className="quiet" onClick={() => zoomBy(1 / 2)} disabled={view.zoom <= 1} aria-label="zoom out" title="zoom out (⌘-wheel on the waveform)">
          −
        </button>
        <span className="mono smp-zoom">{view.zoom < 10 ? view.zoom.toFixed(1) : Math.round(view.zoom)}×</span>
        <button className="quiet" onClick={() => zoomBy(2)} disabled={view.zoom >= MAX_WAVE_ZOOM} aria-label="zoom in" title="zoom in (⌘-wheel on the waveform)">
          +
        </button>
        <button className="chip tiny" onClick={() => setView({ offset: 0, zoom: 1 })} disabled={view.zoom === 1} title="show the whole source">
          fit
        </button>
        <span
          className="chip tiny"
          style={{ cursor: "help" }}
          title={
            "the region (the lit band on the ruler) is where the tools cut · drag its flags to move an edge, or drag along the ruler to sweep a new one (⇧ snaps to a hit): it re-chops with the lit tool, chops keeping their names\n" +
            "click a chop: select + hear it · drag an edge: move the cut (⌥ only this chop, ⇧ snap to a hit)\ndouble-click: cut there · right-click a chop: delete it\n⌘-wheel: zoom at the pointer · wheel: pan · the strip at the bottom pans too\n" +
            "pads: click, or Z X C V / A S D F / Q W E R / 1 2 3 4 · armed and playing, they record into the pattern below"
          }
        >
          ?
        </span>
      </div>
      <div className="smp-split">
        <SampleWave
          sampler={sampler}
          source={source}
          selected={sel}
          playing={playing}
          view={view}
          setView={setView}
          onSelect={select}
          onAudition={play}
          onEdit={edit}
          onCommit={commit}
          onRefuse={(why) => showToast(why, true)}
          beatsPerBar={bpb}
        />
        <Splitter panel="sampleStrip" axis="y" grows="before" label="the pattern strip" />
        <PatternStrip track={track} />
      </div>
      <div className="smp-bottom">
        <Pads sampler={sampler} selected={sel} held={held} page={page} pages={pages} onPage={(p) => select(p * PADS_PER_PAGE)} onPadDown={padDown} onPadUp={padUp} onBasePitch={(p) => edit({ ...sampler, basePitch: p }, false, "sampler base key")} />
        {chop ? (
          <ChopInspector key={chop.id} index={sel} chop={chop} sampler={sampler} duration={duration} onEdit={edit} onCommit={commit} sounding={sounding?.index === sel} onPlay={() => play(sel)} onStop={stopSamplePreview} onSelect={select} />
        ) : (
          <div className="smp-insp smp-none">
            <span className="mono">no chops. pick a generator above, or double-click the waveform to cut.</span>
          </div>
        )}
      </div>
    </>
  );
}

/**
 * The sampler track's pattern under the waveform, in the chop lanes PAT edits it with — the
 * same pattern, so an edit in either shows in the other. It is the track's selected pattern:
 * that is what recording and the live keys write into, so the pads record where you look.
 */
function PatternStrip({ track }: { track: Track }) {
  const height = usePanelSize("sampleStrip");
  const model = useStore((s) => s.snapshot!.model);
  const selectedId = useStore((s) => s.selectedPatternId);
  const selectPattern = useStore((s) => s.selectPattern);
  const openPattern = useStore((s) => s.openPattern);
  const dispatch = useStore((s) => s.dispatch);
  const patterns = model.patterns.filter((p) => p.trackId === track.id);
  const pattern = patterns.find((p) => p.id === selectedId) ?? patterns[0] ?? null;
  // The selection already keeps to the selected track's patterns; this only covers a pattern
  // arriving where there was none, so recording always lands in the one shown.
  useEffect(() => {
    if (pattern && pattern.id !== selectedId) selectPattern(pattern.id);
  }, [pattern, selectedId, selectPattern]);
  const placed = pattern ? model.clips.some((c) => c.patternId === pattern.id) : false;
  const addPattern = async () => {
    const before = new Set(patterns.map((p) => p.id));
    await dispatch({ type: "addPattern", trackId: track.id });
    const added = useStore.getState().snapshot?.model.patterns.find((p) => p.trackId === track.id && !before.has(p.id));
    if (added) selectPattern(added.id);
  };
  return (
    <div className="smp-strip" style={{ height }}>
      <div className="smp-striphead">
        <span className="cap">pattern</span>
        {pattern ? (
          <select value={pattern.id} onChange={(e) => selectPattern(e.target.value)} aria-label="the pattern the strip edits">
            {patterns.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
        ) : (
          <span className="mono smp-dim">none on {track.name}</span>
        )}
        <button className="chip tiny" onClick={() => void addPattern()} title={`a new empty pattern on ${track.name}, shown here`}>
          + pattern
        </button>
        {pattern && (
          <span className="mono smp-dim smp-placed" title="SAMPLE plays the song: the strip's playhead and recording follow the pattern only where a clip of it plays">
            {pattern.notes.length} hit{pattern.notes.length === 1 ? "" : "s"} · {placed ? "in the song" : "not in the song yet — drag it from the tree onto SONG to hear it here"}
          </span>
        )}
        <span className="spacer" />
        {pattern && (
          <button className="chip tiny" onClick={() => openPattern(pattern.id)} title="edit this pattern on PAT, with the velocity lane">
            PAT ↗
          </button>
        )}
      </div>
      {pattern ? (
        <ChopLanes track={track} pattern={pattern} compact />
      ) : (
        <div className="smp-empty">
          <span className="mono hint">a pattern is where the chops get played: make one, then click a chop's row to place hits, or arm and play the pads.</span>
        </div>
      )}
    </div>
  );
}

/** The source's title, where it came from, and what the analysis heard (with "use" buttons). */
function SourceBar({ track, sampler, source }: { track: Track; sampler: Sampler; source: AudioSource }) {
  const dispatch = useStore((s) => s.dispatch);
  const openUrl = useStore((s) => s.openUrl);
  const songKey = useStore((s) => s.snapshot!.model.key);
  const tempo = useStore((s) => s.snapshot!.model.tempo);
  const a = sampler.analysis;
  const bpm = a?.bpm != null ? Math.round(a.bpm * 10) / 10 : null;
  const keyInUse = a?.keyRoot != null && songKey.rootPitchClass === a.keyRoot && songKey.scale === a.keyScale && songKey.isLocked;
  const conf = (c: number) => `${Math.round(c * 100)}%`;
  return (
    <div className="smp-src">
      <span className="smp-title" title={source.name}>
        {source.name}
      </span>
      <span className="mono smp-dim">
        {sourceSeconds(source).toFixed(2)}s · {source.channels === 1 ? "mono" : "stereo"} · {(source.sampleRate / 1000).toFixed(1)}k · on {track.name}
      </span>
      {source.originUrl && (
        <button className="linkish smp-origin" onClick={() => void openUrl(source.originUrl!)} title={source.originUrl}>
          source ↗
        </button>
      )}
      <div className="vdiv" />
      <span className="cap" title="how many chops the track sounds at once">voice</span>
      <div className="seg sm" role="radiogroup" aria-label="sampler voice">
        {VOICES.map(([voice, hint]) => (
          <div
            key={voice}
            className={sampler.voice === voice ? "on" : ""}
            role="radio"
            aria-checked={sampler.voice === voice}
            onClick={() => sampler.voice !== voice && void dispatch({ type: "setSampler", id: track.id, sampler: { ...sampler, voice } }, false, `${voice} sampler`)}
            title={hint}
          >
            {voice}
          </div>
        ))}
      </div>
      <span className="spacer" />
      {a ? (
        <>
          <span className="cap">tempo</span>
          <span className="mono smp-val" title={`how sure the analysis is: ${a.bpmConfidence.toFixed(2)}`}>
            {bpm !== null ? `${bpm.toFixed(1)}` : "—"}
            {bpm !== null && <span className="smp-conf"> {conf(a.bpmConfidence)}</span>}
          </span>
          <button className="chip tiny" disabled={bpm === null || bpm === tempo} onClick={() => bpm !== null && void dispatch({ type: "setTempo", bpm }, false, "use sample tempo")} title={bpm === tempo ? "the song already runs at this tempo" : "set the song tempo to the sample's"}>
            {bpm !== null && bpm === tempo ? "✓ song tempo" : "use tempo"}
          </button>
          <div className="vdiv" />
          <span className="cap">key</span>
          <span className="mono smp-val" title={`how sure the analysis is: ${a.keyConfidence.toFixed(2)}`}>
            {a.keyRoot !== null ? `${noteName(a.keyRoot)} ${a.keyScale}` : "—"}
            {a.keyRoot !== null && <span className="smp-conf"> {conf(a.keyConfidence)}</span>}
          </span>
          <button
            className={`chip tiny${keyInUse ? " on" : ""}`}
            disabled={a.keyRoot === null || keyInUse}
            onClick={() => a.keyRoot !== null && void dispatch({ type: "setKey", key: { rootPitchClass: a.keyRoot, scale: a.keyScale, isLocked: true } }, false, "use sample key")}
            title={keyInUse ? "the song is locked to this key" : "lock the song to the sample's key, so the tiers judge against it"}
          >
            {keyInUse ? "✓ song key" : "use key"}
          </button>
        </>
      ) : (
        <span className="mono smp-dim">not analysed</span>
      )}
    </div>
  );
}

/** One pad per chop, a page of 16 at a time, labelled with its key; and the base key. */
function Pads({
  sampler,
  selected,
  held,
  page,
  pages,
  onPage,
  onPadDown,
  onPadUp,
  onBasePitch,
}: {
  sampler: Sampler;
  selected: number;
  held: ReadonlySet<number>;
  page: number;
  pages: number;
  onPage: (p: number) => void;
  /** Pointer down on a pad; `onPadUp` follows when it lets go (the pointer is captured). */
  onPadDown: (i: number) => void;
  onPadUp: (i: number) => void;
  onBasePitch: (p: number) => void;
}) {
  const n = sampler.chops.length;
  const slots = pageSlots(page, n);
  const maxBase = Math.max(0, 128 - Math.max(1, n));
  return (
    <div className="smp-pads">
      <div className="smp-padhead">
        <span className="cap">pads</span>
        {pages > 1 && (
          <div className="seg sm" role="radiogroup" aria-label="pad page">
            {Array.from({ length: pages }, (_, p) => (
              <div key={p} className={p === page ? "on" : ""} role="radio" aria-checked={p === page} onClick={() => onPage(p)}>
                {String.fromCharCode(65 + p)}
              </div>
            ))}
          </div>
        )}
        <span className="spacer" />
        <span className="flabel" title="the key chop 1 plays on; every next chop is one semitone up">base</span>
        <NumField value={sampler.basePitch} min={0} max={maxBase} step={1} digits={0} onCommit={(v) => onBasePitch(Math.round(v))} label="base key (MIDI note of chop 1)" />
        <span className="mono smp-dim">{midiName(sampler.basePitch)}</span>
      </div>
      <div className="smp-grid">
        {slots.map((i, slot) => {
          if (i < 0) return <div key={`empty-${slot}`} className="smp-pad empty" />;
          const c = sampler.chops[i];
          const keyLetter = PAD_KEYS[i - page * PADS_PER_PAGE];
          return (
            <button
              key={c.id}
              className={`smp-pad${i === selected ? " sel" : ""}${held.has(i) ? " on" : ""}`}
              style={{ "--hue": CHOP_HUES[i % CHOP_HUES.length] } as React.CSSProperties}
              onPointerDown={(e) => {
                if (e.button !== 0) return;
                e.currentTarget.setPointerCapture(e.pointerId);
                onPadDown(i);
              }}
              onPointerUp={() => onPadUp(i)}
              onPointerCancel={() => onPadUp(i)}
              title={`chop ${i + 1} · ${midiName(sampler.basePitch + i)} · key ${keyLetter.toUpperCase()}${c.name ? ` · ${c.name}` : ""}`}
            >
              <span className="num">{i + 1}</span>
              <span className="ltr">{keyLetter}</span>
              <span className="nm">{midiName(sampler.basePitch + i)}</span>
              <span className="sub">{c.name ?? `${(c.endSeconds - c.startSeconds).toFixed(2)}s`}</span>
            </button>
          );
        })}
      </div>
    </div>
  );
}

/** The selected chop's shape: where it sits, how loud, which way, how it answers note-off. */
function ChopInspector({
  index,
  chop,
  sampler,
  duration,
  onEdit,
  onCommit,
  sounding,
  onPlay,
  onStop,
  onSelect,
}: {
  index: number;
  chop: Chop;
  sampler: Sampler;
  duration: number;
  onEdit: (next: Sampler, transient: boolean, label: string) => void;
  onCommit: () => void;
  /** This chop is the preview being heard: the button stops it instead. */
  sounding: boolean;
  onPlay: () => void;
  onStop: () => void;
  onSelect: (i: number) => void;
}) {
  const [name, setName] = useState(chop.name ?? "");
  // An undo (or any edit from elsewhere) renames the chop under the field.
  useEffect(() => setName(chop.name ?? ""), [chop.name]);
  const set = (patch: Partial<Chop>, transient: boolean, label: string) => {
    const chops = sampler.chops.map((c, i) => (i === index ? { ...c, ...patch } : c));
    onEdit({ ...sampler, chops }, transient, label);
  };
  const commitName = () => {
    const trimmed = name.trim();
    if (trimmed === (chop.name ?? "")) return;
    const chops = sampler.chops.map((c, i) => {
      if (i !== index) return c;
      const { name: _old, ...rest } = c;
      // Absent, never "": the pad shows its key name instead.
      return trimmed ? { ...rest, name: trimmed } : rest;
    });
    onEdit({ ...sampler, chops }, false, "rename chop");
  };
  const n = sampler.chops.length;
  return (
    <div className="smp-insp" style={{ "--hue": CHOP_HUES[index % CHOP_HUES.length] } as React.CSSProperties}>
      <div className="smp-ihead">
        <span className="smp-chopno">{index + 1}</span>
        <span className="mono smp-dim">{midiName(sampler.basePitch + index)}</span>
        <input
          type="text"
          className="smp-name"
          value={name}
          placeholder={`chop ${index + 1}`}
          onChange={(e) => setName(e.target.value)}
          onBlur={commitName}
          onKeyDown={(e) => {
            if (e.key === "Enter") (e.target as HTMLInputElement).blur();
            if (e.key === "Escape") {
              setName(chop.name ?? "");
              (e.target as HTMLInputElement).blur();
            }
          }}
          spellCheck={false}
          aria-label="chop name"
        />
        <span className="spacer" />
        <button className="quiet" disabled={index === 0} onClick={() => onSelect(index - 1)} aria-label="previous chop" title="previous chop">
          ‹
        </button>
        <button className="quiet" disabled={index >= n - 1} onClick={() => onSelect(index + 1)} aria-label="next chop" title="next chop">
          ›
        </button>
        {sounding ? (
          <button onClick={onStop} title="stop this chop (Esc, or Space)">
            ■ stop
          </button>
        ) : (
          <button onClick={onPlay} title="hear this chop">
            ▶ play
          </button>
        )}
      </div>
      <div className="smp-ibody">
        <div className="smp-times">
          <label className="smp-field">
            <span className="flabel">start</span>
            <NumField value={chop.startSeconds} min={0} max={chop.endSeconds - MIN_CHOP_SECONDS} step={0.001} digits={3} onCommit={(v) => set({ startSeconds: v }, false, "chop start")} label="chop start, seconds" />
          </label>
          <label className="smp-field">
            <span className="flabel">end</span>
            <NumField value={chop.endSeconds} min={chop.startSeconds + MIN_CHOP_SECONDS} max={duration} step={0.001} digits={3} onCommit={(v) => set({ endSeconds: v }, false, "chop end")} label="chop end, seconds" />
          </label>
          <span className="mono smp-dim">{secs(chop.endSeconds - chop.startSeconds)} long</span>
        </div>
        <div className="knobrow">
          <Knob label="gain" value={GAIN.to(chop.gain)} display={gainDb(chop.gain)} defaultValue={GAIN.to(1)} color="#9a9aa4" onChange={(p) => set({ gain: GAIN.from(p) }, true, "chop gain")} onCommit={onCommit} />
          <Knob label="attack" value={ATTACK.to(chop.attackSeconds)} display={secs(chop.attackSeconds)} defaultValue={0} color="#3dc8ff" onChange={(p) => set({ attackSeconds: ATTACK.from(p) }, true, "chop attack")} onCommit={onCommit} />
          <Knob
            label="release"
            value={RELEASE.to(chop.releaseSeconds)}
            display={secs(chop.releaseSeconds)}
            defaultValue={RELEASE.to(0.01)}
            color="#3dc8ff"
            hint="gate chops fade out over this after you let go"
            onChange={(p) => set({ releaseSeconds: RELEASE.from(p) }, true, "chop release")}
            onCommit={onCommit}
          />
        </div>
        <div className="smp-opts">
          <div className="smp-field">
            <span className="flabel">mode</span>
            <div className="seg sm" role="radiogroup" aria-label="how the chop answers note-off">
              <div className={chop.mode === "oneShot" ? "on" : ""} role="radio" aria-checked={chop.mode === "oneShot"} onClick={() => chop.mode !== "oneShot" && set({ mode: "oneShot" }, false, "chop mode")} title="one-shot: plays to its end whatever the note length">
                one-shot
              </div>
              <div className={chop.mode === "gate" ? "on" : ""} role="radio" aria-checked={chop.mode === "gate"} onClick={() => chop.mode !== "gate" && set({ mode: "gate" }, false, "chop mode")} title="gate: plays while the note is held, then releases">
                gate
              </div>
            </div>
          </div>
          <div className="smp-field">
            <span className="flabel">play</span>
            <button className={`chip tiny${chop.reverse ? " on" : ""}`} aria-pressed={chop.reverse} onClick={() => set({ reverse: !chop.reverse }, false, chop.reverse ? "chop forward" : "reverse chop")} title="play the chop backwards">
              {chop.reverse ? "◀ reverse" : "▶ forward"}
            </button>
          </div>
          {/* Mono cuts on every hit, so a choke group would have nothing left to do. */}
          {sampler.voice === "poly" && (
            <label className="smp-field">
              <span className="flabel">choke</span>
              <select value={chop.choke} onChange={(e) => set({ choke: Number(e.target.value) }, false, "chop choke")} title="chops in the same group cut each other off (0: none)" aria-label="choke group">
                {Array.from({ length: MAX_CHOKE_GROUP + 1 }, (_, g) => (
                  <option key={g} value={g}>
                    {g === 0 ? "none" : g}
                  </option>
                ))}
              </select>
            </label>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * A number typed in: edits stay in the field until Enter or leaving it, then one commit, so an
 * undo step is a value and not every keystroke. Out-of-range input is clamped, nonsense ignored.
 */
function NumField({ value, min, max, step, digits, onCommit, label }: { value: number; min: number; max: number; step: number; digits: number; onCommit: (v: number) => void; label: string }) {
  const [draft, setDraft] = useState<string | null>(null);
  const commit = (text: string) => {
    setDraft(null);
    const typed = parseFloat(text);
    if (!Number.isFinite(typed)) return;
    const v = Math.min(max, Math.max(min, typed));
    if (Math.abs(v - value) > 1e-9) onCommit(v);
  };
  return (
    <input
      type="number"
      className="mono smp-num"
      min={min}
      max={max}
      step={step}
      value={draft ?? value.toFixed(digits)}
      aria-label={label}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={(e) => commit(e.target.value)}
      onKeyDown={(e) => {
        if (e.key === "Enter") (e.target as HTMLInputElement).blur();
        if (e.key === "Escape") {
          setDraft(null);
          (e.target as HTMLInputElement).blur();
        }
      }}
    />
  );
}
