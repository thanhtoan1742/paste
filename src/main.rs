use axum::{routing::get, Router, extract::{State, Path, Form}, response::IntoResponse};
use serde::Deserialize;
use std::sync::Arc;
use tokio::sync::RwLock;
use std::collections::HashMap;
use std::time::Instant;

#[derive(Deserialize)]
struct PasteForm {
    content: String,
}

#[derive(Deserialize)]
struct Config {
    #[serde(default = "default_bind")]
    bind: String,
    #[serde(default = "default_ttl_secs")]
    ttl_secs: u64,
    #[serde(default = "default_max_size")]
    max_size: usize,
    #[serde(default = "default_max_pastes")]
    max_pastes: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind: default_bind(),
            ttl_secs: default_ttl_secs(),
            max_size: default_max_size(),
            max_pastes: default_max_pastes(),
        }
    }
}

fn default_bind() -> String { "0.0.0.0:3000".to_string() }
fn default_ttl_secs() -> u64 { 3600 }
fn default_max_size() -> usize { 8_388_608 }
fn default_max_pastes() -> usize { 512 }

const SUBMIT_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body>
<form method="POST" action="/">
<textarea name="content" rows="20" cols="80"></textarea><br>
<input type="submit" value="paste">
</form>
</body></html>"#;

const VIEW_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body><pre>{}</pre></body></html>"#;

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

    let html = VIEW_PAGE.replace("{}", &html_escape(&entry.content));
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

    let app = Router::new()
        .route("/", get(home).post(create_paste))
        .route("/{id}", get(get_paste))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}
