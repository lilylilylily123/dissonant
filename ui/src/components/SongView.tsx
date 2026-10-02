import { useStore } from "../store";

const PX_PER_BEAT = 6;

/** Song mode: arrange patterns into a full track. Click a pattern to append it; click a
 *  block to remove it; ◂ ▸ nudge a block. A playhead sweeps during playback. */
export function SongView() {
  const model = useStore((s) => s.snapshot!.model);
  const playhead = useStore((s) => s.playhead);
  const dispatch = useStore((s) => s.dispatch);

  const name = (id: string) => model.patterns.find((p) => p.id === id)?.name ?? "?";
  const length = (id: string) => model.patterns.find((p) => p.id === id)?.lengthBeats ?? 0;
  const setArrangement = (arrangement: string[]) => void dispatch({ type: "setArrangement", arrangement });

  const move = (index: number, dir: -1 | 1) => {
    const a = [...model.arrangement];
    const j = index + dir;
    if (j < 0 || j >= a.length) return;
    [a[index], a[j]] = [a[j], a[index]];
    setArrangement(a);
  };

  return (
    <div className="song">
      <div>
        <div className="label" style={{ marginBottom: 6 }}>patterns — click to add to the song</div>
        <div className="row tight">
          {model.patterns.map((p) => (
            <button key={p.id} className="chip" onClick={() => setArrangement([...model.arrangement, p.id])}>
              {p.name} <span className="label">{p.lengthBeats / 4} bars</span>
            </button>
          ))}
        </div>
      </div>
      <div>
        <div className="label" style={{ marginBottom: 6 }}>song — click a block to remove · ◂ ▸ to reorder</div>
        <div className="songrow">
          {model.arrangement.map((pid, i) => (
            <div
              key={`${pid}-${i}`}
              className="songblock"
              style={{ width: Math.max(72, length(pid) * PX_PER_BEAT), position: "relative" }}
              onClick={() => setArrangement(model.arrangement.filter((_, j) => j !== i))}
              title="click to remove"
            >
              <button
                className="ghost faded"
                style={{ position: "absolute", left: 2, fontSize: 10 }}
                onClick={(e) => (e.stopPropagation(), move(i, -1))}
              >
                ◂
              </button>
              {name(pid)}
              <button
                className="ghost faded"
                style={{ position: "absolute", right: 2, fontSize: 10 }}
                onClick={(e) => (e.stopPropagation(), move(i, 1))}
              >
                ▸
              </button>
            </div>
          ))}
          {model.arrangement.length === 0 && <span className="label" style={{ alignSelf: "center" }}>add patterns above to build your song →</span>}
          <div className="playhead" style={{ left: playhead * PX_PER_BEAT + Math.floor(playhead / 4) * 0 }} />
        </div>
      </div>
    </div>
  );
}
