# Image Paste Support — Design Spec

## Overview

Add copy/paste image support to paste, allowing users to create image pastes via clipboard paste (Ctrl+V) or file upload from the admin dashboard. Each paste is either text or an image, never both.

## Data Model

### PasteContent enum

```rust
pub enum PasteContent {
    Text(String),
    Image {
        data: Vec<u8>,
        mime_type: String,
        filename: String,
    },
}
```

- `filename` derived from the uploaded file name, or defaults to `"paste.png"` for clipboard pastes.
- `mime_type` validated against an allowlist: `image/png`, `image/jpeg`, `image/gif`, `image/webp`, `image/svg+xml`.

### PasteEntry

Replace `content: String` with `content: PasteContent`.

### Config

Add one new field:

```rust
#[serde(default = "default_max_image_size")]
pub max_image_size: usize,  // default 20_971_520 (20 MB)
```

## Routes

No new routes. Existing routes modified:

### POST / (create paste)

Detect content type from the request:

- **`multipart/form-data`**: Extract image file from the `image` field. Validate MIME type against allowlist, validate size against `max_image_size`. Store as `PasteContent::Image`.
- **`application/x-www-form-urlencoded`**: Existing behavior. Store as `PasteContent::Text`.

Both paths apply TTL, max_pastes, and auth checks identically.

### GET /{id} (view paste)

Check `PasteContent` variant:

- **Text**: Render with existing `view_page` template (line numbers, linkification, copy button).
- **Image**: Render with new `view_image_page` template. Base64-encode the raw bytes at serve time and embed as a data URI in `<img src="data:{mime_type};base64,...">`. Single HTTP request, no extra route needed. The page includes copy/home buttons and a filename + size caption.

### Other routes

- `GET /` admin: add "type" column. Image preview column shows a small thumbnail via data URI.
- `POST /{id}/delete`: unchanged.
- `GET /{id}` (not found/expired): unchanged.

## Admin Form UI

Single HTML form containing both input modes, mutually exclusive via JavaScript:

- **Drop zone** (top): dashed border, upload icon, text "Drop an image here, or click to browse", subtext "or paste from clipboard (Ctrl+V)", accepted formats, max size note. Contains a hidden `<input type="file" accept="image/png,image/jpeg,image/gif,image/webp,image/svg+xml">`.
- **Divider**: "or paste text below"
- **Textarea** (below): as today.
- **TTL controls**: unchanged, below both inputs.
- **Submit button**: unchanged.

JavaScript behavior:

- When an image is selected/dropped/pasted into the drop zone: disable the textarea, show a thumbnail preview of the selected image in the drop zone.
- When the user types in the textarea: clear any selected image, re-enable normal textarea behavior.
- On form submit: if an image is selected, construct a `FormData` with the image file and send as `multipart/form-data`. Otherwise, send as `application/x-www-form-urlencoded` (existing behavior).

## Image View Page

New `view_image_page` template:

- Image displayed centered, constrained with `max-width: 100%; max-height: 80vh`.
- Data URI in the `src` attribute: `data:{mime_type};base64,{encoded}`.
- Caption below: filename + human-readable size (e.g., "screenshot.png — 1.2 MB").
- Same "copy" and "home" buttons as text view.
- Copy button copies the image itself to clipboard (via `navigator.clipboard.write` with a `ClipboardItem` constructed from a fetch of the data URI).

## Admin Table

Add a "type" column between "id" and "expires in":

| id | type | expires in | preview | actions |
|----|------|------------|---------|---------|
| aB3x | text | 14m | hello world... | [delete] |
| cD4y | image | 28m | [40px thumbnail] | [delete] |

- Type column shows `text` or `image`.
- Image rows: thumbnail in preview column (scaled `<img>` tag, ~40px height, data URI).
- Text rows: text preview as today.

## Validation & Limits

- **MIME type**: checked against allowlist on upload. Rejected with 400 + error page if unsupported.
- **Image size**: checked against `max_image_size` config. Rejected with 413 (Payload Too Large) if exceeded.
- **Text size**: checked against `max_size` as today.
- **Total pastes**: `max_pastes` applies to combined count of both types.
- **TTL**: applies identically to both types. `max_ttl_secs` caps both.

## Security

- SVG images served via `<img src="...">` tag, not inline. Scripts in SVGs do not execute when loaded this way.
- MIME type allowlist prevents uploading arbitrary files disguised as images.
- Existing security headers (HSTS, X-Content-Type-Options, X-Frame-Options) apply to all responses.
- Image size limit prevents memory exhaustion.

## Non-goals

- No mixed text+image pastes.
- No EXIF stripping or image re-encoding — raw bytes stored as-is.
- No image-specific TTL or expiry behavior.
- No server-side thumbnail generation — admin thumbnails use browser-scaling via `<img>` dimensions.
- No new routes — everything fits into the existing route structure.

## Implementation Notes

- Use `axum`'s built-in `Multipart` extractor for the image upload path.
- Base64 encoding at serve time via the `base64` crate (add to Cargo.toml) or a simple hand-rolled encoder.
- Human-readable file size formatting: bytes → KB/MB with one decimal place.
- The drop zone and JavaScript should degrade gracefully — without JS, the file input still works for uploads, and the textarea works as before.
