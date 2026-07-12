const STYLE_STR: &str = include_str!("style.css");

pub fn not_found_page() -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<p>paste not found or expired</p>
</main>
</body></html>"#,
        STYLE_STR
    )
}

pub fn view_page(content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<button onclick="navigator.clipboard.writeText(document.getElementById('content').textContent);this.textContent='copied!';setTimeout(()=>this.textContent='copy',1500)">copy</button>
<pre id="content">{}</pre>
</main>
</body></html>"#,
        STYLE_STR,
        linkify(content)
    )
}

fn linkify(content: &str) -> String {
    let bytes = content.as_bytes();
    let mut out = String::with_capacity(content.len());
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = match_url(&bytes[i..]) {
            let url = &content[i..i + end];
            let url = strip_trailing_punct(url);
            let esc = escape_attr(url);
            out.push_str(&format!(
                "<a href=\"{}\" target=\"_blank\" rel=\"noopener noreferrer\">{}</a>",
                esc, esc
            ));
            i += end;
        } else {
            push_escaped_char(&mut out, bytes[i]);
            i += 1;
        }
    }
    out
}

fn match_url(bytes: &[u8]) -> Option<usize> {
    const SCHEMES: [&[u8]; 2] = [b"http://", b"https://"];
    for scheme in SCHEMES {
        if bytes.len() >= scheme.len() && &bytes[..scheme.len()] == scheme {
            let mut end = scheme.len();
            while end < bytes.len() {
                let c = bytes[end];
                if c.is_ascii_whitespace() || matches!(c, b'<' | b'>' | b'"' | b'\'') {
                    break;
                }
                end += 1;
            }
            return Some(end);
        }
    }
    None
}

fn strip_trailing_punct(url: &str) -> &str {
    let mut end = url.len();
    while end > 0 {
        match url.as_bytes()[end - 1] {
            b'.' | b',' | b';' | b'!' | b'?' | b')' => end -= 1,
            _ => break,
        }
    }
    &url[..end]
}

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

fn push_escaped_char(out: &mut String, b: u8) {
    match b {
        b'&' => out.push_str("&amp;"),
        b'<' => out.push_str("&lt;"),
        b'>' => out.push_str("&gt;"),
        _ => out.push(b as char),
    }
}

pub fn admin_page(prefix: &str, count: usize, rows: &str) -> String {
    let action = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<form method="POST" action="{}">
<textarea name="content" rows="20" autofocus></textarea>
<select name="ttl">
<option value="5">5 minutes</option>
<option value="15" selected>15 minutes</option>
<option value="30">30 minutes</option>
<option value="60">1 hour</option>
<option value="360">6 hours</option>
<option value="720">12 hours</option>
<option value="1440">24 hours</option>
</select>
<input name="ttl_custom" type="number" min="1" placeholder="or custom (minutes)">
<input type="submit" value="paste">
</form>
<h1>{} pastes</h1>
<table>
<tr><th>id</th><th>expires in</th><th>preview</th><th>actions</th></tr>
{}
</table>
</main>
</body></html>"#,
        STYLE_STR, action, count, rows
    )
}

