import { useEffect, useRef, type KeyboardEvent, type PointerEvent as RPointerEvent } from "react";
import { engine } from "../audio/host";
import { STATE_HEADER, STATE_TRACK_STRIDE } from "../audio/ids";

// ------------------------------------------------------------ icons

export const Icon = {
  play: <svg viewBox="0 0 12 12" fill="currentColor"><path d="M2 1l9 5-9 5z" /></svg>,
  stop: <svg viewBox="0 0 12 12" fill="currentColor"><rect x="2" y="2" width="8" height="8" rx="1" /></svg>,
  rec: <svg viewBox="0 0 12 12" fill="currentColor"><circle cx="6" cy="6" r="4.2" /></svg>,
  plus: <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M6 1.5v9M1.5 6h9" /></svg>,
  search: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6"><circle cx="7" cy="7" r="4.5" /><path d="M10.5 10.5L14 14" /></svg>,
  star: <svg viewBox="0 0 16 16" fill="currentColor" width="13" height="13"><path d="M8 1.5l1.9 4 4.3.5-3.2 3 .9 4.3L8 11.2l-3.9 2.1.9-4.3-3.2-3 4.3-.5z" /></svg>,
  undo: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M5 3L2 6l3 3" /><path d="M2 6h7a4 4 0 010 8H6" /></svg>,
  redo: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M11 3l3 3-3 3" /><path d="M14 6H7a4 4 0 000 8h3" /></svg>,
  folder: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M1.5 4.5h5l1.5 1.5h6.5v7.5h-13z" /></svg>,
  panel: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><rect x="1.5" y="2.5" width="13" height="11" rx="1" /><path d="M6 2.5v11" /></svg>,
  metro: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M5.5 1.5h5L13 14.5H3z" /><path d="M8 11l3.5-6" /></svg>,
  export: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M8 2v8M4.5 6.5L8 10l3.5-3.5" /><path d="M2.5 11.5v2h11v-2" /></svg>,
  midi: <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.5"><circle cx="8" cy="8" r="6.5" /><circle cx="5" cy="7" r=".8" fill="currentColor" /><circle cx="8" cy="5.5" r=".8" fill="currentColor" /><circle cx="11" cy="7" r=".8" fill="currentColor" /><circle cx="6" cy="10" r=".8" fill="currentColor" /><circle cx="10" cy="10" r=".8" fill="currentColor" /></svg>,
  close: <svg viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6"><path d="M2.5 2.5l7 7M9.5 2.5l-7 7" /></svg>,
  logo: (
    <svg viewBox="0 0 32 32"><rect x="3" y="4" width="11" height="11" rx="2" fill="#56d9a8" /><rect x="18" y="4" width="11" height="11" rx="2" fill="#e8a94a" /><rect x="3" y="18" width="11" height="11" rx="2" fill="#8b9cff" /><path d="M19 18.5l10 5-10 5z" fill="#e4e9e6" /></svg>
  ),
};

// ------------------------------------------------------------ knob

export type Curve = "lin" | "log" | { pow: number };

function toNorm(v: number, min: number, max: number, curve: Curve) {
  if (curve === "log") return Math.log(v / min) / Math.log(max / min);
  if (typeof curve === "object") return Math.pow((v - min) / (max - min), 1 / curve.pow);
  return (v - min) / (max - min);
}

function fromNorm(n: number, min: number, max: number, curve: Curve) {
  n = Math.min(1, Math.max(0, n));
  if (curve === "log") return min * Math.pow(max / min, n);
  if (typeof curve === "object") return min + (max - min) * Math.pow(n, curve.pow);
  return min + (max - min) * n;
}

export interface KnobProps {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (v: number) => void;
  curve?: Curve;
  step?: number;
  format?: (v: number) => string;
  defaultValue?: number;
  size?: number;
  bipolar?: boolean;
  color?: string;
}

