"""同一时间只保留一台电视。新连接替换旧连接。"""

from __future__ import annotations

import logging

from starlette.websockets import WebSocket

logger = logging.getLogger("tvdisplay")


class TvHub:
    def __init__(self) -> None:
        self._current: WebSocket | None = None

    @property
    def online(self) -> bool:
        return self._current is not None

    async def replace(self, websocket: WebSocket) -> WebSocket | None:
        previous = self._current
        self._current = websocket
        return previous

    async def release(self, websocket: WebSocket) -> None:
        if self._current is websocket:
            self._current = None

    async def push(self, message: dict) -> bool:
        websocket = self._current
        if websocket is None:
            return False
        try:
            await websocket.send_json(message)
        except Exception:
            logger.info("push to tv failed")
            await self.release(websocket)
            return False
        return True
