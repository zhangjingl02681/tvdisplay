use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use url::Url;

pub const COOKIE_NAME: &str = "tvdisplay_session";
const SESSION_LABEL: &[u8] = b"tvdisplay-admin-session-v1";

type HmacSha256 = Hmac<Sha256>;

pub fn passwords_match(provided: &str, expected: &str) -> bool {
    if provided.is_empty() || expected.is_empty() {
        return false;
    }
    let left = provided.as_bytes();
    let right = expected.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).into()
}

pub fn issue_session_token(password: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(password.as_bytes())
        .expect("HMAC-SHA256 accepts a key of any size");
    mac.update(SESSION_LABEL);
    hex::encode(mac.finalize().into_bytes())
}

pub fn session_token_valid(token: &str, password: &str) -> bool {
    if token.is_empty() || password.is_empty() {
        return false;
    }
    let expected = issue_session_token(password);
    if token.len() != expected.len() {
        return false;
    }
    token.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(host) = host else {
        return false;
    };
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let Some(authority) = origin_authority(&parsed) else {
        return false;
    };
    authority == host
}

fn origin_authority(parsed: &Url) -> Option<String> {
    // url 2.5 的 host_str() 对 IPv6 已带方括号；authority() 就是要和 Host 比较的 netloc。
    let authority = parsed.authority();
    if authority.is_empty() {
        None
    } else {
        Some(authority.to_string())
    }
}
