"""当前这一张图片、可选文字和强制弹出开关。全部落在磁盘上。"""

from __future__ import annotations

import base64
import json
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path

MAX_IMAGE_BYTES = 10 * 1024 * 1024
TEXT_MAX_CHARS = 4000

_JPEG_MAGIC = b"\xff\xd8\xff"
_PNG_MAGIC = b"\x89PNG\r\n\x1a\n"


class ContentValidationError(Exception):
    def __init__(self, message: str) -> None:
        self.message = message
        super().__init__(message)


@dataclass(frozen=True)
class DisplayContent:
    text: str
    force_popup: bool
    mime: str | None
    revision: int
    updated_at: str | None
    image: bytes | None

    @property
    def has_image(self) -> bool:
        return bool(self.image)


def sniff_image_mime(data: bytes) -> str | None:
    if data.startswith(_PNG_MAGIC):
        return "image/png"
    if data.startswith(_JPEG_MAGIC):
        return "image/jpeg"
    return None


def validate_image(data: bytes, max_bytes: int = MAX_IMAGE_BYTES) -> str:
    if not data:
        raise ContentValidationError("请上传图片")
    if len(data) > max_bytes:
        raise ContentValidationError("图片不能超过 10 MB")
    mime = sniff_image_mime(data)
    if mime is None:
        raise ContentValidationError("只支持 JPEG 或 PNG")
    return mime


def normalize_text(text: str | None, max_chars: int = TEXT_MAX_CHARS) -> str:
    value = (text or "").replace("\r\n", "\n").strip()
    if len(value) > max_chars:
        raise ContentValidationError("说明文字不能超过 4000 字")
    return value


def parse_force_popup(value: str | None) -> bool:
    return (value or "").strip().lower() in {"1", "true", "on", "yes"}


def _image_name(mime: str) -> str:
    return "image.png" if mime == "image/png" else "image.jpg"


class ContentStore:
    def __init__(self, root: Path) -> None:
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)
        self._meta_path = root / "content.json"

    def load(self) -> DisplayContent:
        meta = self._read_meta()
        image: bytes | None = None
        name = meta.get("image_name")
        mime = meta.get("mime")
        if isinstance(name, str) and name:
            path = self.root / name
            if path.is_file():
                image = path.read_bytes()
            else:
                mime = None
        return DisplayContent(
            text=str(meta.get("text") or ""),
            force_popup=bool(meta.get("force_popup")),
            mime=mime if isinstance(mime, str) else None,
            revision=int(meta.get("revision") or 0),
            updated_at=meta.get("updated_at") if isinstance(meta.get("updated_at"), str) else None,
            image=image,
        )

    def save_upload(self, image: bytes, text: str, force_popup: bool, max_bytes: int = MAX_IMAGE_BYTES) -> DisplayContent:
        mime = validate_image(image, max_bytes=max_bytes)
        current = self.load()
        revision = current.revision + 1
        updated_at = _now()
        name = _image_name(mime)
        self._write_bytes(self.root / name, image)
        other = "image.jpg" if name == "image.png" else "image.png"
        leftover = self.root / other
        if leftover.exists():
            leftover.unlink()
        self._write_meta(
            {
                "text": text,
                "force_popup": force_popup,
                "mime": mime,
                "revision": revision,
                "updated_at": updated_at,
                "image_name": name,
            }
        )
        return self.load()

    def set_force_popup(self, force_popup: bool) -> DisplayContent:
        current = self.load()
        meta = self._read_meta()
        meta["text"] = current.text
        meta["force_popup"] = force_popup
        meta["mime"] = current.mime
        meta["revision"] = current.revision
        meta["updated_at"] = current.updated_at
        meta["image_name"] = meta.get("image_name") if current.has_image else None
        self._write_meta(meta)
        return self.load()

    def message(self) -> dict:
        content = self.load()
        encoded = base64.b64encode(content.image).decode("ascii") if content.image else None
        return {
            "type": "content",
            "revision": content.revision,
            "text": content.text,
            "force_popup": content.force_popup,
            "image_mime": content.mime,
            "image_base64": encoded,
            "updated_at": content.updated_at,
        }

    def _read_meta(self) -> dict:
        if not self._meta_path.is_file():
            return {}
        try:
            loaded = json.loads(self._meta_path.read_text(encoding="utf-8"))
        except json.JSONDecodeError:
            return {}
        return loaded if isinstance(loaded, dict) else {}

    def _write_meta(self, meta: dict) -> None:
        payload = json.dumps(meta, ensure_ascii=False, indent=2)
        self._write_bytes(self._meta_path, payload.encode("utf-8"))

    def _write_bytes(self, path: Path, data: bytes) -> None:
        temporary = path.with_suffix(path.suffix + ".tmp")
        temporary.write_bytes(data)
        temporary.replace(path)


def _now() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


async def read_limited(upload, limit: int) -> bytes:
    """Read an UploadFile, refusing bodies larger than `limit`."""
    buffer = bytearray()
    while True:
        chunk = await upload.read(256 * 1024)
        if not chunk:
            break
        if len(buffer) + len(chunk) > limit:
            raise ContentValidationError("图片不能超过 10 MB")
        buffer.extend(chunk)
    return bytes(buffer)
