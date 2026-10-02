import { selectedTrack, useStore } from "../store";
import type { MasterSettings, TrackParam } from "../types";

function Slider({
  label,
  value,
  min,
  max,
  step = 0.01,
  onChange,
  onCommit,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step?: number;
  onChange: (v: number) => void;
  onCommit: () => void;
}) {
  return (
    <label className="slider">
      <span className="label">{label}</span>
      <input
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        onChange={(e) => onChange(parseFloat(e.target.value))}
        onPointerUp={onCommit}
        onKeyUp={onCommit}
        onBlur={onCommit}
      />
    </label>
  );
}

export function FxBar() {
  const track = useStore(selectedTrack);
  const master = useStore((s) => s.snapshot!.model.master);
  const masterPeak = useStore((s) => s.masterPeak);
  const dispatch = useStore((s) => s.dispatch);
  const commitGesture = useStore((s) => s.commitGesture);

  const trackParam = (param: TrackParam, value: number) => {
    if (track) void dispatch({ type: "setTrackParam", id: track.id, param, value }, true);
  };
  const setMaster = (patch: Partial<MasterSettings>) => void dispatch({ type: "setMaster", master: { ...master, ...patch } }, true);
  const commit = () => void commitGesture();

  return (
    <div className="fx">
      {track && (
        <div className="row">
          <span className="label brand" style={{ width: 44 }}>track</span>
          <Slider label="vol" value={track.volume} min={0} max={1.5} onChange={(v) => trackParam("volume", v)} onCommit={commit} />
          <Slider label="pan" value={track.pan} min={-1} max={1} onChange={(v) => trackParam("pan", v)} onCommit={commit} />
          <Slider label="reverb" value={track.reverbSend} min={0} max={1} onChange={(v) => trackParam("reverbSend", v)} onCommit={commit} />
          <Slider label="tone" value={track.tone} min={800} max={18000} step={10} onChange={(v) => trackParam("tone", v)} onCommit={commit} />
        </div>
      )}
      <div className="row">
        <span className="label" style={{ width: 44 }}>master</span>
        <Slider label="gain" value={master.gain} min={0} max={1.5} onChange={(v) => setMaster({ gain: v })} onCommit={commit} />
        <Slider label="reverb" value={master.reverbWet} min={0} max={1} onChange={(v) => setMaster({ reverbWet: v })} onCommit={commit} />
        <Slider label="low cut" value={master.lowCutHz} min={20} max={1000} step={1} onChange={(v) => setMaster({ lowCutHz: v })} onCommit={commit} />
        <Slider label="tone" value={master.highCutHz} min={800} max={18000} step={10} onChange={(v) => setMaster({ highCutHz: v })} onCommit={commit} />
        <span className="divider" />
        <span className="label">eq</span>
        <Slider label="low" value={master.lowEq} min={0} max={2} onChange={(v) => setMaster({ lowEq: v })} onCommit={commit} />
        <Slider label="mid" value={master.midEq} min={0} max={2} onChange={(v) => setMaster({ midEq: v })} onCommit={commit} />
        <Slider label="high" value={master.highEq} min={0} max={2} onChange={(v) => setMaster({ highEq: v })} onCommit={commit} />
        <span className="spacer" />
        <div className="meter" style={{ width: 90 }}>
          <div style={{ width: `${Math.min(100, Math.max(masterPeak[0], masterPeak[1]) * 100)}%` }} />
        </div>
      </div>
    </div>
  );
}
