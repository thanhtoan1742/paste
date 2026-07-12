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
        html_escape(content)
    )
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
}
