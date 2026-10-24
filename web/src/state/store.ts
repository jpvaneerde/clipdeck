import { useSyncExternalStore } from "react";
import { emptyProject } from "./defaults";
import type { Project } from "./types";

export interface Selection {
  track: number;
  slot: number | null;
}

export interface AppState {
  project: Project;
  projectId: string | null;
  selection: Selection;
  dirty: boolean;
  savedAt: number | null;
  recordArmed: boolean;
  metronome: boolean;
  browserOpen: boolean;
  octave: number;
}

type Listener = () => void;

const HISTORY_LIMIT = 200;

class Store {
  private state: AppState;
  private listeners = new Set<Listener>();
  private past: Project[] = [];
  private future: Project[] = [];
  private lastGroup: string | null = null;
  private lastGroupAt = 0;

  constructor(initial: AppState) {
    this.state = initial;
  }

  get = () => this.state;

  subscribe = (l: Listener) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  private emit() {
    for (const l of this.listeners) l();
  }

  set(patch: Partial<AppState>) {
    this.state = { ...this.state, ...patch };
    this.emit();
  }

  /**
   * Change the project. Edits with the same `group` within a second collapse
   * into one undo step (e.g. dragging a knob).
   */
  edit(fn: (p: Project) => Project, group?: string) {
    const prev = this.state.project;
    const next = fn(prev);
    if (next === prev) return;
    const now = performance.now();
    const coalesce = group && group === this.lastGroup && now - this.lastGroupAt < 1000;
    if (!coalesce) {
      this.past.push(prev);
      if (this.past.length > HISTORY_LIMIT) this.past.shift();
    }
    this.future = [];
    this.lastGroup = group ?? null;
    this.lastGroupAt = now;
    this.state = { ...this.state, project: next, dirty: true };
    this.emit();
  }

  /** Replace the project without recording history (loading). */
  load(project: Project, projectId: string | null) {
    this.past = [];
    this.future = [];
    const track = Math.min(this.state.selection.track, Math.max(0, project.tracks.length - 1));
    this.state = { ...this.state, project, projectId, dirty: false, selection: { track, slot: null } };
    this.emit();
  }

  undo() {
    const prev = this.past.pop();
    if (!prev) return;
    this.future.push(this.state.project);
    this.lastGroup = null;
    this.state = { ...this.state, project: prev, dirty: true };
    this.emit();
  }

  redo() {
    const next = this.future.pop();
    if (!next) return;
    this.past.push(this.state.project);
    this.lastGroup = null;
    this.state = { ...this.state, project: next, dirty: true };
    this.emit();
  }

  canUndo = () => this.past.length > 0;
  canRedo = () => this.future.length > 0;
}

export const store = new Store({
  project: emptyProject(),
  projectId: null,
  selection: { track: 0, slot: null },
  dirty: false,
  savedAt: null,
  recordArmed: false,
  metronome: false,
  browserOpen: typeof window === "undefined" || window.innerWidth > 860,
  octave: 4,
});

export function useStore<T>(selector: (s: AppState) => T): T {
  return useSyncExternalStore(store.subscribe, () => selector(store.get()));
}

// ------------------------------------------------------------ immutable helpers

export function updateTrack(p: Project, ti: number, fn: (t: Project["tracks"][number]) => Project["tracks"][number]): Project {
  const t = p.tracks[ti];
  if (!t) return p;
  const tracks = p.tracks.slice();
  tracks[ti] = fn(t);
  return { ...p, tracks };
}
