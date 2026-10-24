//! SQLite storage: sample index (with FTS5 search), favorites, projects.

use crate::audio;
use anyhow::Result;
use rusqlite::{params, params_from_iter, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub struct Db {
    conn: Mutex<Connection>,
    pub library_dir: PathBuf,
    pub user_dir: PathBuf,
    pub indexing: AtomicBool,
    pub index_progress: AtomicUsize,
    pub index_total: AtomicUsize,
}

pub type Shared = Arc<Db>;

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS samples (
    id TEXT PRIMARY KEY,
    root TEXT NOT NULL,
    path TEXT NOT NULL,
    name TEXT NOT NULL,
    category TEXT NOT NULL,
    instrument TEXT NOT NULL,
    genre TEXT,
    bpm REAL,
    key TEXT,
    bars INTEGER,
    beats REAL,
    kit TEXT,
    tags TEXT NOT NULL DEFAULT '',
    duration REAL NOT NULL,
    channels INTEGER NOT NULL,
    sample_rate INTEGER NOT NULL,
    source TEXT NOT NULL,
    peaks BLOB,
    favorite INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX IF NOT EXISTS samples_category ON samples(category);
CREATE INDEX IF NOT EXISTS samples_genre ON samples(genre);
CREATE INDEX IF NOT EXISTS samples_key ON samples(key);
CREATE INDEX IF NOT EXISTS samples_bpm ON samples(bpm);
CREATE INDEX IF NOT EXISTS samples_kit ON samples(kit);
CREATE VIRTUAL TABLE IF NOT EXISTS samples_fts USING fts5(
    id UNINDEXED, name, tags, category, instrument, genre, kit,
    tokenize = "unicode61 tokenchars '-#'"
);
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    data TEXT NOT NULL,
    created INTEGER NOT NULL DEFAULT (unixepoch()),
    updated INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
"#;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct SampleRow {
    pub id: String,
    pub name: String,
    pub category: String,
    pub instrument: String,
    pub genre: Option<String>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub bars: Option<u32>,
    pub beats: Option<f64>,
    pub kit: Option<String>,
    pub tags: Vec<String>,
    pub duration: f32,
    pub channels: u32,
    pub source: String,
    pub favorite: bool,
    pub peaks: Vec<u8>,
}

/// One line of the generator's manifest.jsonl.
#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    id: String,
    path: String,
    name: String,
    category: String,
    instrument: String,
    genre: Option<String>,
    bpm: Option<f32>,
    key: Option<String>,
    bars: Option<u32>,
    beats: Option<f64>,
    kit: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    duration: f32,
    channels: u32,
    sample_rate: u32,
    #[serde(default)]
    source: Option<String>,
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Query {
    pub q: Option<String>,
    pub category: Option<String>,
    pub instrument: Option<String>,
    pub genre: Option<String>,
    pub key: Option<String>,
    /// Include the relative major/minor of `key`.
    pub compatible: Option<bool>,
    pub bpm: Option<f32>,
    pub bpm_tolerance: Option<f32>,
    pub kit: Option<String>,
    pub favorites: Option<bool>,
    pub source: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

#[derive(Serialize)]
pub struct Page {
    pub total: u64,
    pub items: Vec<SampleRow>,
}

#[derive(Serialize)]
pub struct Facet {
    pub value: String,
    pub count: u64,
}

#[derive(Serialize)]
pub struct Facets {
    pub categories: Vec<Facet>,
    pub genres: Vec<Facet>,
    pub instruments: Vec<Facet>,
    pub keys: Vec<Facet>,
    pub kits: Vec<Facet>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    pub id: String,
    pub name: String,
    pub updated: i64,
}

pub struct NewSample {
    pub id: String,
    pub root: &'static str,
    pub path: String,
    pub name: String,
    pub category: String,
    pub instrument: String,
    pub genre: Option<String>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub bars: Option<u32>,
    pub beats: Option<f64>,
    pub kit: Option<String>,
    pub tags: Vec<String>,
    pub duration: f32,
    pub channels: u32,
    pub sample_rate: u32,
    pub source: String,
    pub peaks: Vec<u8>,
}

/// Turn free text into an FTS5 prefix query: `dark pad` → `"dark"* "pad"*`.
fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| t.chars().filter(|c| c.is_alphanumeric() || *c == '-' || *c == '#').collect::<String>())
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"*", t.to_lowercase()))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" "))
    }
}

