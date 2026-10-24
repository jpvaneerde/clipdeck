//! End-to-end sign-in tests against a fake Entra ID.
//!
//! The fake identity provider serves OpenID discovery, a JWKS with a freshly
//! generated RSA key, and a token endpoint that checks the client secret,
//! redirect URI and PKCE verifier before issuing a signed ID token. Clipdeck
//! runs on a real socket and is driven over HTTP like a browser would.

use crate::auth::{Auth, Oidc, OidcConfig};
use crate::db::Db;
use axum::extract::{Form, State};
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine as _;
use reqwest::header::{COOKIE, LOCATION, ORIGIN, SET_COOKIE};
use reqwest::StatusCode;
use rsa::pkcs8::EncodePrivateKey;
use rsa::traits::PublicKeyParts;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

const TENANT: &str = "11111111-2222-3333-4444-555555555555";
const CLIENT_ID: &str = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
const SECRET: &str = "s3cret";
const PUBLIC: &str = "http://clipdeck.test";

#[derive(Clone, Default)]
struct Grant {
    nonce: String,
    challenge: String,
    oid: String,
    name: String,
    roles: Vec<String>,
    aud: Option<String>,
    tid: Option<String>,
    foreign_key: bool,
    expired: bool,
}

#[derive(Clone)]
struct Idp {
    base: String,
    key: Arc<rsa::RsaPrivateKey>,
    other_key: Arc<rsa::RsaPrivateKey>,
    grants: Arc<Mutex<HashMap<String, Grant>>>,
}

impl Idp {
    fn issuer(&self) -> String {
        format!("{}/{TENANT}/v2.0", self.base)
    }
}

async fn discovery(State(i): State<Idp>) -> Json<Value> {
    Json(json!({
        "issuer": i.issuer(),
        "authorization_endpoint": format!("{}/authorize", i.base),
        "token_endpoint": format!("{}/token", i.base),
        "jwks_uri": format!("{}/keys", i.base),
        "end_session_endpoint": format!("{}/logout", i.base),
    }))
}

async fn keys(State(i): State<Idp>) -> Json<Value> {
    let p = i.key.to_public_key();
    Json(json!({ "keys": [{
        "kty": "RSA", "use": "sig", "kid": "k1", "alg": "RS256",
        "n": B64.encode(p.n().to_bytes_be()), "e": B64.encode(p.e().to_bytes_be()),
    }]}))
}

async fn token(State(i): State<Idp>, Form(f): Form<HashMap<String, String>>) -> (axum::http::StatusCode, Json<Value>) {
    let bad = |e: &str| (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "error": e, "error_description": e })));
    if f.get("client_secret").map(String::as_str) != Some(SECRET) {
        return bad("invalid_client");
    }
    if f.get("redirect_uri").map(String::as_str) != Some(&format!("{PUBLIC}/auth/callback")) {
        return bad("redirect_uri_mismatch");
    }
    let Some(g) = i.grants.lock().unwrap().remove(f.get("code").map(String::as_str).unwrap_or("")) else {
        return bad("invalid_grant");
    };
    let verifier = f.get("code_verifier").cloned().unwrap_or_default();
    if B64.encode(Sha256::digest(verifier.as_bytes())) != g.challenge {
        return bad("invalid_grant: PKCE verifier does not match");
    }
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    let exp = if g.expired { now - 3600 } else { now + 3600 };
    let claims = json!({
        "iss": i.issuer(), "aud": g.aud.clone().unwrap_or(CLIENT_ID.into()), "sub": format!("sub-{}", g.oid),
        "oid": g.oid, "tid": g.tid.clone().unwrap_or(TENANT.into()), "nonce": g.nonce, "name": g.name,
        "preferred_username": format!("{}@contoso.test", g.name.to_lowercase()), "roles": g.roles,
        "iat": now, "nbf": now, "exp": exp,
    });
    let key = if g.foreign_key { &i.other_key } else { &i.key };
    let pem = key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF).unwrap();
    let mut h = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    h.kid = Some("k1".into());
    let jwt = jsonwebtoken::encode(&h, &claims, &jsonwebtoken::EncodingKey::from_rsa_pem(pem.as_bytes()).unwrap()).unwrap();
    (axum::http::StatusCode::OK, Json(json!({ "id_token": jwt, "token_type": "Bearer" })))
}

