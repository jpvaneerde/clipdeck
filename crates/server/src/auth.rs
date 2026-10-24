//! Sign-in with Microsoft Entra ID (OpenID Connect).
//!
//! Authorization code flow with PKCE, run by the server as a confidential
//! client. The ID token is validated (RS256 signature against the tenant's
//! JWKS, issuer, audience, expiry, nonce, tenant). Access comes from the app
//! roles assigned in Entra: `Admin` or `User`. Each sign-in provisions or
//! updates the user in Clipdeck's database with the role from the token;
//! users without a role are refused. Sessions are random tokens kept
//! server-side (only a SHA-256 hash is stored) in an HttpOnly cookie.
//!
//! Environment:
//!   PUBLIC_URL           e.g. https://clipdeck.jplay.cloud (redirect URIs derive from it)
//!   OIDC_TENANT_ID       directory (tenant) ID
//!   OIDC_CLIENT_ID       application (client) ID
//!   OIDC_CLIENT_SECRET   client secret
//!   OIDC_AUTHORITY       optional; default https://login.microsoftonline.com/<tenant>/v2.0
//!   SESSION_HOURS        optional; default 12. Role changes apply at the next sign-in.
//!   CLIPDECK_AUTH=disabled  local development only: everyone is a local admin.

use crate::db::{Role, Shared, UserRow};
use anyhow::{anyhow, bail, Context, Result};
use axum::extract::{Query, Request, State};
use axum::http::{header, HeaderMap, HeaderValue, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine as _;
use jsonwebtoken::jwk::JwkSet;
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use rand::RngCore;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub const ROLE_ADMIN: &str = "Admin";
pub const ROLE_USER: &str = "User";

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub tenant: String,
    pub client_id: String,
    pub client_secret: String,
    pub authority: String,
    pub public_url: String,
    pub session_hours: i64,
}

impl OidcConfig {
    fn redirect_uri(&self) -> String {
        format!("{}/auth/callback", self.public_url)
    }
    fn signed_out_uri(&self) -> String {
        format!("{}/auth/signed-out", self.public_url)
    }
    fn secure(&self) -> bool {
        self.public_url.starts_with("https://")
    }
}

pub enum Auth {
    Oidc(Oidc),
    /// Local development: no sign-in, a single local admin.
    Disabled {
        dev_user: UserRow,
    },
}

pub type SharedAuth = Arc<Auth>;

/// Read auth settings from the environment. Refuses to start half-configured.
pub fn from_env(db: &Shared) -> Result<Auth> {
    let var = |k: &str| std::env::var(k).ok().filter(|v| !v.trim().is_empty());
    if var("CLIPDECK_AUTH").as_deref() == Some("disabled") {
        tracing::warn!("CLIPDECK_AUTH=disabled: no sign-in, everyone is an admin. Use this for local development only.");
        let dev_user = db.upsert_user("local", "dev", "Local admin", "", Role::Admin)?;
        return Ok(Auth::Disabled { dev_user });
    }
    let missing: Vec<&str> =
        ["PUBLIC_URL", "OIDC_TENANT_ID", "OIDC_CLIENT_ID", "OIDC_CLIENT_SECRET"].into_iter().filter(|k| var(k).is_none()).collect();
    if !missing.is_empty() {
        bail!(
            "sign-in is not configured: set {} (see README, \"Sign-in with Entra ID\"), or CLIPDECK_AUTH=disabled for local development",
            missing.join(", ")
        );
    }
    let tenant = var("OIDC_TENANT_ID").unwrap();
    let cfg = OidcConfig {
        authority: var("OIDC_AUTHORITY").unwrap_or_else(|| format!("https://login.microsoftonline.com/{tenant}/v2.0")),
        tenant,
        client_id: var("OIDC_CLIENT_ID").unwrap(),
        client_secret: var("OIDC_CLIENT_SECRET").unwrap(),
        public_url: var("PUBLIC_URL").unwrap().trim_end_matches('/').to_string(),
        session_hours: var("SESSION_HOURS").and_then(|v| v.parse().ok()).unwrap_or(12).clamp(1, 24 * 30),
    };
    Ok(Auth::Oidc(Oidc::new(cfg)))
}

