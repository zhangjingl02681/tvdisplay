"""管理页会话。令牌是密码的 HMAC，日志和 Cookie 里都不是密码原文。"""

from __future__ import annotations

import hashlib
import hmac
from urllib.parse import urlparse

COOKIE_NAME = "tvdisplay_session"
_SESSION_LABEL = b"tvdisplay-admin-session-v1"


def passwords_match(provided: str, expected: str) -> bool:
    if not provided or not expected:
        return False
    try:
        return hmac.compare_digest(provided.encode("utf-8"), expected.encode("utf-8"))
    except UnicodeError:
        return False


def issue_session_token(password: str) -> str:
    return hmac.new(password.encode("utf-8"), _SESSION_LABEL, hashlib.sha256).hexdigest()


def session_token_valid(token: str, password: str) -> bool:
    if not token or not password:
        return False
    expected = issue_session_token(password)
    return hmac.compare_digest(token, expected)


def origin_allowed(origin: str | None, host: str | None) -> bool:
    """浏览器带了 Origin 时，必须和 Host 一致。没带 Origin 的本机客户端放行。"""
    if not origin:
        return True
    if not host:
        return False
    parsed = urlparse(origin)
    if parsed.scheme not in {"http", "https"} or not parsed.netloc:
        return False
    return parsed.netloc == host
