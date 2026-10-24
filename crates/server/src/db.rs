//! SQLite storage: sample index (with FTS5 search), users and sessions,
//! per-user favorites, and projects owned by users.

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
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tenant TEXT NOT NULL,
    subject TEXT NOT NULL,          -- Entra object ID (oid)
    name TEXT NOT NULL,
    email TEXT NOT NULL,
    role TEXT NOT NULL,             -- admin | user | none (from the token's roles claim)
    blocked INTEGER NOT NULL DEFAULT 0,
    created INTEGER NOT NULL DEFAULT (unixepoch()),
    last_login INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (tenant, subject)
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created INTEGER NOT NULL DEFAULT (unixepoch()),
    expires INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions(user_id);
CREATE TABLE IF NOT EXISTS auth_pending (
    state TEXT PRIMARY KEY,
    nonce TEXT NOT NULL,
    verifier TEXT NOT NULL,
    return_to TEXT NOT NULL,
    created INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE TABLE IF NOT EXISTS favorites (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    sample_id TEXT NOT NULL,
    PRIMARY KEY (user_id, sample_id)
);
"#;

/// Columns added after the first release; created on open if missing.
const MIGRATIONS: &[(&str, &str, &str)] = &[("samples", "owner_id", "INTEGER"), ("projects", "owner_id", "INTEGER")];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Admin,
    User,
    None,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Admin => "admin",
            Role::User => "user",
            Role::None => "none",
        }
    }
    fn parse(s: &str) -> Role {
        match s {
            "admin" => Role::Admin,
            "user" => Role::User,
            _ => Role::None,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UserRow {
    pub id: i64,
    pub name: String,
    pub email: String,
    pub role: Role,
    pub blocked: bool,
    pub created: i64,
    pub last_login: i64,
}

#[derive(Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UserSummary {
    #[serde(flatten)]
    pub user: UserRow,
    pub projects: i64,
    pub uploads: i64,
}

pub struct Pending {
    pub nonce: String,
    pub verifier: String,
    pub return_to: String,
}

/// Who is asking: used to scope favorites and ownership.
#[derive(Clone, Copy, Debug)]
pub struct Viewer {
    pub user_id: i64,
    pub admin: bool,
}

pub enum Access<T> {
    Ok(T),
    NotFound,
    Forbidden,
}

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
    pub owner_id: Option<i64>,
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

#[derive(Deserialize, Debug, Default, Clone)]
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
    /// Only samples the viewer uploaded.
    pub mine: Option<bool>,
    pub source: Option<String>,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
    /// Set by the server from the session, never from the query string.
    #[serde(skip)]
    pub user_id: i64,
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
    pub owner_id: Option<i64>,
    pub owner_name: Option<String>,
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
    pub owner_id: Option<i64>,
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
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        for (table, col, decl) in MIGRATIONS {
            let has: bool = conn
                .prepare(&format!("PRAGMA table_info({table})"))?
                .query_map([], |r| r.get::<_, String>(1))?
                .filter_map(|r| r.ok())
                .any(|c| c == *col);
            if !has {
                conn.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {col} {decl}"))?;
            }
        }
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
        // Keep the uploader across re-indexing.
        let prev_owner: Option<i64> =
            tx.query_row("SELECT owner_id FROM samples WHERE id = ?", [&s.id], |r| r.get(0)).optional()?.flatten();
        let owner = s.owner_id.or(prev_owner);
        tx.execute("DELETE FROM samples WHERE id = ?", [&s.id])?;
        tx.execute("DELETE FROM samples_fts WHERE id = ?", [&s.id])?;
        tx.execute(
            "INSERT INTO samples (id, root, path, name, category, instrument, genre, bpm, key, bars, beats, kit, tags, duration, channels, sample_rate, source, peaks, owner_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
            params![
                s.id, s.root, s.path, s.name, s.category, s.instrument, s.genre, s.bpm, s.key, s.bars, s.beats, s.kit, tags,
                s.duration, s.channels, s.sample_rate, s.source, s.peaks, owner
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
                        owner_id: None,
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
            favorite: r.get::<_, i64>("fav")? != 0,
            owner_id: r.get("owner_id")?,
            peaks: r.get::<_, Option<Vec<u8>>>("peaks")?.unwrap_or_default(),
        })
    }

    fn filters(q: &Query) -> (String, Vec<rusqlite::types::Value>) {
        let fts = Query { q: q.q.as_deref().and_then(fts_query), ..q.clone() };
        Self::filters_raw(&fts)
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
            wh.push("id IN (SELECT sample_id FROM favorites WHERE user_id = ?)".into());
            args.push(Value::Integer(q.user_id));
        }
        if q.mine.unwrap_or(false) {
            wh.push("owner_id = ?".into());
            args.push(Value::Integer(q.user_id));
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
            let loose = Query { q: q.q.as_deref().and_then(fts_query).map(|f| f.replace("* \"", "* OR \"")), ..q.clone() };
            let (w2, a2) = Self::filters_raw(&loose);
            total = c.query_row(&format!("SELECT COUNT(*) FROM samples WHERE {w2}"), params_from_iter(a2.iter()), |r| r.get(0))?;
            wh = w2;
            args = a2;
        }
        let sql = format!("SELECT {} FROM samples WHERE {wh} ORDER BY {order} LIMIT {limit} OFFSET {offset}", Self::columns(q.user_id));
        let items = c.prepare(&sql)?.query_map(params_from_iter(args.iter()), Self::row)?.collect::<Result<Vec<_>, _>>()?;
        Ok(Page { total, items })
    }

    /// Sample columns plus whether `user_id` marked it as a favorite.
    fn columns(user_id: i64) -> String {
        format!("samples.*, EXISTS(SELECT 1 FROM favorites f WHERE f.sample_id = samples.id AND f.user_id = {user_id}) AS fav")
    }

    pub fn get(&self, id: &str, user_id: i64) -> Result<Option<SampleRow>> {
        let sql = format!("SELECT {} FROM samples WHERE id = ?", Self::columns(user_id));
        Ok(self.conn().query_row(&sql, [id], Self::row).optional()?)
    }

    pub fn exists(&self, id: &str) -> Result<bool> {
        Ok(self.conn().query_row("SELECT 1 FROM samples WHERE id = ?", [id], |_| Ok(())).optional()?.is_some())
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
            let mut s = q.clone();
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

    pub fn set_favorite(&self, user_id: i64, id: &str, fav: bool) -> Result<bool> {
        if !self.exists(id)? {
            return Ok(false);
        }
        let c = self.conn();
        if fav {
            c.execute("INSERT OR IGNORE INTO favorites (user_id, sample_id) VALUES (?, ?)", params![user_id, id])?;
        } else {
            c.execute("DELETE FROM favorites WHERE user_id = ? AND sample_id = ?", params![user_id, id])?;
        }
        Ok(true)
    }

    pub fn update_sample(&self, id: &str, patch: &SamplePatch, user_id: i64) -> Result<Option<SampleRow>> {
        let Some(mut row) = self.get(id, user_id)? else { return Ok(None) };
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

    /// Delete an uploaded sample. Owners may delete their own; admins any upload.
    /// Generated library samples cannot be deleted.
    pub fn delete_sample(&self, id: &str, v: Viewer) -> Result<Access<Option<PathBuf>>> {
        let row: Option<(String, Option<i64>)> =
            self.conn().query_row("SELECT root, owner_id FROM samples WHERE id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        let Some((root, owner)) = row else { return Ok(Access::NotFound) };
        if root != "user" {
            return Ok(Access::Ok(None));
        }
        if !v.admin && owner != Some(v.user_id) {
            return Ok(Access::Forbidden);
        }
        let path = self.file_path(id)?;
        let c = self.conn();
        c.execute("DELETE FROM samples WHERE id = ?", [id])?;
        c.execute("DELETE FROM samples_fts WHERE id = ?", [id])?;
        c.execute("DELETE FROM favorites WHERE sample_id = ?", [id])?;
        Ok(Access::Ok(path))
    }

    pub fn count(&self) -> u64 {
        self.conn().query_row("SELECT COUNT(*) FROM samples", [], |r| r.get(0)).unwrap_or(0)
    }

    // -------------------------------------------------------- projects

    /// A user's own projects; admins see everyone's (including projects from
    /// before sign-in existed, which have no owner).
    pub fn list_projects(&self, v: Viewer) -> Result<Vec<ProjectSummary>> {
        let c = self.conn();
        let sql = "SELECT p.id, p.name, p.updated, p.owner_id, u.name FROM projects p LEFT JOIN users u ON u.id = p.owner_id
                   WHERE ?1 OR p.owner_id = ?2 ORDER BY p.updated DESC";
        let rows = c
            .prepare(sql)?
            .query_map(params![v.admin, v.user_id], |r| {
                Ok(ProjectSummary { id: r.get(0)?, name: r.get(1)?, updated: r.get(2)?, owner_id: r.get(3)?, owner_name: r.get(4)? })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    fn project_owner(&self, id: &str) -> Result<Option<Option<i64>>> {
        Ok(self.conn().query_row("SELECT owner_id FROM projects WHERE id = ?", [id], |r| r.get(0)).optional()?)
    }

    fn may_use(v: Viewer, owner: Option<i64>) -> bool {
        v.admin || owner == Some(v.user_id)
    }

    pub fn get_project(&self, id: &str, v: Viewer) -> Result<Access<String>> {
        let row: Option<(String, Option<i64>)> =
            self.conn().query_row("SELECT data, owner_id FROM projects WHERE id = ?", [id], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        Ok(match row {
            None => Access::NotFound,
            // Someone else's project looks the same as a missing one.
            Some((_, owner)) if !Self::may_use(v, owner) => Access::NotFound,
            Some((data, _)) => Access::Ok(data),
        })
    }

    pub fn create_project(&self, id: &str, name: &str, data: &str, owner: i64) -> Result<()> {
        self.conn().execute("INSERT INTO projects (id, name, data, owner_id) VALUES (?1, ?2, ?3, ?4)", params![id, name, data, owner])?;
        Ok(())
    }

    pub fn put_project(&self, id: &str, name: &str, data: &str, v: Viewer) -> Result<Access<()>> {
        match self.project_owner(id)? {
            None => {
                self.create_project(id, name, data, v.user_id)?;
                Ok(Access::Ok(()))
            }
            Some(owner) if !Self::may_use(v, owner) => Ok(Access::NotFound),
            Some(_) => {
                self.conn()
                    .execute("UPDATE projects SET name = ?2, data = ?3, updated = unixepoch() WHERE id = ?1", params![id, name, data])?;
                Ok(Access::Ok(()))
            }
        }
    }

    pub fn delete_project(&self, id: &str, v: Viewer) -> Result<Access<()>> {
        match self.project_owner(id)? {
            None => Ok(Access::NotFound),
            Some(owner) if !Self::may_use(v, owner) => Ok(Access::NotFound),
            Some(_) => {
                self.conn().execute("DELETE FROM projects WHERE id = ?", [id])?;
                Ok(Access::Ok(()))
            }
        }
    }

    // -------------------------------------------------------- users and sessions

    fn user_row(r: &rusqlite::Row) -> rusqlite::Result<UserRow> {
        Ok(UserRow {
            id: r.get("id")?,
            name: r.get("name")?,
            email: r.get("email")?,
            role: Role::parse(&r.get::<_, String>("role")?),
            blocked: r.get::<_, i64>("blocked")? != 0,
            created: r.get("created")?,
            last_login: r.get("last_login")?,
        })
    }

    /// Provision a user at sign-in: create on first sign-in, otherwise refresh
    /// name, email and role from the token. Users without a role are only
    /// updated (to record that their access was removed), never created.
    pub fn upsert_user(&self, tenant: &str, subject: &str, name: &str, email: &str, role: Role) -> Result<UserRow> {
        let c = self.conn();
        let existing: Option<i64> =
            c.query_row("SELECT id FROM users WHERE tenant = ? AND subject = ?", [tenant, subject], |r| r.get(0)).optional()?;
        let id = match existing {
            Some(id) => {
                c.execute(
                    "UPDATE users SET name = ?2, email = ?3, role = ?4, last_login = unixepoch() WHERE id = ?1",
                    params![id, name, email, role.as_str()],
                )?;
                if role == Role::None {
                    c.execute("DELETE FROM sessions WHERE user_id = ?", [id])?;
                }
                id
            }
            None if role == Role::None => {
                return Ok(UserRow { id: 0, name: name.into(), email: email.into(), role, blocked: false, created: 0, last_login: 0 });
            }
            None => {
                c.execute(
                    "INSERT INTO users (tenant, subject, name, email, role) VALUES (?, ?, ?, ?, ?)",
                    params![tenant, subject, name, email, role.as_str()],
                )?;
                c.last_insert_rowid()
            }
        };
        Ok(c.query_row("SELECT * FROM users WHERE id = ?", [id], Self::user_row)?)
    }

    pub fn list_users(&self) -> Result<Vec<UserSummary>> {
        let c = self.conn();
        let rows = c
            .prepare(
                "SELECT u.*, (SELECT COUNT(*) FROM projects p WHERE p.owner_id = u.id) AS projects,
                        (SELECT COUNT(*) FROM samples s WHERE s.owner_id = u.id) AS uploads
                 FROM users u WHERE u.tenant != 'local' ORDER BY u.last_login DESC",
            )?
            .query_map([], |r| Ok(UserSummary { user: Self::user_row(r)?, projects: r.get("projects")?, uploads: r.get("uploads")? }))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Block or unblock a user in Clipdeck. Blocking signs them out everywhere.
    pub fn set_blocked(&self, id: i64, blocked: bool) -> Result<bool> {
        let c = self.conn();
        let n = c.execute("UPDATE users SET blocked = ? WHERE id = ?", params![blocked as i64, id])?;
        if blocked {
            c.execute("DELETE FROM sessions WHERE user_id = ?", [id])?;
        }
        Ok(n > 0)
    }

    pub fn create_session(&self, token_hash: &str, user_id: i64, hours: i64) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM sessions WHERE expires < unixepoch()", [])?;
        c.execute(
            "INSERT INTO sessions (token_hash, user_id, expires) VALUES (?, ?, unixepoch() + ?)",
            params![token_hash, user_id, hours * 3600],
        )?;
        Ok(())
    }

    pub fn session_user(&self, token_hash: &str) -> Result<Option<UserRow>> {
        Ok(self
            .conn()
            .query_row(
                "SELECT u.* FROM sessions s JOIN users u ON u.id = s.user_id WHERE s.token_hash = ? AND s.expires > unixepoch()",
                [token_hash],
                Self::user_row,
            )
            .optional()?)
    }

    pub fn delete_session(&self, token_hash: &str) -> Result<()> {
        self.conn().execute("DELETE FROM sessions WHERE token_hash = ?", [token_hash])?;
        Ok(())
    }

    /// Remember an in-flight sign-in (state → nonce, PKCE verifier, return path).
    pub fn put_pending(&self, state: &str, nonce: &str, verifier: &str, return_to: &str) -> Result<()> {
        let c = self.conn();
        c.execute("DELETE FROM auth_pending WHERE created < unixepoch() - 600", [])?;
        c.execute(
            "INSERT INTO auth_pending (state, nonce, verifier, return_to) VALUES (?, ?, ?, ?)",
            params![state, nonce, verifier, return_to],
        )?;
        Ok(())
    }

    /// Use up an in-flight sign-in; each state works once and for 10 minutes.
    pub fn take_pending(&self, state: &str) -> Result<Option<Pending>> {
        let c = self.conn();
        let p = c
            .query_row(
                "SELECT nonce, verifier, return_to FROM auth_pending WHERE state = ? AND created >= unixepoch() - 600",
                [state],
                |r| Ok(Pending { nonce: r.get(0)?, verifier: r.get(1)?, return_to: r.get(2)? }),
            )
            .optional()?;
        c.execute("DELETE FROM auth_pending WHERE state = ?", [state])?;
        Ok(p)
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
                owner_id: None,
            })
            .unwrap();
        }
        let p = db.search(&Query { q: Some("deep".into()), ..Default::default() }).unwrap();
        assert_eq!(p.total, 3);
        let p = db.search(&Query { key: Some("Am".into()), compatible: Some(true), ..Default::default() }).unwrap();
        assert_eq!(p.total, 2);
        let p = db.search(&Query { bpm: Some(126.0), category: Some("bass,chords".into()), ..Default::default() }).unwrap();
        assert_eq!(p.total, 2);
        // Users are provisioned from sign-in; unassigned people are not created.
        let a = db.upsert_user("t", "oid-a", "Ann", "ann@x", Role::Admin).unwrap();
        let b = db.upsert_user("t", "oid-b", "Bo", "bo@x", Role::User).unwrap();
        let nobody = db.upsert_user("t", "oid-c", "Cy", "cy@x", Role::None).unwrap();
        assert_eq!(nobody.id, 0);
        assert_eq!(db.list_users().unwrap().len(), 2);
        // Role changes in Entra apply at the next sign-in.
        let b2 = db.upsert_user("t", "oid-b", "Bo B", "bo@x", Role::Admin).unwrap();
        assert_eq!((b2.id, b2.role, b2.name.as_str()), (b.id, Role::Admin, "Bo B"));
        db.upsert_user("t", "oid-b", "Bo B", "bo@x", Role::User).unwrap();

        // Favorites are per user.
        db.set_favorite(a.id, "s2", true).unwrap();
        assert_eq!(db.search(&Query { favorites: Some(true), user_id: a.id, ..Default::default() }).unwrap().total, 1);
        assert_eq!(db.search(&Query { favorites: Some(true), user_id: b.id, ..Default::default() }).unwrap().total, 0);
        assert!(db.get("s2", a.id).unwrap().unwrap().favorite);
        assert!(!db.get("s2", b.id).unwrap().unwrap().favorite);
        let f = db.facets(&Query { category: Some("bass".into()), ..Default::default() }).unwrap();
        assert_eq!(f.categories.len(), 3);

        // Projects belong to their creator; admins see all.
        let va = Viewer { user_id: a.id, admin: true };
        let vb = Viewer { user_id: b.id, admin: false };
        db.create_project("p1", "Song", "{}", b.id).unwrap();
        assert!(matches!(db.put_project("p1", "Song 2", "{\"a\":1}", vb).unwrap(), Access::Ok(())));
        assert_eq!(db.list_projects(vb).unwrap()[0].name, "Song 2");
        assert_eq!(db.list_projects(va).unwrap()[0].owner_name.as_deref(), Some("Bo B"));
        db.create_project("p2", "Ann's", "{}", a.id).unwrap();
        assert_eq!(db.list_projects(vb).unwrap().len(), 1);
        assert!(matches!(db.get_project("p2", vb).unwrap(), Access::NotFound));
        assert!(matches!(db.put_project("p2", "x", "{}", vb).unwrap(), Access::NotFound));
        assert!(matches!(db.delete_project("p2", vb).unwrap(), Access::NotFound));
        assert!(matches!(db.get_project("p1", va).unwrap(), Access::Ok(_)));

        // Sessions end when a user is blocked or loses the role.
        db.create_session("h1", b.id, 1).unwrap();
        assert_eq!(db.session_user("h1").unwrap().unwrap().id, b.id);
        db.set_blocked(b.id, true).unwrap();
        assert!(db.session_user("h1").unwrap().is_none());
        db.set_blocked(b.id, false).unwrap();
        db.create_session("h2", b.id, 1).unwrap();
        db.upsert_user("t", "oid-b", "Bo B", "bo@x", Role::None).unwrap();
        assert!(db.session_user("h2").unwrap().is_none());

        // Sign-in state is single use.
        db.put_pending("st", "n", "v", "/").unwrap();
        assert!(db.take_pending("st").unwrap().is_some());
        assert!(db.take_pending("st").unwrap().is_none());
        std::fs::remove_dir_all(dir).ok();
    }
}
