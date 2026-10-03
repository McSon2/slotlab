//! Shared access code in front of a public share.
//!
//! One code for everyone, typed once and remembered in a cookie: the point
//! is to keep a game that is not published yet out of reach of whoever
//! stumbles on the URL, not to identify visitors.

use axum::body::Body;
use axum::extract::{ConnectInfo, Form, Extension};
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const COOKIE: &str = "slotlab_access";
/// A month: the client types the code once for the whole evaluation.
const COOKIE_MAX_AGE: u64 = 30 * 24 * 3600;
/// Wrong tries tolerated per IP before it is turned away for a while.
const MAX_FAILURES: u32 = 10;
const FAILURE_WINDOW: Duration = Duration::from_secs(600);

pub struct Gate {
    /// Cookie value proving the code was typed. Derived from the code, so
    /// changing the code invalidates every cookie already handed out.
    token: String,
    code: String,
    failures: Mutex<HashMap<String, (u32, Instant)>>,
}

impl Gate {
    pub fn new(code: &str) -> Self {
        Self {
            token: format!("{:032x}", xxhash_rust::xxh3::xxh3_128(code.as_bytes())),
            code: code.to_string(),
            failures: Mutex::new(HashMap::new()),
        }
    }

    /// Gate that lets everyone through: a game published without a code.
    pub fn open() -> Self {
        Self::new("")
    }

    pub fn is_open(&self) -> bool {
        self.code.is_empty()
    }

    /// Ce que l'adresse porte une fois la porte franchie (`/k/<token>/…`) :
    /// dérivé du code, jamais le code lui-même.
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Clé lue dans l'adresse : le code (le lien d'entrée) ou le jeton (tout
    /// ce que la page charge ensuite).
    fn accepts_key(&self, key: &str) -> bool {
        key == self.token || key.trim() == self.code
    }

    fn unlocked(&self, headers: &HeaderMap) -> bool {
        if self.is_open() {
            return true;
        }
        headers
            .get(header::COOKIE)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|raw| {
                raw.split(';').any(|c| {
                    c.trim()
                        .strip_prefix(&format!("{COOKIE}="))
                        .is_some_and(|v| v == self.token)
                })
            })
    }

    /// Registers a wrong try; true once the caller must be turned away.
    fn note_failure(&self, ip: &str) -> bool {
        let mut map = self.failures.lock().unwrap();
        let now = Instant::now();
        map.retain(|_, (_, seen)| now.duration_since(*seen) < FAILURE_WINDOW);
        let entry = map.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= FAILURE_WINDOW {
            *entry = (0, now);
        }
        entry.0 += 1;
        entry.1 = now;
        entry.0 > MAX_FAILURES
    }

    fn locked_out(&self, ip: &str) -> bool {
        let map = self.failures.lock().unwrap();
        map.get(ip)
            .is_some_and(|(n, seen)| *n > MAX_FAILURES && seen.elapsed() < FAILURE_WINDOW)
    }
}

/// Visitor's address: behind the tunnel the socket is Cloudflare's, the
/// real one arrives in a header.
fn client_ip(headers: &HeaderMap, peer: Option<SocketAddr>) -> String {
    for h in ["cf-connecting-ip", "x-forwarded-for"] {
        if let Some(v) = headers.get(h).and_then(|v| v.to_str().ok()) {
            if let Some(first) = v.split(',').next() {
                return first.trim().to_string();
            }
        }
    }
    peer.map(|a| a.ip().to_string()).unwrap_or_default()
}

/// Clé d'accès lue dans l'adresse (`/k/<clé>/…`) par [`strip_key`].
#[derive(Clone)]
pub struct PathKey(pub String);

