//! HTTP API.

use crate::audio;
use crate::db::{NewSample, Query, SamplePatch, Shared};
use crate::render::{render_wav, RenderOptions};
use axum::body::Body;
use axum::extract::{DefaultBodyLimit, Multipart, Path, Query as Q, Request, State};
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tower::ServiceExt;
use tower_http::services::ServeFile;

pub struct AppError(anyhow::Error);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        tracing::error!(error = %self.0, "request failed");
        (StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": self.0.to_string() }))).into_response()
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        AppError(e.into())
    }
}

type R<T> = Result<T, AppError>;

fn not_found(what: &str) -> Response {
    (StatusCode::NOT_FOUND, Json(json!({ "error": format!("{what} not found") }))).into_response()
}

fn bad_request(msg: impl Into<String>) -> Response {
    (StatusCode::BAD_REQUEST, Json(json!({ "error": msg.into() }))).into_response()
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> anyhow::Result<T> {
    tokio::task::spawn_blocking(f).await?
}

pub fn router(db: Shared) -> Router {
    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .route("/api/status", get(status))
        .route("/api/samples", get(search))
        .route("/api/facets", get(facets))
        .route("/api/sample", get(sample_get).patch(sample_patch).delete(sample_delete))
        .route("/api/favorite", post(favorite))
        .route("/api/audio/{*id}", get(audio_file))
        .route("/api/upload", post(upload).layer(DefaultBodyLimit::max(512 * 1024 * 1024)))
        .route("/api/rescan", post(rescan))
        .route("/api/projects", get(projects_list).post(project_create))
        .route("/api/projects/{id}", get(project_get).put(project_put).delete(project_delete))
        .route("/api/render", post(render).layer(DefaultBodyLimit::max(32 * 1024 * 1024)))
        .with_state(db)
}

async fn status(State(db): State<Shared>) -> Json<serde_json::Value> {
    Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "samples": db.count(),
        "indexing": db.indexing.load(Ordering::SeqCst),
        "indexProgress": db.index_progress.load(Ordering::SeqCst),
        "indexTotal": db.index_total.load(Ordering::SeqCst),
    }))
}

async fn search(State(db): State<Shared>, Q(q): Q<Query>) -> R<Response> {
    let page = blocking(move || db.search(&q)).await?;
    Ok(Json(page).into_response())
}

async fn facets(State(db): State<Shared>, Q(q): Q<Query>) -> R<Response> {
    let f = blocking(move || db.facets(&q)).await?;
    Ok(Json(f).into_response())
}

#[derive(Deserialize)]
struct IdQuery {
    id: String,
}

async fn sample_get(State(db): State<Shared>, Q(IdQuery { id }): Q<IdQuery>) -> R<Response> {
    Ok(match blocking(move || db.get(&id)).await? {
        Some(s) => Json(s).into_response(),
        None => not_found("sample"),
    })
}

async fn sample_patch(State(db): State<Shared>, Q(IdQuery { id }): Q<IdQuery>, Json(patch): Json<SamplePatch>) -> R<Response> {
    Ok(match blocking(move || db.update_sample(&id, &patch)).await? {
        Some(s) => Json(s).into_response(),
        None => not_found("sample"),
    })
}

async fn sample_delete(State(db): State<Shared>, Q(IdQuery { id }): Q<IdQuery>) -> R<Response> {
    let path = blocking(move || db.delete_sample(&id)).await?;
    Ok(match path {
        Some(p) => {
            let _ = tokio::fs::remove_file(p).await;
            StatusCode::NO_CONTENT.into_response()
        }
        None => bad_request("only uploaded samples can be deleted"),
    })
}

#[derive(Deserialize)]
struct FavBody {
    id: String,
    favorite: bool,
}

async fn favorite(State(db): State<Shared>, Json(b): Json<FavBody>) -> R<Response> {
    let ok = blocking(move || db.set_favorite(&b.id, b.favorite)).await?;
    Ok(if ok { StatusCode::NO_CONTENT.into_response() } else { not_found("sample") })
}

async fn audio_file(State(db): State<Shared>, Path(id): Path<String>, req: Request) -> R<Response> {
    let id2 = id.clone();
    let Some(path) = blocking(move || db.file_path(&id2)).await? else { return Ok(not_found("sample")) };
    let mut res = ServeFile::new(path).oneshot(req).await?.map(Body::new);
    if res.status().is_success() || res.status() == StatusCode::PARTIAL_CONTENT {
        // Sample ids never change content, so browsers may cache forever.
        res.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=31536000, immutable"));
    }
    Ok(res)
}

