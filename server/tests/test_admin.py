from tests.conftest import JPEG, PASSWORD, PNG


def test_health_is_public(client):
    response = client.get("/health")
    assert response.status_code == 200
    assert response.json() == {"ok": True}


def test_admin_requires_login(client):
    response = client.get("/", follow_redirects=False)
    assert response.status_code == 303
    assert response.headers["location"] == "/login"
    assert client.get("/api/status").status_code == 401
    assert client.get("/media/current").status_code == 401


def test_wrong_password_is_rejected_and_not_echoed(client):
    response = client.post("/login", data={"password": "definitely-not-the-password"})
    assert response.status_code == 401
    assert "密码不正确" in response.text
    assert "definitely-not-the-password" not in response.text


def test_login_shows_chinese_admin(admin):
    page = admin.get("/")
    assert page.status_code == 200
    assert "发布到电视" in page.text
    assert "强制弹出" in page.text
    assert "电视状态" in page.text
    assert "离线" in page.text
    assert "尚未发布内容" in page.text
    assert "选择 JPEG 或 PNG" in page.text


def test_cross_site_login_is_rejected(client):
    response = client.post(
        "/login",
        data={"password": PASSWORD},
        headers={"Origin": "https://evil.example", "Host": "localhost"},
    )
    assert response.status_code == 403


def test_publish_requires_an_image(admin):
    response = admin.post("/content", data={"text": "只有字"})
    assert response.status_code == 400
    assert "请上传图片" in response.text


def test_publish_jpeg_with_text_and_preview(admin):
    response = admin.post(
        "/content",
        data={"text": "今晚菜单", "force_popup": "on"},
        files={"image": ("menu.jpg", JPEG, "image/jpeg")},
        follow_redirects=False,
    )
    assert response.status_code == 303
    page = admin.get("/")
    assert "今晚菜单" in page.text
    assert "强制弹出：开" in page.text
    media = admin.get("/media/current")
    assert media.status_code == 200
    assert media.content == JPEG
    assert media.headers["content-type"].startswith("image/jpeg")
    status = admin.get("/api/status").json()
    assert status["has_image"] is True
    assert status["revision"] == 1
    assert status["force_popup"] is True
    assert status["tv_online"] is False


def test_png_without_text_is_accepted(admin):
    response = admin.post(
        "/content",
        files={"image": ("x.png", PNG, "application/octet-stream")},
        follow_redirects=False,
    )
    assert response.status_code == 303
    media = admin.get("/media/current")
    assert media.headers["content-type"].startswith("image/png")
    assert admin.get("/api/status").json()["text"] == ""


def test_gif_disguised_as_jpeg_is_rejected(admin):
    response = admin.post(
        "/content",
        files={"image": ("photo.jpg", b"GIF89ahello", "image/jpeg")},
    )
    assert response.status_code == 400
    assert "只支持 JPEG 或 PNG" in response.text


def test_oversize_upload_is_rejected(admin):
    blob = b"\xff\xd8\xff" + b"x" * 80
    response = admin.post("/content", files={"image": ("big.jpg", blob, "image/jpeg")})
    assert response.status_code == 400
    assert "10 MB" in response.text


def test_force_popup_toggle_persists(admin):
    admin.post(
        "/content",
        files={"image": ("a.jpg", JPEG, "image/jpeg")},
        follow_redirects=False,
    )
    admin.post("/force-popup", data={"force_popup": "on"}, follow_redirects=False)
    assert admin.get("/api/status").json()["force_popup"] is True
    assert admin.get("/api/status").json()["revision"] == 1
    admin.post("/force-popup", data={"force_popup": "off"}, follow_redirects=False)
    assert admin.get("/api/status").json()["force_popup"] is False


def test_logout_returns_to_login(admin):
    response = admin.post("/logout", follow_redirects=False)
    assert response.status_code == 303
    page = admin.get("/", follow_redirects=False)
    assert page.status_code == 303
    assert page.headers["location"] == "/login"
