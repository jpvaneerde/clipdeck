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
  /** only samples I uploaded */
  mine?: boolean;
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

export interface Me {
  id: number;
  name: string;
  email: string;
  role: "admin" | "user";
}

export interface UserInfo {
  id: number;
  name: string;
  email: string;
  role: "admin" | "user" | "none";
  blocked: boolean;
  created: number;
  lastLogin: number;
  projects: number;
  uploads: number;
}

export interface ProjectInfo {
  id: string;
  name: string;
  updated: number;
  ownerId: number | null;
  ownerName: string | null;
}

let redirecting = false;

/** Send the browser to sign-in, then back to the current page. */
export function signIn() {
  if (redirecting) return;
  redirecting = true;
  window.location.href = `/auth/login?return=${encodeURIComponent(location.pathname + location.search)}`;
}

/** fetch() that sends the user to sign-in when the session has ended. */
async function af(input: string, init?: RequestInit): Promise<Response> {
  const res = await fetch(input, { credentials: "same-origin", ...init });
  if (res.status === 401) {
    signIn();
    throw new Error("Your session ended. Signing you in again…");
  }
  return res;
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
  me: () => af("/api/me").then((r) => json<Me>(r)),
  users: () => af("/api/admin/users").then((r) => json<UserInfo[]>(r)),
  setBlocked: async (id: number, blocked: boolean) => {
    const r = await af(`/api/admin/users/${id}`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify({ blocked }) });
    if (!r.ok) throw new Error((await r.json().catch(() => null))?.error ?? `Change failed (${r.status})`);
  },
  status: () => af("/api/status").then((r) => json<{ samples: number; indexing: boolean; indexProgress: number; indexTotal: number }>(r)),
  search: (q: SampleQuery, signal?: AbortSignal) => af(`/api/samples?${qs(q)}`, { signal }).then((r) => json<{ total: number; items: SampleInfo[] }>(r)),
  facets: (q: SampleQuery, signal?: AbortSignal) => af(`/api/facets?${qs(q)}`, { signal }).then((r) => json<Facets>(r)),
  sample: (id: string) => af(`/api/sample?${qs({ id })}`).then((r) => json<SampleInfo>(r)),
  audioUrl: (id: string) => `/api/audio/${id.split("/").map(encodeURIComponent).join("/")}`,
  favorite: (id: string, favorite: boolean) =>
    af("/api/favorite", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ id, favorite }) }),
  patchSample: (id: string, patch: Partial<SampleInfo>) =>
    af(`/api/sample?${qs({ id })}`, { method: "PATCH", headers: { "content-type": "application/json" }, body: JSON.stringify(patch) }).then((r) => json<SampleInfo>(r)),
  deleteSample: (id: string) => af(`/api/sample?${qs({ id })}`, { method: "DELETE" }),
  upload: (files: File[]) => {
    const fd = new FormData();
    for (const f of files) fd.append("file", f, f.name);
    return af("/api/upload", { method: "POST", body: fd }).then((r) => json<{ items: SampleInfo[]; errors: string[] }>(r));
  },
  rescan: () => af("/api/rescan", { method: "POST" }).then((r) => json<{ added: number; errors: string[] }>(r)),
  projects: () => af("/api/projects").then((r) => json<ProjectInfo[]>(r)),
  project: (id: string) => af(`/api/projects/${id}`).then((r) => json<Project>(r)),
  createProject: (p: Project) =>
    af("/api/projects", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(p) }).then((r) => json<{ id: string }>(r)),
  saveProject: async (id: string, p: Project) => {
    const r = await af(`/api/projects/${id}`, { method: "PUT", headers: { "content-type": "application/json" }, body: JSON.stringify(p) });
    if (!r.ok) throw new Error(`Saving failed (${r.status})`);
  },
  deleteProject: (id: string) => af(`/api/projects/${id}`, { method: "DELETE" }),
  render: async (p: Project, opts: { bits: number; sampleRate: number; tail: number }) => {
    const r = await af(`/api/render?${qs(opts)}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(p) });
    if (!r.ok) throw new Error((await r.json().catch(() => null))?.error ?? `Export failed (${r.status})`);
    return r.blob();
  },
};
