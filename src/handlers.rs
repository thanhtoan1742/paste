use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use std::sync::Arc;
use std::time::Instant;
use tracing::{debug, info, warn};

use crate::auth::{constant_time_eq, create_token, validate_token};
use crate::config::Config;
use crate::multipart;
use crate::state::{AppState, PasteContent};
use crate::templates;

pub const SWEEPER_INTERVAL_SECS: u64 = 60;

const COOKIE_NAME: &str = "paste_session";

fn token_from_headers(headers: &HeaderMap) -> Option<String> {
    if let Some(cookie) = headers.get(header::COOKIE).and_then(|v| v.to_str().ok()) {
        for part in cookie.split(';') {
            let part = part.trim();
            if let Some((k, v)) = part.split_once('=') {
                if k == COOKIE_NAME {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

fn validate_session(state: &AppState, headers: &HeaderMap) -> bool {
    let Some(token) = token_from_headers(headers) else {
        return false;
    };
    validate_token(state.config.secret.as_bytes(), &token)
        .map(|_| true)
        .unwrap_or(false)
}

#[derive(Deserialize)]
struct PasteForm {
    content: String,
    ttl: Option<u64>,
    ttl_custom: Option<String>,
}

const ALLOWED_IMAGE_TYPES: &[&str] = &[
    "image/png",
    "image/jpeg",
    "image/gif",
    "image/webp",
    "image/svg+xml",
];

fn is_allowed_image_type(mime: &str) -> bool {
    ALLOWED_IMAGE_TYPES.contains(&mime)
}

fn resolve_ttl(ttl: Option<u64>, ttl_custom: Option<&str>, config: &Config) -> u64 {
    match ttl_custom
        .and_then(|s: &str| s.parse::<u64>().ok())
        .filter(|&m| m > 0)
    {
        Some(mins) => mins * 60,
        None => match ttl {
            Some(mins) if mins > 0 => mins * 60,
            _ => config.default_ttl_mins * 60,
        },
    }
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

async fn request_logger(req: Request<Body>, next: Next) -> Response {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let started = Instant::now();
    let response = next.run(req).await;
    let status = response.status();
    let duration = started.elapsed();
    info!(
        method = %method,
        path = %path,
        status = %status.as_u16(),
        duration_ms = duration.as_millis(),
        "request"
    );
    response
}

async fn lockdown_auth(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    req: Request<Body>,
    next: Next,
) -> Response {
    let path = req.uri().path().to_string();

    // Login endpoints are always public so users can authenticate.
    if path.ends_with("/login") {
        return next.run(req).await;
    }

    if !state.config.lockdown {
        return next.run(req).await;
    }

    if validate_session(&state, &headers) {
        next.run(req).await
    } else {
        warn!(path = %path, "lockdown rejected unauthenticated request");
        crate::auth::login_redirect(&state.config.prefix).into_response()
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

async fn create_paste(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Body,
) -> impl IntoResponse {
    // Creating a paste is public when lockdown is off (the dashboard is the
    // only protected surface). In lockdown mode, the middleware has already
    // enforced a valid session.
    let _ = (&state, &headers);

    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    let max_body = state.config.max_size.max(state.config.max_image_size) + 4096;
    let bytes = match axum::body::to_bytes(body, max_body).await {
        Ok(b) => b,
        Err(_) => {
            warn!(max_body = max_body, "create paste rejected: body too large");
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }
    };

    if bytes.len() > max_body {
        warn!(len = bytes.len(), max_body = max_body, "create paste rejected: body too large");
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    if content_type.starts_with("multipart/form-data") {
        return handle_multipart_create(state, content_type, &bytes).await;
    }

    // Form-encoded text paste (existing behavior)
    handle_form_create(state, &bytes).await
}

async fn handle_multipart_create(
    state: Arc<AppState>,
    content_type: &str,
    body: &[u8],
) -> Response {
    let boundary = content_type
        .strip_prefix("multipart/form-data")
        .and_then(|s| s.split('=').nth(1).map(|s| s.trim()))
        .unwrap_or("");

    if boundary.is_empty() {
        return (StatusCode::BAD_REQUEST, "missing boundary").into_response();
    }

    let fields = match multipart::parse_multipart(body, boundary) {
        Ok(f) => f,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::response::Html(templates::error_page(
                    &state.config.prefix,
                    "invalid multipart data",
                )),
            )
                .into_response();
        }
    };

    let mut content = String::new();
    let mut ttl: Option<u64> = None;
    let mut ttl_custom: Option<String> = None;
    let mut image_data: Option<Vec<u8>> = None;
    let mut image_mime: Option<String> = None;
    let mut image_filename: Option<String> = None;

    for field in &fields {
        match field.name.as_str() {
            "content" => {
                content = String::from_utf8_lossy(&field.data).into_owned();
            }
            "ttl" => {
                if let Ok(v) = std::str::from_utf8(&field.data) {
                    ttl = v.trim().parse::<u64>().ok();
                }
            }
            "ttl_custom" => {
                let s = String::from_utf8_lossy(&field.data);
                let trimmed = s.trim();
                if !trimmed.is_empty() {
                    ttl_custom = Some(trimmed.to_string());
                }
            }
            "image" => {
                if !field.data.is_empty() {
                    image_data = Some(field.data.clone());
                    image_mime = field.content_type.clone();
                    image_filename = field.filename.clone();
                }
            }
            _ => {}
        }
    }

    // Image paste
    if let Some(data) = image_data {
        let mime = image_mime.unwrap_or_else(|| "image/png".to_string());
        let filename = image_filename.unwrap_or_else(|| "paste.png".to_string());

        if !is_allowed_image_type(&mime) {
            return (
                StatusCode::BAD_REQUEST,
                axum::response::Html(templates::error_page(
                    &state.config.prefix,
                    &format!("unsupported image type: {}", mime),
                )),
            )
                .into_response();
        }

        if data.len() > state.config.max_image_size {
            return StatusCode::PAYLOAD_TOO_LARGE.into_response();
        }

        let ttl_secs = resolve_ttl(ttl, ttl_custom.as_deref(), &state.config);
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

        let prefix = state.config.prefix.clone();
        let mut pastes = state.pastes.write().await;
        if pastes.len() >= state.config.max_pastes {
            warn!("create paste rejected: max_pastes reached");
            return StatusCode::INSUFFICIENT_STORAGE.into_response();
        }

        let id = loop {
            let id = nanoid::nanoid!(4);
            if !pastes.contains_key(&id) {
                break id;
            }
        };

        pastes.insert(
            id.clone(),
            crate::state::PasteEntry {
                content: PasteContent::Image {
                    data: data.clone(),
                    mime_type: mime.clone(),
                    filename: filename.clone(),
                },
                expires_at: Instant::now() + std::time::Duration::from_secs(ttl_secs),
            },
        );

        info!(
            id = %id,
            kind = "image",
            bytes = data.len(),
            mime = %mime,
            filename = %filename,
            ttl_secs = ttl_secs,
            total_pastes = pastes.len(),
            "paste created"
        );

        return axum::response::Redirect::to(&format!("{}/{}", prefix, id)).into_response();
    }

    // Text paste from multipart
    if content.len() > state.config.max_size {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let ttl_secs = resolve_ttl(ttl, ttl_custom.as_deref(), &state.config);
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

    let prefix = state.config.prefix.clone();
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

    let content_len = content.len();
    pastes.insert(
        id.clone(),
        crate::state::PasteEntry {
            content: PasteContent::Text(content),
            expires_at: Instant::now() + std::time::Duration::from_secs(ttl_secs),
        },
    );

    info!(
        id = %id,
        kind = "text",
        bytes = content_len,
        ttl_secs = ttl_secs,
        total_pastes = pastes.len(),
        "paste created (multipart)"
    );

    axum::response::Redirect::to(&format!("{}/{}", prefix, id)).into_response()
}

fn parse_form_body(body: &[u8]) -> Result<PasteForm, String> {
    let body_str = String::from_utf8_lossy(body);
    let mut content = String::new();
    let mut ttl: Option<u64> = None;
    let mut ttl_custom: Option<String> = None;

    for pair in body_str.split('&') {
        let (key, value) = match pair.split_once('=') {
            Some((k, v)) => (k, v),
            None => continue,
        };
        let decoded = percent_decode(value);
        match key {
            "content" => content = decoded,
            "ttl" => ttl = decoded.parse::<u64>().ok(),
            "ttl_custom" => {
                if !decoded.is_empty() {
                    ttl_custom = Some(decoded);
                }
            }
            _ => {}
        }
    }

    Ok(PasteForm {
        content,
        ttl,
        ttl_custom,
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out_bytes = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(hex) = u8::from_str_radix(
                std::str::from_utf8(&bytes[i + 1..i + 3]).unwrap_or(""),
                16,
            ) {
                out_bytes.push(hex);
                i += 3;
                continue;
            }
        } else if bytes[i] == b'+' {
            out_bytes.push(b' ');
            i += 1;
            continue;
        }
        out_bytes.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out_bytes).into_owned()
}

async fn handle_form_create(state: Arc<AppState>, body: &[u8]) -> Response {
    let form = match parse_form_body(body) {
        Ok(f) => f,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::response::Html(templates::error_page(
                    &state.config.prefix,
                    "invalid form data",
                )),
            )
                .into_response();
        }
    };

    if form.content.len() > state.config.max_size {
        warn!(bytes = form.content.len(), max_size = state.config.max_size, "create paste rejected: text too large");
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    }

    let ttl_secs = match form
        .ttl_custom
        .as_deref()
        .and_then(|s: &str| s.parse::<u64>().ok())
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

    let prefix = state.config.prefix.clone();
    let mut pastes = state.pastes.write().await;

    if pastes.len() >= state.config.max_pastes {
        warn!("create paste rejected: max_pastes reached");
        return StatusCode::INSUFFICIENT_STORAGE.into_response();
    }

    let id = loop {
        let id = nanoid::nanoid!(4);
        if !pastes.contains_key(&id) {
            break id;
        }
    };

    let content_len = form.content.len();
    pastes.insert(
        id.clone(),
        crate::state::PasteEntry {
            content: PasteContent::Text(form.content),
            expires_at: Instant::now() + std::time::Duration::from_secs(ttl_secs),
        },
    );

    info!(
        id = %id,
        kind = "text",
        bytes = content_len,
        ttl_secs = ttl_secs,
        total_pastes = pastes.len(),
        "paste created"
    );

    axum::response::Redirect::to(&format!("{}/{}", prefix, id)).into_response()
}

async fn get_paste(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let pastes = state.pastes.read().await;
    let Some(entry) = pastes.get(&id) else {
        debug!(paste_id = %id, "paste not found");
        return (
            StatusCode::NOT_FOUND,
            axum::response::Html(templates::not_found_page()),
        )
            .into_response();
    };

    if Instant::now() > entry.expires_at {
        drop(pastes);
        state.pastes.write().await.remove(&id);
        info!(paste_id = %id, "paste expired and removed");
        return (
            StatusCode::GONE,
            axum::response::Html(templates::not_found_page()),
        )
            .into_response();
    }

    match &entry.content {
        PasteContent::Text(text) => {
            axum::response::Html(templates::view_page(&state.config.prefix, text))
                .into_response()
        }
        PasteContent::Image {
            data,
            mime_type,
            filename,
        } => {
            let b64 = crate::base64::encode(data);
            axum::response::Html(templates::view_image_page(
                &state.config.prefix,
                mime_type,
                filename,
                data.len(),
                &b64,
            ))
                .into_response()
        }
    }
}

async fn admin_page(State(state): State<Arc<AppState>>, headers: HeaderMap) -> impl IntoResponse {
    if !validate_session(&state, &headers) {
        return crate::auth::login_redirect(&state.config.prefix).into_response();
    }
    render_admin(&state).await
}

#[derive(Deserialize)]
struct LoginForm {
    user: String,
    password: String,
}

async fn login_page(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    axum::response::Html(templates::login_page(&state.config.prefix)).into_response()
}

async fn login_submit(
    State(state): State<Arc<AppState>>,
    body: String,
) -> impl IntoResponse {
    let form: LoginForm = match parse_login_form(&body) {
        Ok(f) => f,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                axum::response::Html(templates::login_page_error(
                    &state.config.prefix,
                    "invalid form data",
                )),
            )
                .into_response();
        }
    };

    let user_ok = constant_time_eq(
        form.user.as_bytes(),
        state.config.user.as_bytes(),
    );
    let pass_ok = constant_time_eq(
        form.password.as_bytes(),
        state.config.password.as_bytes(),
    );

    if user_ok && pass_ok {
        let token = create_token(
            state.config.secret.as_bytes(),
            &state.config.user,
            state.config.session_ttl_secs,
        );
        info!(user = %state.config.user, "login successful");

        let cookie = format!(
            "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age={}",
            COOKIE_NAME, token, state.config.session_ttl_secs
        );
        let home = if state.config.prefix.is_empty() {
            "/".to_string()
        } else {
            state.config.prefix.clone()
        };
        let mut resp = axum::response::Redirect::to(&home).into_response();
        resp.headers_mut().insert(
            header::SET_COOKIE,
            cookie.parse().expect("valid cookie value"),
        );
        return resp;
    }

    warn!("login failed: invalid credentials");
    (
        StatusCode::UNAUTHORIZED,
        axum::response::Html(templates::login_page_error(
            &state.config.prefix,
            "invalid username or password",
        )),
    )
        .into_response()
}

fn parse_login_form(body: &str) -> Result<LoginForm, String> {
    let mut user = String::new();
    let mut password = String::new();
    for pair in body.split('&') {
        let (k, v) = match pair.split_once('=') {
            Some(x) => x,
            None => continue,
        };
        let decoded = percent_decode(v);
        match k {
            "user" => user = decoded,
            "password" => password = decoded,
            _ => {}
        }
    }
    if user.is_empty() {
        return Err("missing user".to_string());
    }
    Ok(LoginForm { user, password })
}

async fn logout(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let cookie = format!(
        "{}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=0",
        COOKIE_NAME, ""
    );
    let home = if state.config.prefix.is_empty() {
        "/".to_string()
    } else {
        state.config.prefix.clone()
    };
    let mut resp = axum::response::Redirect::to(&home).into_response();
    resp.headers_mut().insert(
        header::SET_COOKIE,
        cookie.parse().expect("valid cookie value"),
    );
    resp
}

async fn delete_paste(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !validate_session(&state, &headers) {
        warn!(paste_id = %id, "delete rejected: not authenticated");
        return crate::auth::login_redirect(&state.config.prefix).into_response();
    }

    state.pastes.write().await.remove(&id);
    info!(paste_id = %id, "paste deleted");

    let home_path = if state.config.prefix.is_empty() {
        "/".to_string()
    } else {
        state.config.prefix.clone()
    };
    axum::response::Redirect::to(&home_path).into_response()
}

async fn render_admin(state: &Arc<AppState>) -> axum::response::Response {
    let pastes = state.pastes.read().await;
    let now = Instant::now();

    let mut rows = String::new();
    let prefix = &state.config.prefix;
    for (id, entry) in pastes.iter() {
        let escaped_id = templates::html_escape(id);
        let (type_label, preview_html) = match &entry.content {
            PasteContent::Text(t) => {
                let preview = templates::html_escape(&t.chars().take(100).collect::<String>());
                ("text", format!("{}...", preview))
            }
            PasteContent::Image {
                data,
                mime_type,
                filename: _,
            } => {
                let b64 = crate::base64::encode(data);
                (
                    "image",
                    format!(
                        "<img src=\"data:{};base64,{}\" style=\"max-height:40px;max-width:80px\" alt=\"\">",
                        mime_type, b64
                    ),
                )
            }
        };
        let secs_left = entry.expires_at.duration_since(now).as_secs();
        let human = format_duration(secs_left);
        rows.push_str(&format!(
            "<tr><td><a href=\"{}/{}\">{}</a></td><td>{}</td><td>{}</td><td>{}</td><td><form method=\"POST\" action=\"{}/{}/delete\"><button type=\"submit\">delete</button></form></td></tr>",
            prefix, escaped_id, escaped_id, type_label, human, preview_html, prefix, escaped_id
        ));
    }

    axum::response::Html(templates::admin_page(prefix, pastes.len(), &rows)).into_response()
}

pub fn build_app(state: Arc<AppState>) -> Router {
    let body_limit = state.config.max_size.max(state.config.max_image_size) + 4096;
    let prefix = state.config.prefix.clone();
    let state_for_lockdown = state.clone();

    let inner = Router::new()
        .route("/", get(admin_page).post(create_paste))
        .route("/login", get(login_page).post(login_submit))
        .route("/logout", get(logout))
        .route("/{id}", get(get_paste))
        .route("/{id}/delete", post(delete_paste))
        .with_state(state)
        .layer(axum::extract::DefaultBodyLimit::max(body_limit))
        .layer(middleware::from_fn_with_state(
            state_for_lockdown,
            lockdown_auth,
        ))
        .layer(middleware::from_fn(request_logger))
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
                max_image_size: 20_971_520,
                lockdown: false,
                user: "user".to_string(),
                password: "secret".to_string(),
                secret: "test-secret".to_string(),
                session_ttl_secs: 28800,
            },
        })
    }

    fn test_app() -> Router {
        build_app(test_state())
    }

    fn make_session_cookie(username: &str) -> String {
        let token = crate::auth::create_token(b"test-secret", username, 28800);
        format!("paste_session={}", token)
    }

    #[tokio::test]
    async fn root_rejects_no_auth() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn root_shows_dashboard_with_auth() {
        let state = test_state();
        state.pastes.write().await.insert(
            "dash01".to_string(),
            PasteEntry {
                content: PasteContent::Text("dash content".to_string()),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<textarea"));
        assert!(html.contains("1 pastes"));
        assert!(html.contains("dash01"));
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
                    .header(header::COOKIE, make_session_cookie("user"))
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
                    .header(header::COOKIE, make_session_cookie("user"))
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
                    content: PasteContent::Text("x".to_string()),
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
                    .header(header::COOKIE, make_session_cookie("user"))
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
                content: PasteContent::Text("hello world".to_string()),
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
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
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
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
    }

    #[tokio::test]
    async fn get_paste_expired_returns_gone() {
        let state = test_state();
        state.pastes.write().await.insert(
            "expired1".to_string(),
            PasteEntry {
                content: PasteContent::Text("old".to_string()),
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
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
        assert!(state.pastes.read().await.get("expired1").is_none());
    }

    #[tokio::test]
    async fn admin_rejects_no_auth() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn admin_rejects_wrong_auth() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, "paste_session=invalid.token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn admin_shows_pastes_with_correct_auth() {
        let state = test_state();
        state.pastes.write().await.insert(
            "abc12345".to_string(),
            PasteEntry {
                content: PasteContent::Text("test content".to_string()),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("user"))
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
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
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
                content: PasteContent::Text("to be deleted".to_string()),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/del1/delete")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap().to_str().unwrap(),
            "/"
        );
        assert!(state.pastes.read().await.get("del1").is_none());
    }

    #[tokio::test]
    async fn delete_paste_rejects_no_auth() {
        let state = test_state();
        state.pastes.write().await.insert(
            "del2".to_string(),
            PasteEntry {
                content: PasteContent::Text("still here".to_string()),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/del2/delete")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
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
                    .header(header::COOKIE, make_session_cookie("user"))
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
                    .header(header::COOKIE, make_session_cookie("user"))
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
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from("content=hello&ttl=30&ttl_custom="))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn create_and_get_utf8_content() {
        let state = test_state();
        let app = build_app(state.clone());

        let body = "content=h%C3%A9llo+%E6%97%A5%E6%9C%AC+%F0%9F%8E%89";
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from(body))
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
            .unwrap()
            .to_string();

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(&loc)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("héllo"));
        assert!(html.contains("日本"));
        assert!(html.contains("🎉"));
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
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from("content=hello&ttl_custom=1500"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
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
                max_image_size: 20_971_520,
                lockdown: false,
                user: "user".to_string(),
                password: "secret".to_string(),
                secret: "test-secret".to_string(),
                session_ttl_secs: 28800,
            },
        })
    }

    #[tokio::test]
    async fn prefixed_routes_work() {
        let state = prefixed_state();
        state.pastes.write().await.insert(
            "ab01".to_string(),
            PasteEntry {
                content: PasteContent::Text("prefixed content".to_string()),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state.clone());

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/paste")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
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
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("prefixed content"));

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/paste")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .header(header::COOKIE, make_session_cookie("user"))
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
                max_image_size: 20_971_520,
                lockdown: true,
                user: "lockuser".to_string(),
                password: "lockpass".to_string(),
                secret: "test-secret".to_string(),
                session_ttl_secs: 28800,
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
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn lockdown_rejects_wrong_auth() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, "paste_session=invalid.token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn lockdown_allows_correct_auth() {
        let app = build_app(lockdown_state());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("lockuser"))
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
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("lockuser"))
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
                    .uri("/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/login");
    }

    #[tokio::test]
    async fn trailing_slash_redirects() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/foo/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(resp.headers().get(header::LOCATION).unwrap(), "/foo");
    }

    #[tokio::test]
    async fn trailing_slash_redirects_with_prefix() {
        let app = build_app(prefixed_state());
        let resp = app
            .oneshot(Request::builder().uri("/paste/foo/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::PERMANENT_REDIRECT);
        assert_eq!(
            resp.headers().get(header::LOCATION).unwrap(),
            "/paste/foo"
        );
    }

    #[tokio::test]
    async fn root_no_redirect() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    fn make_multipart_body(boundary: &str, fields: &[(&str, &[u8])], file: Option<(&str, &str, &str, &[u8])>) -> Vec<u8> {
        let mut body = Vec::new();
        for (name, data) in fields {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{}\"\r\n\r\n", name).as_bytes(),
            );
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        if let Some((name, filename, mime, data)) = file {
            body.extend_from_slice(format!("--{}\r\n", boundary).as_bytes());
            body.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
                    name, filename, mime
                )
                .as_bytes(),
            );
            body.extend_from_slice(data);
            body.extend_from_slice(b"\r\n");
        }
        body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());
        body
    }

    #[tokio::test]
    async fn create_image_paste_redirects() {
        let app = test_app();
        let body = make_multipart_body(
            "boundary",
            &[],
            Some(("image", "test.png", "image/png", b"fake-png-data")),
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(
                        header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from(body))
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
        assert_eq!(loc.len(), 5);
    }

    #[tokio::test]
    async fn get_image_paste_returns_html_with_img() {
        let state = test_state();
        state.pastes.write().await.insert(
            "img01".to_string(),
            PasteEntry {
                content: PasteContent::Image {
                    data: b"test-image-bytes".to_vec(),
                    mime_type: "image/png".to_string(),
                    filename: "test.png".to_string(),
                },
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/img01")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<img src=\"data:image/png;base64,"));
        assert!(html.contains("test.png"));
        assert!(html.contains("paste-image"));
    }

    #[tokio::test]
    async fn admin_shows_image_type_and_thumbnail() {
        let state = test_state();
        state.pastes.write().await.insert(
            "img02".to_string(),
            PasteEntry {
                content: PasteContent::Image {
                    data: b"thumb-data".to_vec(),
                    mime_type: "image/gif".to_string(),
                    filename: "anim.gif".to_string(),
                },
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            },
        );
        let app = build_app(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<td>image</td>"));
        assert!(html.contains("data:image/gif;base64,"));
        assert!(html.contains("max-height:40px"));
    }

    #[tokio::test]
    async fn create_image_paste_rejects_wrong_mime() {
        let app = test_app();
        let body = make_multipart_body(
            "boundary",
            &[],
            Some(("image", "test.txt", "text/plain", b"not-an-image")),
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(
                        header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("unsupported image type"));
    }

    #[tokio::test]
    async fn multipart_text_paste_works() {
        let app = test_app();
        let body = make_multipart_body(
            "boundary",
            &[("content", b"hello from multipart")],
            None,
        );
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/")
                    .header(
                        header::CONTENT_TYPE,
                        "multipart/form-data; boundary=boundary",
                    )
                    .header(header::COOKIE, make_session_cookie("user"))
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
    }

    #[tokio::test]
    async fn login_page_serves_html() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/login").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 131072).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("name=\"user\""));
        assert!(html.contains("name=\"password\""));
    }

    #[tokio::test]
    async fn login_submit_valid_credentials_sets_cookie() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("user=user&password=secret"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let set_cookie = resp.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(set_cookie.starts_with("paste_session="));
        assert!(set_cookie.contains("HttpOnly"));
        assert!(set_cookie.contains("SameSite=Lax"));
    }

    #[tokio::test]
    async fn login_submit_invalid_credentials_rejected() {
        let app = test_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/login")
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(Body::from("user=user&password=wrong"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::SET_COOKIE).is_none());
    }

    #[tokio::test]
    async fn logout_clears_cookie() {
        let app = test_app();
        let resp = app
            .oneshot(Request::builder().uri("/logout").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let set_cookie = resp.headers().get(header::SET_COOKIE).unwrap().to_str().unwrap();
        assert!(set_cookie.contains("Max-Age=0"));
    }
}
