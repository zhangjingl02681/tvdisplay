use tvdisplay::auth::{
    issue_session_token, origin_allowed, passwords_match, session_token_valid, COOKIE_NAME,
};

const SECRET: &str = "unit-test-password";
const PYTHON_TOKEN: &str = "31ab44cf12e015ceb7436a0b696dad9e2725da70819a6ee62f373dc7c5612d53";

#[test]
fn cookie_name_matches_existing_service() {
    assert_eq!(COOKIE_NAME, "tvdisplay_session");
}

#[test]
fn matching_passwords() {
    assert!(passwords_match(SECRET, SECRET));
    assert!(!passwords_match("other-value", SECRET));
    assert!(!passwords_match("", SECRET));
    assert!(!passwords_match(SECRET, ""));
}

#[test]
fn session_token_matches_existing_python_hmac_and_is_not_the_password() {
    let token = issue_session_token(SECRET);
    assert_eq!(token, PYTHON_TOKEN);
    assert_ne!(token, SECRET);
    assert!(!token.contains(SECRET));
    assert!(session_token_valid(&token, SECRET));
    assert!(!session_token_valid(&token, "other-value"));
    assert!(!session_token_valid("short", SECRET));
    assert!(!session_token_valid("", SECRET));
}

#[test]
fn origin_must_match_host_when_present() {
    assert!(origin_allowed(None, Some("localhost")));
    assert!(origin_allowed(
        Some("http://[2001:db8::1]:8080"),
        Some("[2001:db8::1]:8080"),
    ));
    assert!(!origin_allowed(
        Some("https://evil.example"),
        Some("[2001:db8::1]:8080"),
    ));
    assert!(!origin_allowed(Some("http://evil.example"), None));
    assert!(!origin_allowed(Some("ftp://localhost"), Some("localhost")));
}
