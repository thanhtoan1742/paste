pub struct Field {
    pub name: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
    pub data: Vec<u8>,
}

/// Parse a multipart/form-data body given the raw bytes and boundary string.
/// The boundary should be the value from the Content-Type header (without leading --).
pub fn parse_multipart(body: &[u8], boundary: &str) -> Result<Vec<Field>, String> {
    let delimiter = format!("--{}", boundary);
    let delimiter_bytes = delimiter.as_bytes();
    let close_delimiter = format!("--{}--", boundary);
    let close_bytes = close_delimiter.as_bytes();

    let mut fields = Vec::new();
    let mut pos = 0;

    // Find first boundary
    pos = find_boundary(body, delimiter_bytes, pos)?;

    loop {
        // Move past the boundary line (skip \r\n)
        pos = skip_line(body, pos);

        // Check if we hit the closing boundary
        if pos + close_bytes.len() <= body.len() && &body[pos..pos + close_bytes.len()] == close_bytes {
            break;
        }

        // Parse headers until empty line (\r\n\r\n)
        let mut name: Option<String> = None;
        let mut filename: Option<String> = None;
        let mut content_type: Option<String> = None;

        loop {
            if pos + 2 <= body.len() && body[pos] == b'\r' && body[pos + 1] == b'\n' {
                pos += 2; // skip empty line
                break;
            }
            let line_end = find_crlf(body, pos)?;
            let line = &body[pos..line_end];
            pos = line_end + 2; // skip \r\n

            if let Ok(line_str) = std::str::from_utf8(line) {
                if let Some(value) = line_str.strip_prefix("Content-Disposition: ") {
                    for part in value.split(';') {
                        let part = part.trim();
                        if let Some(v) = part.strip_prefix("name=") {
                            name = Some(unquote(v));
                        } else if let Some(v) = part.strip_prefix("filename=") {
                            filename = Some(unquote(v));
                        }
                    }
                } else if let Some(value) = line_str.strip_prefix("Content-Type: ") {
                    content_type = Some(value.trim().to_string());
                }
            }
        }

        // Read data until next boundary
        let data_start = pos;
        pos = find_boundary(body, delimiter_bytes, pos)?;
        let data_end = if pos >= 2 && body[pos - 2] == b'\r' && body[pos - 1] == b'\n' {
            pos - 2
        } else {
            pos
        };

        fields.push(Field {
            name: name.unwrap_or_default(),
            filename,
            content_type,
            data: body[data_start..data_end].to_vec(),
        });
    }

    Ok(fields)
}

fn find_boundary(body: &[u8], delimiter: &[u8], start: usize) -> Result<usize, String> {
    // Linear search for delimiter
    for i in start..body.len() {
        if i + delimiter.len() <= body.len() && &body[i..i + delimiter.len()] == delimiter {
            return Ok(i);
        }
    }
    // If not found, return end of body (we're done)
    Ok(body.len())
}

fn find_crlf(body: &[u8], start: usize) -> Result<usize, String> {
    for i in start..body.len() {
        if i + 1 < body.len() && body[i] == b'\r' && body[i + 1] == b'\n' {
            return Ok(i);
        }
    }
    Err("expected CRLF".to_string())
}

fn skip_line(body: &[u8], pos: usize) -> usize {
    if pos + 2 <= body.len() && body[pos] == b'\r' && body[pos + 1] == b'\n' {
        pos + 2
    } else if pos < body.len() && body[pos] == b'\n' {
        pos + 1
    } else {
        pos
    }
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if s.len() >= 2 && s.starts_with('"') && s.ends_with('"') {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_single_text_field() {
        let body = b"------boundary\r\n\
Content-Disposition: form-data; name=\"content\"\r\n\
\r\n\
hello world\r\n\
------boundary--\r\n";
        let fields = parse_multipart(body, "----boundary").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "content");
        assert_eq!(fields[0].data, b"hello world");
        assert!(fields[0].filename.is_none());
    }

    #[test]
    fn parse_multiple_fields() {
        let body = b"------boundary\r\n\
Content-Disposition: form-data; name=\"content\"\r\n\
\r\n\
hello\r\n\
------boundary\r\n\
Content-Disposition: form-data; name=\"ttl\"\r\n\
\r\n\
30\r\n\
------boundary--\r\n";
        let fields = parse_multipart(body, "----boundary").unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].name, "content");
        assert_eq!(fields[0].data, b"hello");
        assert_eq!(fields[1].name, "ttl");
        assert_eq!(fields[1].data, b"30");
    }

    #[test]
    fn parse_file_field() {
        let body = b"------boundary\r\n\
Content-Disposition: form-data; name=\"image\"; filename=\"test.png\"\r\n\
Content-Type: image/png\r\n\
\r\n\
fakeimagedata\r\n\
------boundary--\r\n";
        let fields = parse_multipart(body, "----boundary").unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "image");
        assert_eq!(fields[0].filename.as_deref(), Some("test.png"));
        assert_eq!(fields[0].content_type.as_deref(), Some("image/png"));
        assert_eq!(fields[0].data, b"fakeimagedata");
    }

    #[test]
    fn parse_empty_body() {
        let body = b"------boundary--\r\n";
        let fields = parse_multipart(body, "----boundary").unwrap();
        assert_eq!(fields.len(), 0);
    }
}