impl Db {
    pub fn open(path: &Path, library_dir: PathBuf, user_dir: PathBuf) -> Result<Shared> {
        let conn = Connection::open(path)?;
        conn.execute_batch(SCHEMA)?;
        Ok(Arc::new(Db {
            conn: Mutex::new(conn),
            library_dir,
            user_dir,
            indexing: AtomicBool::new(false),
            index_progress: AtomicUsize::new(0),
            index_total: AtomicUsize::new(0),
        }))
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn meta(&self, key: &str) -> Option<String> {
        self.conn().query_row("SELECT value FROM meta WHERE key = ?", [key], |r| r.get(0)).optional().ok().flatten()
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.conn()
            .execute("INSERT INTO meta(key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value", [key, value])?;
        Ok(())
    }

    pub fn insert(&self, s: &NewSample) -> Result<()> {
        let mut c = self.conn();
        let tx = c.transaction()?;
        Self::insert_tx(&tx, s)?;
        tx.commit()?;
        Ok(())
    }

    fn insert_tx(tx: &rusqlite::Transaction, s: &NewSample) -> Result<()> {
        let tags = s.tags.join(" ");
        // Keep a user's favorite flag across re-indexing.
        let fav: i64 = tx.query_row("SELECT favorite FROM samples WHERE id = ?", [&s.id], |r| r.get(0)).optional()?.unwrap_or(0);
        tx.execute("DELETE FROM samples WHERE id = ?", [&s.id])?;
        tx.execute("DELETE FROM samples_fts WHERE id = ?", [&s.id])?;
        tx.execute(
            "INSERT INTO samples (id, root, path, name, category, instrument, genre, bpm, key, bars, beats, kit, tags, duration, channels, sample_rate, source, peaks, favorite)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                s.id, s.root, s.path, s.name, s.category, s.instrument, s.genre, s.bpm, s.key, s.bars, s.beats, s.kit, tags,
                s.duration, s.channels, s.sample_rate, s.source, s.peaks, fav
            ],
        )?;
        tx.execute(
            "INSERT INTO samples_fts (id, name, tags, category, instrument, genre, kit) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![s.id, s.name, tags, s.category, s.instrument, s.genre.clone().unwrap_or_default(), s.kit.clone().unwrap_or_default()],
        )?;
        Ok(())
    }

