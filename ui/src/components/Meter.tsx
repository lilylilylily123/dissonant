import { meterPos } from "../store";

/** Vertical meter: gradient bar with a dark cover from the top down to the level. */
export function VMeter({ peak, height }: { peak: number; height?: number | string }) {
  const cover = `${(1 - meterPos(peak)) * 100}%`;
  return (
    <div className="vmeter" style={{ height }}>
      <div style={{ height: cover }} />
    </div>
  );
}

/** Horizontal meter (master): cover drawn from the right. */
export function HMeter({ peak }: { peak: number }) {
  const cover = `${(1 - meterPos(peak)) * 100}%`;
  return (
    <div className="hbar">
      <div style={{ width: cover }} />
    </div>
  );
}
