"""管理页和电视 WebSocket。"""

from __future__ import annotations

import asyncio
import json
import logging
from contextlib import asynccontextmanager
from pathlib import Path

from fastapi import FastAPI, Request, WebSocket, WebSocketDisconnect
from fastapi.responses import JSONResponse, RedirectResponse, Response
from fastapi.templating import Jinja2Templates
from starlette.datastructures import UploadFile

from tvdisplay.auth import COOKIE_NAME, issue_session_token, origin_allowed, passwords_match, session_token_valid
from tvdisplay.config import Settings
from tvdisplay.content import ContentStore, ContentValidationError, normalize_text, parse_force_popup, read_limited
from tvdisplay.hub import TvHub
from tvdisplay.logging_redact import install_redaction

logger = logging.getLogger("tvdisplay")
_TEMPLATES = Jinja2Templates(directory=str(Path(__file__).parent / "templates"))


def create_app(settings: Settings) -> FastAPI:
    @asynccontextmanager
    async def lifespan(app: FastAPI):
        install_redaction(app.state.settings.admin_password)
        yield

    app = FastAPI(docs_url=None, redoc_url=None, openapi_url=None, lifespan=lifespan)
    app.state.settings = settings
    app.state.store = ContentStore(settings.data_dir)
    app.state.hub = TvHub()

    @app.get("/health")
    async def health() -> dict:
        return {"ok": True}

    @app.get("/login")
    async def login_form(request: Request):
        if _is_admin(request):
            return RedirectResponse("/", status_code=303)
        return _render(request, "login.html", {"error": None})

    @app.post("/login")
    async def login(request: Request):
        if not origin_allowed(request.headers.get("origin"), request.headers.get("host")):
            return JSONResponse({"detail": "跨站请求已被拒绝"}, status_code=403)
        form = await request.form()
        provided = form.get("password")
        password = provided if isinstance(provided, str) else ""
        if not passwords_match(password, settings.admin_password):
            logger.info("admin login failed")
            return _render(request, "login.html", {"error": "密码不正确"}, status_code=401)
        response = RedirectResponse("/", status_code=303)
        response.set_cookie(
            COOKIE_NAME,
            issue_session_token(settings.admin_password),
            httponly=True,
            samesite="lax",
            max_age=60 * 60 * 24 * 30,
            path="/",
        )
        logger.info("admin login succeeded")
        return response

    @app.post("/logout")
    async def logout(request: Request):
        if not origin_allowed(request.headers.get("origin"), request.headers.get("host")):
            return JSONResponse({"detail": "跨站请求已被拒绝"}, status_code=403)
        response = RedirectResponse("/login", status_code=303)
        response.delete_cookie(COOKIE_NAME, path="/")
        return response

    @app.get("/")
    async def admin(request: Request):
        if not _is_admin(request):
            return RedirectResponse("/login", status_code=303)
        return _render_admin(request)

    @app.get("/api/status")
    async def status(request: Request):
        if not _is_admin(request):
            return JSONResponse({"detail": "未登录"}, status_code=401)
        content = app.state.store.load()
        return {
            "tv_online": app.state.hub.online,
            "force_popup": content.force_popup,
            "has_image": content.has_image,
            "revision": content.revision,
            "text": content.text,
        }

    @app.get("/media/current")
    async def media(request: Request):
        if not _is_admin(request):
            return JSONResponse({"detail": "未登录"}, status_code=401)
        content = app.state.store.load()
        if not content.image or not content.mime:
            return JSONResponse({"detail": "还没有图片"}, status_code=404)
        return Response(
            content=content.image,
            media_type=content.mime,
            headers={"Cache-Control": "no-store"},
        )

    @app.post("/content")
    async def publish(request: Request):
        if not origin_allowed(request.headers.get("origin"), request.headers.get("host")):
            return JSONResponse({"detail": "跨站请求已被拒绝"}, status_code=403)
        if not _is_admin(request):
            return RedirectResponse("/login", status_code=303)
        form = await request.form()
        draft = form.get("text")
        draft_text = draft if isinstance(draft, str) else ""
        try:
            text = normalize_text(draft_text, max_chars=settings.text_max_chars)
            upload = form.get("image")
            if not isinstance(upload, UploadFile) or not upload.filename:
                raise ContentValidationError("请上传图片")
            image = await read_limited(upload, settings.max_image_bytes)
            force_popup = parse_force_popup(_form_value(form.get("force_popup")))
            app.state.store.save_upload(
                image,
                text,
                force_popup,
                max_bytes=settings.max_image_bytes,
            )
        except ContentValidationError as exc:
            return _render_admin(request, error=exc.message, draft_text=draft_text, status_code=400)
        await app.state.hub.push(app.state.store.message())
        logger.info("content published")
        return RedirectResponse("/", status_code=303)

    @app.post("/force-popup")
    async def update_force_popup(request: Request):
        if not origin_allowed(request.headers.get("origin"), request.headers.get("host")):
            return JSONResponse({"detail": "跨站请求已被拒绝"}, status_code=403)
        if not _is_admin(request):
            return RedirectResponse("/login", status_code=303)
        form = await request.form()
        force_popup = parse_force_popup(_form_value(form.get("force_popup")))
        app.state.store.set_force_popup(force_popup)
        if app.state.store.load().has_image:
            await app.state.hub.push(app.state.store.message())
        logger.info("force popup updated")
        return RedirectResponse("/", status_code=303)

    @app.websocket("/ws/tv")
    async def tv_socket(websocket: WebSocket) -> None:
        await websocket.accept()
        try:
            raw = await asyncio.wait_for(websocket.receive_text(), timeout=settings.auth_timeout_seconds)
        except (TimeoutError, WebSocketDisconnect):
            await _close_quietly(websocket, 1008)
            return
        try:
            payload = json.loads(raw)
        except json.JSONDecodeError:
            await _close_quietly(websocket, 1008)
            return
        provided = payload.get("password") if isinstance(payload, dict) else None
        authed = (
            isinstance(payload, dict)
            and payload.get("type") == "auth"
            and isinstance(provided, str)
            and passwords_match(provided, settings.admin_password)
        )
        del raw, payload, provided
        if not authed:
            logger.info("tv auth rejected")
            await _close_quietly(websocket, 1008)
            return
        previous = await app.state.hub.replace(websocket)
        if previous is not None:
            logger.info("tv connection replaced the previous one")
            await _close_quietly(previous, 4000)
        try:
            await websocket.send_json(app.state.store.message())
            logger.info("tv connected")
            while True:
                await websocket.receive_text()
        except WebSocketDisconnect:
            pass
        finally:
            await app.state.hub.release(websocket)
            logger.info("tv disconnected")

    return app


def _is_admin(request: Request) -> bool:
    token = request.cookies.get(COOKIE_NAME, "")
    return session_token_valid(token, request.app.state.settings.admin_password)


def _form_value(value: object) -> str | None:
    return value if isinstance(value, str) else None


def _render(request: Request, template: str, context: dict, status_code: int = 200):
    return _TEMPLATES.TemplateResponse(request, template, context, status_code=status_code)


def _render_admin(request: Request, error: str | None = None, draft_text: str = "", status_code: int = 200):
    content = request.app.state.store.load()
    preview = f"/media/current?v={content.revision}" if content.has_image else None
    return _render(
        request,
        "admin.html",
        {
            "error": error,
            "content": content,
            "preview_url": preview,
            "draft_text": draft_text if error else content.text,
            "tv_online": request.app.state.hub.online,
        },
        status_code=status_code,
    )


async def _close_quietly(websocket: WebSocket, code: int) -> None:
    try:
        await websocket.close(code=code)
    except RuntimeError:
        return