    /// Index the generated library from its manifest if it changed since last time.
    pub fn index_library(&self, force: bool) -> Result<usize> {
        let manifest = self.library_dir.join("manifest.jsonl");
        let Ok(meta) = std::fs::metadata(&manifest) else {
            tracing::warn!("no library manifest at {}", manifest.display());
            return Ok(0);
        };
        let stamp = format!("{}:{:?}", meta.len(), meta.modified().ok());
        if !force && self.meta("library_manifest").as_deref() == Some(stamp.as_str()) {
            return Ok(0);
        }
        let text = std::fs::read_to_string(&manifest)?;
        let entries: Vec<ManifestEntry> =
            text.lines().filter(|l| !l.trim().is_empty()).filter_map(|l| serde_json::from_str(l).ok()).collect();
        self.indexing.store(true, Ordering::SeqCst);
        self.index_total.store(entries.len(), Ordering::SeqCst);
        self.index_progress.store(0, Ordering::SeqCst);
        let ids: std::collections::HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();

        let mut count = 0;
        for chunk in entries.chunks(100) {
            // Decode outside the lock for peaks.
            let rows: Vec<NewSample> = chunk
                .iter()
                .map(|e| {
                    let peaks = audio::decode(&self.library_dir.join(&e.path)).map(|d| audio::peaks(&d, 96)).unwrap_or_default();
                    NewSample {
                        id: e.id.clone(),
                        root: "library",
                        path: e.path.clone(),
                        name: e.name.clone(),
                        category: e.category.clone(),
                        instrument: e.instrument.clone(),
                        genre: e.genre.clone(),
                        bpm: e.bpm,
                        key: e.key.clone(),
                        bars: e.bars,
                        beats: e.beats,
                        kit: e.kit.clone(),
                        tags: e.tags.clone(),
                        duration: e.duration,
                        channels: e.channels,
                        sample_rate: e.sample_rate,
                        source: e.source.clone().unwrap_or_else(|| "generated".into()),
                        peaks,
                    }
                })
                .collect();
            let mut c = self.conn();
            let tx = c.transaction()?;
            for r in &rows {
                Self::insert_tx(&tx, r)?;
            }
            tx.commit()?;
            count += rows.len();
            self.index_progress.store(count, Ordering::SeqCst);
        }
        // Drop library rows that disappeared from the manifest.
        {
            let c = self.conn();
            let existing: Vec<String> =
                c.prepare("SELECT id FROM samples WHERE root = 'library'")?.query_map([], |r| r.get(0))?.filter_map(|r| r.ok()).collect();
            for id in existing.into_iter().filter(|id| !ids.contains(id)) {
                c.execute("DELETE FROM samples WHERE id = ?", [&id])?;
                c.execute("DELETE FROM samples_fts WHERE id = ?", [&id])?;
            }
        }
        self.set_meta("library_manifest", &stamp)?;
        self.indexing.store(false, Ordering::SeqCst);
        Ok(count)
    }

    fn row(r: &rusqlite::Row) -> rusqlite::Result<SampleRow> {
        let tags: String = r.get("tags")?;
        Ok(SampleRow {
            id: r.get("id")?,
            name: r.get("name")?,
            category: r.get("category")?,
            instrument: r.get("instrument")?,
            genre: r.get("genre")?,
            bpm: r.get("bpm")?,
            key: r.get("key")?,
            bars: r.get("bars")?,
            beats: r.get("beats")?,
            kit: r.get("kit")?,
            tags: tags.split_whitespace().map(String::from).collect(),
            duration: r.get("duration")?,
            channels: r.get("channels")?,
            source: r.get("source")?,
            favorite: r.get::<_, i64>("favorite")? != 0,
            peaks: r.get::<_, Option<Vec<u8>>>("peaks")?.unwrap_or_default(),
        })
    }

    fn filters(q: &Query) -> (String, Vec<rusqlite::types::Value>) {
        let fts = Query { q: q.q.as_deref().and_then(fts_query), ..Self::copy(q) };
        Self::filters_raw(&fts)
    }

    fn copy(q: &Query) -> Query {
        Query {
            q: q.q.clone(),
            category: q.category.clone(),
            instrument: q.instrument.clone(),
            genre: q.genre.clone(),
            key: q.key.clone(),
            compatible: q.compatible,
            bpm: q.bpm,
            bpm_tolerance: q.bpm_tolerance,
            kit: q.kit.clone(),
            favorites: q.favorites,
            source: q.source.clone(),
            sort: q.sort.clone(),
            limit: q.limit,
            offset: q.offset,
        }
    }

