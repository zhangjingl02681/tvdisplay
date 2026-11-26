"""启动遥显。日志里只写监听地址，不写密码。"""

from __future__ import annotations

import logging

import uvicorn

from tvdisplay.app import create_app
from tvdisplay.config import load_settings

logger = logging.getLogger("tvdisplay")


def main() -> None:
    settings = load_settings()
    app = create_app(settings)
    logger.info("listening on %s port %s", settings.host, settings.port)
    uvicorn.run(app, host=settings.host, port=settings.port, access_log=True)


if __name__ == "__main__":
    main()
