use axum::{
    body::Body,
    extract::{Form, Path, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;

use crate::auth::{check_basic_auth, unauthorized_response};
use crate::state::AppState;
use crate::templates;

pub const SWEEPER_INTERVAL_SECS: u64 = 60;

#[derive(Deserialize)]
struct PasteForm {
    content: String,
    ttl: Option<u64>,
    ttl_custom: Option<String>,
}

fn format_duration(secs: u64) -> String {
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    let mut parts = Vec::new();
    if days > 0 {
        parts.push(format!("{}d", days));
    }
    if hours > 0 {
        parts.push(format!("{}h", hours));
    }
    if minutes > 0 {
        parts.push(format!("{}m", minutes));
    }
    if seconds > 0 || parts.is_empty() {
        parts.push(format!("{}s", seconds));
    }
    parts.join(" ")
}

async fn lockdown_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request<Body>,
    next: Next,
) -> Response {
    if !state.config.lockdown {
        return next.run(req).await;
    }
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if check_basic_auth(auth, &state.config.user, &state.config.password) {
        next.run(req).await
    } else {
        unauthorized_response().into_response()
    }
}

async fn strip_trailing_slash(req: Request<Body>, next: Next) -> Response {
    let path = req.uri().path();
    if path != "/" && path.ends_with('/') {
        let mut new_path = path.to_string();
        while new_path.len() > 1 && new_path.ends_with('/') {
            new_path.pop();
        }
        let location = match req.uri().query() {
            Some(q) => format!("{}?{}", new_path, q),
            None => new_path,
        };
        return axum::response::Redirect::permanent(&location).into_response();
    }
    next.run(req).await
}

async fn security_headers(req: Request<Body>, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        "max-age=63072000; includeSubDomains".parse().unwrap(),
    );
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());
    response
}

async fn home(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    axum::response::Html(templates::submit_page(&state.config.prefix))
}

async fn create_paste(
    State(state): State<Arc<AppState>>,
    Form(form): Form<PasteForm>,
) -> impl IntoResponse {
    if form.content.len() > state.config.max_size {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let prefix = state.config.prefix.clone();

    let ttl_secs = match form
        .ttl_custom
        .as_deref()
        .and_then(|s| s.parse::<u64>().ok())
        .filter(|&m| m > 0)
    {
        Some(mins) => mins * 60,
        None => match form.ttl {
            Some(mins) if mins > 0 => mins * 60,
            _ => state.config.default_ttl_mins * 60,
        },
    };

    if ttl_secs > state.config.max_ttl_secs {
        return (
            StatusCode::BAD_REQUEST,
            axum::response::Html(templates::error_page(
                &state.config.prefix,
                &format!(
                    "TTL exceeds maximum of {} minutes",
                    state.config.max_ttl_secs / 60
                ),
            )),
        )
            .into_response();
    }

    let mut pastes = state.pastes.write().await;

    if pastes.len() >= state.config.max_pastes {
        return StatusCode::INSUFFICIENT_STORAGE.into_response();
    }

    let id = loop {
        let id = nanoid::nanoid!(4);
        if !pastes.contains_key(&id) {
            break id;
        }
    };

    let entry = crate::state::PasteEntry {
        content: form.content,
        expires_at: Instant::now() + std::time::Duration::from_secs(ttl_secs),
    };

    pastes.insert(id.clone(), entry);

    axum::response::Redirect::to(&format!("{}/{}", prefix, id)).into_response()
}

async fn get_paste(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pastes = state.pastes.read().await;
    let Some(entry) = pastes.get(&id) else {
        return (
            StatusCode::NOT_FOUND,
            axum::response::Html(templates::not_found_page()),
        )
            .into_response();
    };

    if Instant::now() > entry.expires_at {
        drop(pastes);
        state.pastes.write().await.remove(&id);
        return (
            StatusCode::GONE,
            axum::response::Html(templates::not_found_page()),
        )
            .into_response();
    }

    axum::response::Html(templates::view_page(&entry.content)).into_response()
}

async fn admin_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if check_basic_auth(auth, &state.config.user, &state.config.password) {
        return render_admin(&state).await;
    }

    unauthorized_response().into_response()
}

async fn delete_paste(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if !check_basic_auth(auth, &state.config.user, &state.config.password) {
        return unauthorized_response().into_response();
    }

    state.pastes.write().await.remove(&id);

    let admin_path = if state.config.prefix.is_empty() {
        "/admin".to_string()
    } else {
        format!("{}/admin", state.config.prefix)
    };
    axum::response::Redirect::to(&admin_path).into_response()
}

