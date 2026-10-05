import { useRef } from "react";

/**
 * The shared knob from the handoff: 34×34 SVG, 270° sweep starting bottom-left.
 * Vertical drag (shift = fine), double-click = reset, wheel = step.
 * `value` is normalized 0..1; the caller formats `display`.
 */
export function Knob({
  label,
  value,
  display,
  color = "#b48cff",
  size = 34,
  defaultValue,
  onChange,
  onCommit,
}: {
  label: string;
  value: number;
  display: string;
  color?: string;
  size?: 34 | 30;
  defaultValue?: number;
  onChange: (v: number) => void;
  onCommit?: () => void;
}) {
  const drag = useRef<{ y: number; v: number } | null>(null);
  const p = Math.min(1, Math.max(0, value));
  const a = ((135 + p * 270) * Math.PI) / 180;
  const px = 17 + 9 * Math.cos(a);
  const py = 17 + 9 * Math.sin(a);

  return (
    <div
      className={`knob${size === 30 ? " small" : ""}`}
      style={{ width: size === 30 ? 44 : 50 }}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        drag.current = { y: e.clientY, v: p };
        (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        const d = drag.current;
        if (!d) return;
        const scale = e.shiftKey ? 1500 : 150;
        onChange(Math.min(1, Math.max(0, d.v + (d.y - e.clientY) / scale)));
      }}
      onPointerUp={() => {
        if (drag.current) onCommit?.();
        drag.current = null;
      }}
      onDoubleClick={() => {
        if (defaultValue !== undefined) {
          onChange(defaultValue);
          onCommit?.();
        }
      }}
      onWheel={(e) => {
        e.preventDefault();
        onChange(Math.min(1, Math.max(0, p + (e.deltaY < 0 ? 0.02 : -0.02))));
        onCommit?.();
      }}
      title={`${label} ${display}`}
    >
      <svg width={size} height={size} viewBox="0 0 34 34">
        <circle cx="17" cy="17" r="13" fill="none" stroke="#2a2a31" strokeWidth="3" strokeDasharray="61.26 200" transform="rotate(135 17 17)" />
        <circle
          cx="17"
          cy="17"
          r="13"
          fill="none"
          stroke={color}
          strokeWidth="3"
          strokeDasharray={`${(p * 61.26).toFixed(2)} 200`}
          transform="rotate(135 17 17)"
        />
        <circle cx="17" cy="17" r="8" fill="#202026" />
        <line x1="17" y1="17" x2={px.toFixed(2)} y2={py.toFixed(2)} stroke="#e6e6ea" strokeWidth="1.6" strokeLinecap="round" />
      </svg>
      <span className="kl">{label}</span>
      <span className="kv">{display}</span>
    </div>
  );
}

// ─── Mapping helpers ───────────────────────────────────────────────────────────────────────

export const lin = (min: number, max: number) => ({
  to: (v: number) => (v - min) / (max - min),
  from: (p: number) => min + p * (max - min),
});

export const log = (min: number, max: number) => ({
  to: (v: number) => Math.log(Math.max(min, v) / min) / Math.log(max / min),
  from: (p: number) => min * Math.pow(max / min, p),
});

export function hz(v: number): string {
  return v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 1 : 2)}k` : `${Math.round(v)}`;
}

export function pct(v: number): string {
  return `${Math.round(v * 100)}%`;
}

export function panText(pan: number): string {
  const v = Math.round(Math.abs(pan) * 100);
  return v === 0 ? "C" : pan < 0 ? `L${v}` : `R${v}`;
}

export function eqDb(gain: number): string {
  const db = 20 * Math.log10(Math.max(0.02, gain));
  return `${db >= 0 ? "+" : ""}${db.toFixed(1)}`;
}
