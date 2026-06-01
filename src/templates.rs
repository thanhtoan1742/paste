pub const SUBMIT_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body>
<form method="POST" action="/">
<textarea name="content" rows="20" cols="80"></textarea><br>
<input type="submit" value="paste">
</form>
</body></html>"#;

pub const NOT_FOUND_PAGE: &str = r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body><p>paste not found or expired</p></body></html>"#;

pub fn view_page(content: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html><head><title>paste</title></head>
<body><pre>{}</pre></body></html>"#, html_escape(content))
}

pub fn admin_page(count: usize, rows: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html><head><title>paste admin</title></head>
<body>
<h1>{} pastes</h1>
<table>
<tr><th>id</th><th>expires in</th><th>preview</th></tr>
{}
</table>
</body></html>"#, count, rows)
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
}