    /// Like `filters`, but `q.q` is already an FTS5 expression.
    fn filters_raw(q: &Query) -> (String, Vec<rusqlite::types::Value>) {
        use rusqlite::types::Value;
        let mut wh = vec!["1=1".to_string()];
        let mut args: Vec<Value> = Vec::new();
        if let Some(fq) = q.q.as_deref().filter(|s| !s.is_empty()) {
            wh.push("id IN (SELECT id FROM samples_fts WHERE samples_fts MATCH ?)".into());
            args.push(Value::Text(fq.to_string()));
        }
        for (col, val) in
            [("category", &q.category), ("instrument", &q.instrument), ("genre", &q.genre), ("kit", &q.kit), ("source", &q.source)]
        {
            if let Some(v) = val.as_deref().filter(|v| !v.is_empty()) {
                let vals: Vec<&str> = v.split(',').filter(|s| !s.is_empty()).collect();
                wh.push(format!("{col} IN ({})", vec!["?"; vals.len()].join(",")));
                args.extend(vals.into_iter().map(|s| Value::Text(s.to_string())));
            }
        }
        if let Some(k) = q.key.as_deref().filter(|k| !k.is_empty()) {
            let keys = if q.compatible.unwrap_or(false) { audio::compatible_keys(k) } else { vec![k.to_string()] };
            wh.push(format!("key IN ({})", vec!["?"; keys.len()].join(",")));
            args.extend(keys.into_iter().map(Value::Text));
        }
        if let Some(bpm) = q.bpm {
            let tol = q.bpm_tolerance.unwrap_or(4.0) as f64;
            wh.push("bpm BETWEEN ? AND ?".into());
            args.push(Value::Real(bpm as f64 - tol));
            args.push(Value::Real(bpm as f64 + tol));
        }
        if q.favorites.unwrap_or(false) {
            wh.push("favorite = 1".into());
        }
        (wh.join(" AND "), args)
    }

