macro_rules! styled_page {
    ($title:expr, $body:expr) => {
        concat!(
            r#"<!DOCTYPE html>
<html><head><title>"#,
            $title,
            r#"</title><style>"#,
            include_str!("style.css"),
            r#"</style></head>
<body>
<main>
"#,
            $body,
            r#"
</main>
</body></html>"#
        )
    };
}

pub const SUBMIT_PAGE: &str = styled_page!(
    "paste",
    r#"
<h1>paste</h1>
<form method="POST" action="/">
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
"#
);

pub const NOT_FOUND_PAGE: &str = styled_page!(
    "paste",
    r#"
<p>paste not found or expired</p>
"#
);

const STYLE_STR: &str = include_str!("style.css");

pub fn view_page(content: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<pre>{}</pre>
</main>
</body></html>"#,
        STYLE_STR,
        html_escape(content)
    )
}

pub fn admin_page(count: usize, rows: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste admin</title><style>{}</style></head>
<body>
<main>
<h1>{} pastes</h1>
<table>
<tr><th>id</th><th>expires in</th><th>preview</th></tr>
{}
</table>
</main>
</body></html>"#,
        STYLE_STR, count, rows
    )
}

pub fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn error_page(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<h1>error</h1>
<p>{}</p>
<p><a href="/">back</a></p>
</main>
</body></html>"#,
        STYLE_STR,
        html_escape(message)
    )
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
}
