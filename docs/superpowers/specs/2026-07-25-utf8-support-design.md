# UTF-8 Support

## Problem

Paste content containing non-ASCII characters is corrupted in the paste view. For example, "é" (UTF-8 bytes `C3 A9`) renders as "Ã©" — two wrong codepoints. The root cause is in `src/templates.rs`: `escape_attr` and `push_escaped_char` iterate over bytes and cast each byte with `b as char`, which takes the raw byte value as a Unicode codepoint instead of decoding the UTF-8 sequence. These functions feed the `data-line` attribute and the line display text in the paste view, so any multi-byte UTF-8 content is mangled.

## Goal

Non-ASCII paste content renders correctly in the paste view, and all HTML templates explicitly declare UTF-8 so the page renders correctly even when viewed outside the browser's HTTP response (e.g. saved to disk and reopened).

## Non-Goals

- No new crates (the project stays dependency-free beyond its existing set).
- No changes to paste storage (`String` is already valid UTF-8).
- No changes to form parsing (Axum/`serde_urlencoded` already decode UTF-8 correctly).
- No changes to the URL-linkify scheme *detection* (`match_url` keeps matching ASCII `http://`/`https://` only). (Note: during implementation, `match_url`'s *scan loop* was found to absorb trailing non-ASCII bytes into the URL — see "Implementation deviation" below — and was fixed.)
- No internationalization of the UI text itself (button labels, headings stay English).

## Approach

Change is localized to `src/templates.rs`. Switch the two corrupting helpers from byte iteration to char iteration, and add a `<meta charset="utf-8">` declaration to each of the four HTML page templates. (Implementation also fixed `match_url`'s scan loop and added tests in `src/handlers.rs` and a README bullet — see "Implementation Deviation" and "Files Touched".)

### Audit Summary

A full audit of every string-handling path in the codebase:

| Path | Status | Notes |
|------|--------|-------|
| `templates::escape_attr` | **Bug** | Iterates `s.bytes()`, pushes `b as char` → corrupts multi-byte UTF-8. |
| `templates::push_escaped_char` | **Bug** | Takes `u8`, pushes `b as char` → same corruption. |
| `templates::linkify` | **Bug** | Byte-by-byte loop calls `push_escaped_char` per byte → corrupts non-ASCII. |
| `templates::html_escape` | Safe | Uses `str::replace`, operates on `str`. |
| `templates::match_url` | Fixed (was: Safe-ish) | Scheme detection is ASCII-only and correct, but the scan loop absorbed trailing non-ASCII bytes into the URL. Fixed during implementation (see below). |
| `templates::strip_trailing_punct` | Safe | Matches ASCII punctuation only. |
| `handlers::render_admin` preview | Safe | Uses `.chars().take(100)`. |
| `auth::check_basic_auth` | Safe | Uses `std::str::from_utf8`. |
| `config::normalize_prefix` | Safe | ASCII-only `/` operations. |
| Form parsing (`PasteForm`) | Safe | Axum/`serde_urlencoded` handles UTF-8. |
| Paste storage (`PasteEntry::content`) | Safe | `String` is always valid UTF-8. |
| `main::parse_config_path` | Safe | CLI args, not user paste content. |

Only the three functions flagged **Bug** need code changes; everything else is already UTF-8 correct.

## Components

### 1. `escape_attr` — char-level iteration

Change `for b in s.bytes()` to `for c in s.chars()`. Match on `char` for the four HTML-special characters (`&`, `<`, `>`, `"`); push any other `char` directly. `char` is a full Unicode scalar value, so multi-byte sequences decode before escaping and re-encode correctly when pushed to the `String`.

### 2. `push_escaped_char` — accept `char`

Change the signature from `(out: &mut String, b: u8)` to `(out: &mut String, c: char)`. Match on `char` for the three HTML specials (`&`, `<`, `>`); push `c` directly otherwise.

### 3. `linkify` — advance by char width

In the non-URL branch, instead of consuming one byte, consume one `char`:
- `let ch = content[i..].chars().next().unwrap();`
- `push_escaped_char(&mut out, ch);`
- `i += ch.len_utf8();`

`content` is valid UTF-8 and `i` starts at 0, so `content[i..]` is always on a char boundary. URL detection (`match_url`) only advances `i` to the end of an ASCII `http://`/`https://` run, which is also a char boundary, so the invariant holds.

### 4. `<meta charset="utf-8">` in all templates

Add `<meta charset="utf-8">` immediately after `<head>` in:
- `not_found_page`
- `view_page`
- `admin_page`
- `error_page`

Axum's `Html` response type already sends `Content-Type: text/html; charset=utf-8`, so this is belt-and-suspenders for cases where the page is consumed outside the HTTP response (saved to disk, proxied with stripped headers, etc.). The in-page declaration takes precedence in browsers when both are present and consistent, and is authoritative when the HTTP header is missing.

## Data Flow

```
Paste content (valid UTF-8 String)
  -> render_lines
       -> escape_attr (char-level)        for data-line attribute
       -> linkify
            -> match_url (byte-level)     detects ASCII URL run
            -> push_escaped_char (char)   escapes non-URL text
       -> <div class="line"> with correct UTF-8 in both attribute and display
  -> view_page HTML
       -> <meta charset="utf-8"> declared
  -> axum::response::Html
       -> Content-Type: text/html; charset=utf-8
  -> browser renders "é" as "é"
```

## Testing

### Unit tests (`templates.rs`)

- `escape_attr` preserves a single multi-byte char: `escape_attr("é") == "é"`.
- `escape_attr` preserves CJK and emoji: `escape_attr("日本") == "日本"`, `escape_attr("a🎉b") == "a🎉b"`.
- `escape_attr` still escapes specials adjacent to multi-byte chars: `escape_attr("é&<") == "é&amp;&lt;"`.
- `linkify` preserves multi-byte UTF-8 in plain text: `linkify("héllo") == "héllo"`.
- `linkify` handles a URL adjacent to multi-byte UTF-8: `linkify("éhttps://x.com")` wraps the URL and leaves `é` intact.
- `view_page` renders multi-byte content in both the `data-line` attribute and the display span.
- Each of the four templates includes `<meta charset="utf-8">`.

### Integration test (`handlers.rs`)

- Create a paste with multi-byte UTF-8 content via `POST /`, follow the redirect, `GET /{id}`, and assert the response body contains the original content uncorrupted.

### Existing tests

The existing template and handler tests must continue to pass. The byte→char switch changes no behavior for ASCII inputs, so all existing assertions hold.

## Files Touched

- `src/templates.rs` — rewrite `escape_attr`, `push_escaped_char`, `linkify` non-URL branch; add `<meta charset="utf-8">` to all four page functions; add unit tests.
- `src/handlers.rs` — add end-to-end UTF-8 round-trip test (test-only).
- `README.md` — document UTF-8 support in the features list.
- `src/templates.rs` `match_url` — see "Implementation deviation" below.

## Implementation Deviation

During implementation (Task 2), the audit's "Safe" label for `match_url` proved inaccurate for its *scan loop* (not its scheme detection). The original `match_url` advanced `end` until it hit ASCII whitespace or `< > " '`, but did not break on non-ASCII bytes — so a URL adjacent to multi-byte text (e.g. `https://x.comé`) absorbed the `é` into the URL (`<a href="https://x.comé">`), contradicting this spec's design note that "`match_url` only advances `i` past ASCII bytes."

Fix (authorized scope expansion): added `!c.is_ascii()` as a URL terminator in `match_url`'s scan loop. The URL now ends at `.com` and the `é` is emitted by the (fixed) non-URL branch. ASCII URL behavior is unchanged (all existing `linkify_*` tests pass). The char-boundary invariant is preserved: non-ASCII break points are lead bytes (char boundaries), and ASCII terminators are 1-byte chars.

The scheme *detection* (matching `http://`/`https://`) was always ASCII-only and remains unchanged.
