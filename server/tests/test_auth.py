from tvdisplay.auth import issue_session_token, origin_allowed, passwords_match, session_token_valid

SECRET = "unit-test-password"


def test_matching_passwords():
    assert passwords_match(SECRET, SECRET)
    assert not passwords_match("other-value", SECRET)
    assert not passwords_match("", SECRET)
    assert not passwords_match(SECRET, "")


def test_session_token_is_not_the_password():
    token = issue_session_token(SECRET)
    assert token != SECRET
    assert SECRET not in token
    assert session_token_valid(token, SECRET)
    assert not session_token_valid(token, "other-value")
    assert not session_token_valid("short", SECRET)


def test_origin_must_match_host_when_present():
    assert origin_allowed(None, "localhost")
    assert origin_allowed("http://[2001:db8::1]:8080", "[2001:db8::1]:8080")
    assert not origin_allowed("https://evil.example", "[2001:db8::1]:8080")
    assert not origin_allowed("http://evil.example", None)
