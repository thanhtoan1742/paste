# UTF-8 Support Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the paste view render non-ASCII paste content correctly (e.g. "é" stays "é", not "Ã©") and declare UTF-8 in every HTML page template.

**Architecture:** Three helpers in `src/templates.rs` (`escape_attr`, `push_escaped_char`, `linkify`) currently iterate over bytes and cast each byte with `b as char`, which corrupts multi-byte UTF-8 sequences. Switch them to char-level iteration: `escape_attr` over `s.chars()`, and `linkify`'s non-URL branch advances by `ch.len_utf8()` and passes a `char` to `push_escaped_char`. URL detection (`match_url`) stays byte-level because it only matches ASCII schemes (`http://`/`https://`) and `i` always lands on char boundaries. Add `<meta charset="utf-8">` to all four page templates for belt-and-suspenders correctness outside the HTTP response.

**Tech Stack:** Rust 2021, axum 0.8 (existing). No new crates.

**Spec:** `docs/superpowers/specs/2026-07-25-utf8-support-design.md`

**Testing note:** Pure TDD — each helper change is covered by a failing unit test written before the implementation edit. An integration test in `handlers.rs` confirms end-to-end UTF-8 round-trip. Baseline test count: 70 passing.

---

## File Structure

- **`src/templates.rs`** — the only file touched. Rewrite `escape_attr`, `push_escaped_char`, and `linkify`'s non-URL branch to operate on `char` instead of `u8`. Add `<meta charset="utf-8">` to `not_found_page`, `view_page`, `admin_page`, `error_page`. Add unit tests for all three helpers and the template changes, plus an integration test in the existing `tests` module.