struct Harness {
    app: String,
    idp: Idp,
    http: reqwest::Client,
    db: crate::db::Shared,
    _dir: tempdir::Dir,
}

mod tempdir {
    pub struct Dir(pub std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

async fn serve(router: Router) -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(l, router).await.unwrap() });
    format!("http://{addr}")
}

async fn harness() -> Harness {
    let mut rng = rand::thread_rng();
    let key = Arc::new(rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap());
    let other_key = Arc::new(rsa::RsaPrivateKey::new(&mut rng, 2048).unwrap());

    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let idp = Idp { base: base.clone(), key, other_key, grants: Default::default() };
    let r = Router::new()
        .route(&format!("/{TENANT}/v2.0/.well-known/openid-configuration"), get(discovery))
        .route("/keys", get(keys))
        .route("/token", post(token))
        .with_state(idp.clone());
    tokio::spawn(async move { axum::serve(l, r).await.unwrap() });

    let dir = std::env::temp_dir().join(format!("clipdeck-auth-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(dir.join("web")).unwrap();
    std::fs::write(dir.join("web/index.html"), "<!doctype html><title>Clipdeck</title>").unwrap();
    let db = Db::open(&dir.join("t.db"), dir.join("lib"), dir.join("user")).unwrap();
    let cfg = OidcConfig {
        tenant: TENANT.into(),
        client_id: CLIENT_ID.into(),
        client_secret: SECRET.into(),
        authority: format!("{base}/{TENANT}/v2.0"),
        public_url: PUBLIC.into(),
        session_hours: 12,
    };
    let app = serve(crate::build_app(db.clone(), Arc::new(Auth::Oidc(Oidc::new(cfg))), &dir.join("web"))).await;
    let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
    Harness { app, idp, http, db, _dir: tempdir::Dir(dir) }
}

fn query(url: &str) -> HashMap<String, String> {
    url::Url::parse(url).unwrap().query_pairs().into_owned().collect()
}

impl Harness {
    /// Run the sign-in flow. Returns the callback response.
    async fn sign_in(&self, g: Grant, ret: &str) -> reqwest::Response {
        let res = self.http.get(format!("{}/auth/login?return={}", self.app, urlencode(ret))).send().await.unwrap();
        assert!(res.status().is_redirection());
        let loc = res.headers()[LOCATION].to_str().unwrap().to_string();
        assert!(loc.starts_with(&format!("{}/authorize?", self.idp.base)), "{loc}");
        let q = query(&loc);
        assert_eq!(q["client_id"], CLIENT_ID);
        assert_eq!(q["response_type"], "code");
        assert_eq!(q["code_challenge_method"], "S256");
        assert_eq!(q["redirect_uri"], format!("{PUBLIC}/auth/callback"));
        assert!(q["scope"].split(' ').any(|s| s == "openid"));
        let code = format!("code-{}", uuid::Uuid::new_v4());
        let nonce = if g.nonce.is_empty() { q["nonce"].clone() } else { g.nonce.clone() };
        self.idp.grants.lock().unwrap().insert(code.clone(), Grant { nonce, challenge: q["code_challenge"].clone(), ..g });
        self.http.get(format!("{}/auth/callback?code={code}&state={}", self.app, q["state"])).send().await.unwrap()
    }

    async fn session_for(&self, oid: &str, name: &str, roles: &[&str]) -> String {
        let res = self.sign_in(grant(oid, name, roles), "/").await;
        assert!(res.status().is_redirection(), "sign-in failed: {}", res.text().await.unwrap());
        cookie(&res).expect("session cookie")
    }

    async fn get(&self, path: &str, cookie: Option<&str>) -> reqwest::Response {
        let mut r = self.http.get(format!("{}{path}", self.app));
        if let Some(c) = cookie {
            r = r.header(COOKIE, c);
        }
        r.send().await.unwrap()
    }
}

fn urlencode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

fn cookie(res: &reqwest::Response) -> Option<String> {
    res.headers().get(SET_COOKIE).map(|v| v.to_str().unwrap().split(';').next().unwrap().to_string()).filter(|c| !c.ends_with('='))
}

fn grant(oid: &str, name: &str, roles: &[&str]) -> Grant {
    Grant { oid: oid.into(), name: name.into(), roles: roles.iter().map(|s| s.to_string()).collect(), ..Default::default() }
}

#[tokio::test]
async fn sign_in_provisioning_roles_and_ownership() {
    let h = harness().await;

    // Nothing but health and sign-in is reachable without a session.
    assert_eq!(h.get("/healthz", None).await.status(), StatusCode::OK);
    assert_eq!(h.get("/api/samples", None).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(h.get("/api/audio/x", None).await.status(), StatusCode::UNAUTHORIZED);
    let res = h.get("/some/page?x=1", None).await;
    assert!(res.status().is_redirection());
    assert_eq!(res.headers()[LOCATION], "/auth/login?return=%2Fsome%2Fpage%3Fx%3D1");
    let res = h.http.post(format!("{}/api/projects", h.app)).body("{}").send().await.unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // An Admin signs in and lands where they were going.
    let res = h.sign_in(grant("oid-ann", "Ann", &["Admin"]), "/studio?p=1").await;
    assert!(res.status().is_redirection());
    assert_eq!(res.headers()[LOCATION], "/studio?p=1");
    let set = res.headers()[SET_COOKIE].to_str().unwrap().to_string();
    assert!(set.contains("HttpOnly") && set.contains("SameSite=Lax"), "{set}");
    let ann = cookie(&res).unwrap();
    let me: Value = h.get("/api/me", Some(&ann)).await.json().await.unwrap();
    assert_eq!((me["name"].as_str(), me["role"].as_str()), (Some("Ann"), Some("admin")));
    assert_eq!(h.get("/", Some(&ann)).await.status(), StatusCode::OK);

    // A User is provisioned too, but admin routes refuse them.
    let bo = h.session_for("oid-bo", "Bo", &["User"]).await;
    let me: Value = h.get("/api/me", Some(&bo)).await.json().await.unwrap();
    assert_eq!(me["role"], "user");
    assert_eq!(h.get("/api/admin/users", Some(&bo)).await.status(), StatusCode::FORBIDDEN);
    let res = h.http.post(format!("{}/api/rescan", h.app)).header(COOKIE, &bo).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let users: Value = h.get("/api/admin/users", Some(&ann)).await.json().await.unwrap();
    let names: Vec<&str> = users.as_array().unwrap().iter().map(|u| u["name"].as_str().unwrap()).collect();
    assert_eq!(names.len(), 2);
    assert!(names.contains(&"Ann") && names.contains(&"Bo"));

    // Nobody without a role gets in or gets provisioned.
    let res = h.sign_in(grant("oid-cy", "Cy", &[]), "/").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(cookie(&res).is_none());
    assert!(res.text().await.unwrap().contains("no Clipdeck role"));
    assert_eq!(h.db.list_users().unwrap().len(), 2);

    // Projects belong to their creator.
    let project = json!({ "name": "Bo's song", "bpm": 120, "tracks": [] });
    let res = h.http.post(format!("{}/api/projects", h.app)).header(COOKIE, &bo).json(&project).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let id = res.json::<Value>().await.unwrap()["id"].as_str().unwrap().to_string();
    let ann2 = h.session_for("oid-ann2", "Ann Two", &["User"]).await;
    assert_eq!(h.get(&format!("/api/projects/{id}"), Some(&ann2)).await.status(), StatusCode::NOT_FOUND);
    let res = h.http.put(format!("{}/api/projects/{id}", h.app)).header(COOKIE, &ann2).json(&project).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
    assert_eq!(h.get(&format!("/api/projects/{id}"), Some(&bo)).await.status(), StatusCode::OK);
    assert_eq!(h.get(&format!("/api/projects/{id}"), Some(&ann)).await.status(), StatusCode::OK);
    let mine: Value = h.get("/api/projects", Some(&ann2)).await.json().await.unwrap();
    assert_eq!(mine.as_array().unwrap().len(), 0);

    // Cross-site writes are refused even with a valid session.
    let res = h
        .http
        .put(format!("{}/api/projects/{id}", h.app))
        .header(COOKIE, &bo)
        .header(ORIGIN, "https://evil.example")
        .json(&project)
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    let res =
        h.http.put(format!("{}/api/projects/{id}", h.app)).header(COOKIE, &bo).header(ORIGIN, PUBLIC).json(&project).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    // An admin blocks Bo: his session ends at once and he can't sign in again.
    let bo_id = h.db.list_users().unwrap().iter().find(|u| u.user.name == "Bo").unwrap().user.id;
    let res = h
        .http
        .patch(format!("{}/api/admin/users/{bo_id}", h.app))
        .header(COOKIE, &ann)
        .json(&json!({ "blocked": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
    assert_eq!(h.get("/api/me", Some(&bo)).await.status(), StatusCode::UNAUTHORIZED);
    let res = h.sign_in(grant("oid-bo", "Bo", &["User"]), "/").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Removing the role in Entra takes effect at the next sign-in.
    let res = h.sign_in(grant("oid-ann2", "Ann Two", &[]), "/").await;
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(h.get("/api/me", Some(&ann2)).await.status(), StatusCode::UNAUTHORIZED);

    // Sign-out ends the session and hands over to Entra's sign-out.
    let res = h.http.post(format!("{}/auth/logout", h.app)).header(COOKIE, &ann).send().await.unwrap();
    assert!(res.status().is_redirection());
    let loc = res.headers()[LOCATION].to_str().unwrap().to_string();
    assert!(loc.starts_with(&format!("{}/logout?", h.idp.base)));
    assert_eq!(query(&loc)["post_logout_redirect_uri"], format!("{PUBLIC}/auth/signed-out"));
    assert_eq!(h.get("/api/me", Some(&ann)).await.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn rejects_bad_tokens_and_replays() {
    let h = harness().await;
    let cases: Vec<(&str, Grant)> = vec![
        ("wrong nonce", Grant { nonce: "not-the-nonce".into(), ..grant("o1", "N", &["User"]) }),
        ("wrong audience", Grant { aud: Some("someone-else".into()), ..grant("o2", "A", &["User"]) }),
        ("other tenant", Grant { tid: Some("99999999-2222-3333-4444-555555555555".into()), ..grant("o3", "T", &["User"]) }),
        ("forged signature", Grant { foreign_key: true, ..grant("o4", "F", &["Admin"]) }),
        ("expired", Grant { expired: true, ..grant("o5", "E", &["User"]) }),
    ];
    for (what, g) in cases {
        let res = h.sign_in(g, "/").await;
        assert_eq!(res.status(), StatusCode::FORBIDDEN, "{what} was accepted");
        assert!(cookie(&res).is_none(), "{what} set a cookie");
    }
    assert_eq!(h.db.list_users().unwrap().len(), 0);

    // A state value works once.
    let res = h.http.get(format!("{}/auth/login", h.app)).send().await.unwrap();
    let q = query(res.headers()[LOCATION].to_str().unwrap());
    let g = Grant { nonce: q["nonce"].clone(), challenge: q["code_challenge"].clone(), ..grant("o6", "R", &["User"]) };
    h.idp.grants.lock().unwrap().insert("c1".into(), g);
    let ok = h.http.get(format!("{}/auth/callback?code=c1&state={}", h.app, q["state"])).send().await.unwrap();
    assert!(ok.status().is_redirection());
    let again = h.http.get(format!("{}/auth/callback?code=c1&state={}", h.app, q["state"])).send().await.unwrap();
    assert_eq!(again.status(), StatusCode::BAD_REQUEST);

    // A wrong PKCE verifier can't redeem the code: the state's verifier is tied to its challenge.
    let res = h.http.get(format!("{}/auth/login", h.app)).send().await.unwrap();
    let q = query(res.headers()[LOCATION].to_str().unwrap());
    let g = Grant { nonce: q["nonce"].clone(), challenge: "not-the-challenge".into(), ..grant("o7", "P", &["User"]) };
    h.idp.grants.lock().unwrap().insert("c2".into(), g);
    let res = h.http.get(format!("{}/auth/callback?code=c2&state={}", h.app, q["state"])).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // Errors from Entra (e.g. the user cancelled) show a page, not a crash.
    let res = h.http.get(format!("{}/auth/callback?error=access_denied&error_description=User+cancelled", h.app)).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert!(res.text().await.unwrap().contains("User cancelled"));

    // Off-site return paths are ignored.
    let res = h.sign_in(grant("o8", "Z", &["User"]), "//evil.example/x").await;
    assert_eq!(res.headers()[LOCATION], "/");
}
