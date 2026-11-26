import io
import logging

from tests.conftest import JPEG, PASSWORD
from tvdisplay.logging_redact import RedactingFilter, install_redaction


def test_filter_redacts_message_and_args():
    logger = logging.getLogger("tvdisplay.test.redact")
    logger.handlers.clear()
    logger.filters.clear()
    logger.setLevel(logging.INFO)
    logger.propagate = False
    buffer = io.StringIO()
    logger.addHandler(logging.StreamHandler(buffer))
    logger.addFilter(RedactingFilter(PASSWORD))
    logger.info("rejected %s", PASSWORD)
    rendered = buffer.getvalue()
    assert PASSWORD not in rendered
    assert "***" in rendered


def test_requests_do_not_log_the_password(client, caplog):
    caplog.set_level(logging.DEBUG)
    install_redaction(PASSWORD)
    client.post("/login", data={"password": PASSWORD}, follow_redirects=False)
    client.post("/login", data={"password": "another-secret-value"})
    with client.websocket_connect("/ws/tv") as ws:
        ws.send_json({"type": "auth", "password": PASSWORD})
        ws.receive_json()
    text = caplog.text
    assert PASSWORD not in text
    assert "another-secret-value" not in text


def test_publish_logs_do_not_include_image_or_password(admin, caplog):
    caplog.set_level(logging.DEBUG)
    admin.post(
        "/content",
        data={"text": "日志里不要出现正文标记XYZZY"},
        files={"image": ("a.jpg", JPEG, "image/jpeg")},
        follow_redirects=False,
    )
    assert "XYZZY" not in caplog.text
    assert PASSWORD not in caplog.text
