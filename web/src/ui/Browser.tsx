import { useEffect, useMemo, useRef, useState } from "react";
import { engine } from "../audio/host";
import { quickAdd } from "../state/actions";
import { api, type Facets, type SampleQuery } from "../state/api";
import { store, useStore } from "../state/store";
import type { SampleInfo } from "../state/types";
import { Icon, Waveform } from "./controls";
import { SAMPLE_MIME, toast, useDebounced, useFrame } from "./hooks";

const CATEGORY_LABEL: Record<string, string> = {
  drums: "One-shots",
  "drum-loop": "Drum loops",
  bass: "Bass",
  chords: "Chords",
  hook: "Hooks",
  arp: "Arps",
  vocal: "Vocals",
  fx: "FX",
  "synth-hit": "Synth hits",
};
const CATEGORY_ORDER = ["drum-loop", "drums", "bass", "chords", "hook", "arp", "vocal", "synth-hit", "fx"];

const GENRE_LABEL = (g: string) => g.replace(/-/g, " ").replace(/\b\w/g, (m) => m.toUpperCase()).replace("And", "&").replace("Uk", "UK");

const PAGE = 60;

export function Browser() {
  const bpm = useStore((s) => s.project.bpm);
  const [text, setText] = useState("");
  const q = useDebounced(text, 200);
  const [category, setCategory] = useState("");
  const [genre, setGenre] = useState("");
  const [key, setKey] = useState("");
  const [compatible, setCompatible] = useState(true);
  const [matchTempo, setMatchTempo] = useState(false);
  const [favorites, setFavorites] = useState(false);
  const [source, setSource] = useState("");
  const [kit, setKit] = useState("");
  const [items, setItems] = useState<SampleInfo[]>([]);
  const [total, setTotal] = useState(0);
  const [facets, setFacets] = useState<Facets | null>(null);
  const [loading, setLoading] = useState(false);
  const [previewing, setPreviewing] = useState<string | null>(null);
  const [over, setOver] = useState(false);
  const [indexing, setIndexing] = useState<string | null>(null);
  const [reload, setReload] = useState(0);
  const listRef = useRef<HTMLDivElement>(null);

  const query: SampleQuery = useMemo(
    () => ({
      q: q || undefined,
      category: category || undefined,
      genre: genre || undefined,
      key: key || undefined,
      compatible: key ? compatible : undefined,
      bpm: matchTempo ? bpm : undefined,
      bpmTolerance: matchTempo ? 4 : undefined,
      favorites: favorites || undefined,
      source: source || undefined,
      kit: kit || undefined,
    }),
    [q, category, genre, key, compatible, matchTempo, bpm, favorites, source, kit],
  );

  useEffect(() => {
    const ac = new AbortController();
    setLoading(true);
    Promise.all([api.search({ ...query, limit: PAGE }, ac.signal), api.facets(query, ac.signal)])
      .then(([page, f]) => {
        setItems(page.items);
        setTotal(page.total);
        setFacets(f);
        listRef.current?.scrollTo({ top: 0 });
      })
      .catch((e) => {
        if (e.name !== "AbortError") toast(`Library unavailable: ${e.message}`, true);
      })
      .finally(() => setLoading(false));
    return () => ac.abort();
  }, [query, reload]);

  // While the server is still indexing a fresh library, show progress and refresh.
  useEffect(() => {
    let stop = false;
    const poll = async () => {
      try {
        const s = await api.status();
        if (stop) return;
        if (s.indexing) {
          setIndexing(`Indexing library… ${s.indexProgress}/${s.indexTotal}`);
          setTimeout(poll, 1500);
        } else {
          setIndexing((was) => {
            if (was) setReload((r) => r + 1);
            return s.samples === 0 ? "The sample library is empty. It is generated on first start; check back in a few minutes." : null;
          });
          if (s.samples === 0) setTimeout(poll, 5000);
        }
      } catch {
        /* server unreachable; search shows the error */
      }
    };
    poll();
    return () => {
      stop = true;
    };
  }, []);

  useFrame(() => {
    if (previewing && engine.state[5] < 0.5) setPreviewing(null);
  });

  const loadMore = () => {
    if (loading || items.length >= total) return;
    setLoading(true);
    api
      .search({ ...query, limit: PAGE, offset: items.length })
      .then((page) => setItems((cur) => [...cur, ...page.items]))
      .finally(() => setLoading(false));
  };

  const preview = (s: SampleInfo) => {
    if (previewing === s.id) {
      engine.stopPreview();
      setPreviewing(null);
      return;
    }
    setPreviewing(s.id);
    engine.preview(s.id);
  };

  const toggleFav = async (s: SampleInfo) => {
    await api.favorite(s.id, !s.favorite);
    setItems((cur) => cur.map((x) => (x.id === s.id ? { ...x, favorite: !s.favorite } : x)));
  };

  const onDropFiles = async (files: File[]) => {
    if (!files.length) return;
    toast(`Uploading ${files.length} file${files.length > 1 ? "s" : ""}…`);
    try {
      const r = await api.upload(files);
      if (r.items.length) toast(`Added ${r.items.length} sample${r.items.length > 1 ? "s" : ""} to My samples`);
      for (const e of r.errors) toast(e, true);
      setSource("user");
      setReload((x) => x + 1);
    } catch (e) {
      toast(`Upload failed: ${(e as Error).message}`, true);
    }
  };

  const cats = useMemo(() => {
    const counts = new Map(facets?.categories.map((f) => [f.value, f.count]) ?? []);
    return CATEGORY_ORDER.filter((c) => counts.has(c)).map((c) => ({ value: c, count: counts.get(c)! }));
  }, [facets]);

  return (
    <aside className="browser" aria-label="Sample browser">
      <div className="browser-head">
        <button className="btn show-narrow" onClick={() => store.set({ browserOpen: false })} style={{ justifySelf: "end" }}>
          {Icon.close} Close
        </button>
        <div className="search">
          {Icon.search}
          <input id="sample-search" placeholder="Search samples (dark pad, 808, vox…)" value={text} onChange={(e) => setText(e.target.value)} />
          {text && (
            <button className="btn ghost icon" aria-label="Clear search" onClick={() => setText("")} style={{ height: 22, width: 22 }}>
              {Icon.close}
            </button>
          )}
        </div>
        <div className="chips">
          <button className={`chip ${category === "" ? "on" : ""}`} onClick={() => setCategory("")}>
            All
          </button>
          {cats.map((c) => (
            <button key={c.value} className={`chip ${category === c.value ? "on" : ""}`} onClick={() => setCategory(category === c.value ? "" : c.value)}>
              {CATEGORY_LABEL[c.value] ?? c.value} <span className="n">{c.count}</span>
            </button>
          ))}
        </div>
        <div className="filters">
          <select id="f-genre" aria-label="Genre" value={genre} onChange={(e) => { setGenre(e.target.value); setKit(""); }}>
            <option value="">All genres</option>
            {facets?.genres.map((g) => (
              <option key={g.value} value={g.value}>
                {GENRE_LABEL(g.value)} ({g.count})
              </option>
            ))}
          </select>
          <select id="f-key" aria-label="Key" value={key} onChange={(e) => setKey(e.target.value)}>
            <option value="">Any key</option>
            {facets?.keys
              .slice()
              .sort((a, b) => a.value.localeCompare(b.value))
              .map((k) => (
                <option key={k.value} value={k.value}>
                  {k.value} ({k.count})
                </option>
              ))}
          </select>
          {genre && (
            <select id="f-kit" aria-label="Kit" value={kit} onChange={(e) => setKit(e.target.value)} style={{ gridColumn: "1 / 3" }}>
              <option value="">All kits</option>
              {facets?.kits.map((k) => (
                <option key={k.value} value={k.value}>
                  {k.value.replace(/^.*-kit/, "Kit ")} ({k.count})
                </option>
              ))}
            </select>
          )}
          <label className="check">
            <input id="f-tempo" type="checkbox" checked={matchTempo} onChange={(e) => setMatchTempo(e.target.checked)} /> Near {Math.round(bpm)} BPM
          </label>
          <label className="check" title="Also show the relative major or minor">
            <input id="f-compat" type="checkbox" checked={compatible} disabled={!key} onChange={(e) => setCompatible(e.target.checked)} /> Relative key
          </label>
          <label className="check">
            <input id="f-fav" type="checkbox" checked={favorites} onChange={(e) => setFavorites(e.target.checked)} /> Favorites
          </label>
          <select id="f-source" aria-label="Source" value={source} onChange={(e) => setSource(e.target.value)}>
            <option value="">Library + mine</option>
            <option value="generated">Library</option>
            <option value="user">My samples</option>
          </select>
        </div>
      </div>
      <div className="results-meta">
        <span className="mono">{indexing ?? `${total.toLocaleString()} samples`}</span>
        <span>Click to hear, drag into the session</span>
      </div>
      <div
        className="results"
        ref={listRef}
        onScroll={(e) => {
          const el = e.currentTarget;
          if (el.scrollTop + el.clientHeight > el.scrollHeight - 200) loadMore();
        }}
      >
        {items.map((s) => (
          <div
            key={s.id}
            className={`result ${previewing === s.id ? "previewing" : ""}`}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.setData(SAMPLE_MIME, JSON.stringify(s));
              e.dataTransfer.effectAllowed = "copy";
              engine.loadSample(s.id);
            }}
            onDoubleClick={() => quickAdd(s)}
            title={`${s.name}\nDouble-click to add to the selected track`}
          >
            <button className="play" aria-label={previewing === s.id ? "Stop preview" : `Preview ${s.name}`} onClick={() => preview(s)}>
              {previewing === s.id ? Icon.stop : Icon.play}
            </button>
            <span className="name">{s.name}</span>
            <button className={`fav ${s.favorite ? "on" : ""}`} aria-label={s.favorite ? "Remove from favorites" : "Add to favorites"} onClick={() => toggleFav(s)}>
              {Icon.star}
            </button>
            <Waveform className="wave" peaks={s.peaks} color={previewing === s.id ? "var(--accent)" : "var(--dim)"} />
            <span className="meta mono">
              {s.bpm ? <span>{Math.round(s.bpm)} bpm</span> : null}
              {s.key ? <span>{s.key}</span> : null}
              {s.bars ? <span>{s.bars} bar{s.bars > 1 ? "s" : ""}</span> : <span>{s.duration.toFixed(2)}s</span>}
              <span>{s.instrument}</span>
            </span>
          </div>
        ))}
        {!loading && items.length === 0 && <p className="empty-note">No samples match. Clear a filter or try another word.</p>}
        <div
          className={`dropzone ${over ? "over" : ""}`}
          onDragOver={(e) => {
            if (e.dataTransfer.types.includes("Files")) {
              e.preventDefault();
              setOver(true);
            }
          }}
          onDragLeave={() => setOver(false)}
          onDrop={(e) => {
            e.preventDefault();
            setOver(false);
            onDropFiles([...e.dataTransfer.files]);
          }}
        >
          Drop WAV, MP3, FLAC or OGG files here to add your own samples.{" "}
          <label style={{ color: "var(--accent)", cursor: "pointer" }}>
            Choose files
            <input id="upload-input" type="file" multiple accept="audio/*" hidden onChange={(e) => onDropFiles([...(e.target.files ?? [])])} />
          </label>
          {" · "}
          <button
            className="btn ghost"
            style={{ height: 20, padding: "0 4px", color: "var(--accent)" }}
            onClick={async () => {
              const r = await api.rescan();
              toast(r.added ? `Found ${r.added} new sample${r.added > 1 ? "s" : ""}` : "No new samples found");
              setReload((x) => x + 1);
            }}
          >
            Rescan folder
          </button>
        </div>
      </div>
    </aside>
  );
}