No other files change (confirmed by the spec's audit table).

---

### Task 1: Fix `escape_attr` to iterate over chars

**Files:**
- Modify: `src/templates.rs:134-146` (`escape_attr`)
- Test: `src/templates.rs` (in the existing `#[cfg(test)] mod tests` block, after the existing `view_page_home_link_prefix` test around line 457, before the `strip_tags_and_decode` helper around line 458)

`escape_attr` is used by `render_lines` for the `data-line` attribute. Currently it iterates over `s.bytes()` and pushes `b as char`, which corrupts multi-byte UTF-8.

- [ ] **Step 1: Add failing tests for multi-byte UTF-8 preservation in `escape_attr`**

Insert these tests immediately after the `view_page_home_link_prefix` test (currently ending around line 456, just before `fn strip_tags_and_decode`):

```rust
    #[test]
    fn escape_attr_preserves_single_multibyte() {
        assert_eq!(escape_attr("é"), "é");
    }

    #[test]
    fn escape_attr_preserves_cjk() {
        assert_eq!(escape_attr("日本"), "日本");
    }

    #[test]
    fn escape_attr_preserves_emoji() {
        assert_eq!(escape_attr("a🎉b"), "a🎉b");
    }

    #[test]
    fn escape_attr_escapes_specials_adjacent_to_multibyte() {
        assert_eq!(escape_attr("é&<"), "é&amp;&lt;");
    }

    #[test]
    fn escape_attr_escapes_quote() {
        assert_eq!(escape_attr("\""), "&quot;");
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test --lib templates::tests::escape_attr`
Expected: 5 FAIL. `escape_attr_preserves_single_multibyte` fails because the byte `0xC3` is pushed as char `U+00C3` ('Ã') and `0xA9` as `U+00A9` ('©'), producing "Ã©" instead of "é". The `escape_attr_escapes_quote` test may also fail if `"` wasn't handled — confirm it does fail (the current code handles `b'"'` via the match arm, so it should pass; if it passes, that's fine, the other four failures are the signal).

- [ ] **Step 3: Rewrite `escape_attr` to iterate over chars**

Replace the current `escape_attr` function (lines 134-146):

```rust
fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'&' => out.push_str("&amp;"),
            b'<' => out.push_str("&lt;"),
            b'>' => out.push_str("&gt;"),
            b'"' => out.push_str("&quot;"),
            _ => out.push(b as char),
        }
    }
    out
}
```

with:

```rust
fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}
```

Design notes for the implementer:
- `s.chars()` yields `char` values (full Unicode scalar values), so a multi-byte sequence like "é" decodes into a single `char` (`U+00E9`) and `out.push(c)` re-encodes it back to UTF-8 bytes `C3 A9` in the output `String`.
- `String::with_capacity(s.len())` is still a reasonable capacity hint — escaped output is at least as long as the input.
- The `b'"'` arm becomes `'"'` to match on `char` instead of `u8`.

- [ ] **Step 4: Run the new tests to verify they pass**

Run: `cargo test --lib templates::tests::escape_attr`
Expected: 5 PASS.

- [ ] **Step 5: Run the full test suite to confirm no regressions**

Run: `cargo test --quiet`
Expected: 75 passed; 0 failed (70 baseline + 5 new). Existing tests like `view_page_line_button_escapes_data_line` (which checks `data-line="a &amp; b &lt; c &gt; d"` for input `"a & b < c > d"`) must still pass — ASCII behavior is unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/templates.rs
git commit -m "fix: preserve multi-byte UTF-8 in escape_attr"
```

---

### Task 2: Fix `push_escaped_char` and `linkify` non-URL branch to use char

**Files:**
- Modify: `src/templates.rs:148-155` (`push_escaped_char`)
- Modify: `src/templates.rs:83-103` (`linkify` non-URL branch, lines 97-99)
- Test: `src/templates.rs` (in the existing `#[cfg(test)] mod tests` block)

`push_escaped_char` takes `u8` and pushes `b as char`, and `linkify` calls it per byte in the non-URL branch. Both must switch to `char`.

- [ ] **Step 1: Add failing tests for `linkify` multi-byte UTF-8 preservation**

Insert these tests immediately after the `escape_attr_escapes_quote` test added in Task 1:

```rust
    #[test]
    fn linkify_preserves_multibyte_plain_text() {
        assert_eq!(linkify("héllo"), "héllo");
    }

    #[test]
    fn linkify_preserves_cjk_plain_text() {
        assert_eq!(linkify("日本語"), "日本語");
    }

    #[test]
    fn linkify_preserves_emoji_plain_text() {
        assert_eq!(linkify("x🎉y"), "x🎉y");
    }

    #[test]
    fn linkify_url_adjacent_to_multibyte() {
        let out = linkify("éhttps://x.com");
        assert!(out.contains("é"));
        assert!(out.contains("<a href=\"https://x.com\""));
    }

    #[test]
    fn linkify_multibyte_after_url() {
        let out = linkify("https://x.comé");
        assert!(out.contains("<a href=\"https://x.com\""));
        assert!(out.contains("é"));
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test --lib templates::tests::linkify_preserves templates::tests::linkify_url_adjacent templates::tests::linkify_multibyte_after`
Expected: 5 FAIL. The plain-text tests fail because each byte of a multi-byte sequence is pushed as a separate (wrong) codepoint. The URL-adjacent tests fail because the multi-byte char is corrupted.

- [ ] **Step 3: Change `push_escaped_char` signature from `u8` to `char`**

Replace the current `push_escaped_char` function (lines 148-155):

```rust
fn push_escaped_char(out: &mut String, b: u8) {
    match b {
        b'&' => out.push_str("&amp;"),
        b'<' => out.push_str("&lt;"),
        b'>' => out.push_str("&gt;"),
        _ => out.push(b as char),
    }
}
```

with:

```rust
fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        _ => out.push(c),
    }
}
```

- [ ] **Step 4: Update `linkify`'s non-URL branch to consume a `char`**

In `linkify` (lines 83-103), replace the non-URL branch (lines 97-99):

```rust
        } else {
            push_escaped_char(&mut out, bytes[i]);
            i += 1;
        }
```

with:

```rust
        } else {
            let ch = content[i..].chars().next().unwrap();
            push_escaped_char(&mut out, ch);
            i += ch.len_utf8();
        }
```

Design notes for the implementer:
- `content` is a `&str` (valid UTF-8) and `i` starts at `0`, so `content[i..]` is always on a char boundary.
- `match_url` only advances `i` past an ASCII `http://`/`https://` run (all ASCII bytes, all char boundaries), so the invariant holds after a URL match too.
- `.chars().next().unwrap()` is safe: `content[i..]` is non-empty in the `else` branch (the `if let Some(end) = match_url(...)` only fires when there's a URL; otherwise we're at a real character). The `unwrap()` cannot panic.
- `ch.len_utf8()` advances `i` by the correct number of bytes (1 for ASCII, 2-4 for multi-byte).
- The `bytes` variable (line 84, `let bytes = content.as_bytes();`) is still used by `match_url(&bytes[i..])` on line 88 — keep it.

- [ ] **Step 5: Run the new tests to verify they pass**

Run: `cargo test --lib templates::tests::linkify_preserves templates::tests::linkify_url_adjacent templates::tests::linkify_multibyte_after`
Expected: 5 PASS.

- [ ] **Step 6: Run the full test suite to confirm no regressions**

Run: `cargo test --quiet`
Expected: 80 passed; 0 failed (75 from Task 1 + 5 new). All existing `linkify_*` tests (e.g. `linkify_wraps_http_url`, `linkify_escapes_ampersand_in_query`, `linkify_strips_trailing_punct`) must still pass — ASCII behavior is unchanged.

- [ ] **Step 7: Commit**

```bash
git add src/templates.rs
git commit -m "fix: preserve multi-byte UTF-8 in linkify and push_escaped_char"
```

---

### Task 3: Add `<meta charset="utf-8">` to all HTML templates

**Files:**
- Modify: `src/templates.rs:3-14` (`not_found_page`)
- Modify: `src/templates.rs:22-66` (`view_page`)
- Modify: `src/templates.rs:163-191` (`admin_page`)
- Modify: `src/templates.rs:199-213` (`error_page`)
- Test: `src/templates.rs` (in the existing `#[cfg(test)] mod tests` block)

Each of the four page functions emits `<html><head><title>paste</title>...`. Insert `<meta charset="utf-8">` immediately after `<head>` and before `<title>`.

- [ ] **Step 1: Add failing tests for charset declaration in each template**

Insert these tests immediately after the `linkify_multibyte_after_url` test added in Task 2:

```rust
    #[test]
    fn not_found_page_declares_utf8_charset() {
        let html = not_found_page();
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }

    #[test]
    fn view_page_declares_utf8_charset() {
        let html = view_page("", "anything");
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }

    #[test]
    fn admin_page_declares_utf8_charset() {
        let html = admin_page("", 0, "");
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }

    #[test]
    fn error_page_declares_utf8_charset() {
        let html = error_page("", "oops");
        assert!(html.contains("<meta charset=\"utf-8\">"));
    }
```

- [ ] **Step 2: Run the new tests to verify they fail**

Run: `cargo test --lib templates::tests::declares_utf8_charset`
Expected: 4 FAIL. None of the templates currently emit the meta tag.

- [ ] **Step 3: Add the meta tag to `not_found_page`**

In `not_found_page` (lines 3-14), change:

```rust
<html><head><title>paste</title><style>{}</style></head>
```

to:

```rust
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
```

- [ ] **Step 4: Add the meta tag to `view_page`**

In `view_page` (lines 22-66), change the line (currently line 24):

```rust
<html><head><title>paste</title><style>{}</style></head>
```

to:

```rust
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
```

- [ ] **Step 5: Add the meta tag to `admin_page`**

In `admin_page` (lines 163-191), change the line (currently line 165):

```rust
<html><head><title>paste</title><style>{}</style></head>
```

to:

```rust
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
```

- [ ] **Step 6: Add the meta tag to `error_page`**

In `error_page` (lines 199-213), change the line (currently line 201):

```rust
<html><head><title>paste</title><style>{}</style></head>
```

to:

```rust
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
```

- [ ] **Step 7: Run the new tests to verify they pass**

Run: `cargo test --lib templates::tests::declares_utf8_charset`
Expected: 4 PASS.

- [ ] **Step 8: Run the full test suite to confirm no regressions**

Run: `cargo test --quiet`
Expected: 84 passed; 0 failed (80 from Task 2 + 4 new).

- [ ] **Step 9: Commit**

```bash
git add src/templates.rs
git commit -m "feat: declare UTF-8 charset in all HTML page templates"
```

---

### Task 4: Add `view_page` UTF-8 integration test and end-to-end handler test

**Files:**
- Test: `src/templates.rs` (in the existing `#[cfg(test)] mod tests` block) — `view_page` multi-byte test
- Test: `src/handlers.rs` (in the existing `#[cfg(test)] mod tests` block, after the `create_paste_with_empty_custom_ttl` test around line 696) — end-to-end round-trip test

This task locks in the end-to-end behavior: a paste with multi-byte UTF-8 content renders correctly in the view HTML, and an HTTP round-trip (create then GET) preserves the content.

- [ ] **Step 1: Add a failing `view_page` integration test for multi-byte content**

Insert this test in `src/templates.rs` immediately after the `error_page_declares_utf8_charset` test added in Task 3:

```rust
    #[test]
    fn view_page_renders_multibyte_utf8() {
        let html = view_page("", "héllo\n日本\n🎉");
        assert!(html.contains("data-line=\"héllo\""));
        assert!(html.contains("data-line=\"日本\""));
        assert!(html.contains("data-line=\"🎉\""));
        assert!(html.contains(">héllo<"));
        assert!(html.contains(">日本<"));
        assert!(html.contains(">🎉<"));
    }
```

Note for the implementer: the `>X<` assertions check the rendered span text (between `<span class="lt">` and `</span>`); the `data-line="X"` assertions check the button's attribute. Both must contain the original UTF-8 uncorrupted. If `escape_attr`/`linkify` were still byte-level, "héllo" would render as "hÃ©llo" in both the attribute and the span, failing these assertions.

- [ ] **Step 2: Run the new test to verify it passes (it should already pass after Tasks 1-2)**

Run: `cargo test --lib templates::tests::view_page_renders_multibyte_utf8`
Expected: PASS. This test documents and locks in the end-to-end template behavior. It should pass immediately because Tasks 1-2 fixed the underlying helpers. If it fails, Task 1 or Task 2 was incomplete — go back and fix before proceeding.

- [ ] **Step 3: Add a failing end-to-end handler test for UTF-8 round-trip**

In `src/handlers.rs`, insert this test immediately after the `create_paste_with_empty_custom_ttl` test (currently ending around line 696, before `create_paste_rejects_ttl_exceeds_max`):

```rust
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
                    .header(header::AUTHORIZATION, encode_basic_auth("user", "secret"))
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
        let body = axum::body::to_bytes(resp.into_body(), 8192).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("héllo"));
        assert!(html.contains("日本"));
        assert!(html.contains("🎉"));
    }
```

Design notes for the implementer:
- `body` is the percent-encoded form body. `h%C3%A9llo` is "héllo", `%E6%97%A5%E6%9C%AC` is "日本", `%F0%9F%8E%89` is "🎉". Axum's `serde_urlencoded` form decoder percent-decodes and validates UTF-8, producing a `String` containing "héllo 日本 🎉".
- `test_state()` sets `max_size: 100`, which is well above the encoded body length (~60 bytes) and the decoded content length (~14 bytes), so the paste is accepted.
- The test follows the redirect by reading the `Location` header and issuing a GET to that path, then asserts the response HTML contains the original UTF-8 strings uncorrupted. This is the real end-to-end check: form submission → storage → view rendering.

- [ ] **Step 4: Run the new handler test to verify it passes**

Run: `cargo test --lib handlers::tests::create_and_get_utf8_content`
Expected: PASS. If it fails, the most likely cause is an incomplete helper fix in Task 1 or Task 2 — the assertions on the rendered HTML will catch any remaining byte-level corruption.

- [ ] **Step 5: Run the full test suite to confirm everything passes**

Run: `cargo test --quiet`
Expected: 86 passed; 0 failed (70 baseline + 5 from Task 1 + 5 from Task 2 + 4 from Task 3 + 1 template test from Task 4 Step 1 + 1 handler test from Task 4 Step 3 = 86).

- [ ] **Step 6: Run clippy**

Run: `cargo clippy`
Expected: no warnings. If clippy flags anything, fix it before committing.

- [ ] **Step 7: Commit**

```bash
git add src/templates.rs src/handlers.rs
git commit -m "test: add UTF-8 end-to-end round-trip coverage"
```

---

### Task 5: Update README to document UTF-8 support

**Files:**
- Modify: `README.md:7-19` (Features list)

The README lists features. UTF-8 support is now a real feature worth documenting, since the bug previously corrupted non-ASCII content.

- [ ] **Step 1: Add a "Full UTF-8 support" bullet to the Features list**

In `README.md`, the Features section currently reads:

```markdown
## Features

- Short 4-character IDs with collision handling
- Auto-expiring pastes with configurable TTL (presets or custom)
- Copy-to-clipboard button on paste view
- Admin dashboard at `/` with HTTP Basic Auth (uses the regular user credentials) showing all active pastes and a submit form
- Optional lockdown mode requiring authentication for all routes
- Dark mode via `prefers-color-scheme`
- Security headers (HSTS, X-Content-Type-Options, X-Frame-Options)
- Constant-time credential comparison
- Request body size limit
- TOML configuration file
- Single binary, no external dependencies
```

Insert a new bullet after the "Copy-to-clipboard button on paste view" line:

```markdown
- Full UTF-8 support (non-ASCII paste content renders correctly)
```

- [ ] **Step 2: Commit**

```bash
git add README.md
git commit -m "docs: document UTF-8 support in README features list"
```

---

## Self-Review

**Spec coverage:**
- Spec: "`escape_attr` — char-level iteration" → Task 1, Step 3. ✓
- Spec: "`push_escaped_char` — accept `char`" → Task 2, Step 3. ✓
- Spec: "`linkify` — advance by char width" → Task 2, Step 4. ✓
- Spec: "`<meta charset="utf-8">` in `not_found_page`" → Task 3, Step 3. ✓
- Spec: "`<meta charset="utf-8">` in `view_page`" → Task 3, Step 4. ✓
- Spec: "`<meta charset="utf-8">` in `admin_page`" → Task 3, Step 5. ✓
- Spec: "`<meta charset="utf-8">` in `error_page`" → Task 3, Step 6. ✓
- Spec unit test: "`escape_attr` preserves a single multi-byte char: `escape_attr("é") == "é"`" → Task 1, Step 1 (`escape_attr_preserves_single_multibyte`). ✓
- Spec unit test: "preserves CJK and emoji" → Task 1, Step 1 (`escape_attr_preserves_cjk`, `escape_attr_preserves_emoji`). ✓
- Spec unit test: "still escapes specials adjacent to multi-byte chars: `escape_attr("é&<") == "é&amp;&lt;"`" → Task 1, Step 1 (`escape_attr_escapes_specials_adjacent_to_multibyte`). ✓
- Spec unit test: "`linkify` preserves multi-byte UTF-8 in plain text: `linkify("héllo") == "héllo"`" → Task 2, Step 1 (`linkify_preserves_multibyte_plain_text`). ✓
- Spec unit test: "`linkify` handles a URL adjacent to multi-byte UTF-8" → Task 2, Step 1 (`linkify_url_adjacent_to_multibyte`, `linkify_multibyte_after_url`). ✓
- Spec unit test: "`view_page` renders multi-byte content in both the `data-line` attribute and the display span" → Task 4, Step 1 (`view_page_renders_multibyte_utf8`). ✓
- Spec unit test: "Each of the four templates includes `<meta charset="utf-8">`" → Task 3, Step 1 (four `*_declares_utf8_charset` tests). ✓
- Spec integration test: "Create a paste with multi-byte UTF-8 content via `POST /`, follow the redirect, `GET /{id}`, and assert the response body contains the original content uncorrupted" → Task 4, Step 3 (`create_and_get_utf8_content`). ✓
- Spec: "No other files change" → confirmed. Only `src/templates.rs`, `src/handlers.rs` (test only), and `README.md` are touched. ✓

**Placeholder scan:** No TBDs, TODOs, or "add appropriate error handling" vagueness. All code blocks show the exact code to write. All test functions have bodies. All commands include expected output. ✓

**Type consistency:**
- `escape_attr(s: &str) -> String` — signature unchanged; only the loop variable type changes from `u8` to `char`. Callers (`render_lines` on line 75, `linkify` on line 91) are unaffected. ✓
- `push_escaped_char(out: &mut String, c: char)` — signature changed from `b: u8` to `c: char`. The only caller is `linkify`'s non-URL branch (Task 2, Step 4), which is updated in the same task to pass a `char`. No other callers exist (confirmed by the audit). ✓
- `linkify` keeps `let bytes = content.as_bytes();` (line 84) for `match_url(&bytes[i..])` (line 88) — both unchanged. The non-URL branch switches from `bytes[i]` to `content[i..].chars().next().unwrap()`. The `bytes` and `content` views of the same underlying data are consistent because `i` is always on a char boundary. ✓
- `view_page(prefix: &str, content: &str) -> String` — signature unchanged; Task 3 only edits the format string. ✓

**Test count reconciliation:**
- Baseline: 70.
- Task 1 adds 5 (`escape_attr_*`): 75.
- Task 2 adds 5 (`linkify_preserves_*`, `linkify_url_adjacent_to_multibyte`, `linkify_multibyte_after_url`): 80.
- Task 3 adds 4 (`*_declares_utf8_charset`): 84.
- Task 4 adds 1 in `templates.rs` (`view_page_renders_multibyte_utf8`) and 1 in `handlers.rs` (`create_and_get_utf8_content`): 86.

Final expected count: **86 passed; 0 failed**.