export function Knob({ label, value, min, max, onChange, curve = "lin", step, format, defaultValue, size = 34, bipolar, color }: KnobProps) {
  const drag = useRef<{ y: number; n: number } | null>(null);
  const n = Math.min(1, Math.max(0, toNorm(value, min, max, curve)));
  const set = (nn: number) => {
    let v = fromNorm(nn, min, max, curve);
    if (step) v = Math.round(v / step) * step;
    onChange(Math.min(max, Math.max(min, v)));
  };
  const onDown = (e: RPointerEvent) => {
    (e.target as Element).setPointerCapture(e.pointerId);
    drag.current = { y: e.clientY, n };
  };
  const onMove = (e: RPointerEvent) => {
    if (!drag.current) return;
    const sens = e.shiftKey ? 600 : 150;
    set(drag.current.n + (drag.current.y - e.clientY) / sens);
  };
  const onKey = (e: KeyboardEvent) => {
    const d = e.key === "ArrowUp" || e.key === "ArrowRight" ? 1 : e.key === "ArrowDown" || e.key === "ArrowLeft" ? -1 : 0;
    if (d) {
      e.preventDefault();
      set(n + d * (e.shiftKey ? 0.002 : 0.02));
    }
  };
  const r = size / 2 - 3;
  const c = size / 2;
  const a0 = Math.PI * 0.75;
  const sweep = Math.PI * 1.5;
  const arc = (from: number, to: number) => {
    const x1 = c + r * Math.cos(a0 + sweep * from);
    const y1 = c + r * Math.sin(a0 + sweep * from);
    const x2 = c + r * Math.cos(a0 + sweep * to);
    const y2 = c + r * Math.sin(a0 + sweep * to);
    const large = sweep * Math.abs(to - from) > Math.PI ? 1 : 0;
    return `M${x1} ${y1} A${r} ${r} 0 ${large} 1 ${x2} ${y2}`;
  };
  const start = bipolar ? 0.5 : 0;
  const ang = a0 + sweep * n;
  return (
    <div className="knob">
      <svg
        width={size}
        height={size}
        role="slider"
        tabIndex={0}
        aria-label={label}
        aria-valuemin={min}
        aria-valuemax={max}
        aria-valuenow={value}
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={() => (drag.current = null)}
        onDoubleClick={() => defaultValue !== undefined && onChange(defaultValue)}
        onWheel={(e) => set(n - Math.sign(e.deltaY) * 0.02)}
        onKeyDown={onKey}
      >
        <path d={arc(0, 1)} stroke="var(--line)" strokeWidth="3" fill="none" strokeLinecap="round" />
        {Math.abs(n - start) > 0.004 && (
          <path d={arc(Math.min(start, n), Math.max(start, n))} stroke={color ?? "var(--accent)"} strokeWidth="3" fill="none" strokeLinecap="round" />
        )}
        <line x1={c} y1={c} x2={c + (r - 4) * Math.cos(ang)} y2={c + (r - 4) * Math.sin(ang)} stroke="var(--text)" strokeWidth="2" strokeLinecap="round" />
      </svg>
      <span className="value">{format ? format(value) : value.toFixed(2)}</span>
      <span className="label">{label}</span>
    </div>
  );
}

export const fmt = {
  pct: (v: number) => `${Math.round(v * 100)}%`,
  hz: (v: number) => (v >= 1000 ? `${(v / 1000).toFixed(v >= 10000 ? 0 : 1)}k` : `${Math.round(v)}`),
  sec: (v: number) => (v < 1 ? `${Math.round(v * 1000)}ms` : `${v.toFixed(2)}s`),
  ms: (v: number) => `${Math.round(v)}ms`,
  db: (v: number) => `${v.toFixed(1)}dB`,
  gainDb: (v: number) => (v <= 0.0001 ? "-inf" : `${(20 * Math.log10(v)).toFixed(1)}`),
  pan: (v: number) => (Math.abs(v) < 0.01 ? "C" : v < 0 ? `${Math.round(-v * 50)}L` : `${Math.round(v * 50)}R`),
  semi: (v: number) => `${v > 0 ? "+" : ""}${Math.round(v)}st`,
  ratio: (v: number) => `${v.toFixed(1)}:1`,
  num: (v: number) => v.toFixed(2),
};

// ------------------------------------------------------------ fader

