use axum::{routing::get, Router, extract::{State, Path, Form}, response::IntoResponse, http::{StatusCode, header, Request}};
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
        let v: [i8; 4] = std::array::from_fn(|i| {
            if chunk[i] == b'=' { 0 } else { T.get(chunk[i] as usize).copied().unwrap_or(-1) }
        });
        if v.iter().any(|&x| x < 0) { return Err(()); }

        let pad2 = chunk.len() > 2 && chunk[2] == b'=';
        let pad3 = chunk.len() > 3 && chunk[3] == b'=';

        out.push(((v[0] as u8) << 2) | ((v[1] as u8) >> 4));
        if !pad2 { out.push(((v[1] as u8 & 0xf) << 4) | ((v[2] as u8) >> 2)); }
        if !pad3 { out.push(((v[2] as u8 & 0x3) << 6) | (v[3] as u8)); }
    }
    Ok(out)
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
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
    req: Request<axum::body::Body>,
) -> impl IntoResponse {
    let auth = req.headers().get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    if let Some(encoded) = auth.strip_prefix("Basic ") {
        if let Ok(decoded) = base64_decode(encoded) {
            if let Ok(creds) = std::str::from_utf8(&decoded) {
                if let Some((user, pass)) = creds.split_once(':') {
                    if user == state.config.admin_user && pass == state.config.admin_pass {
                        return render_admin(&state).await;
                    }
                }
            }
        }
    }

    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, r#"Basic realm="paste admin""#)],
        axum::response::Html(String::new()),
    ).into_response()
}

