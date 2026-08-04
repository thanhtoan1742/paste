const STYLE_STR: &str = include_str!("style.css");

pub fn not_found_page() -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
<body>
<main>
<p>paste not found or expired</p>
</main>
</body></html>"#,
        STYLE_STR
    )
}

pub fn view_page(prefix: &str, content: &str) -> String {
    let home = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
<body>
<main>
<button class="copy" onclick="copyText([...document.querySelectorAll('.ln')].map(b=>b.dataset.line).join('\n'));flashAllCopied();this.textContent='copied!';setTimeout(()=>this.textContent='copy',1500)">copy</button>
<a class="home" href="{}">home</a>
<div class="lines">
{}
</div>
<script>
function copyText(text){{
 if(navigator.clipboard && window.isSecureContext){{ return navigator.clipboard.writeText(text); }}
 var ta=document.createElement('textarea'); ta.value=text;
 ta.style.position='fixed'; ta.style.opacity='0';
 document.body.appendChild(ta); ta.focus(); ta.select();
 try{{ document.execCommand('copy'); }}catch(e){{}}
 document.body.removeChild(ta);
}}
let sel=null;
function toggleLine(el){{
 if(sel===el){{
  copyText(el.querySelector('.ln').dataset.line);
  el.classList.add('copied');setTimeout(()=>el.classList.remove('copied'),1200);
 }}else{{ if(sel)sel.classList.remove('selected'); sel=el; el.classList.add('selected'); }}
}}
function copyLine(e,btn){{
 e.stopPropagation();
 copyText(btn.dataset.line);
 var el=btn.closest('.line');
 el.classList.add('copied');setTimeout(()=>el.classList.remove('copied'),1200);
}}
function flashAllCopied(){{
 var ls=document.querySelectorAll('.line');
 ls.forEach(el=>el.classList.add('copied'));
 setTimeout(()=>ls.forEach(el=>el.classList.remove('copied')),1200);
}}
</script>
</main>
</body></html>"#,
        STYLE_STR,
        home,
        render_lines(content)
    )
}

