# Image Paste Support — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add clipboard paste and file upload image support to the pastebin. Each paste is either text or an image.

**Architecture:** Replace `PasteEntry.content: String` with a `PasteContent` enum. The `POST /` handler parses both multipart (image) and form-encoded (text) from raw body bytes based on content-type header. The `GET /{id}` handler base64-encodes image bytes at serve time, embeds as data URI in HTML. No new routes.

**Tech Stack:** Rust, axum 0.8, tokio. Hand-rolled base64 encoder (no new crate). Multipart parsing via manual boundary-based parsing (avoids pulling in `multer`/`axum-multipart`).

## Global Constraints

- No new crate dependencies
- `max_image_size` config defaults to 20_971_520 (20 MB)
- Accepted MIME types: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`
- Image pastes served as inline data URIs in HTML
- All existing tests must continue to pass
- Dark mode and CSS variables respected

---

### Task 1: Add max_image_size to Config

**Files:** `src/config.rs`, `src/handlers.rs` (test helpers)

**Produces:** `Config.max_image_size: usize` (default 20_971_520)

- [ ] Add field `pub max_image_size: usize` with `#[serde(default = "default_max_image_size")]` after `max_size` in Config struct
- [ ] Add `fn default_max_image_size() -> usize { 20_971_520 }`
- [ ] Add `max_image_size: default_max_image_size(),` to `impl Default for Config`
- [ ] Fix all `Config { .. }` literals: in `config.rs` tests add `max_image_size: 20_971_520,` to each; in `handlers.rs` add to `test_state()`, `prefixed_state()`, `lockdown_state()`
- [ ] Add `assert_eq!(config.max_image_size, 20_971_520);` to `config_partial_toml_uses_defaults`
- [ ] Run `cargo test config::` — all pass
- [ ] Commit: `feat: add max_image_size config field`

---

### Task 2: Add PasteContent enum and update PasteEntry

**Files:** `src/state.rs`

**Produces:** `PasteContent::Text(String)`, `PasteContent::Image { data: Vec<u8>, mime_type: String, filename: String }`

- [ ] Replace `src/state.rs` — change `content: String` to `content: PasteContent` in PasteEntry. Define the enum with `Text(String)` and `Image { data, mime_type, filename }` variants.
- [ ] Run `cargo check` — expect compile errors in handlers.rs and templates.rs (they access `entry.content` as String). This is expected.
- [ ] Commit: `feat: add PasteContent enum, update PasteEntry`

---

### Task 3: Add base64 encoder and image view template

**Files:** Create `src/base64.rs`, modify `src/main.rs`, `src/templates.rs`, `src/style.css`

**Produces:** `base64::encode(&[u8]) -> String`, `templates::view_image_page(...)`, `templates::format_size(usize) -> String`

- [ ] Create `src/base64.rs` with `pub fn encode(data: &[u8]) -> String` using the standard base64 algorithm. Include tests: empty, "foo" → "Zm9v", single-byte padding "Zg==", two-byte padding "Zm8=", roundtrip via `crate::auth::base64_decode`.
- [ ] Add `mod base64;` to `src/main.rs` after `mod auth;`
- [ ] Add `view_image_page(prefix, mime_type, filename, size_bytes, b64_data) -> String` to templates.rs. Renders HTML with `<img src="data:{mime};base64,{b64}">`, copy button (copies image blob to clipboard via fetch + ClipboardItem), home link, filename + human-readable size caption.
- [ ] Add `format_size(bytes: usize) -> String` helper: "<1024 → X B", "<1MB → X.X KB", else "X.X MB"
- [ ] Add CSS: `.image-container`, `.paste-image`, `.image-info`
- [ ] Add tests: `view_image_page_contains_img`, `view_image_page_escapes_filename`, `format_size_values`
- [ ] Run `cargo test base64:: templates::view_image templates::format_size` — all pass
- [ ] Commit: `feat: add base64 encoder and image view template`

---

### Task 4: Update admin page with drop zone and type column

**Files:** `src/templates.rs` (admin_page), `src/style.css`

