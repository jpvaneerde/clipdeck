import { useEffect, useRef, useState, useSyncExternalStore } from "react";
import { engine } from "../audio/host";

export function useTransport() {
  return useSyncExternalStore(
    (l) => engine.onTransport(l),
    () => engine.transport,
  );
}

export function useEngineStatus() {
  const subscribe = (l: () => void) => engine.onStatus(l);
  const status = useSyncExternalStore(subscribe, () => engine.status);
  const running = useSyncExternalStore(subscribe, () => engine.running);
  return { status, running, error: engine.error };
}

/** Run `fn` every animation frame while mounted. */
export function useFrame(fn: () => void) {
  const ref = useRef(fn);
  ref.current = fn;
  useEffect(() => {
    let raf = 0;
    const loop = () => {
      ref.current();
      raf = requestAnimationFrame(loop);
    };
    raf = requestAnimationFrame(loop);
    return () => cancelAnimationFrame(raf);
  }, []);
}

export function useDebounced<T>(value: T, ms: number): T {
  const [v, setV] = useState(value);
  useEffect(() => {
    const t = setTimeout(() => setV(value), ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  return v;
}

// ------------------------------------------------------------ toasts

export interface Toast {
  id: number;
  text: string;
  error?: boolean;
}

let toasts: Toast[] = [];
const toastListeners = new Set<() => void>();
let nextToast = 1;

export function toast(text: string, error = false) {
  const t = { id: nextToast++, text, error };
  toasts = [...toasts, t];
  toastListeners.forEach((l) => l());
  setTimeout(() => {
    toasts = toasts.filter((x) => x.id !== t.id);
    toastListeners.forEach((l) => l());
  }, error ? 6000 : 3000);
}

export function useToasts() {
  return useSyncExternalStore(
    (l) => {
      toastListeners.add(l);
      return () => toastListeners.delete(l);
    },
    () => toasts,
  );
}

/** Drag payload for samples dragged from the browser. */
export const SAMPLE_MIME = "application/x-clipdeck-sample";