/// Retire `/k/<clé>` du début du chemin et garde la clé pour la porte.
///
/// C'est ce qui permet de jouer dans une iframe (l'onglet Play de Reelhouse) :
/// là, le navigateur n'envoie pas le cookie de la porte — il est posé par un
/// autre site que la page qui l'affiche. La clé, elle, voyage dans l'adresse,
/// et les builds du jeu chargent tout en relatif (`./_app/…`, et le RGS via
/// `rgs_url`), donc chaque requête de la page la porte à son tour.
///
/// Posé AUTOUR du routeur, pas avec `Router::layer` : ce dernier s'exécute
/// après le routage, trop tard pour réécrire le chemin.
pub async fn strip_key(mut req: Request<Body>, next: Next) -> Response {
    let Some(rest) = req.uri().path().strip_prefix("/k/") else {
        return next.run(req).await;
    };
    let (key, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let key = percent_decode(key);
    let pq = match req.uri().query() {
        Some(q) => format!("/{tail}?{q}"),
        None => format!("/{tail}"),
    };
    let Ok(pq) = pq.parse::<axum::http::uri::PathAndQuery>() else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let mut parts = req.uri().clone().into_parts();
    parts.path_and_query = Some(pq);
    let Ok(uri) = axum::http::Uri::from_parts(parts) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    *req.uri_mut() = uri;
    req.extensions_mut().insert(PathKey(key));
    next.run(req).await
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Lets `/gate` and `/healthz` through, sends everything else to the form
/// until the cookie is there.
pub async fn middleware(req: Request<Body>, next: Next) -> Response {
    // No gate in the extensions = nothing to protect (admin routes, or a
    // game published without a code).
    let Some(gate) = req.extensions().get::<Arc<Gate>>().cloned() else {
        return next.run(req).await;
    };
    let path = req.uri().path().to_string();
    if path == "/gate" || path == "/healthz" || gate.unlocked(req.headers()) {
        return next.run(req).await;
    }
    // Clé dans l'adresse : même limite d'essais que le formulaire, sinon
    // l'adresse deviendrait le moyen de deviner le code.
    if let Some(PathKey(key)) = req.extensions().get::<PathKey>().cloned() {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0);
        let ip = client_ip(req.headers(), peer);
        if gate.locked_out(&ip) {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        }
        if gate.accepts_key(&key) {
            return next.run(req).await;
        }
        gate.note_failure(&ip);
        tracing::warn!("wrong access key in the address from {ip}");
        if path.starts_with("/api/") {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "status": { "statusCode": "ERR_ATE", "statusMessage": "access code rejected" }
                })),
            )
                .into_response();
        }
        // Pas de formulaire ici : dans une iframe, le cookie qu'il poserait ne
        // reviendrait jamais, et l'on tournerait en rond.
        return (
            StatusCode::UNAUTHORIZED,
            Html(notice(
                "Code refusé",
                "Le code d'accès de ce lien n'est plus le bon. Mets-le à jour là où le lien a été créé.",
            )),
        )
            .into_response();
    }
    // The game's own calls must not be answered with an HTML form.
    if path.starts_with("/api/") {
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "status": { "statusCode": "ERR_ATE", "statusMessage": "access code required" }
            })),
        )
            .into_response();
    }
    let next_url = req
        .uri()
        .path_and_query()
        .map(|p| p.as_str())
        .unwrap_or("/play");
    Redirect::to(&format!("/gate?next={}", urlencode(next_url))).into_response()
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[derive(Deserialize)]
pub struct GateQuery {
    #[serde(default)]
    next: Option<String>,
}

#[derive(Deserialize)]
pub struct GateForm {
    code: String,
    #[serde(default)]
    next: Option<String>,
}

pub async fn show(axum::extract::Query(q): axum::extract::Query<GateQuery>) -> Html<String> {
    Html(page(q.next.as_deref().unwrap_or("/play"), None))
}

