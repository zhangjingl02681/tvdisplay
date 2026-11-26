"""从日志记录里抹掉管理密码。过滤器只替换，不记录原文。"""

from __future__ import annotations

import logging

_REDACTED = "***"
_LOGGER_NAMES = ("tvdisplay", "uvicorn", "uvicorn.access", "uvicorn.error")


class RedactingFilter(logging.Filter):
    def __init__(self, secret: str) -> None:
        super().__init__()
        self._secret = secret

    def filter(self, record: logging.LogRecord) -> bool:
        secret = self._secret
        if not secret:
            return True
        if isinstance(record.msg, str) and secret in record.msg:
            record.msg = record.msg.replace(secret, _REDACTED)
        record.args = _redact_args(record.args, secret)
        return True


def install_redaction(secret: str) -> None:
    redactor = RedactingFilter(secret)
    for name in _LOGGER_NAMES:
        logger = logging.getLogger(name)
        if not any(isinstance(existing, RedactingFilter) and existing._secret == secret for existing in logger.filters):
            logger.addFilter(redactor)


def _redact_args(args, secret: str):
    if isinstance(args, tuple):
        return tuple(_redact_value(item, secret) for item in args)
    if isinstance(args, dict):
        return {key: _redact_value(value, secret) for key, value in args.items()}
    return args


def _redact_value(value, secret: str):
    if isinstance(value, str) and secret in value:
        return value.replace(secret, _REDACTED)
    return value