fn render_lines(content: &str) -> String {
    let mut out = String::new();
    for (i, line) in content.split('\n').enumerate() {
        let line = line.strip_suffix('\r').unwrap_or(line);
        let n = i + 1;
        out.push_str(&format!(
            "<div class=\"line\" onclick=\"toggleLine(this)\"><button class=\"ln\" data-line=\"{}\" onclick=\"copyLine(event,this)\">{}</button><span class=\"lt\">{}</span></div>",
            escape_attr(line),
            n,
            linkify(line)
        ));
    }
    out
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
            let ch = content[i..].chars().next().unwrap();
            push_escaped_char(&mut out, ch);
            i += ch.len_utf8();
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
                if !c.is_ascii() || c.is_ascii_whitespace() || matches!(c, b'<' | b'>' | b'"' | b'\'') {
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

fn push_escaped_char(out: &mut String, c: char) {
    match c {
        '&' => out.push_str("&amp;"),
        '<' => out.push_str("&lt;"),
        '>' => out.push_str("&gt;"),
        _ => out.push(c),
    }
}

pub fn view_image_page(prefix: &str, mime_type: &str, filename: &str, size_bytes: usize, b64_data: &str) -> String {
    let home = if prefix.is_empty() {
        "/".to_string()
    } else {
        prefix.to_string()
    };
    let size_human = format_size(size_bytes);
    let data_uri = format!("data:{};base64,{}", mime_type, b64_data);
    format!(
        r#"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>paste</title><style>{style}</style></head>
<body>
<main>
<button class="copy" onclick="copyImage('{data_uri_esc}');this.textContent='copied!';setTimeout(()=>this.textContent='copy',1500)">copy</button>
<a class="home" href="{home}">home</a>
<div class="image-container"><img src="{data_uri}" alt="{esc_filename}" class="paste-image"></div>
<p class="image-info">{esc_filename} &mdash; {size_human}</p>
<script>
async function copyImage(uri){{
 try{{
  const r=await fetch(uri);
  const b=await r.blob();
  await navigator.clipboard.write([new ClipboardItem({{[b.type]:b}})]);
 }}catch(e){{window.open(uri,'_blank');}}
}}
</script>
</main>
</body></html>"#,
        style = STYLE_STR,
        data_uri_esc = html_escape(&data_uri),
        data_uri = data_uri,
        home = home,
        esc_filename = html_escape(filename),
        size_human = size_human,
    )
}

pub fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
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
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
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
<html><head><meta charset="utf-8"><title>paste</title><style>{}</style></head>
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
        let html = view_page("", "visit https://example.com");
        assert!(html.contains("<a href=\"https://example.com\""));
    }

    #[test]
    fn view_page_copy_all_uses_line_buttons() {
        let html = view_page("", "anything");
        assert!(html.contains("querySelectorAll('.ln')"));
        assert!(html.contains("join('\\n')"));
        assert!(html.contains(">copy<"));
    }

    #[test]
    fn view_page_renders_line_numbers() {
        let html = view_page("", "line one\nline two\nline three");
        assert!(html.contains(">1<"));
        assert!(html.contains(">2<"));
        assert!(html.contains(">3<"));
    }

    #[test]
    fn view_page_line_button_escapes_data_line() {
        let html = view_page("", "a & b < c > d");
        assert!(html.contains("data-line=\"a &amp; b &lt; c &gt; d\""));
    }

    #[test]
    fn view_page_empty_lines_keep_numbers() {
        let html = view_page("", "first\n\nthird");
        assert!(html.contains(">1<"));
        assert!(html.contains(">2<"));
        assert!(html.contains(">3<"));
    }

    #[test]
    fn view_page_strips_carriage_returns() {
        let html = view_page("", "a\r\nb");
        assert!(html.contains("data-line=\"a\""));
        assert!(html.contains("data-line=\"b\""));
        assert!(!html.contains("\r"));
    }

    #[test]
    fn view_page_linkify_per_line() {
        let html = view_page("", "see https://example.com\nno url here");
        assert!(html.contains("<a href=\"https://example.com\""));
    }

    #[test]
    fn view_page_line_has_toggle_handler() {
        let html = view_page("", "line one\nline two");
        assert!(html.contains("onclick=\"toggleLine(this)\""));
        assert!(html.contains("class=\"line\""));
    }

    #[test]
    fn view_page_line_number_direct_copy_handler() {
        let html = view_page("", "anything");
        assert!(html.contains("copyLine(event,this)"));
        assert!(html.contains("stopPropagation"));
    }

    #[test]
    fn view_page_has_selection_script() {
        let html = view_page("", "anything");
        assert!(html.contains("<script>"));
        assert!(html.contains("function toggleLine"));
        assert!(html.contains("function copyLine"));
        assert!(html.contains("classList.add('selected')"));
        assert!(html.contains("classList.add('copied')"));
    }

    #[test]
    fn view_page_has_copy_text_helper() {
        let html = view_page("", "anything");
        assert!(html.contains("function copyText"));
        assert!(html.contains("isSecureContext"));
    }

    #[test]
    fn view_page_copy_falls_back_to_exec_command() {
        let html = view_page("", "anything");
        assert!(html.contains("document.execCommand('copy')"));
    }

    #[test]
    fn view_page_copy_all_uses_copy_text() {
        let html = view_page("", "anything");
        assert!(html.contains("copyText([...document.querySelectorAll('.ln')"));
    }

    #[test]
    fn view_page_copy_all_flashes_all_lines() {
        let html = view_page("", "anything");
        assert!(html.contains("function flashAllCopied"));
        assert!(html.contains("flashAllCopied()"));
    }

    #[test]
    fn view_page_home_link_root() {
        let html = view_page("", "anything");
        assert!(html.contains("href=\"/\""));
        assert!(html.contains(">home<"));
    }

    #[test]
    fn view_page_home_link_prefix() {
        let html = view_page("/paste", "anything");
        assert!(html.contains("href=\"/paste\""));
        assert!(html.contains(">home<"));
    }

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

    #[test]
    fn view_image_page_contains_img() {
        let html = view_image_page("", "image/png", "s.png", 1024, "abc");
        assert!(html.contains("<img src=\"data:image/png;base64,abc\""));
        assert!(html.contains("s.png"));
        assert!(html.contains("1.0 KB"));
    }

    #[test]
    fn view_image_page_escapes_filename() {
        let html = view_image_page("", "image/png", "<x>.png", 500, "a");
        assert!(html.contains("&lt;x&gt;.png"));
    }

    #[test]
    fn view_image_page_home_link_prefix() {
        let html = view_image_page("/paste", "image/png", "x.png", 100, "abc");
        assert!(html.contains("href=\"/paste\""));
    }

    #[test]
    fn format_size_values() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(500), "500 B");
        assert_eq!(format_size(1536), "1.5 KB");
        assert_eq!(format_size(1048576), "1.0 MB");
        assert_eq!(format_size(20971520), "20.0 MB");
    }
}
