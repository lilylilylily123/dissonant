import { useEffect } from "react";
import { AmpRack } from "./AmpRack";
import { WaveformLane } from "./WaveformLane";
import { HMeter } from "./Meter";
import { selectedAudioTrack, useStore } from "../store";
import { ticksToBeats } from "../types";

/** `m:ss` for a clip length. */
function clock(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * The guitar screen: audio tracks and the input on the left, the selected track's clips and
 * its rig on the right.
 *
 * Owns both columns rather than reusing the inspector: nothing here is about notes.
 */
export function GuitarTab() {
  const model = useStore((s) => s.snapshot!.model);

  const selectTrack = useStore((s) => s.selectTrack);
  const addAudioTrack = useStore((s) => s.addAudioTrack);
  const audio = useStore((s) => s.audio);
  const inputDevices = useStore((s) => s.inputDevices);
  const inputPeak = useStore((s) => s.inputPeak);
  const armedId = useStore((s) => s.armedAudioTrackId);
  const setArm = useStore((s) => s.setArm);
  const monitor = useStore((s) => s.monitor);
  const toggleMonitor = useStore((s) => s.toggleMonitor);
  const settings = useStore((s) => s.settings);
  const updateSettings = useStore((s) => s.updateSettings);
  const importAudioFile = useStore((s) => s.importAudioFile);
  const importing = useStore((s) => s.importing);
  const selectedAudioClipId = useStore((s) => s.selectedAudioClipId);
  const selectAudioClip = useStore((s) => s.selectAudioClip);
  const dispatch = useStore((s) => s.dispatch);
  const refreshInputDevices = useStore((s) => s.refreshInputDevices);

  useEffect(() => {
    void refreshInputDevices();
  }, [refreshInputDevices]);

  const audioTracks = model.tracks.filter((t) => t.isAudio);
  const selected = useStore(selectedAudioTrack);
  const clips = (model.audioClips ?? []).filter(
    (c) => c.trackId === selected?.id,
  );
  const sourceName = (id: string) =>
    (model.audioSources ?? []).find((s) => s.id === id)?.name ?? "missing";
  const device = inputDevices.find(
    (d) => d.name === (settings.audio.inputDevice ?? audio?.inputDevice),
  );
  const channels = device?.channels ?? 1;

  return (
    <>
      <div className="side guitar">
        <div className="section">
          <div className="shead">
            <span>audio tracks</span>
            <span
              className="act"
              onClick={() => void addAudioTrack()}
              title="add an audio track"
            >
              ＋
            </span>
          </div>
          {audioTracks.length === 0 ? (
            <div className="empty mono">no audio tracks yet — press ＋</div>
          ) : (
            audioTracks.map((t) => (
              <div
                key={t.id}
                className={`row${t.id === selected?.id ? " sel" : ""}`}
                onClick={() => selectTrack(t.id)}
                title={
                  t.amp?.enabled ? `rig: ${t.amp.amp.model}` : "rig bypassed"
                }
              >
                <span className={`led${t.id === armedId ? " rec" : ""}`} />
                <span className="name">{t.name}</span>
                <span className="meta mono">
                  {
                    (model.audioClips ?? []).filter((c) => c.trackId === t.id)
                      .length
                  }
                </span>
              </div>
            ))
          )}
        </div>

        <div className="section">
          <div className="shead">
            <span>input</span>
            <span
              className="act"
              onClick={() => void refreshInputDevices()}
              title="look for devices again"
            >
              ↻
            </span>
          </div>
          <div className="field">
            <select
              value={settings.audio.inputDevice ?? ""}
              onChange={(e) =>
                void updateSettings("audio", {
                  inputDevice: e.target.value || null,
                })
              }
            >
              <option value="">system default</option>
              {inputDevices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                  {d.isDefault ? " (default)" : ""}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <span className="cap">channel</span>
            <select
              value={settings.audio.inputChannel}
              onChange={(e) =>
                void updateSettings("audio", {
                  inputChannel: Number(e.target.value),
                })
              }
            >
              {Array.from({ length: Math.max(1, channels) }, (_, i) => (
                <option key={i} value={i}>
                  {i + 1}
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <span className="cap">buffer</span>
            <select
              value={settings.audio.bufferSize ?? ""}
              onChange={(e) =>
                void updateSettings("audio", {
                  bufferSize: e.target.value ? Number(e.target.value) : null,
                })
              }
              title="the single biggest lever on how direct the guitar feels — smaller is tighter, at more CPU"
            >
              <option value="">device default</option>
              {[64, 128, 256, 512, 1024].map((n) => (
                <option key={n} value={n}>
                  {n} frames
                </option>
              ))}
            </select>
          </div>
          <div className="field">
            <HMeter peak={inputPeak} />
          </div>
          <div className="field">
            <button
              className={`armbtn${armedId && armedId === selected?.id ? " on" : ""}`}
              disabled={!selected}
              onClick={() =>
                void setArm(
                  armedId === selected?.id ? null : (selected?.id ?? null),
                )
              }
              title="send the input to this track"
            >
              {armedId === selected?.id ? "armed" : "arm"}
            </button>
            <span
              className={`chip${monitor ? " on" : ""}`}
              onClick={() => void toggleMonitor()}
              title="hear the input through the rig"
            >
              monitor
            </span>
          </div>
          <div className="mono tiny">
            {audio?.inputError ? (
              <span style={{ color: "var(--rec-text)" }}>
                {audio.inputError}
              </span>
            ) : (
              <>
                round-trip {(audio?.roundTripMs ?? 0).toFixed(1)} ms
                {audio?.inputUnderruns
                  ? ` · ${audio.inputUnderruns} drops`
                  : ""}
              </>
            )}
          </div>
        </div>

        <div className="section">
          <div className="shead">
            <span>clips</span>
            <span className="meta mono">{clips.length}</span>
          </div>
          {clips.length === 0 ? (
            <div className="empty mono">
              record, drop a file, or import audio
            </div>
          ) : (
            clips.map((c) => (
              <div
                key={c.id}
                className={`takerow${c.id === selectedAudioClipId ? " sel" : ""}`}
                onClick={() => selectAudioClip(c.id)}
                onContextMenu={(e) => {
                  e.preventDefault();
                  void dispatch({ type: "removeAudioClip", id: c.id });
                }}
                title="click: select · right-click: remove"
              >
                <span className="name">{sourceName(c.sourceId)}</span>
                <span className="meta mono">
                  {clock(c.lengthSeconds)} · bar{" "}
                  {Math.floor(ticksToBeats(c.startTick) / 4) + 1}
                </span>
              </div>
            ))
          )}
        </div>
      </div>

      <div className="editor">
        <div className="toolbar">
          <span className="cap">{selected?.name ?? "no audio track"}</span>
          {selected && (
            <>
              <span
                className={`chip${selected.amp?.enabled ? " on" : ""}`}
                onClick={() =>
                  void useStore
                    .getState()
                    .setAmp(selected.id, { enabled: !selected.amp?.enabled })
                }
                title="bypass the whole rig"
              >
                rig
              </span>
              <span
                className="chip"
                onClick={() => void importAudioFile(selected.id)}
              >
                import audio…
              </span>
            </>
          )}
          <span className="spacer" />
          {importing && (
            <span className="mono tiny">
              importing {importing.name} —{" "}
              {Math.round(importing.fraction * 100)}%
            </span>
          )}
          <span
            className="chip"
            title={
              "drag a clip to move it (⇧ for beats) · drag an edge to trim · drag a top corner for a fade · right-click to remove"
            }
          >
            ?
          </span>
        </div>
        {selected ? (
          <>
            <WaveformLane track={selected} />
            <AmpRack track={selected} />
          </>
        ) : (
          <div className="empty mono" style={{ padding: 24 }}>
            add an audio track to record or import a guitar part
          </div>
        )}
      </div>
    </>
  );
}