/** Volume fader: 0..2 linear gain with a dB-ish feel. */
export function Fader({ value, onChange, height = 104 }: { value: number; onChange: (v: number) => void; height?: number }) {
  const ref = useRef<HTMLDivElement>(null);
  const toPos = (v: number) => Math.pow(v / 2, 1 / 2.2);
  const fromPos = (p: number) => 2 * Math.pow(Math.min(1, Math.max(0, p)), 2.2);
  const drag = useRef<{ y: number; p: number } | null>(null);
  const p = toPos(value);
  const travel = height - 18;
  return (
    <div
      ref={ref}
      className="fader"
      style={{ height }}
      role="slider"
      tabIndex={0}
      aria-label="Volume"
      aria-valuenow={value}
      onPointerDown={(e) => {
        (e.target as Element).setPointerCapture(e.pointerId);
        drag.current = { y: e.clientY, p };
      }}
      onPointerMove={(e) => {
        if (!drag.current) return;
        onChange(fromPos(drag.current.p + (drag.current.y - e.clientY) / (e.shiftKey ? travel * 4 : travel)));
      }}
      onPointerUp={() => (drag.current = null)}
      onDoubleClick={() => onChange(0.8)}
      onWheel={(e) => onChange(fromPos(p - Math.sign(e.deltaY) * 0.02))}
      onKeyDown={(e) => {
        if (e.key === "ArrowUp") onChange(fromPos(p + 0.02));
        if (e.key === "ArrowDown") onChange(fromPos(p - 0.02));
      }}
    >
      <div className="track" />
      <div className="cap" style={{ top: 4 + (1 - p) * travel }} />
    </div>
  );
}

// ------------------------------------------------------------ meter

/** Stereo peak meter drawn from engine state on every animation frame. */
export function Meter({ track, height = 104 }: { track: number | "master"; height?: number }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    let raf = 0;
    const cv = ref.current!;
    const g = cv.getContext("2d")!;
    const dpr = window.devicePixelRatio || 1;
    cv.width = 10 * dpr;
    cv.height = height * dpr;
    const draw = () => {
      const s = engine.state;
      const o = track === "master" ? 3 : STATE_HEADER + track * STATE_TRACK_STRIDE + 4;
      const l = s[o] ?? 0;
      const r = s[o + 1] ?? 0;
      g.clearRect(0, 0, cv.width, cv.height);
      const bar = (v: number, x: number) => {
        const db = 20 * Math.log10(Math.max(v, 1e-6));
        const n = Math.max(0, Math.min(1, (db + 54) / 54));
        const h = n * cv.height;
        const grad = g.createLinearGradient(0, cv.height, 0, 0);
        grad.addColorStop(0, "#3fa684");
        grad.addColorStop(0.75, "#56d9a8");
        grad.addColorStop(0.9, "#e8a94a");
        grad.addColorStop(1, "#f0565a");
        g.fillStyle = "#1c2125";
        g.fillRect(x, 0, 4 * dpr, cv.height);
        g.fillStyle = grad;
        g.fillRect(x, cv.height - h, 4 * dpr, h);
      };
      bar(l, 0);
      bar(r, 6 * dpr);
      raf = requestAnimationFrame(draw);
    };
    raf = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(raf);
  }, [track, height]);
  return (
    <div className="meter" style={{ height }}>
      <canvas ref={ref} />
    </div>
  );
}

// ------------------------------------------------------------ waveform

export function Waveform({ peaks, color = "var(--muted)", className, height = 18 }: { peaks: number[]; color?: string; className?: string; height?: number }) {
  const w = peaks.length;
  if (!w) return <svg className={className} height={height} />;
  const mid = height / 2;
  let d = "";
  peaks.forEach((p, i) => {
    const h = Math.max(0.5, (p / 255) * mid);
    d += `M${i + 0.5} ${mid - h}V${mid + h}`;
  });
  return (
    <svg className={className} viewBox={`0 0 ${w} ${height}`} preserveAspectRatio="none" height={height}>
      <path d={d} stroke={color} strokeWidth="0.7" />
    </svg>
  );
}

export function Seg<T extends string | number>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <div className="seg" role="radiogroup">
      {options.map(([v, label]) => (
        <button key={String(v)} role="radio" aria-checked={v === value} className={v === value ? "on" : ""} onClick={() => onChange(v)}>
          {label}
        </button>
      ))}
    </div>
  );
}