async fn render_admin(state: &Arc<AppState>) -> axum::response::Response {
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

fn build_app(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(home).post(create_paste))
        .route("/{id}", get(get_paste))
        .route("/admin", get(admin_page))
        .with_state(state)
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

    let app = build_app(state);

    let listener = tokio::net::TcpListener::bind(&bind).await.unwrap();
    println!("listening on {}", bind);
    axum::serve(listener, app).await.unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::{Request, header};
    use tower::ServiceExt;

    fn test_state() -> Arc<AppState> {
        Arc::new(AppState {
            pastes: RwLock::new(HashMap::new()),
            config: Config {
                bind: "127.0.0.1:0".to_string(),
                ttl_secs: 3600,
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
            out.push(if chunk.len() > 1 { TABLE[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
            out.push(if chunk.len() > 2 { TABLE[(triple & 0x3F) as usize] as char } else { '=' });
        }
        out
    }

    #[test]
    fn html_escape_special_chars() {
        assert_eq!(html_escape("&<>"), "&amp;&lt;&gt;");
    }

    #[test]
    fn html_escape_no_special_chars() {
        assert_eq!(html_escape("hello"), "hello");
    }

    #[test]
    fn html_escape_mixed() {
        assert_eq!(html_escape("a<b&c>d"), "a&lt;b&amp;c&gt;d");
    }

    #[test]
    fn base64_decode_valid() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn base64_decode_invalid_length() {
        assert!(base64_decode("abc").is_err());
    }

    #[test]
    fn base64_decode_invalid_chars() {
        assert!(base64_decode("!!!!").is_err());
    }

    #[test]
    fn base64_decode_padding() {
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
    }

    #[tokio::test]
    async fn home_returns_submit_form() {
        let app = test_app();
        let resp = app.oneshot(Request::builder().uri("/").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("<textarea"));
        assert!(html.contains("paste"));
    }

    #[tokio::test]
    async fn create_paste_redirects() {
        let app = test_app();
        let resp = app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("content=hello"))
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::SEE_OTHER);
        let loc = resp.headers().get(header::LOCATION).unwrap().to_str().unwrap();
        assert!(loc.starts_with('/'));
        assert!(loc.len() == 9);
    }

    #[tokio::test]
    async fn create_paste_rejects_oversized() {
        let app = test_app();
        let big = "x".repeat(200);
        let resp = app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!("content={}", big)))
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn create_paste_rejects_at_max_pastes() {
        let state = test_state();
        let app = build_app(state.clone());
        for i in 0..2 {
            state.pastes.write().await.insert(format!("id{}", i), PasteEntry {
                content: "x".to_string(),
                expires_at: Instant::now() + std::time::Duration::from_secs(3600),
            });
        }
        let resp = app.oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from("content=hello"))
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);
    }

    #[tokio::test]
    async fn get_paste_returns_content() {
        let state = test_state();
        state.pastes.write().await.insert("testid1".to_string(), PasteEntry {
            content: "hello world".to_string(),
            expires_at: Instant::now() + std::time::Duration::from_secs(3600),
        });
        let app = build_app(state);
        let resp = app.oneshot(Request::builder().uri("/testid1").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("hello world"));
    }

    #[tokio::test]
    async fn get_paste_not_found() {
        let app = test_app();
        let resp = app.oneshot(Request::builder().uri("/nonexistent").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
    }

    #[tokio::test]
    async fn get_paste_expired_returns_404() {
        let state = test_state();
        state.pastes.write().await.insert("expired1".to_string(), PasteEntry {
            content: "old".to_string(),
            expires_at: Instant::now() - std::time::Duration::from_secs(1),
        });
        let app = build_app(state.clone());
        let resp = app.oneshot(Request::builder().uri("/expired1").body(Body::empty()).unwrap()).await.unwrap();
        let body = axum::body::to_bytes(resp.into_body(), 1024).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("not found or expired"));
        assert!(state.pastes.read().await.get("expired1").is_none());
    }

    #[tokio::test]
    async fn admin_rejects_no_auth() {
        let app = test_app();
        let resp = app.oneshot(Request::builder().uri("/admin").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        let www_auth = resp.headers().get(header::WWW_AUTHENTICATE).unwrap().to_str().unwrap();
        assert_eq!(www_auth, r#"Basic realm="paste admin""#);
    }

    #[tokio::test]
    async fn admin_rejects_wrong_auth() {
        let app = test_app();
        let resp = app.oneshot(
            Request::builder()
                .uri("/admin")
                .header(header::AUTHORIZATION, encode_basic_auth("admin", "wrong"))
                .body(Body::empty())
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        assert!(resp.headers().get(header::WWW_AUTHENTICATE).is_some());
    }

    #[tokio::test]
    async fn admin_shows_pastes_with_correct_auth() {
        let state = test_state();
        state.pastes.write().await.insert("abc12345".to_string(), PasteEntry {
            content: "test content".to_string(),
            expires_at: Instant::now() + std::time::Duration::from_secs(3600),
        });
        let app = build_app(state);
        let resp = app.oneshot(
            Request::builder()
                .uri("/admin")
                .header(header::AUTHORIZATION, encode_basic_auth("admin", "secret"))
                .body(Body::empty())
                .unwrap()
        ).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let content_type = resp.headers().get(header::CONTENT_TYPE).unwrap().to_str().unwrap();
        assert!(content_type.contains("text/html"));
        let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("1 pastes"));
        assert!(html.contains("abc12345"));
        assert!(html.contains("test content"));
    }

    #[tokio::test]
    async fn config_defaults() {
        let config = Config::default();
        assert_eq!(config.bind, "0.0.0.0:3000");
        assert_eq!(config.ttl_secs, 3600);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_pass, "admin");
    }

    #[tokio::test]
    async fn config_from_toml() {
        let toml = r#"
bind = "0.0.0.0:8080"
ttl_secs = 120
max_size = 2048
max_pastes = 10
admin_user = "root"
admin_pass = "pass123"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:8080");
        assert_eq!(config.ttl_secs, 120);
        assert_eq!(config.max_size, 2048);
        assert_eq!(config.max_pastes, 10);
        assert_eq!(config.admin_user, "root");
        assert_eq!(config.admin_pass, "pass123");
    }

    #[tokio::test]
    async fn config_partial_toml_uses_defaults() {
        let toml = r#"bind = "0.0.0.0:9999""#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.bind, "0.0.0.0:9999");
        assert_eq!(config.ttl_secs, 3600);
        assert_eq!(config.max_size, 8_388_608);
        assert_eq!(config.max_pastes, 512);
        assert_eq!(config.admin_user, "admin");
        assert_eq!(config.admin_pass, "admin");
    }
}