async fn render_admin(state: &Arc<AppState>) -> axum::response::Response {
    let pastes = state.pastes.read().await;
    let now = Instant::now();

    let mut rows = String::new();
    let prefix = &state.config.prefix;
    for (id, entry) in pastes.iter() {
        let escaped_id = templates::html_escape(id);
        let preview = templates::html_escape(&entry.content.chars().take(100).collect::<String>());
        let secs_left = entry.expires_at.duration_since(now).as_secs();
        let human = format_duration(secs_left);
        rows.push_str(&format!(
            "<tr><td><a href=\"{}/{}\">{}</a></td><td>{}</td><td>{}...</td><td><form method=\"POST\" action=\"{}/admin/{}/delete\"><button type=\"submit\">delete</button></form></td></tr>",
            prefix, escaped_id, escaped_id, human, preview, prefix, escaped_id
        ));
    }

    axum::response::Html(templates::admin_page(pastes.len(), &rows)).into_response()
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let body_limit = state.config.max_size + 4096;
    let prefix = state.config.prefix.clone();
    let state_for_lockdown = state.clone();

    let inner = Router::new()
        .route("/", get(home).post(create_paste))
        .route("/{id}", get(get_paste))
        .route("/admin", get(admin_page))
        .route("/admin/{id}/delete", post(delete_paste))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(
            state_for_lockdown,
            lockdown_auth,
        ))
        .layer(middleware::from_fn(security_headers));

    let router = if prefix.is_empty() {
        inner
    } else {
        Router::new().nest(&prefix, inner)
    };

    router.layer(middleware::from_fn(strip_trailing_slash))
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
                prefix: String::new(),
                max_ttl_secs: 86400,
                default_ttl_mins: 15,
                max_size: 100,
                max_pastes: 2,
                lockdown: false,
                user: "user".to_string(),
                password: "secret".to_string(),
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
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
    }

    #[tokio::test]
    async fn get_paste_expired_returns_gone() {
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
        assert_eq!(resp.status(), StatusCode::GONE);
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
                    .header(header::AUTHORIZATION, encode_basic_auth("user", "wrong"))
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
                    .header(header::AUTHORIZATION, encode_basic_auth("user", "secret"))
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
        assert!(html.contains("actions"));
    }

    #[tokio::test]
    async fn delete_paste_removes_entry() {
        let state = test_state();
        state.pastes.write().await.insert(
            "del1".to_string(),
            PasteEntry {
                content: "to be deleted".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/del1/delete")
                    .header(header::AUTHORIZATION, encode_basic_auth("user", "secret"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
            "/admin"
        );
        assert!(state.pastes.read().await.get("del1").is_none());
    }

    #[tokio::test]
    async fn delete_paste_rejects_no_auth() {
        let state = test_state();
        state.pastes.write().await.insert(
            "del2".to_string(),
            PasteEntry {
                content: "still here".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/admin/del2/delete")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(state.pastes.read().await.get("del2").is_some());
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

    fn prefixed_state() -> Arc<AppState> {
        Arc::new(AppState {
            pastes: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            config: Config {
                bind: "127.0.0.1:0".to_string(),
                prefix: "/paste".to_string(),
                max_ttl_secs: 86400,
                default_ttl_mins: 15,
                max_size: 100,
                max_pastes: 2,
                lockdown: false,
                user: "user".to_string(),
                password: "secret".to_string(),
            },
        })
    }

    #[tokio::test]
    async fn prefixed_routes_work() {
        let state = prefixed_state();
        state.pastes.write().await.insert(
            "ab01".to_string(),
            PasteEntry {
                content: "prefixed content".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/paste")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("action=\"/paste\""));

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/paste/ab01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("prefixed content"));

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/paste")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("content=new"))
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
        assert!(loc.starts_with("/paste/"));
    }

    fn lockdown_state() -> Arc<AppState> {
        Arc::new(AppState {
            pastes: tokio::sync::RwLock::new(std::collections::HashMap::new()),
            config: Config {
                bind: "127.0.0.1:0".to_string(),
                prefix: String::new(),
                max_ttl_secs: 86400,
                default_ttl_mins: 15,
                max_size: 100,
                max_pastes: 2,
                lockdown: true,
                user: "lockuser".to_string(),
                password: "lockpass".to_string(),
            },
        })
    }

    #[tokio::test]
    async fn lockdown_rejects_no_auth() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn lockdown_rejects_wrong_auth() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::AUTHORIZATION, encode_basic_auth("lockuser", "wrong"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn lockdown_allows_correct_auth() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(
                        header::AUTHORIZATION,
                        encode_basic_auth("lockuser", "lockpass"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn lockdown_admin_uses_user_creds() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header(
                        header::AUTHORIZATION,
                        encode_basic_auth("lockuser", "lockpass"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/admin")
                    .header(
                        header::AUTHORIZATION,
                        encode_basic_auth("wrong", "creds"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn trailing_slash_redirects() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/admin/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/admin");
    }

    #[tokio::test]
    async fn trailing_slash_redirects_with_prefix() {
        let app = build_app(prefixed_state());
        let resp = app
            .oneshot(Request::builder().uri("/paste/admin/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/paste/admin"
        );
    }

    #[tokio::test]
    async fn root_no_redirect() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