const AUDIO_EXT: [&str; 6] = ["wav", "mp3", "flac", "ogg", "m4a", "aac"];

/// Index one audio file that lives under the user directory.
pub fn index_user_file(db: &Shared, rel: &std::path::Path, tags: Vec<String>) -> anyhow::Result<NewSample> {
    let abs = db.user_dir.join(rel);
    let d = audio::decode(&abs)?;
    let stem = rel.file_stem().and_then(|s| s.to_str()).unwrap_or("sample").to_string();
    let rel_str = rel.to_string_lossy().replace('\\', "/");
    let id = format!("user/{}", rel_str.rsplit_once('.').map_or(rel_str.as_str(), |(a, _)| a));
    let g = audio::guess_from_name(&stem);
    let display = stem.split(|c: char| c == '_' || c == '-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
    let loop_like = g.bpm.is_some() && !matches!(g.category, "drums" | "synth-hit" | "fx");
    let bars = match (loop_like, g.bpm) {
        (true, Some(bpm)) => {
            let beats = d.duration() as f64 * bpm as f64 / 60.0;
            let bars = (beats / 4.0).round().max(1.0) as u32;
            // Only trust it if the file is close to a whole number of bars.
            if ((beats / 4.0) - bars as f64).abs() < 0.08 {
                Some(bars)
            } else {
                None
            }
        }
        _ => None,
    };
    let mut all_tags = tags;
    all_tags.push("uploaded".into());
    if bars.is_some() {
        all_tags.push("loop".into());
    }
    let s = NewSample {
        id,
        root: "user",
        path: rel_str,
        name: display,
        category: g.category.to_string(),
        instrument: g.instrument,
        genre: None,
        bpm: g.bpm,
        key: g.key,
        bars,
        beats: bars.map(|b| b as f64 * 4.0),
        kit: None,
        tags: all_tags,
        duration: d.duration(),
        channels: d.channels as u32,
        sample_rate: d.sample_rate,
        source: "user".into(),
        peaks: audio::peaks(&d, 96),
    };
    db.insert(&s)?;
    Ok(s)
}

fn sanitize(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, '.' | '-' | '_' | '#' | ' ') { c } else { '_' }).collect();
    let s = s.trim().trim_start_matches('.').to_string();
    if s.is_empty() {
        "sample".into()
    } else {
        s
    }
}

async fn upload(State(db): State<Shared>, mut mp: Multipart) -> R<Response> {
    let mut saved = Vec::new();
    let mut errors = Vec::new();
    while let Some(field) = mp.next_field().await? {
        let Some(fname) = field.file_name().map(sanitize) else { continue };
        let ext = fname.rsplit('.').next().unwrap_or("").to_lowercase();
        if !AUDIO_EXT.contains(&ext.as_str()) {
            errors.push(format!("{fname}: unsupported file type"));
            continue;
        }
        let bytes = field.bytes().await?;
        let short = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
        let rel = PathBuf::from("uploads").join(format!("{short}-{fname}"));
        let abs = db.user_dir.join(&rel);
        tokio::fs::create_dir_all(abs.parent().unwrap()).await?;
        tokio::fs::write(&abs, &bytes).await?;
        let db2 = db.clone();
        let rel2 = rel.clone();
        match blocking(move || index_user_file(&db2, &rel2, vec![])).await {
            Ok(s) => saved.push(s.id),
            Err(e) => {
                let _ = tokio::fs::remove_file(&abs).await;
                errors.push(format!("{fname}: {e}"));
            }
        }
    }
    let db2 = db.clone();
    let items = blocking(move || Ok(saved.iter().filter_map(|id| db2.get(id).ok().flatten()).collect::<Vec<_>>())).await?;
    Ok(Json(json!({ "items": items, "errors": errors })).into_response())
}

