import base64

from starlette.websockets import WebSocketDisconnect

from tests.conftest import JPEG, PASSWORD


def _auth(ws, password=PASSWORD):
    ws.send_json({"type": "auth", "password": password})


def test_bad_password_is_rejected(client):
    with client.websocket_connect("/ws/tv") as ws:
        _auth(ws, "wrong-password")
        with pytest_raises_disconnect():
            ws.receive_json()


def test_query_string_password_does_not_authenticate(client):
    with client.websocket_connect(f"/ws/tv?password={PASSWORD}") as ws:
        with pytest_raises_disconnect():
            ws.receive_json()


def test_connect_receives_current_content_after_offline(admin):
    admin.post(
        "/content",
        data={"text": "补上的内容", "force_popup": "on"},
        files={"image": ("a.jpg", JPEG, "image/jpeg")},
        follow_redirects=False,
    )
    with admin.websocket_connect("/ws/tv") as ws:
        _auth(ws)
        message = ws.receive_json()
    assert message["type"] == "content"
    assert message["text"] == "补上的内容"
    assert message["force_popup"] is True
    assert base64.b64decode(message["image_base64"]) == JPEG
    assert message["revision"] == 1


def test_empty_catalog_is_sent_when_nothing_published(client):
    with client.websocket_connect("/ws/tv") as ws:
        _auth(ws)
        message = ws.receive_json()
    assert message["revision"] == 0
    assert message["image_base64"] is None


def test_publish_pushes_to_the_connected_tv(admin):
    with admin.websocket_connect("/ws/tv") as ws:
        _auth(ws)
        assert ws.receive_json()["revision"] == 0
        assert admin.get("/api/status").json()["tv_online"] is True
        admin.post(
            "/content",
            data={"text": "新海报"},
            files={"image": ("a.jpg", JPEG, "image/jpeg")},
            follow_redirects=False,
        )
        pushed = ws.receive_json()
    assert pushed["text"] == "新海报"
    assert base64.b64decode(pushed["image_base64"]) == JPEG
    assert admin.get("/api/status").json()["tv_online"] is False


def test_latest_connection_replaces_the_previous_one(client):
    with client.websocket_connect("/ws/tv") as first:
        _auth(first)
        assert first.receive_json()["type"] == "content"
        with client.websocket_connect("/ws/tv") as second:
            _auth(second)
            assert second.receive_json()["type"] == "content"
            with pytest_raises_disconnect():
                first.receive_json()
            status = client.post("/login", data={"password": PASSWORD}, follow_redirects=False)
            assert status.status_code == 303
            assert client.get("/api/status").json()["tv_online"] is True


def test_toggle_push_keeps_revision(admin):
    admin.post(
        "/content",
        files={"image": ("a.jpg", JPEG, "image/jpeg")},
        follow_redirects=False,
    )
    with admin.websocket_connect("/ws/tv") as ws:
        _auth(ws)
        assert ws.receive_json()["revision"] == 1
        admin.post("/force-popup", data={"force_popup": "on"}, follow_redirects=False)
        pushed = ws.receive_json()
    assert pushed["revision"] == 1
    assert pushed["force_popup"] is True


def pytest_raises_disconnect():
    import pytest

    return pytest.raises(WebSocketDisconnect)