    pub fn search(&self, q: &Query) -> Result<Page> {
        let (wh, args) = Self::filters(q);
        let order = match q.sort.as_deref() {
            Some("bpm") => "bpm IS NULL, bpm, name",
            Some("newest") => "created DESC, name",
            Some("random") => "random()",
            Some("duration") => "duration, name",
            _ => "category, name",
        };
        let limit = q.limit.unwrap_or(60).min(500);
        let offset = q.offset.unwrap_or(0);
        let c = self.conn();
        let mut total: u64 =
            c.query_row(&format!("SELECT COUNT(*) FROM samples WHERE {wh}"), params_from_iter(args.iter()), |r| r.get(0))?;
        let (mut wh, mut args) = (wh, args);
        // Every word must match by default; if that finds nothing, accept any word.
        if total == 0 && q.q.as_deref().map_or(0, |t| t.split_whitespace().count()) > 1 {
            let mut loose = Query { q: q.q.as_deref().and_then(fts_query).map(|f| f.replace("* \"", "* OR \"")), ..Default::default() };
            loose.category = q.category.clone();
            loose.instrument = q.instrument.clone();
            loose.genre = q.genre.clone();
            loose.key = q.key.clone();
            loose.compatible = q.compatible;
            loose.bpm = q.bpm;
            loose.bpm_tolerance = q.bpm_tolerance;
            loose.kit = q.kit.clone();
            loose.favorites = q.favorites;
            loose.source = q.source.clone();
            let (w2, a2) = Self::filters_raw(&loose);
            total = c.query_row(&format!("SELECT COUNT(*) FROM samples WHERE {w2}"), params_from_iter(a2.iter()), |r| r.get(0))?;
            wh = w2;
            args = a2;
        }
        let sql = format!("SELECT * FROM samples WHERE {wh} ORDER BY {order} LIMIT {limit} OFFSET {offset}");
        let items = c.prepare(&sql)?.query_map(params_from_iter(args.iter()), Self::row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(Page { total, items })
    }

    pub fn get(&self, id: &str) -> Result<Option<SampleRow>> {
        Ok(self.conn().query_row("SELECT * FROM samples WHERE id = ?", [id], Self::row).optional()?)
    }

    /// Absolute path of a sample's audio file.
    pub fn file_path(&self, id: &str) -> Result<Option<PathBuf>> {
        let r: Option<(String, String)> =
            self.conn().query_row("SELECT root, path FROM samples WHERE id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        Ok(r.map(|(root, path)| if root == "user" { self.user_dir.join(path) } else { self.library_dir.join(path) }))
    }

    pub fn facets(&self, q: &Query) -> Result<Facets> {
        let c = self.conn();
        let group = |col: &str, scoped: &Query| -> Result<Vec<Facet>> {
            let (wh, args) = Self::filters(scoped);
            let sql = format!(
                "SELECT {col}, COUNT(*) FROM samples WHERE {wh} AND {col} IS NOT NULL GROUP BY {col} ORDER BY COUNT(*) DESC, {col}"
            );
            let rows = c
                .prepare(&sql)?
                .query_map(params_from_iter(args.iter()), |r| Ok(Facet { value: r.get(0)?, count: r.get(1)? }))?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        };
        // Each facet is counted with every other active filter, but not its own,
        // so the list keeps showing alternatives.
        let without = |f: &dyn Fn(&mut Query)| {
            let mut s = Query {
                q: q.q.clone(),
                category: q.category.clone(),
                instrument: q.instrument.clone(),
                genre: q.genre.clone(),
                key: q.key.clone(),
                compatible: q.compatible,
                bpm: q.bpm,
                bpm_tolerance: q.bpm_tolerance,
                kit: q.kit.clone(),
                favorites: q.favorites,
                source: q.source.clone(),
                ..Default::default()
            };
            f(&mut s);
            s
        };
        Ok(Facets {
            categories: group(
                "category",
                &without(&|s| {
                    s.category = None;
                    s.instrument = None;
                }),
            )?,
            genres: group("genre", &without(&|s| s.genre = None))?,
            instruments: group("instrument", &without(&|s| s.instrument = None))?,
            keys: group("key", &without(&|s| s.key = None))?,
            kits: group("kit", &without(&|s| s.kit = None))?,
        })
    }

    pub fn set_favorite(&self, id: &str, fav: bool) -> Result<bool> {
        Ok(self.conn().execute("UPDATE samples SET favorite = ? WHERE id = ?", params![fav as i64, id])? > 0)
    }

    pub fn update_sample(&self, id: &str, patch: &SamplePatch) -> Result<Option<SampleRow>> {
        let Some(mut row) = self.get(id)? else { return Ok(None) };
        if let Some(v) = &patch.name {
            row.name = v.clone();
        }
        if let Some(v) = &patch.category {
            row.category = v.clone();
        }
        if let Some(v) = &patch.instrument {
            row.instrument = v.clone();
        }
        if let Some(v) = patch.bpm {
            row.bpm = if v > 0.0 { Some(v) } else { None };
        }
        if let Some(v) = &patch.key {
            row.key = if v.is_empty() { None } else { Some(v.clone()) };
        }
        if let Some(v) = patch.bars {
            row.bars = if v > 0 { Some(v) } else { None };
            row.beats = row.bars.map(|b| b as f64 * 4.0);
        }
        if let Some(v) = &patch.tags {
            row.tags = v.clone();
        }
        let tags = row.tags.join(" ");
        let c = self.conn();
        c.execute(
            "UPDATE samples SET name=?, category=?, instrument=?, bpm=?, key=?, bars=?, beats=?, tags=? WHERE id=?",
            params![row.name, row.category, row.instrument, row.bpm, row.key, row.bars, row.beats, tags, id],
        )?;
        c.execute(
            "UPDATE samples_fts SET name=?, tags=?, category=?, instrument=? WHERE id=?",
            params![row.name, tags, row.category, row.instrument, id],
        )?;
        Ok(Some(row))
    }

    pub fn delete_sample(&self, id: &str) -> Result<Option<PathBuf>> {
        let path = self.file_path(id)?;
        let root: Option<String> = self.conn().query_row("SELECT root FROM samples WHERE id = ?", [id], |r| r.get(0)).optional()?;
        if root.as_deref() != Some("user") {
            return Ok(None);
        }
        let c = self.conn();
        c.execute("DELETE FROM samples WHERE id = ?", [id])?;
        c.execute("DELETE FROM samples_fts WHERE id = ?", [id])?;
        Ok(path)
    }

    pub fn count(&self) -> u64 {
        self.conn().query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0)).unwrap_or(0)
    }