/// Scan the user directory for audio not indexed yet (e.g. packs copied onto the volume).
pub fn scan_user_dir(db: &Shared) -> anyhow::Result<(usize, Vec<String>)> {
    let mut added = 0;
    let mut errors = Vec::new();
    let mut stack = vec![db.user_dir.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        for entry in rd.flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
            if !AUDIO_EXT.contains(&ext.as_str()) {
                continue;
            }
            let rel = p.strip_prefix(&db.user_dir)?.to_path_buf();
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            let id = format!("user/{}", rel_str.rsplit_once('.').map_or(rel_str.as_str(), |(a, _)| a));
            if db.get(&id)?.is_some() {
                continue;
            }
            // Folder names become tags ("packs/Dusty Drums/kicks/k1.wav").
            let tags: Vec<String> = rel
                .parent()
                .map(|pp| {
                    pp.components()
                        .map(|c| c.as_os_str().to_string_lossy().to_lowercase().replace(' ', "-"))
                        .filter(|t| t != "uploads")
                        .collect()
                })
                .unwrap_or_default();
            match index_user_file(db, &rel, tags) {
                Ok(_) => added += 1,
                Err(e) => errors.push(format!("{}: {e}", rel.display())),
            }
        }
    }
    Ok((added, errors))
}

async fn rescan(State(db): State<Shared>) -> R<Response> {
    let db2 = db.clone();
    let (added, errors) = blocking(move || scan_user_dir(&db2)).await?;
    let db3 = db.clone();
    let lib = blocking(move || db3.index_library(false)).await?;
    Ok(Json(json!({ "added": added, "libraryIndexed": lib, "errors": errors })).into_response())
}

// ------------------------------------------------------------ projects

async fn projects_list(State(db): State<Shared>) -> R<Response> {
    Ok(Json(blocking(move || db.list_projects()).await?).into_response())
}

fn project_name(v: &serde_json::Value) -> String {
    v.get("name").and_then(|n| n.as_str()).filter(|s| !s.trim().is_empty()).unwrap_or("Untitled").to_string()
}

async fn project_create(State(db): State<Shared>, Json(v): Json<serde_json::Value>) -> R<Response> {
    if serde_json::from_value::<clipdeck_model::Project>(v.clone()).is_err() {
        return Ok(bad_request("not a valid project"));
    }
    let id = uuid::Uuid::new_v4().simple().to_string()[..12].to_string();
    let name = project_name(&v);
    let data = v.to_string();
    let id2 = id.clone();
    blocking(move || db.put_project(&id2, &name, &data)).await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))).into_response())
}

async fn project_get(State(db): State<Shared>, Path(id): Path<String>) -> R<Response> {
    Ok(match blocking(move || db.get_project(&id)).await? {
        Some(data) => ([(header::CONTENT_TYPE, "application/json")], data).into_response(),
        None => not_found("project"),
    })
}

async fn project_put(State(db): State<Shared>, Path(id): Path<String>, Json(v): Json<serde_json::Value>) -> R<Response> {
    if serde_json::from_value::<clipdeck_model::Project>(v.clone()).is_err() {
        return Ok(bad_request("not a valid project"));
    }
    let name = project_name(&v);
    let data = v.to_string();
    blocking(move || db.put_project(&id, &name, &data)).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn project_delete(State(db): State<Shared>, Path(id): Path<String>) -> R<Response> {
    let ok = blocking(move || db.delete_project(&id)).await?;
    Ok(if ok { StatusCode::NO_CONTENT.into_response() } else { not_found("project") })
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RenderQuery {
    bars: Option<u32>,
    sample_rate: Option<u32>,
    bits: Option<u16>,
    tail: Option<f32>,
}

async fn render(State(db): State<Shared>, Q(q): Q<RenderQuery>, Json(project): Json<clipdeck_model::Project>) -> R<Response> {
    let opts = RenderOptions {
        sample_rate: q.sample_rate.filter(|s| [44100, 48000, 96000].contains(s)).unwrap_or(48000),
        bits: q.bits.filter(|b| [16, 24].contains(b)).unwrap_or(24),
        fallback_bars: q.bars.unwrap_or(8).clamp(1, 512),
        tail_seconds: q.tail.unwrap_or(2.0).clamp(0.0, 10.0),
    };
    let name = project.name.clone();
    let wav = blocking(move || render_wav(&db, &project, &opts)).await?;
    let file = sanitize(&name).replace(' ', "-");
    Ok((
        [(header::CONTENT_TYPE, "audio/wav".to_string()), (header::CONTENT_DISPOSITION, format!("attachment; filename=\"{file}.wav\""))],
        wav,
    )
        .into_response())
}
