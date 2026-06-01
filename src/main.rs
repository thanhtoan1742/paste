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
}

async fn home() -> impl IntoResponse {
    axum::response::Html(SUBMIT_PAGE)
}

async fn create_paste(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasteForm>,
) -> impl IntoResponse {
    let max_size = std::env::var("PASTE_MAX_SIZE")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(1_048_576);

    if form.content.len() > max_size {
        return axum::http::StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let id = nanoid::nanoid!(8);
    let ttl_secs = std::env::var("PASTE_TTL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(86400);

    let entry = PasteEntry {
        content: form.content,
        expires_at: Instant::now() + std::time::Duration::from_secs(ttl_secs),
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
    let state = Arc::new(AppState {
        pastes: RwLock::new(HashMap::new()),
    });

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

    let bind = std::env::var("PASTE_BIND").unwrap_or_else(|_| "0.0.0.0:3000".to_string());
    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}
