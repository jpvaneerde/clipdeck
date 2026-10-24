//! Clipdeck server.
//!
//!   clipdeck-server [serve]          run the web server (default)
//!   clipdeck-server index [--force]  index the generated library and user samples
//!   clipdeck-server render IN.json OUT.wav [--bars N]
//!
//! Environment:
//!   CLIPDECK_DATA     data directory (default ./data): clipdeck.db, user/
//!   CLIPDECK_LIBRARY  generated library (default $CLIPDECK_DATA/library)
//!   CLIPDECK_WEB      built web app (default ./web/dist)
//!   PORT              listen port (default 8080)
//!   Sign-in (Entra ID): PUBLIC_URL, OIDC_TENANT_ID, OIDC_CLIENT_ID, OIDC_CLIENT_SECRET;
//!   see auth.rs. CLIPDECK_AUTH=disabled turns sign-in off for local development.

mod api;
mod audio;
mod auth;
#[cfg(test)]
mod auth_flow_test;
mod db;
mod render;

use anyhow::{Context, Result};
use axum::http::{header, HeaderValue};
use axum::Router;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::compression::predicate::{NotForContentType, Predicate, SizeAbove};
use tower_http::compression::CompressionLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_header::SetResponseHeaderLayer;
use tower_http::trace::TraceLayer;

struct Config {
    data: PathBuf,
    library: PathBuf,
    web: PathBuf,
    port: u16,
}

fn config() -> Config {
    let data = PathBuf::from(std::env::var("CLIPDECK_DATA").unwrap_or_else(|_| "data".into()));
    let library = std::env::var("CLIPDECK_LIBRARY").map(PathBuf::from).unwrap_or_else(|_| data.join("library"));
    let web = PathBuf::from(std::env::var("CLIPDECK_WEB").unwrap_or_else(|_| "web/dist".into()));
    let port = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8080);
    Config { data, library, web, port }
}

fn open_db(cfg: &Config) -> Result<db::Shared> {
    std::fs::create_dir_all(&cfg.data).with_context(|| format!("create {}", cfg.data.display()))?;
    let user = cfg.data.join("user");
    std::fs::create_dir_all(&user)?;
    db::Db::open(&cfg.data.join("clipdeck.db"), cfg.library.clone(), user)
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info,tower_http=info".into()))
        .init();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cfg = config();
    match args.first().map(String::as_str) {
        None | Some("serve") => serve(cfg).await,
        Some("index") => {
            let db = open_db(&cfg)?;
            let force = args.iter().any(|a| a == "--force");
            let n = db.index_library(force)?;
            let (u, errs) = api::scan_user_dir(&db)?;
            println!("indexed {n} library samples, {u} user samples ({} total)", db.count());
            for e in errs {
                eprintln!("  skipped {e}");
            }
            Ok(())
        }
        Some("render") => {
            let input = args.get(1).context("usage: render IN.json OUT.wav [--bars N]")?;
            let output = args.get(2).context("usage: render IN.json OUT.wav [--bars N]")?;
            let bars = args.iter().position(|a| a == "--bars").and_then(|i| args.get(i + 1)).and_then(|b| b.parse().ok()).unwrap_or(8);
            let db = open_db(&cfg)?;
            let project: clipdeck_model::Project = serde_json::from_str(&std::fs::read_to_string(input)?)?;
            let t0 = std::time::Instant::now();
            let wav = render::render_wav(&db, &project, &render::RenderOptions { fallback_bars: bars, ..Default::default() })?;
            std::fs::write(output, &wav)?;
            println!("rendered {output} ({:.1} MB) in {:.2}s", wav.len() as f64 / 1e6, t0.elapsed().as_secs_f32());
            Ok(())
        }
        Some(other) => anyhow::bail!("unknown command {other}"),
    }
}

async fn serve(cfg: Config) -> Result<()> {
    let db = open_db(&cfg)?;

    // Index in the background so the server is ready immediately.
    let db_bg = db.clone();
    tokio::task::spawn_blocking(move || {
        match db_bg.index_library(false) {
            Ok(0) => {}
            Ok(n) => tracing::info!("indexed {n} library samples"),
            Err(e) => tracing::error!(error = %e, "library indexing failed"),
        }
        match api::scan_user_dir(&db_bg) {
            Ok((n, _)) if n > 0 => tracing::info!("indexed {n} new user samples"),
            Ok(_) => {}
            Err(e) => tracing::error!(error = %e, "user sample scan failed"),
        }
        tracing::info!("{} samples available", db_bg.count());
    });

    let auth = Arc::new(auth::from_env(&db)?);
    if let auth::Auth::Oidc(o) = auth.as_ref() {
        o.warm_up().await;
    }
    let app = build_app(db, auth, &cfg.web);

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("listening on http://{addr} (web: {}, library: {})", cfg.web.display(), cfg.library.display());
    axum::serve(listener, app).with_graceful_shutdown(shutdown()).await?;
    Ok(())
}

/// The whole HTTP app: sign-in routes, then everything else behind sign-in.
fn build_app(db: db::Shared, auth: auth::SharedAuth, web: &std::path::Path) -> Router {
    let index = web.join("index.html");
    let assets = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::overriding(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=31536000, immutable")))
        .service(ServeDir::new(web.join("assets")));
    let spa = ServiceBuilder::new()
        .layer(SetResponseHeaderLayer::if_not_present(header::CACHE_CONTROL, HeaderValue::from_static("no-cache")))
        .service(ServeDir::new(web).fallback(ServeFile::new(index)));

    // Compress text and wasm; audio is already dense and needs byte ranges.
    let compress = CompressionLayer::new().compress_when(SizeAbove::new(1024).and(NotForContentType::new("audio/")));

    let state = auth::AuthState { db: db.clone(), auth };
    api::router(db)
        .nest_service("/assets", assets)
        .fallback_service(spa)
        .merge(auth::routes(state.clone()))
        .layer(axum::middleware::from_fn_with_state(state, auth::require_user))
        .layer(compress)
        .layer(TraceLayer::new_for_http())
}

async fn shutdown() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let term = async {
        if let Ok(mut s) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            s.recv().await;
        }
    };
    #[cfg(not(unix))]
    let term = std::future::pending::<()>();
    tokio::select! { _ = ctrl_c => {}, _ = term => {} }
    tracing::info!("shutting down");
}
