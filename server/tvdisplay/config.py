"""进程配置。密码只从环境读入，调用方不得把它写进日志。"""

from __future__ import annotations

import os
from collections.abc import Mapping
from dataclasses import dataclass
from pathlib import Path

PLACEHOLDER_PASSWORDS = frozenset(
    {
        "change-me",
        "password",
        "admin",
        "请改成一长串随机密码",
    }
)


@dataclass(frozen=True)
class Settings:
    admin_password: str
    data_dir: Path
    host: str = "::"
    port: int = 8080
    auth_timeout_seconds: float = 10.0
    max_image_bytes: int = 10 * 1024 * 1024
    text_max_chars: int = 4000


def load_settings(environ: Mapping[str, str] | None = None) -> Settings:
    env = os.environ if environ is None else environ
    password = env.get("ADMIN_PASSWORD", "")
    if password.strip() == "":
        raise SystemExit("请设置环境变量 ADMIN_PASSWORD")
    if password in PLACEHOLDER_PASSWORDS:
        raise SystemExit("ADMIN_PASSWORD 还是示例值，请换成你自己的随机密码")
    host = env.get("HOST", "::") or "::"
    raw_port = env.get("PORT", "8080")
    try:
        port = int(raw_port)
    except ValueError:
        raise SystemExit("PORT 必须是整数") from None
    if not 1 <= port <= 65535:
        raise SystemExit("PORT 必须在 1 到 65535 之间")
    data_dir = Path(env.get("DATA_DIR", "data"))
    return Settings(admin_password=password, data_dir=data_dir, host=host, port=port)