pub async fn submit(
    Extension(gate): Extension<Arc<Gate>>,
    headers: HeaderMap,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    Form(form): Form<GateForm>,
) -> Response {
    let ip = client_ip(&headers, Some(peer));
    let next = form.next.as_deref().unwrap_or("/play").to_string();
    // Only relative paths: a `next` pointing elsewhere would turn this page
    // into an open redirect.
    let next = if next.starts_with('/') && !next.starts_with("//") {
        next
    } else {
        "/play".into()
    };
    if gate.locked_out(&ip) {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Html(page(&next, Some("Trop de tentatives. Réessaie dans 10 minutes."))),
        )
            .into_response();
    }
    if form.code.trim() != gate.code {
        gate.note_failure(&ip);
        tracing::warn!("wrong access code from {ip}");
        return (
            StatusCode::UNAUTHORIZED,
            Html(page(&next, Some("Code incorrect."))),
        )
            .into_response();
    }
    let secure = headers
        .get("x-forwarded-proto")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|p| p.eq_ignore_ascii_case("https"));
    let cookie = format!(
        "{COOKIE}={}; Path=/; Max-Age={COOKIE_MAX_AGE}; HttpOnly; SameSite=Lax{}",
        gate.token,
        if secure { "; Secure" } else { "" }
    );
    ([(header::SET_COOKIE, cookie)], Redirect::to(&next)).into_response()
}

/// Page sans formulaire, dans le style de la porte.
fn notice(title: &str, text: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="fr"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>{title}</title>
<style>
  :root {{ color-scheme: dark; }}
  body {{
    margin: 0; min-height: 100svh; display: grid; place-items: center;
    background: #0d0b0f; color: #e8e2d9; padding: 24px;
    font: 16px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  }}
  .card {{
    max-width: 380px; background: #17141a; border: 1px solid #2c2731;
    border-radius: 14px; padding: 28px;
  }}
  h1 {{ margin: 0 0 6px; font-size: 19px; }}
  p {{ margin: 0; color: #8d8593; font-size: 14px; }}
</style>
</head><body><div class="card"><h1>{title}</h1><p>{text}</p></div></body></html>"##
    )
}

fn page(next: &str, error: Option<&str>) -> String {
    let err = error
        .map(|e| format!(r#"<p class="err">{e}</p>"#))
        .unwrap_or_default();
    format!(
        r##"<!doctype html>
<html lang="fr"><head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<meta name="robots" content="noindex,nofollow">
<title>Accès réservé</title>
<style>
  :root {{ color-scheme: dark; }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0; min-height: 100svh; display: grid; place-items: center;
    background: #0d0b0f; color: #e8e2d9; padding: 24px;
    font: 16px/1.5 ui-sans-serif, system-ui, -apple-system, "Segoe UI", sans-serif;
  }}
  .card {{
    width: 100%; max-width: 380px; background: #17141a;
    border: 1px solid #2c2731; border-radius: 14px; padding: 32px 28px;
    box-shadow: 0 18px 50px rgb(0 0 0 / 0.45);
  }}
  h1 {{ margin: 0 0 6px; font-size: 19px; letter-spacing: .01em; }}
  p.sub {{ margin: 0 0 22px; color: #8d8593; font-size: 14px; }}
  label {{ display: block; font-size: 12px; text-transform: uppercase;
           letter-spacing: .09em; color: #8d8593; margin-bottom: 8px; }}
  input {{
    width: 100%; padding: 13px 14px; font-size: 19px; letter-spacing: .22em;
    text-align: center; color: #e8e2d9; background: #100e13;
    border: 1px solid #35303b; border-radius: 9px; outline: none;
  }}
  input:focus {{ border-color: #b8892f; }}
  button {{
    width: 100%; margin-top: 16px; padding: 13px; font-size: 15px;
    font-weight: 600; color: #150f05; background: #d2a03c;
    border: 0; border-radius: 9px; cursor: pointer;
  }}
  button:hover {{ background: #e0ae47; }}
  .err {{ margin: 14px 0 0; color: #e2757c; font-size: 14px; text-align: center; }}
</style>
</head><body>
  <form class="card" method="post" action="/gate">
    <h1>Accès réservé</h1>
    <p class="sub">Ce jeu est en cours d'évaluation. Saisis le code qui t'a été communiqué.</p>
    <label for="code">Code d'accès</label>
    <input id="code" name="code" autocomplete="off" autocapitalize="off"
           autocorrect="off" spellcheck="false" autofocus>
    <input type="hidden" name="next" value="{next}">
    <button type="submit">Entrer</button>
    {err}
  </form>
</body></html>"##
    )
}