pub fn error_page(prefix: &str, message: &str) -> String {
    let back = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<h1>error</h1>
<p>{}</p>
<p><a href="{}">back</a></p>
</main>
</body></html>"#,
        STYLE_STR,
        html_escape(message),
        back
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn admin_page_form_action_root() {
        let html = admin_page("", 0, "");
        assert!(html.contains("action=\"/\""));
    }

    #[test]
    fn admin_page_form_action_prefix() {
        let html = admin_page("/paste", 0, "");
        assert!(html.contains("action=\"/paste\""));
    }

    #[test]
    fn error_page_back_link_root() {
        let html = error_page("", "oops");
        assert!(html.contains("href=\"/\""));
    }

    #[test]
    fn error_page_back_link_prefix() {
        let html = error_page("/paste", "oops");
        assert!(html.contains("href=\"/paste\""));
    }

    #[test]
    fn linkify_wraps_http_url() {
        assert_eq!(
            linkify("http://example.com"),
            "<a href=\"http://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">http://example.com</a>"
        );
    }

    #[test]
    fn linkify_wraps_https_url() {
        assert_eq!(
            linkify("https://example.com"),
            "<a href=\"https://example.com\" target=\"_blank\" rel=\"noopener noreferrer\">https://example.com</a>"
        );
    }

    #[test]
    fn linkify_plain_text_unchanged() {
        assert_eq!(linkify("hello world"), "hello world");
    }

    #[test]
    fn linkify_escapes_html_in_plain_text() {
        assert_eq!(linkify("a < b & c > d"), "a &lt; b &amp; c &gt; d");
    }

    #[test]
    fn linkify_multiple_urls() {
        let out = linkify("see https://a.com and https://b.com");
        assert!(out.contains("<a href=\"https://a.com\""));
        assert!(out.contains("<a href=\"https://b.com\""));
    }

    #[test]
    fn linkify_strips_trailing_punct() {
        let out = linkify("visit https://example.com.");
        assert!(out.contains("href=\"https://example.com\""));
        assert!(!out.contains("example.com.\""));
        let out = linkify("(see https://example.com)");
        assert!(out.contains("href=\"https://example.com\""));
        assert!(out.contains("(see <a "));
        assert!(!out.contains("example.com)"));
    }

    #[test]
    fn linkify_escapes_ampersand_in_query() {
        let out = linkify("https://example.com/?a=1&b=2");
        assert!(out.contains("href=\"https://example.com/?a=1&amp;b=2\""));
    }

    #[test]
    fn linkify_preserves_surrounding_escape() {
        let out = linkify("<b> see https://x.com </b>");
        assert!(out.contains("&lt;b&gt;"));
        assert!(out.contains("&lt;/b&gt;"));
        assert!(out.contains("<a href=\"https://x.com\""));
    }

    #[test]
    fn linkify_opens_new_tab() {
        let out = linkify("https://example.com");
        assert!(out.contains("target=\"_blank\""));
        assert!(out.contains("rel=\"noopener noreferrer\""));
    }

    #[test]
    fn linkify_text_content_yields_raw() {
        let original = "see https://example.com/?a=1&b=2 here";
        let linkified = linkify(original);
        let stripped = strip_tags_and_decode(&linkified);
        assert_eq!(stripped, original);
    }

    #[test]
    fn linkify_no_javascript_scheme() {
        let out = linkify("javascript:alert(1)");
        assert!(!out.contains("<a "));
        assert!(out.contains("javascript:alert(1)"));
    }

    #[test]
    fn view_page_linkifies_content() {
        let html = view_page("visit https://example.com");
        assert!(html.contains("<a href=\"https://example.com\""));
    }

    #[test]
    fn view_page_copy_button_targets_content() {
        let html = view_page("anything");
        assert!(html.contains("getElementById('content')"));
        assert!(html.contains("id=\"content\""));
        assert!(html.contains(">copy<"));
    }

    fn strip_tags_and_decode(s: &str) -> String {
        let mut out = String::new();
        let mut in_tag = false;
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '<' {
                in_tag = true;
                continue;
            }
            if c == '>' {
                in_tag = false;
                continue;
            }
            if in_tag {
                continue;
            }
            if c == '&' {
                let mut entity = String::from('&');
                while let Some(&nc) = chars.peek() {
                    entity.push(nc);
                    chars.next();
                    if nc == ';' {
                        break;
                    }
                }
                match entity.as_str() {
                    "&amp;" => out.push('&'),
                    "&lt;" => out.push('<'),
                    "&gt;" => out.push('>'),
                    "&quot;" => out.push('"'),
                    other => out.push_str(other),
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}
