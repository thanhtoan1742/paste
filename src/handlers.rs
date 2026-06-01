use axum::{
    extract::{Form, Path, State},
    http::{header, Request, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

use crate::auth::{check_basic_auth, unauthorized_response};
use crate::state::AppState;
use crate::templates;

#[derive(Deserialize)]
struct PasteForm {
    content: String,
    ttl: Option<u64>,
    ttl_custom: Option<String>,
}

async fn home() -> impl IntoResponse {
    axum::response::Html(templates::SUBMIT_PAGE)
}

async fn create_paste(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasteForm>,
) -> impl IntoResponse {
    if form.content.len() > state.config.max_size {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    {
        let pastes = state.pastes.read().await;
        if pastes.len() >= state.config.max_pastes {
            return StatusCode::INSUFFICIENT_STORAGE.into_response();
        }
    }

    let ttl_secs = match form
        .ttl_custom
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&m| m > 0)
    {
        Some(mins) => mins * 60,
        None => match form.ttl {
            Some(mins) if mins > 0 => mins * 60,
            _ => state.config.default_ttl_secs,
        },
    };

    if ttl_secs > state.config.ttl_secs {
        return (
            StatusCode::BAD_REQUEST,
            axum::response::Html(templates::error_page(&format!(
                "TTL exceeds maximum of {} minutes",
                state.config.ttl_secs / 60
            ))),
        )
            .into_response();
    }

    let id = loop {
        let id = nanoid::nanoid!(4);
        if !state.pastes.read().await.contains_key(&id) {
            break id;
        }
    };

    let entry = crate::state::PasteEntry {
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
        return axum::response::Html(templates::NOT_FOUND_PAGE).into_response();
    };

    if Instant::now() > entry.expires_at {
        drop(pastes);
        state.pastes.write().await.remove(&id);
        return axum::response::Html(templates::NOT_FOUND_PAGE).into_response();
    }

    axum::response::Html(templates::view_page(&entry.content)).into_response()
}

async fn admin_page(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
) -> impl IntoResponse {
    let auth = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if check_basic_auth(auth, &state.config.admin_user, &state.config.admin_pass) {
        return render_admin(&state).await;
    }

    unauthorized_response().into_response()
}

async fn render_admin(state: &Arc<AppState>) -> axum::response::Response {
    let pastes = state.pastes.read().await;
    let now = Instant::now();

    let mut rows = String::new();
    for (id, entry) in pastes.iter() {
        let preview = templates::html_escape(&entry.content.chars().take(100).collect::<String>());
        let secs_left = entry.expires_at.duration_since(now).as_secs();
        rows.push_str(&format!(
            "<tr><td><a href=\"/{}\">{}</a></td><td>{}s</td><td>{}...</td></tr>",
            id, id, secs_left, preview
        ));
    }

    axum::response::Html(templates::admin_page(pastes.len(), &rows)).into_response()
}

pub fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(home).post(create_paste))
        .route("/{id}", get(get_paste))
        .route("/admin", get(admin_page))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::state::PasteEntry;
    use axum::body::Body;
    use http::{header, Request};
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState {
            pastes: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            config: Config {
                bind: "127.0.0.1:0".to_string(),
                ttl_secs: 86400,
                default_ttl_secs: 900,
                max_size: 100,
                max_pastes: 2,
                admin_user: "admin".to_string(),
                admin_pass: "secret".to_string(),
            },
        })
    }

    fn test_app() -> Router {
        build_app(test_state())
    }

    fn encode_basic_auth(user: &str, pass: &str) -> String {
        let creds = format!("{}:{}", user, pass);
        format!("Basic {}", base64_encode(creds.as_bytes()))
    }

    fn base64_encode(input: &[u8]) -> String {
        const TABLE: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for chunk in input.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
            let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
            let triple = (b0 << 16) | (b1 << 8) | b2;
            out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
            out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
            out.push(if chunk.len() > 1 {
                TABLE[((triple >> 6) & 0x3F) as usize] as char
            } else {
                '='
            });
            out.push(if chunk.len() > 2 {
                TABLE[(triple & 0x3F) as usize] as char
            } else {
                '='
            });
        }
        out
    }

    #[tokio::test]
    async fn home_returns_submit_form() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<textarea"));
        assert!(html.contains("paste"));
    }

    #[tokio::test]
    async fn create_paste_redirects() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp
            .headers()
            .get(header::LOCATION)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(loc.starts_with('/'));
        assert!(loc.len() == 5);
    }

    #[tokio::test]
    async fn create_paste_rejects_oversized() {
        let app = test_app();
        let big = "x".repeat(200);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from(format!("content={}", big)))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn create_paste_rejects_at_max_pastes() {
        let state = test_state();
        let app = build_app(state.clone());
        for i in 0..2 {
            state.pastes.write().await.insert(
                format!("id{}", i),
                PasteEntry {
                    content: "x".to_string(),
                    expires_at: Instant::now() + std::time::Duration::from_secs(3600),
                },
            );
        }
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);
    }

    #[tokio::test]
    async fn get_paste_returns_content() {
        let state = test_state();
        state.pastes.write().await.insert(
            "testid1".to_string(),
            PasteEntry {
                content: "hello world".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/testid1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("hello world"));
    }

    #[tokio::test]
    async fn get_paste_not_found() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
    }

    #[tokio::test]
    async fn get_paste_expired_returns_404() {
        let state = test_state();
        state.pastes.write().await.insert(
            "expired1".to_string(),
            PasteEntry {
                content: "old".to_string(),
                expires_at: Instant::now() - std::time::Duration::from_secs(1),
            },
        );
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/expired1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
        assert!(state.pastes.read().await.get("expired1").is_none());
    }

    #[tokio::test]
    async fn admin_rejects_no_auth() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www_auth = resp
            .headers()
            .get(header::WWW_AUTHENTICATE)
            .unwrap()
            .to_str()
            .unwrap();
        assert_eq!(www_auth, r#"Basic realm="paste admin""#);
    }

    #[tokio::test]
    async fn admin_rejects_wrong_auth() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header(header::AUTHORIZATION, encode_basic_auth("admin", "wrong"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::WWW_AUTHENTICATE).is_some());
    }

    #[tokio::test]
    async fn admin_shows_pastes_with_correct_auth() {
        let state = test_state();
        state.pastes.write().await.insert(
            "abc12345".to_string(),
            PasteEntry {
                content: "test content".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header(header::AUTHORIZATION, encode_basic_auth("admin", "secret"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(content_type.contains("text/html"));
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("1 pastes"));
        assert!(html.contains("abc12345"));
        assert!(html.contains("test content"));
    }

    #[tokio::test]
    async fn create_paste_with_preset_ttl() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello&ttl=30"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn create_paste_with_custom_ttl() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello&ttl=15&ttl_custom=45"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn create_paste_with_empty_custom_ttl() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello&ttl=30&ttl_custom="))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn create_paste_rejects_ttl_exceeds_max() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=hello&ttl_custom=1500"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("exceeds maximum"));
    }
}
