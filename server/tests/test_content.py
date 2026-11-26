import base64

import pytest

from tvdisplay.content import (
    MAX_IMAGE_BYTES,
    ContentStore,
    ContentValidationError,
    normalize_text,
    parse_force_popup,
    validate_image,
)

JPEG = b"\xff\xd8\xff\xd9"
PNG = b"\x89PNG\r\n\x1a\n" + b"rest"


def test_default_image_limit_is_ten_mebibytes():
    assert MAX_IMAGE_BYTES == 10 * 1024 * 1024


def test_jpeg_and_png_magic_are_accepted_regardless_of_name():
    assert validate_image(JPEG) == "image/jpeg"
    assert validate_image(PNG) == "image/png"


def test_empty_gif_and_oversize_are_rejected():
    with pytest.raises(ContentValidationError, match="请上传图片"):
        validate_image(b"")
    with pytest.raises(ContentValidationError, match="只支持 JPEG 或 PNG"):
        validate_image(b"GIF89a-not-a-photo")
    with pytest.raises(ContentValidationError, match="10 MB"):
        validate_image(b"\xff\xd8\xff" + b"x" * 32, max_bytes=10)


def test_text_is_optional_and_bounded():
    assert normalize_text(None) == ""
    assert normalize_text("  你好 \r\n") == "你好"
    with pytest.raises(ContentValidationError, match="4000"):
        normalize_text("字" * 12, max_chars=8)


def test_force_popup_parser():
    assert parse_force_popup("on")
    assert parse_force_popup("true")
    assert not parse_force_popup(None)
    assert not parse_force_popup("off")


def test_store_roundtrip_keeps_image_text_and_toggle(tmp_path):
    store = ContentStore(tmp_path)
    saved = store.save_upload(JPEG, "今晚菜单", True)
    assert saved.revision == 1
    assert saved.text == "今晚菜单"
    assert saved.force_popup is True
    assert saved.mime == "image/jpeg"
    assert saved.image == JPEG

    reloaded = ContentStore(tmp_path).load()
    assert reloaded.image == JPEG
    assert reloaded.text == "今晚菜单"
    assert reloaded.force_popup is True
    assert reloaded.revision == 1


def test_replacing_png_removes_previous_jpeg(tmp_path):
    store = ContentStore(tmp_path)
    store.save_upload(JPEG, "", False)
    store.save_upload(PNG, "第二张", False)
    assert not (tmp_path / "image.jpg").exists()
    assert (tmp_path / "image.png").read_bytes() == PNG
    assert store.load().revision == 2


def test_toggle_persists_without_bumping_revision(tmp_path):
    store = ContentStore(tmp_path)
    store.save_upload(JPEG, "字幕", False)
    updated = store.set_force_popup(True)
    assert updated.force_popup is True
    assert updated.revision == 1
    assert ContentStore(tmp_path).load().force_popup is True


def test_toggle_can_be_saved_before_any_image(tmp_path):
    store = ContentStore(tmp_path)
    store.set_force_popup(True)
    assert store.load().force_popup is True
    assert store.load().has_image is False
    assert store.load().revision == 0


def test_message_includes_base64_image(tmp_path):
    store = ContentStore(tmp_path)
    store.save_upload(PNG, "", True)
    message = store.message()
    assert message["type"] == "content"
    assert message["force_popup"] is True
    assert message["image_mime"] == "image/png"
    assert base64.b64decode(message["image_base64"]) == PNG


def test_empty_message_when_nothing_published(tmp_path):
    message = ContentStore(tmp_path).message()
    assert message["revision"] == 0
    assert message["image_base64"] is None
    assert message["force_popup"] is False
