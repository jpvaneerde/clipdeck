import type { Project, SampleInfo } from "./types";

export interface SampleQuery {
  q?: string;
  category?: string;
  instrument?: string;
  genre?: string;
  key?: string;
  compatible?: boolean;
  bpm?: number;
  bpmTolerance?: number;
  kit?: string;
  favorites?: boolean;
  source?: string;
  sort?: string;
  limit?: number;
  offset?: number;
}

export interface Facet {
  value: string;
  count: number;
}

export interface Facets {
  categories: Facet[];
  genres: Facet[];
  instruments: Facet[];
  keys: Facet[];
  kits: Facet[];
}

function qs(q: object): string {
  const p = new URLSearchParams();
  for (const [k, v] of Object.entries(q)) {
    if (v === undefined || v === null || v === "" || v === false) continue;
    p.set(k, String(v));
  }
  return p.toString();
}

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    let msg = `${res.status} ${res.statusText}`;
    try {
      const body = await res.json();
      if (body?.error) msg = body.error;
    } catch {
      /* not JSON */
    }
    throw new Error(msg);
  }
  return res.json() as Promise<T>;
}

export const api = {
  status: () => fetch("/api/status").then((r) => json<{ samples: number; indexing: boolean; indexProgress: number; indexTotal: number }>(r)),
  search: (q: SampleQuery, signal?: AbortSignal) => fetch(`/api/samples?${qs(q)}`, { signal }).then((r) => json<{ total: number; items: SampleInfo[] }>(r)),
  facets: (q: SampleQuery, signal?: AbortSignal) => fetch(`/api/facets?${qs(q)}`, { signal }).then((r) => json<Facets>(r)),
  sample: (id: string) => fetch(`/api/sample?${qs({ id })}`).then((r) => json<SampleInfo>(r)),
  audioUrl: (id: string) => `/api/audio/${id.split("/").map(encodeURIComponent).join("/")}`,
  favorite: (id: string, favorite: boolean) =>
    fetch("/api/favorite", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ id, favorite }) }),
  patchSample: (id: string, patch: Partial<SampleInfo>) =>
    fetch(`/api/sample?${qs({ id })}`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) }).then((r) => json<SampleInfo>(r)),
  deleteSample: (id: string) => fetch(`/api/sample?${qs({ id })}`, { method: "DELETE" }),
  upload: (files: File[]) => {
    const fd = new FormData();
    for (const f of files) fd.append("file", f, f.name);
    return fetch("/api/upload", { method: "POST", body: fd }).then((r) => json<{ items: SampleInfo[]; errors: string[] }>(r));
  },
  rescan: () => fetch("/api/rescan", { method: "POST" }).then((r) => json<{ added: number; errors: string[] }>(r)),
  projects: () => fetch("/api/projects").then((r) => json<{ id: string; name: string; updated: number }[]>(r)),
  project: (id: string) => fetch(`/api/projects/${id}`).then((r) => json<Project>(r)),
  createProject: (p: Project) =>
    fetch("/api/projects", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(p) }).then((r) => json<{ id: string }>(r)),
  saveProject: async (id: string, p: Project) => {
    const r = await fetch(`/api/projects/${id}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(p) });
    if (!r.ok) throw new Error(`Saving failed (${r.status})`);
  },
  deleteProject: (id: string) => fetch(`/api/projects/${id}`, { method: "DELETE" }),
  render: async (p: Project, opts: { bits: number; sampleRate: number; tail: number }) => {
    const r = await fetch(`/api/render?${qs(opts)}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(p) });
    if (!r.ok) throw new Error((await r.json().catch(() => null))?.error ?? `Export failed (${r.status})`);
    return r.blob();
  },
};