- [ ] Replace `admin_page` function: form uses `enctype="multipart/form-data"`, add drop zone div with hidden file input (accepts image/* types), thumbnail preview, clear button, divider "or paste text below", textarea, TTL controls. Add JS for: click-to-browse, drag-and-drop, clipboard paste (document paste event), mutually exclusive image/text selection, clear button. Table header gains `<th>type</th>` between id and expires.
- [ ] Add CSS for: `.drop-zone` (dashed border, hover states), `.drop-zone-inner`, `.drop-icon`, `.drop-sub`, `.drop-formats`, `.image-preview` (in admin form), `#clear-image`, `.divider`
- [ ] Update tests: fix `admin_page_form_action_root` (still checks action attr), add `admin_page_has_type_column_and_dropzone` test
- [ ] Run `cargo test templates::` — all pass
- [ ] Commit: `feat: add drop zone and type column to admin page`

---

### Task 5: Add multipart body parser

**Files:** Create `src/multipart.rs`, modify `src/main.rs`

**Produces:** `multipart::parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<Field>, String>` where `Field { name: String, filename: Option<String>, content_type: Option<String>, data: Vec<u8> }`

We need to parse multipart manually because axum 0.8's Multipart extractor consumes the body and we can't easily have both form-encoded and multipart in one handler. A lightweight manual parser avoids new dependencies.

- [ ] Create `src/multipart.rs` with:
  - `pub struct Field { pub name: String, pub filename: Option<String>, pub content_type: Option<String>, pub data: Vec<u8> }`
  - `pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<Field>, String>` — splits body on `--{boundary}`, parses headers (Content-Disposition for name/filename, Content-Type), extracts data between double CRLF.
  - Handle `\r\n` line endings.
  - Tests: single text field, single file field, multiple fields, empty body, no matching boundary.
- [ ] Add `mod multipart;` to `src/main.rs`
- [ ] Run `cargo test multipart::` — all pass
- [ ] Commit: `feat: add multipart body parser`

---

### Task 6: Update handlers for mixed content types

**Files:** `src/handlers.rs`

**Consumes:** Everything from Tasks 1-5

- [ ] **Update imports:** Add `use crate::state::PasteContent;`, `use crate::templates;`, `use crate::multipart;`. Remove `use serde::Deserialize;` if only PasteForm used it. Keep `PasteForm` struct for form-encoded parsing.

- [ ] **Add MIME allowlist:** `const ALLOWED_IMAGE_TYPES: &[&str]` and `fn is_allowed_image_type(mime: &str) -> bool`

- [ ] **Add TTL resolution helper:** Extract TTL logic into `fn resolve_ttl(ttl: Option<u64>, ttl_custom: Option<&str>, config: &Config) -> u64` — the existing logic from create_paste, re-used for both text and image paths.

- [ ] **Rewrite create_paste handler:** Takes `headers: HeaderMap` and `body: Body` (no Form extractor). Checks auth. Checks content-type header:
  - If `multipart/form-data`: extract boundary from content-type, parse body with `multipart::parse_multipart`, extract `content`/`image`/`ttl`/`ttl_custom` fields. If `image` field present with data → create `PasteContent::Image`. If `content` field present → create `PasteContent::Text`. Validate image MIME type against allowlist, validate size against `max_image_size`/`max_size`.
  - If `application/x-www-form-urlencoded` (or no multipart): parse body as form-encoded via `serde_urlencoded` (axum re-exports this, or parse manually using percent-decoding). Existing behavior for text.
  - Both paths: check `max_pastes`, generate ID, insert, redirect.

- [ ] **Rewrite get_paste handler:** After fetching entry and checking expiry, match on `entry.content`:
  - `PasteContent::Text(text)` → `templates::view_page(prefix, text)` (existing)
  - `PasteContent::Image { data, mime_type, filename }` → base64-encode data, call `templates::view_image_page(prefix, mime_type, filename, data.len(), &b64)`

- [ ] **Update render_admin:** In the loop building table rows, match on `entry.content`:
  - Text: type_label = "text", preview = first 100 chars (existing)
  - Image: type_label = "image", preview = `<img src="data:{mime};base64,{b64}" style="max-height:40px;max-width:80px">`
  - Row format gains type column: `<td>{type_label}</td>` between id and expires.

- [ ] **Update build_app route for create_paste:** The handler signature changed (no longer takes `Form<PasteForm>`). Update the route: `.route("/", get(admin_page).post(create_paste))` stays the same syntactically since axum infers the handler signature.

- [ ] **Update tests:** All existing tests send `application/x-www-form-urlencoded`. They should still work because the handler branches on content-type and the form-encoded path is preserved. However, tests that construct `PasteEntry { content: "..." }` now need `PasteContent::Text("...".to_string())`. Update:
  - `test_state()` — no change (no entries pre-populated)
  - Tests that insert pastes: change `content: "string"` to `content: PasteContent::Text("string".to_string())`
  - `create_and_get_utf8_content` — update content field
  - `root_shows_dashboard_with_auth`, `admin_shows_pastes_with_correct_auth`, `prefixed_routes_work`, etc.
  - Add new tests:
    - `create_image_paste_redirects` — POST multipart with image, expect 303 redirect
    - `create_image_paste_rejects_bad_mime` — POST multipart with text/plain, expect 400
    - `create_image_paste_rejects_oversized` — POST large image, expect 413
    - `get_image_paste_returns_html_with_img` — GET image paste, response contains `<img src="data:image/png;base64,`
    - `admin_shows_image_type_and_thumbnail` — admin page shows type=image and img tag in preview
  - For multipart test helpers, construct raw multipart bodies (or use a small helper function). The multipart parsing is manual so tests can construct the exact byte format.

- [ ] Run `cargo test` — all tests pass (existing + new)
- [ ] Commit: `feat: support image upload and viewing in handlers`

---

### Task 7: Integration testing and polish

**Files:** `src/handlers.rs` (test module), manual testing

- [ ] Run full test suite: `cargo test` — all pass
- [ ] Run `cargo clippy` — fix any warnings
- [ ] Manual test: `cargo run`, open browser, verify:
  - Text paste works as before (create + view + copy + delete)
  - Drop an image on drop zone → thumbnail preview appears, textarea disables
  - Clear image → textarea re-enables
  - Submit image → redirects to image view page (image displayed, filename + size shown)
  - Copy button on image view works
  - Admin table shows "type" column with text/image labels
  - Image thumbnail shown in admin table preview
  - Paste image from clipboard (Ctrl+V on admin page)
  - Dark mode: drop zone and image view respect dark mode
  - Delete image paste works
  - Lockdown mode: image upload requires auth
  - Prefix mode: all routes work under prefix
  - Expired image paste returns gone

- [ ] Commit any fixes from manual testing: `fix: polish image paste behavior`

---

## Self-Review Notes

- Task 6 is the largest — it touches the most code and has the most test updates. Consider committing incrementally within it.
- All existing tests must pass after Task 6. The main risk is the `PasteContent` wrapper in test paste insertions.
- The manual multipart parser (Task 5) only needs to handle the subset of multipart that browsers send: simple fields and single file uploads. No nested parts, no chunked encoding.
