# paste

A minimalist, self-hosted pastebin written in Rust.

Pastes are stored in memory and expire after a configurable TTL. No database, no JavaScript frameworks, no external CSS — just a fast, minimal pastebin.

## Features

- Short 4-character IDs with collision handling
- Auto-expiring pastes with configurable TTL (presets or custom)
- Copy-to-clipboard button on paste view
- Full UTF-8 support (non-ASCII paste content renders correctly)
- Admin dashboard at `/` with a login form (session cookie) showing all active pastes and a submit form
- Optional lockdown mode requiring authentication for all routes
- Dark mode via `prefers-color-scheme`
- Security headers (HSTS, X-Content-Type-Options, X-Frame-Options)
- Constant-time credential comparison
- HMAC-signed session tokens (no external auth service needed)
- Request body size limit
- TOML configuration file
- Single binary, no external dependencies

## Usage

```sh
cargo run
```

The server listens on `0.0.0.0:3000` by default. Open it in a browser, paste your content, and share the short URL.

## Routes

| Method | Path | Description |
|--------|------|-------------|
| GET | `/` | Admin dashboard + submit form (requires login) |
| POST | `/` | Create paste (public when lockdown off; form-encoded `content`, `ttl`, `ttl_custom`) |
| GET | `/{id}` | View paste |
| POST | `/{id}/delete` | Delete paste (requires login) |
| GET | `/login` | Login form |
| POST | `/login` | Submit credentials (sets session cookie) |
| GET | `/logout` | Clear session cookie |

## Authentication

Paste uses its own session login instead of HTTP Basic Auth or external auth services (like Authelia). Logging in at `/login` sets a stateless, HMAC-SHA256-signed cookie (`paste_session`). No server-side session store is kept.

- **Public** (when `lockdown = false`): creating and viewing pastes.
- **Protected** (always): the admin dashboard (`/`) and deleting pastes.
- **`lockdown = true`**: login is required for *all* routes.

## Configuration

Create a `paste.toml` in the working directory. All fields are optional:

```toml
bind = "0.0.0.0:3000"          # Listen address
prefix = ""                    # URL prefix (e.g. "/paste")
max_ttl_secs = 86400           # Maximum paste lifetime (24h)
default_ttl_mins = 15          # Default TTL when none selected
max_size = 8388608             # Max text paste size in bytes (8MB)
max_pastes = 512               # Max active pastes
max_image_size = 20971520      # Max image paste size in bytes (20MB)
lockdown = false               # Require auth for all routes
user = "user"                  # Login username
password = "change_me"         # Login password (plaintext)
secret = "change_me_secret"    # HMAC session-signing secret (strongly recommended)
session_ttl_secs = 28800       # Session cookie lifetime (8h)
```

Defaults are used for any missing fields. Warnings are printed if the default credentials or default session secret are active.

## TTL

Users can select a preset expiration (5m, 15m, 30m, 1h, 6h, 12h, 24h) or enter a custom duration in minutes. The custom value takes precedence when provided. If nothing is selected, the `default_ttl_mins` config value is used. Pastes exceeding `max_ttl_secs` are rejected with an error.

Expired pastes are cleaned up by a background sweeper every 60 seconds and lazily on access.

## Building

```sh
cargo build --release
```

## Testing

```sh
cargo test
```