// ------------------------------------------------------------ OIDC client

#[derive(Deserialize, Clone, Debug)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    end_session_endpoint: Option<String>,
}

#[derive(Deserialize, Debug)]
struct IdClaims {
    sub: String,
    #[serde(default)]
    oid: Option<String>,
    #[serde(default)]
    tid: Option<String>,
    #[serde(default)]
    nonce: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    preferred_username: Option<String>,
    #[serde(default)]
    roles: Vec<String>,
}

pub struct Oidc {
    pub cfg: OidcConfig,
    http: reqwest::Client,
    discovery: RwLock<Option<(Discovery, Instant)>>,
    jwks: RwLock<Option<(JwkSet, Instant)>>,
}

const DISCOVERY_TTL: Duration = Duration::from_secs(24 * 3600);
const JWKS_TTL: Duration = Duration::from_secs(3600);
const JWKS_MIN_REFRESH: Duration = Duration::from_secs(60);

impl Oidc {
    pub fn new(cfg: OidcConfig) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(15))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .expect("http client");
        Self { cfg, http, discovery: RwLock::new(None), jwks: RwLock::new(None) }
    }

    async fn discovery(&self) -> Result<Discovery> {
        if let Some((d, at)) = self.discovery.read().await.as_ref() {
            if at.elapsed() < DISCOVERY_TTL {
                return Ok(d.clone());
            }
        }
        let url = format!("{}/.well-known/openid-configuration", self.cfg.authority);
        let d: Discovery = self.http.get(&url).send().await?.error_for_status()?.json().await.with_context(|| format!("read {url}"))?;
        *self.discovery.write().await = Some((d.clone(), Instant::now()));
        Ok(d)
    }

    /// Load metadata early so configuration mistakes show in the log at startup.
    pub async fn warm_up(&self) {
        match self.discovery().await {
            Ok(d) => tracing::info!(issuer = %d.issuer, "Entra ID sign-in ready; redirect URI {}", self.cfg.redirect_uri()),
            Err(e) => {
                tracing::error!(error = %e, "could not load OpenID configuration from {} (will retry on sign-in)", self.cfg.authority)
            }
        }
    }

    async fn key_for(&self, kid: &str) -> Result<DecodingKey> {
        let d = self.discovery().await?;
        let mut tried_refresh = false;
        loop {
            let fresh = {
                let g = self.jwks.read().await;
                match g.as_ref() {
                    Some((set, at)) => {
                        if let Some(jwk) = set.find(kid) {
                            if at.elapsed() < JWKS_TTL {
                                return Ok(DecodingKey::from_jwk(jwk)?);
                            }
                        }
                        // Unknown key or stale set: refresh, but not more than once a minute.
                        at.elapsed() < JWKS_MIN_REFRESH
                    }
                    None => false,
                }
            };
            if tried_refresh || fresh {
                let g = self.jwks.read().await;
                if let Some(jwk) = g.as_ref().and_then(|(s, _)| s.find(kid)) {
                    return Ok(DecodingKey::from_jwk(jwk)?);
                }
                bail!("ID token signed with unknown key {kid}");
            }
            let set: JwkSet = self.http.get(&d.jwks_uri).send().await?.error_for_status()?.json().await?;
            *self.jwks.write().await = Some((set, Instant::now()));
            tried_refresh = true;
        }
    }

    async fn authorize_url(&self, state: &str, nonce: &str, challenge: &str) -> Result<String> {
        let d = self.discovery().await?;
        let mut u = url::Url::parse(&d.authorization_endpoint)?;
        u.query_pairs_mut()
            .append_pair("client_id", &self.cfg.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &self.cfg.redirect_uri())
            .append_pair("response_mode", "query")
            .append_pair("scope", "openid profile email")
            .append_pair("state", state)
            .append_pair("nonce", nonce)
            .append_pair("code_challenge", challenge)
            .append_pair("code_challenge_method", "S256");
        Ok(u.into())
    }

    async fn redeem(&self, code: &str, verifier: &str, nonce: &str) -> Result<IdClaims> {
        let d = self.discovery().await?;
        #[derive(Deserialize)]
        struct TokenResponse {
            id_token: Option<String>,
            error: Option<String>,
            error_description: Option<String>,
        }
        let res = self
            .http
            .post(&d.token_endpoint)
            .form(&[
                ("grant_type", "authorization_code"),
                ("code", code),
                ("redirect_uri", &self.cfg.redirect_uri()),
                ("client_id", &self.cfg.client_id),
                ("client_secret", &self.cfg.client_secret),
                ("code_verifier", verifier),
                ("scope", "openid profile email"),
            ])
            .send()
            .await?;
        let status = res.status();
        let body: TokenResponse = res.json().await.with_context(|| format!("token endpoint answered {status}"))?;
        if let Some(e) = body.error {
            bail!("{e}: {}", body.error_description.unwrap_or_default().lines().next().unwrap_or(""));
        }
        let token = body.id_token.ok_or_else(|| anyhow!("token response has no id_token"))?;
        self.validate(&token, nonce, &d).await
    }

    async fn validate(&self, token: &str, nonce: &str, d: &Discovery) -> Result<IdClaims> {
        let header = jsonwebtoken::decode_header(token)?;
        if header.alg != Algorithm::RS256 {
            bail!("unexpected token algorithm {:?}", header.alg);
        }
        let kid = header.kid.ok_or_else(|| anyhow!("ID token has no key id"))?;
        let key = self.key_for(&kid).await?;
        let mut v = Validation::new(Algorithm::RS256);
        v.set_audience(&[&self.cfg.client_id]);
        v.set_issuer(&[&d.issuer]);
        v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        v.validate_nbf = true;
        v.leeway = 120;
        let claims = jsonwebtoken::decode::<IdClaims>(token, &key, &v)?.claims;
        if claims.nonce.as_deref() != Some(nonce) {
            bail!("ID token nonce does not match this sign-in");
        }
        let tid = claims.tid.as_deref().unwrap_or("");
        // With a tenant ID configured, require exactly that tenant; with a
        // domain name, the tenant-specific issuer (already checked) carries the ID.
        let same_tenant = if uuid::Uuid::parse_str(&self.cfg.tenant).is_ok() {
            tid.eq_ignore_ascii_case(&self.cfg.tenant)
        } else {
            !tid.is_empty() && d.issuer.contains(tid)
        };
        if !same_tenant {
            bail!("ID token is from another tenant");
        }
        Ok(claims)
    }
}

