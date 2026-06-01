use axum::http::{header, StatusCode};

pub fn base64_decode(input: &str) -> Result<Vec<u8>, ()> {
    const T: [i8; 128] = [
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1,
        -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, -1, 62, -1, -1,
        -1, 63, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, -1, -1, -1, -1, -1, -1, -1, 0, 1, 2, 3, 4,
        5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, -1, -1, -1,
        -1, -1, -1, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45,
        46, 47, 48, 49, 50, 51, -1, -1, -1, -1, -1,
    ];

    let bytes: Vec<u8> = input
        .bytes()
        .filter(|&b| b != b'\n' && b != b'\r')
        .collect();
    if bytes.len() % 4 != 0 {
        return Err(());
    }

    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let v: [i8; 4] = std::array::from_fn(|i| {
            if chunk[i] == b'=' {
                0
            } else {
                T.get(chunk[i] as usize).copied().unwrap_or(-1)
            }
        });
        if v.iter().any(|&x| x < 0) {
            return Err(());
        }

        let pad2 = chunk.len() > 2 && chunk[2] == b'=';
        let pad3 = chunk.len() > 3 && chunk[3] == b'=';

        out.push(((v[0] as u8) << 2) | ((v[1] as u8) >> 4));
        if !pad2 {
            out.push(((v[1] as u8 & 0xf) << 4) | ((v[2] as u8) >> 2));
        }
        if !pad3 {
            out.push(((v[2] as u8 & 0x3) << 6) | (v[3] as u8));
        }
    }
    Ok(out)
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        let mut acc: u8 = 0;
        for &byte in a {
            acc |= byte;
        }
        for &byte in b {
            acc |= byte;
        }
        acc == 0
    } else {
        let mut acc: u8 = 0;
        for i in 0..a.len() {
            acc |= a[i] ^ b[i];
        }
        acc == 0
    }
}

pub fn check_basic_auth(auth_header: &str, expected_user: &str, expected_pass: &str) -> bool {
    let Some(encoded) = auth_header.strip_prefix("Basic ") else {
        return false;
    };
    let Ok(decoded) = base64_decode(encoded) else {
        return false;
    };
    let Ok(creds) = std::str::from_utf8(&decoded) else {
        return false;
    };
    let Some((user, pass)) = creds.split_once(':') else {
        return false;
    };
    constant_time_eq(user.as_bytes(), expected_user.as_bytes())
        && constant_time_eq(pass.as_bytes(), expected_pass.as_bytes())
}

pub fn unauthorized_response() -> (
    StatusCode,
    [(axum::http::header::HeaderName, &'static str); 1],
    axum::response::Html<String>,
) {
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, r#"Basic realm="paste admin""#)],
        axum::response::Html(String::new()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decode_valid() {
        assert_eq!(base64_decode("aGVsbG8=").unwrap(), b"hello");
    }

    #[test]
    fn base64_decode_invalid_length() {
        assert!(base64_decode("abc").is_err());
    }

    #[test]
    fn base64_decode_invalid_chars() {
        assert!(base64_decode("!!!!").is_err());
    }

    #[test]
    fn base64_decode_padding() {
        assert_eq!(base64_decode("YQ==").unwrap(), b"a");
        assert_eq!(base64_decode("YWI=").unwrap(), b"ab");
    }

    #[test]
    fn check_basic_auth_valid() {
        let auth = "Basic YWRtaW46c2VjcmV0";
        assert!(check_basic_auth(auth, "admin", "secret"));
    }

    #[test]
    fn check_basic_auth_wrong_password() {
        let auth = "Basic YWRtaW46d3Jvbmc=";
        assert!(!check_basic_auth(auth, "admin", "secret"));
    }

    #[test]
    fn check_basic_auth_no_prefix() {
        assert!(!check_basic_auth("something", "admin", "secret"));
    }
}
