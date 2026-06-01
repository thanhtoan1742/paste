use axum::{routing::get, Router, extract::{State, Path, Form}, response::IntoResponse, middleware::{self, Next}, http::{StatusCode, header, Request}};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Deserialize)]
struct PasteForm {
    content: String,
}

#[derive(Deserialize, Clone)]
struct Config {
    #[serde(default = "default_bind")]
    bind: String,
    #[serde(default = "default_ttl_secs")]
    ttl_secs: u64,
    #[serde(default = "default_max_size")]
    max_size: usize,
    #[serde(default = "default_max_pastes")]
    max_pastes: usize,
    #[serde(default = "default_admin_user")]
    admin_user: String,
    #[serde(default = "default_admin_pass")]
    admin_pass: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            ttl_secs: default_ttl_secs(),
            max_size: default_max_size(),
            max_pastes: default_max_pastes(),
            admin_user: default_admin_user(),
            admin_pass: default_admin_pass(),
        }
    }
}

fn default_bind() -> String { "0.0.0.0:3000".to_string() }
fn default_ttl_secs() -> u64 { 3600 }
fn default_max_size() -> usize { 8_388_608 }
fn default_max_pastes() -> usize { 512 }
fn default_admin_user() -> String { "admin".to_string() }
fn default_admin_pass() -> String { "admin".to_string() }

const SUBMIT_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body>
<form method="POST" action="/">
<textarea name="content" rows="20" cols="80"></textarea><br>
<input type="submit" value="paste">
</form>
</body></html>"#;

const NOT_FOUND_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body><p>paste not found or expired</p></body></html>"#;

struct PasteEntry {
    content: String,
    expires_at: Instant,
}

struct AppState {
    pastes: RwLock<HashMap<String, PasteEntry>>,
    config: Config,
}

async fn basic_auth(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> impl IntoResponse {
    let auth = req.headers().get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if let Some(encoded) = auth.strip_prefix("Basic ") {
        if let Ok(decoded) = base64_decode(encoded) {
            if let Ok(creds) = std::str::from_utf8(&decoded) {
                if let Some((user, pass)) = creds.split_once(':') {
                    if user == state.config.admin_user && pass == state.config.admin_pass {
                        return Ok(next.run(req).await);
                    }
                }
            }
        }
    }

    Err((
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, r#"Basic realm="paste admin""#)],
    ))
}

fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const T: [i8; 128] = [
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,
        -1,-1,-1,-1,-1,-1,-1,-1,-1,-1,-1,62,-1,-1,-1,63,
        52,53,54,55,56,57,58,59,60,61,-1,-1,-1,-1,-1,-1,
        -1, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9,10,11,12,13,14,
        15,16,17,18,19,20,21,22,23,24,25,-1,-1,-1,-1,-1,
        -1,26,27,28,29,30,31,32,33,34,35,36,37,38,39,40,
        41,42,43,44,45,46,47,48,49,50,51,-1,-1,-1,-1,-1,
    ];

    let bytes: Vec<u8> = input.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    if bytes.len() % 4 != 0 { return Err(()); }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let v: Vec<i8> = chunk.iter().map(|&b| T.get(b as usize).copied().unwrap_or(-1)).collect();
        if v.iter().any(|&x| x < 0) { return Err(()); }
        out.push(((v[0] as u8) << 2) | ((v[1] as u8) >> 4));
        if chunk[1] != b'=' { out.push(((v[1] as u8 & 0xf) << 4) | ((v[2] as u8) >> 2)); }
        if chunk[2] != b'=' { out.push(((v[2] as u8 & 0x3) << 6) | (v[3] as u8)); }
    }
    Ok(out)
}

async fn home() -> impl IntoResponse {
    axum::response::Html(SUBMIT_PAGE)
}

async fn create_paste(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasteForm>,
) -> impl IntoResponse {
    if form.content.len() > state.config.max_size {
        return axum::http::StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    {
        let pastes = state.pastes.read().await;
        if pastes.len() >= state.config.max_pastes {
            return axum::http::StatusCode::INSUFFICIENT_STORAGE.into_response();
        }
    }

    let id = nanoid::nanoid!(8);

    let entry = PasteEntry {
        content: form.content,
        expires_at: Instant::now() + std::time::Duration::from_secs(state.config.ttl_secs),
    };

    state.pastes.write().await.insert(id.clone(), entry);

    axum::response::Redirect::to(&format!("/{}", id)).into_response()
}

async fn get_paste(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pastes = state.pastes.read().await;
    let Some(entry) = pastes.get(&id) else {
        return axum::response::Html(NOT_FOUND_PAGE).into_response();
    };

    if Instant::now() > entry.expires_at {
        drop(pastes);
        state.pastes.write().await.remove(&id);
        return axum::response::Html(NOT_FOUND_PAGE).into_response();
    }

    let html = format!(r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body><pre>{}</pre></body></html>"#, html_escape(&entry.content));
    axum::response::Html(html).into_response()
}

async fn admin_page(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let pastes = state.pastes.read().await;
    let now = Instant::now();

    let mut rows = String::new();
    for (id, entry) in pastes.iter() {
        let preview = html_escape(&entry.content.chars().take(100).collect::<String>());
        let secs_left = entry.expires_at.duration_since(now).as_secs();
        rows.push_str(&format!(
            "<tr><td><a href=\"/{}\">{}</a></td><td>{}s</td><td>{}...</td></tr>",
            id, id, secs_left, preview
        ));
    }

    let html = format!(r#"<!DOCTYPE html>
<html><head><title>paste admin</title></head>
<body>
<h1>{} pastes</h1>
<table>
<tr><th>id</th><th>expires in</th><th>preview</th></tr>
{}
</table>
</body></html>"#, pastes.len(), rows);

    axum::response::Html(html).into_response()
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
}

#[tokio::main]
async fn main() {
    let config: Config = std::fs::read_to_string("paste.toml")
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();

    let state = Arc::new(AppState {
        pastes: RwLock::new(HashMap::new()),
        config,
    });

    let bind = state.config.bind.clone();

    tokio::spawn({
        let state = state.clone();
        async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                state.pastes.write().await.retain(|_, entry| Instant::now() < entry.expires_at);
            }
        }
    });

    let admin = Router::new()
        .route("/admin", get(admin_page))
        .layer(middleware::from_fn_with_state(state.clone(), basic_auth))
        .with_state(state.clone());

    let app = Router::new()
        .route("/", get(home).post(create_paste))
        .route("/{id}", get(get_paste))
        .merge(admin)
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}
