import pytest
from fastapi.testclient import TestClient

from tvdisplay.app import create_app
from tvdisplay.config import Settings

PASSWORD = "unit-test-password"
JPEG = b"\xff\xd8\xff\xd9"
PNG = b"\x89PNG\r\n\x1a\n" + b"rest-of-png"


@pytest.fixture
def settings(tmp_path):
    return Settings(
        admin_password=PASSWORD,
        data_dir=tmp_path,
        auth_timeout_seconds=0.4,
        max_image_bytes=64,
        text_max_chars=20,
    )


@pytest.fixture
def client(settings):
    app = create_app(settings)
    with TestClient(app) as test_client:
        yield test_client


@pytest.fixture
def admin(client):
    response = client.post("/login", data={"password": PASSWORD}, follow_redirects=False)
    assert response.status_code == 303
    return client
