use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Token: `base64url(payload).base64url(hmac_sha256(secret, payload))`
/// payload = `username:expiry_unix_secs`
///
/// The expiry is checked on validation. Tokens are stateless: they can be
/// verified with only the shared secret, so no server-side session store is
/// needed.
pub fn create_token(secret: &[u8], username: &str, ttl_secs: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let expiry = now + ttl_secs;
    let payload = format!("{}:{}", username, expiry);

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let sig = mac.finalize().into_bytes();

    format!("{}.{}", base64url_encode(payload.as_bytes()), base64url_encode(&sig))
}

/// Validate a token, returning `Some(username)` if the signature matches and
/// the token has not expired.
pub fn validate_token(secret: &[u8], token: &str) -> Option<String> {
    let (payload_b64, sig_b64) = token.split_once('.')?;
    let payload = base64url_decode(payload_b64).ok()?;
    let sig = base64url_decode(sig_b64).ok()?;

    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC accepts any key length");
    mac.update(&payload);
    mac.verify_slice(&sig).ok()?;

    let payload_str = std::str::from_utf8(&payload).ok()?;
    let (username, expiry_str) = payload_str.rsplit_once(':')?;
    let expiry: u64 = expiry_str.parse().ok()?;

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(u64::MAX);

    if now >= expiry {
        return None;
    }

    Some(username.to_string())
}

/// Constant-time comparison for legacy checks (kept out of Basic auth path).
pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc: u8 = 0;
    for i in 0..a.len() {
        acc |= a[i] ^ b[i];
    }
    acc == 0
}

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
    if !bytes.len().is_multiple_of(4) {
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

fn base64url_encode(data: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = if chunk.len() > 1 { chunk[1] as u32 } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(TABLE[((triple >> 18) & 0x3F) as usize] as char);
        out.push(TABLE[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            TABLE[((triple >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            TABLE[(triple & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64url_decode(input: &str) -> Result<Vec<u8>, ()> {
    let mut s = input.replace('-', "+").replace('_', "/");
    while !s.len().is_multiple_of(4) {
        s.push('=');
    }
    base64_decode(&s)
}

/// Response for unauthenticated access to protected routes. Redirects to the
/// login page instead of using a WWW-Authenticate challenge.
pub fn login_redirect(prefix: &str) -> axum::response::Redirect {
    let target = if prefix.is_empty() {
        "/login".to_string()
    } else {
        format!("{}/login", prefix)
    };
    axum::response::Redirect::to(&target)
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
    fn token_roundtrip() {
        let secret = b"test-secret";
        let token = create_token(secret, "admin", 3600);
        assert_eq!(validate_token(secret, &token), Some("admin".to_string()));
    }

    #[test]
    fn token_rejects_wrong_secret() {
        let token = create_token(b"secret-a", "admin", 3600);
        assert_eq!(validate_token(b"secret-b", &token), None);
    }

    #[test]
    fn token_rejects_tampered_payload() {
        let secret = b"test-secret";
        let token = create_token(secret, "admin", 3600);
        // Tamper with the username portion of the payload
        let parts: Vec<&str> = token.split('.').collect();
        let payload = base64url_decode(parts[0]).unwrap();
        let tampered = String::from_utf8(payload).unwrap().replace("admin", "attacker");
        let tampered_b64 = base64url_encode(tampered.as_bytes());
        let tampered_token = format!("{}.{}", tampered_b64, parts[1]);
        assert_eq!(validate_token(secret, &tampered_token), None);
    }

    #[test]
    fn token_rejects_expired() {
        let secret = b"test-secret";
        // ttl_secs = 0 means expiry == now, which is already expired
        let token = create_token(secret, "admin", 0);
        assert_eq!(validate_token(secret, &token), None);
    }

    #[test]
    fn token_rejects_malformed() {
        let secret = b"test-secret";
        assert_eq!(validate_token(secret, "not-a-token"), None);
        assert_eq!(validate_token(secret, ""), None);
        assert_eq!(validate_token(secret, "a.b.c"), None);
    }

    #[test]
    fn constant_time_eq_works() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
    }
}
