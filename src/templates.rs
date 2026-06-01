macro_rules! styled_page {
    ($title:expr, $body:expr) => {
        concat!(
r#"<!DOCTYPE html>
<html><head><title>"#,
$title,
r#"</title><style>
:root {
  --sans-font: -apple-system, BlinkMacSystemFont, "Avenir Next", Avenir,
    "Nimbus Sans L", Roboto, "Noto Sans", "Segoe UI", Arial, Helvetica,
    "Helvetica Neue", sans-serif;
  --mono-font: Consolas, Menlo, Monaco, "Andale Mono", "Ubuntu Mono", monospace;
  --bg: #fff;
  --accent-bg: #f5f7ff;
  --text: #212121;
  --text-light: #585858;
  --border: #898EA4;
  --accent: #0d47a1;
  --accent-hover: #1266e2;
  --accent-text: #fff;
}
@media (prefers-color-scheme: dark) {
  :root {
    color-scheme: dark;
    --bg: #212121;
    --accent-bg: #2b2b2b;
    --text: #dcdcdc;
    --text-light: #ababab;
    --accent: #ffb300;
    --accent-hover: #ffe099;
    --accent-text: #212121;
  }
}
*, *::before, *::after { box-sizing: border-box; }
html { font-family: var(--sans-font); }
body {
  color: var(--text);
  background-color: var(--bg);
  font-size: 0.95rem;
  line-height: 1.5;
  display: grid;
  grid-template-columns: 1fr min(45rem, 90%) 1fr;
  margin: 0;
}
body > * { grid-column: 2; }
main { padding-top: 1.5rem; }
a, a:visited { color: var(--accent); }
a:hover { text-decoration: none; }
h1 { font-size: 2rem; line-height: 1.1; }
table { border-collapse: collapse; margin: 1.5rem 0; width: 100%; }
td, th { border: 1px solid var(--border); text-align: start; padding: 0.5rem; }
th { background-color: var(--accent-bg); font-weight: bold; }
tr:nth-child(even) { background-color: var(--accent-bg); }
textarea, input {
  font-size: inherit;
  font-family: var(--mono-font);
  padding: 0.5rem;
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  background-color: var(--bg);
  max-width: 100%;
}
textarea { width: 100%; display: block; margin-bottom: 0.5rem; }
input[type="submit"] {
  background-color: var(--accent);
  color: var(--accent-text);
  border: 1px solid var(--accent);
  border-radius: 5px;
  padding: 0.5em 1.5em;
  cursor: pointer;
  font-family: var(--sans-font);
}
input[type="submit"]:hover {
  background-color: var(--accent-hover);
  border-color: var(--accent-hover);
}
pre {
  font-family: var(--mono-font);
  background-color: var(--accent-bg);
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 1rem 1.4rem;
  overflow: auto;
  white-space: pre-wrap;
  word-wrap: break-word;
}
</style></head>
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

pub const SUBMIT_PAGE: &str = styled_page!("paste", r#"
<h1>paste</h1>
<form method="POST" action="/">
<textarea name="content" rows="20" autofocus></textarea>
<input type="submit" value="paste">
</form>
"#);

pub const NOT_FOUND_PAGE: &str = styled_page!("paste", r#"
<p>paste not found or expired</p>
"#);

const STYLE_STR: &str = r#"
:root {
  --sans-font: -apple-system, BlinkMacSystemFont, "Avenir Next", Avenir,
    "Nimbus Sans L", Roboto, "Noto Sans", "Segoe UI", Arial, Helvetica,
    "Helvetica Neue", sans-serif;
  --mono-font: Consolas, Menlo, Monaco, "Andale Mono", "Ubuntu Mono", monospace;
  --bg: #fff;
  --accent-bg: #f5f7ff;
  --text: #212121;
  --text-light: #585858;
  --border: #898EA4;
  --accent: #0d47a1;
  --accent-hover: #1266e2;
  --accent-text: #fff;
}
@media (prefers-color-scheme: dark) {
  :root {
    color-scheme: dark;
    --bg: #212121;
    --accent-bg: #2b2b2b;
    --text: #dcdcdc;
    --text-light: #ababab;
    --accent: #ffb300;
    --accent-hover: #ffe099;
    --accent-text: #212121;
  }
}
*, *::before, *::after { box-sizing: border-box; }
html { font-family: var(--sans-font); }
body {
  color: var(--text);
  background-color: var(--bg);
  font-size: 0.95rem;
  line-height: 1.5;
  display: grid;
  grid-template-columns: 1fr min(45rem, 90%) 1fr;
  margin: 0;
}
body > * { grid-column: 2; }
main { padding-top: 1.5rem; }
a, a:visited { color: var(--accent); }
a:hover { text-decoration: none; }
h1 { font-size: 2rem; line-height: 1.1; }
table { border-collapse: collapse; margin: 1.5rem 0; width: 100%; }
td, th { border: 1px solid var(--border); text-align: start; padding: 0.5rem; }
th { background-color: var(--accent-bg); font-weight: bold; }
tr:nth-child(even) { background-color: var(--accent-bg); }
textarea, input {
  font-size: inherit;
  font-family: var(--mono-font);
  padding: 0.5rem;
  border: 1px solid var(--border);
  border-radius: 5px;
  color: var(--text);
  background-color: var(--bg);
  max-width: 100%;
}
textarea { width: 100%; display: block; margin-bottom: 0.5rem; }
input[type="submit"] {
  background-color: var(--accent);
  color: var(--accent-text);
  border: 1px solid var(--accent);
  border-radius: 5px;
  padding: 0.5em 1.5em;
  cursor: pointer;
  font-family: var(--sans-font);
}
input[type="submit"]:hover {
  background-color: var(--accent-hover);
  border-color: var(--accent-hover);
}
pre {
  font-family: var(--mono-font);
  background-color: var(--accent-bg);
  border: 1px solid var(--border);
  border-radius: 5px;
  padding: 1rem 1.4rem;
  overflow: auto;
  white-space: pre-wrap;
  word-wrap: break-word;
}
"#;

pub fn view_page(content: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html><head><title>paste</title><style>{}</style></head>
<body>
<main>
<pre>{}</pre>
</main>
</body></html>"#, STYLE_STR, html_escape(content))
}

pub fn admin_page(count: usize, rows: &str) -> String {
    format!(r#"<!DOCTYPE html>
<html><head><title>paste admin</title><style>{}</style></head>
<body>
<main>
<h1>{} pastes</h1>
<table>
<tr><th>id</th><th>expires in</th><th>preview</th></tr>
{}
</table>
</main>
</body></html>"#, STYLE_STR, count, rows)
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
