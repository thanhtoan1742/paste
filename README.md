# paste

A minimalist, self-hosted pastebin written in Rust.

Pastes are stored in memory and expire after a configurable TTL. No database, no JavaScript frameworks, no external CSS — just a fast, minimal pastebin.

## Features

- Short 4-character IDs with collision handling
- Auto-expiring pastes with configurable TTL (presets or custom)
- Copy-to-clipboard button on paste view
- Admin page with HTTP Basic Auth showing all active pastes
- Dark mode via `prefers-color-scheme`
- Security headers (HSTS, X-Content-Type-Options, X-Frame-Options)
- Constant-time credential comparison
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
| GET | `/` | Submit form |
| POST | `/` | Create paste (form-encoded `content`, `ttl`, `ttl_custom`) |
| GET | `/{id}` | View paste |
| GET | `/admin` | Admin dashboard (requires Basic Auth) |

## Configuration

Create a `paste.toml` in the working directory. All fields are optional:

```toml
bind = "0.0.0.0:3000"          # Listen address
max_ttl_secs = 86400           # Maximum paste lifetime (24h)
default_ttl_mins = 15          # Default TTL when none selected
max_size = 8388608             # Max paste size in bytes (8MB)
max_pastes = 512               # Max active pastes
admin_user = "admin"           # Admin username
admin_password = "change_me"    # Admin password
```

Defaults are used for any missing fields. A warning is printed if the default credentials (`admin:admin`) are active.

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