    // -------------------------------------------------------- projects

    pub fn list_projects(&self) -> Result<Vec<ProjectSummary>> {
        let c = self.conn();
        let rows = c
            .prepare("SELECT id, name, updated FROM projects ORDER BY updated DESC")?
            .query_map([], |r| Ok(ProjectSummary { id: r.get(0)?, name: r.get(1)?, updated: r.get(2)? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn get_project(&self, id: &str) -> Result<Option<String>> {
        Ok(self.conn().query_row("SELECT data FROM projects WHERE id = ?", [id], |r| r.get(0)).optional()?)
    }

    pub fn put_project(&self, id: &str, name: &str, data: &str) -> Result<()> {
        self.conn().execute(
            "INSERT INTO projects (id, name, data) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, data = excluded.data, updated = unixepoch()",
            params![id, name, data],
        )?;
        Ok(())
    }

    pub fn delete_project(&self, id: &str) -> Result<bool> {
        Ok(self.conn().execute("DELETE FROM projects WHERE id = ?", [id])? > 0)
    }
}

#[derive(Deserialize, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct SamplePatch {
    pub name: Option<String>,
    pub category: Option<String>,
    pub instrument: Option<String>,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub bars: Option<u32>,
    pub tags: Option<Vec<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_is_sanitized() {
        assert_eq!(fts_query("dark  pad\""), Some("\"dark\"* \"pad\"*".to_string()));
        assert_eq!(fts_query("  "), None);
    }

    #[test]
    fn insert_search_and_projects() {
        let dir = std::env::temp_dir().join(format!("clipdeck-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&dir.join("t.db"), dir.clone(), dir.clone()).unwrap();
        for (i, (cat, key, bpm)) in [("bass", "Am", 124.0), ("chords", "C", 124.0), ("drums", "", 0.0)].iter().enumerate() {
            db.insert(&NewSample {
                id: format!("s{i}"),
                root: "library",
                path: format!("s{i}.wav"),
                name: format!("Deep {cat} {i}"),
                category: cat.to_string(),
                instrument: "x".into(),
                genre: Some("house".into()),
                bpm: if *bpm > 0.0 { Some(*bpm) } else { None },
                key: if key.is_empty() { None } else { Some(key.to_string()) },
                bars: None,
                beats: None,
                kit: Some("house-kit01".into()),
                tags: vec!["loop".into()],
                duration: 1.0,
                channels: 2,
                sample_rate: 44100,
                source: "generated".into(),
                peaks: vec![1, 2, 3],
            })
            .unwrap();
        }
        let p = db.search(&Query { q: Some("deep".into()), ..Default::default() }).unwrap();
        assert_eq!(p.total, 3);
        let p = db.search(&Query { key: Some("Am".into()), compatible: Some(true), ..Default::default() }).unwrap();
        assert_eq!(p.total, 2);
        let p = db.search(&Query { bpm: Some(126.0), category: Some("bass,chords".into()), ..Default::default() }).unwrap();
        assert_eq!(p.total, 2);
        db.set_favorite("s2", true).unwrap();
        assert_eq!(db.search(&Query { favorites: Some(true), ..Default::default() }).unwrap().total, 1);
        let f = db.facets(&Query { category: Some("bass".into()), ..Default::default() }).unwrap();
        assert_eq!(f.categories.len(), 3);
        db.put_project("p1", "Song", "{}").unwrap();
        db.put_project("p1", "Song 2", "{\"a\":1}").unwrap();
        assert_eq!(db.list_projects().unwrap()[0].name, "Song 2");
        assert_eq!(db.get_project("p1").unwrap().as_deref(), Some("{\"a\":1}"));
        std::fs::remove_dir_all(dir).ok();
    }
}