/// Role from the token's `roles` claim. Admin wins if both are assigned.
pub fn role_from_claims(roles: &[String]) -> Role {
    if roles.iter().any(|r| r.eq_ignore_ascii_case(ROLE_ADMIN)) {
        Role::Admin
    } else if roles.iter().any(|r| r.eq_ignore_ascii_case(ROLE_USER)) {
        Role::User
    } else {
        Role::None
    }
}

// ------------------------------------------------------------ helpers

fn random_token() -> String {
    let mut b = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut b);
    B64.encode(b)
}

pub fn hash_token(t: &str) -> String {
    B64.encode(Sha256::digest(t.as_bytes()))
}

fn cookie_name(secure: bool) -> &'static str {
    // The __Host- prefix pins the cookie to this host, https and path /.
    if secure {
        "__Host-clipdeck_session"
    } else {
        "clipdeck_session"
    }
}

fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Only same-site relative paths are allowed as a post-sign-in destination.
fn safe_return(r: Option<&str>) -> String {
    match r {
        Some(p) if p.starts_with('/') && !p.starts_with("//") && !p.starts_with("/\\") && !p.starts_with("/auth/") => p.to_string(),
        _ => "/".into(),
    }
}

fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title} · Clipdeck</title><style>
body{{margin:0;min-height:100vh;display:grid;place-items:center;background:#121517;color:#e4e9e6;font:15px/1.5 system-ui,sans-serif;padding:16px}}
main{{max-width:440px;background:#191d20;border:1px solid #2f363c;border-radius:8px;padding:24px}}
h1{{margin:0 0 8px;font-size:20px}}p{{margin:0 0 12px;color:#8d9792}}
a.btn{{display:inline-block;background:#56d9a8;color:#0d2a20;font-weight:600;padding:8px 14px;border-radius:4px;text-decoration:none}}
code{{color:#e4e9e6}}</style></head><body><main><h1>{title}</h1>{body}</main></body></html>"#
    );
    (status, [(header::CACHE_CONTROL, "no-store")], Html(html)).into_response()
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

// ------------------------------------------------------------ routes

#[derive(Clone)]
pub struct AuthState {
    pub db: Shared,
    pub auth: SharedAuth,
}

pub fn routes(state: AuthState) -> Router {
    Router::new()
        .route("/auth/login", get(login))
        .route("/auth/callback", get(callback))
        .route("/auth/logout", post(logout))
        .route("/auth/signed-out", get(signed_out))
        .with_state(state)
}

#[derive(Deserialize)]
struct LoginQuery {
    #[serde(rename = "return")]
    ret: Option<String>,
}

async fn login(State(s): State<AuthState>, Query(q): Query<LoginQuery>) -> Response {
    let oidc = match s.auth.as_ref() {
        Auth::Oidc(o) => o,
        Auth::Disabled { .. } => return Redirect::to(&safe_return(q.ret.as_deref())).into_response(),
    };
    let state = random_token();
    let nonce = random_token();
    let verifier = random_token();
    let challenge = B64.encode(Sha256::digest(verifier.as_bytes()));
    let ret = safe_return(q.ret.as_deref());
    let db = s.db.clone();
    let (st, no, ve) = (state.clone(), nonce.clone(), verifier.clone());
    if let Err(e) =
        tokio::task::spawn_blocking(move || db.put_pending(&st, &no, &ve, &ret)).await.map_err(anyhow::Error::from).and_then(|r| r)
    {
        tracing::error!(error = %e, "could not start sign-in");
        return page(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Sign-in failed",
            "<p>Clipdeck could not start the sign-in. Try again in a moment.</p>",
        );
    }
    match oidc.authorize_url(&state, &nonce, &challenge).await {
        Ok(url) => Redirect::to(&url).into_response(),
        Err(e) => {
            tracing::error!(error = %e, "OpenID configuration unavailable");
            page(
                StatusCode::BAD_GATEWAY,
                "Microsoft sign-in is unreachable",
                "<p>Clipdeck could not reach Microsoft Entra ID. Check the server's network access and its <code>OIDC_TENANT_ID</code>, then try again.</p><p><a class=\"btn\" href=\"/auth/login\">Try again</a></p>",
            )
        }
    }
}

#[derive(Deserialize)]
struct CallbackQuery {
    code: Option<String>,
    state: Option<String>,
    error: Option<String>,
    error_description: Option<String>,
}

async fn callback(State(s): State<AuthState>, Query(q): Query<CallbackQuery>) -> Response {
    let Auth::Oidc(oidc) = s.auth.as_ref() else { return Redirect::to("/").into_response() };
    let again = "<p><a class=\"btn\" href=\"/auth/login\">Sign in again</a></p>";
    if let Some(err) = q.error {
        let desc = q.error_description.unwrap_or_default();
        tracing::warn!(%err, %desc, "Entra ID returned an error");
        let first = desc.lines().next().unwrap_or("").to_string();
        return page(StatusCode::FORBIDDEN, "Sign-in was not completed", &format!("<p>Microsoft answered: {}</p>{again}", esc(&first)));
    }
    let (Some(code), Some(state)) = (q.code, q.state) else {
        return page(StatusCode::BAD_REQUEST, "Sign-in was not completed", &format!("<p>The sign-in response was incomplete.</p>{again}"));
    };
    let db = s.db.clone();
    let pending = tokio::task::spawn_blocking(move || db.take_pending(&state)).await.ok().and_then(|r| r.ok()).flatten();
    let Some(p) = pending else {
        return page(
            StatusCode::BAD_REQUEST,
            "Sign-in expired",
            &format!("<p>This sign-in link was already used or is more than 10 minutes old.</p>{again}"),
        );
    };
    let claims = match oidc.redeem(&code, &p.verifier, &p.nonce).await {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(error = %e, "sign-in rejected");
            return page(StatusCode::FORBIDDEN, "Sign-in failed", &format!("<p>{}</p>{again}", esc(&e.to_string())));
        }
    };
    let role = role_from_claims(&claims.roles);
    let tenant = claims.tid.clone().unwrap_or_default();
    let subject = claims.oid.clone().unwrap_or_else(|| claims.sub.clone());
    let name = claims.name.clone().unwrap_or_else(|| claims.preferred_username.clone().unwrap_or_else(|| "Unknown".into()));
    let email = claims.email.clone().or(claims.preferred_username.clone()).unwrap_or_default();
    let hours = oidc.cfg.session_hours;
    let db = s.db.clone();
    let result = tokio::task::spawn_blocking(move || -> Result<Option<(UserRow, String)>> {
        let user = db.upsert_user(&tenant, &subject, &name, &email, role)?;
        if role == Role::None || user.blocked {
            return Ok(Some((user, String::new())));
        }
        let token = random_token();
        db.create_session(&hash_token(&token), user.id, hours)?;
        Ok(Some((user, token)))
    })
    .await;
    let (user, token) = match result {
        Ok(Ok(Some(x))) => x,
        other => {
            tracing::error!(?other, "could not provision user");
            return page(StatusCode::INTERNAL_SERVER_ERROR, "Sign-in failed", "<p>Clipdeck could not save your account. Try again.</p>");
        }
    };
    if user.role == Role::None {
        tracing::info!(user = %user.email, "sign-in refused: no Clipdeck role assigned");
        return page(
            StatusCode::FORBIDDEN,
            "No access to Clipdeck",
            &format!(
                "<p>You signed in as <b>{}</b>, but your account has no Clipdeck role.</p><p>Ask an administrator to assign you the <b>User</b> or <b>Admin</b> role on the Clipdeck app in Entra ID, then sign in again.</p>{again}",
                esc(&user.email)
            ),
        );
    }
    if user.blocked {
        return page(StatusCode::FORBIDDEN, "Account blocked", "<p>A Clipdeck administrator has blocked your account.</p>");
    }
    tracing::info!(user = %user.email, role = user.role.as_str(), "signed in");
    let secure = oidc.cfg.secure();
    let cookie = format!(
        "{}={token}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}{}",
        cookie_name(secure),
        hours * 3600,
        if secure { "; Secure" } else { "" }
    );
    let mut res = Redirect::to(&p.return_to).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&cookie).unwrap());
    res
}

async fn logout(State(s): State<AuthState>, headers: HeaderMap) -> Response {
    let Auth::Oidc(oidc) = s.auth.as_ref() else { return Redirect::to("/").into_response() };
    let secure = oidc.cfg.secure();
    if let Some(t) = read_cookie(&headers, cookie_name(secure)) {
        let db = s.db.clone();
        let _ = tokio::task::spawn_blocking(move || db.delete_session(&hash_token(&t))).await;
    }
    let clear = format!("{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0{}", cookie_name(secure), if secure { "; Secure" } else { "" });
    // Also end the Microsoft session, then come back to the signed-out page.
    let target = match oidc.discovery().await.ok().and_then(|d| d.end_session_endpoint) {
        Some(end) => match url::Url::parse(&end) {
            Ok(mut u) => {
                u.query_pairs_mut().append_pair("post_logout_redirect_uri", &oidc.cfg.signed_out_uri());
                u.to_string()
            }
            Err(_) => "/auth/signed-out".into(),
        },
        None => "/auth/signed-out".into(),
    };
    let mut res = Redirect::to(&target).into_response();
    res.headers_mut().insert(header::SET_COOKIE, HeaderValue::from_str(&clear).unwrap());
    res
}

async fn signed_out() -> Response {
    page(StatusCode::OK, "You're signed out", "<p>You have left Clipdeck.</p><p><a class=\"btn\" href=\"/auth/login\">Sign in</a></p>")
}

// ------------------------------------------------------------ middleware

/// The signed-in user, available to handlers as `Extension<CurrentUser>`.
#[derive(Clone, Debug)]
pub struct CurrentUser(pub UserRow);

impl CurrentUser {
    pub fn is_admin(&self) -> bool {
        self.0.role == Role::Admin
    }
}

fn unauthorized() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Sign in to continue.", "login": "/auth/login" }))).into_response()
}

/// Guards everything except health checks and the sign-in routes.
pub async fn require_user(State(s): State<AuthState>, mut req: Request, next: Next) -> Response {
    let path = req.uri().path();
    if path == "/healthz" || path.starts_with("/auth/") {
        return next.run(req).await;
    }
    let oidc = match s.auth.as_ref() {
        Auth::Disabled { dev_user } => {
            req.extensions_mut().insert(CurrentUser(dev_user.clone()));
            return next.run(req).await;
        }
        Auth::Oidc(o) => o,
    };
    let is_api = path.starts_with("/api/");
    let user = match read_cookie(req.headers(), cookie_name(oidc.cfg.secure())) {
        Some(t) => {
            let db = s.db.clone();
            tokio::task::spawn_blocking(move || db.session_user(&hash_token(&t))).await.ok().and_then(|r| r.ok()).flatten()
        }
        None => None,
    };
    let Some(user) = user else {
        if is_api || req.method() != Method::GET {
            return unauthorized();
        }
        let ret = req.uri().path_and_query().map(|p| p.as_str()).unwrap_or("/");
        let mut u = url::Url::parse("http://x/auth/login").unwrap();
        u.query_pairs_mut().append_pair("return", ret);
        return Redirect::to(&format!("/auth/login?{}", u.query().unwrap_or(""))).into_response();
    };
    if user.blocked || user.role == Role::None {
        return if is_api {
            (StatusCode::FORBIDDEN, Json(json!({ "error": "Your Clipdeck access was removed." }))).into_response()
        } else {
            page(StatusCode::FORBIDDEN, "No access to Clipdeck", "<p>Your Clipdeck access was removed by an administrator.</p>")
        };
    }
    // Cross-site writes: SameSite=Lax already blocks them; reject a foreign Origin too.
    if !matches!(*req.method(), Method::GET | Method::HEAD | Method::OPTIONS) {
        if let Some(origin) = req.headers().get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
            if origin != oidc.cfg.public_url {
                return (StatusCode::FORBIDDEN, Json(json!({ "error": "Cross-site request refused." }))).into_response();
            }
        }
    }
    req.extensions_mut().insert(CurrentUser(user));
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roles_map_to_access() {
        let r = |v: &[&str]| role_from_claims(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(r(&["User"]), Role::User);
        assert_eq!(r(&["User", "Admin"]), Role::Admin);
        assert_eq!(r(&["admin"]), Role::Admin);
        assert_eq!(r(&[]), Role::None);
        assert_eq!(r(&["Reader"]), Role::None);
    }

    #[test]
    fn return_paths_stay_on_site() {
        assert_eq!(safe_return(Some("/projects?x=1")), "/projects?x=1");
        assert_eq!(safe_return(Some("//evil.example")), "/");
        assert_eq!(safe_return(Some("/\\evil.example")), "/");
        assert_eq!(safe_return(Some("https://evil.example")), "/");
        assert_eq!(safe_return(Some("/auth/logout")), "/");
        assert_eq!(safe_return(None), "/");
    }

    #[test]
    fn cookie_parsing() {
        let mut h = HeaderMap::new();
        h.insert(header::COOKIE, HeaderValue::from_static("a=1; __Host-clipdeck_session=abc; b=2"));
        assert_eq!(read_cookie(&h, "__Host-clipdeck_session").as_deref(), Some("abc"));
        assert_eq!(read_cookie(&h, "clipdeck_session"), None);
    }
}
