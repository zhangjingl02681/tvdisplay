mod common;
use common::{spawn_app, JPEG, PASSWORD, PNG};
use serde_json::Value;

#[tokio::test]
async fn health_is_public() {
    let app = spawn_app().await;
    let response = app.client.get(app.url("/health")).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let body: Value = response.json().await.unwrap();
    assert_eq!(body, serde_json::json!({"ok": true}));
}

#[tokio::test]
async fn admin_requires_login() {
    let app = spawn_app().await;
    let home = app.client.get(app.url("/")).send().await.unwrap();
    assert_eq!(home.status(), 303);
    assert_eq!(home.headers().get("location").unwrap(), "/login");
    assert_eq!(app.client.get(app.url("/api/status")).send().await.unwrap().status(), 401);
    assert_eq!(app.client.get(app.url("/media/current")).send().await.unwrap().status(), 401);
}

#[tokio::test]
async fn wrong_password_is_rejected_and_not_echoed() {
    let app = spawn_app().await;
    let response = app
        .client
        .post(app.url("/login"))
        .form(&[("password", "definitely-not-the-password")])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 401);
    let text = response.text().await.unwrap();
    assert!(text.contains("密码不正确"));
    assert!(!text.contains("definitely-not-the-password"));
}

#[tokio::test]
async fn login_shows_chinese_admin() {
    let app = spawn_app().await;
    assert_eq!(app.login().await.status(), 303);
    let page = app.client.get(app.url("/")).send().await.unwrap();
    assert_eq!(page.status(), 200);
    let text = page.text().await.unwrap();
    assert!(text.contains("发布到电视"));
    assert!(text.contains("强制弹出"));
    assert!(text.contains("电视状态"));
    assert!(text.contains("离线"));
    assert!(text.contains("尚未发布内容"));
    assert!(text.contains("选择 JPEG 或 PNG"));
}

#[tokio::test]
async fn cross_site_login_is_rejected() {
    let app = spawn_app().await;
    let response = app
        .client
        .post(app.url("/login"))
        .header("Origin", "https://evil.example")
        .header("Host", "localhost")
        .form(&[("password", PASSWORD)])
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 403);
}

#[tokio::test]
async fn publish_text_only_is_accepted() {
    let app = spawn_app().await;
    app.login().await;
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(reqwest::multipart::Form::new().text("text", "只有字"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["has_text"], true);
    assert_eq!(status["has_image"], false);
    assert_eq!(status["revision"], 1);
    assert_eq!(status["text"], "只有字");
    assert_eq!(app.client.get(app.url("/media/current")).send().await.unwrap().status(), 404);
    let page = app.client.get(app.url("/")).send().await.unwrap().text().await.unwrap();
    assert!(page.contains("只有字"));
}

#[tokio::test]
async fn publish_requires_image_or_text() {
    let app = spawn_app().await;
    app.login().await;
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(reqwest::multipart::Form::new().text("text", "   "))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    let text = response.text().await.unwrap();
    assert!(text.contains("请发送图片或文字"));
}

#[tokio::test]
async fn publish_jpeg_with_text_and_preview() {
    let app = spawn_app().await;
    app.login().await;
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("text", "今晚菜单")
                .text("force_popup", "on")
                .part("image", reqwest::multipart::Part::bytes(JPEG.to_vec()).file_name("menu.jpg").mime_str("image/jpeg").unwrap()),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    let page = app.client.get(app.url("/")).send().await.unwrap().text().await.unwrap();
    assert!(page.contains("今晚菜单"));
    assert!(page.contains("强制弹出：开"));
    let media = app.client.get(app.url("/media/current")).send().await.unwrap();
    assert_eq!(media.status(), 200);
    assert_eq!(media.bytes().await.unwrap().as_ref(), JPEG);
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["has_image"], true);
    assert_eq!(status["has_text"], true);
    assert_eq!(status["revision"], 1);
    assert_eq!(status["force_popup"], true);
    assert_eq!(status["tv_online"], false);
}

#[tokio::test]
async fn png_without_text_is_accepted() {
    let app = spawn_app().await;
    app.login().await;
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new().part(
                "image",
                reqwest::multipart::Part::bytes(PNG.to_vec())
                    .file_name("x.png")
                    .mime_str("application/octet-stream")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 303);
    let media = app.client.get(app.url("/media/current")).send().await.unwrap();
    assert!(media
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("image/png"));
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["text"], "");
    assert_eq!(status["has_text"], false);
}

#[tokio::test]
async fn gif_disguised_as_jpeg_is_rejected() {
    let app = spawn_app().await;
    app.login().await;
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new().part(
                "image",
                reqwest::multipart::Part::bytes(b"GIF89ahello".to_vec())
                    .file_name("photo.jpg")
                    .mime_str("image/jpeg")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(response.text().await.unwrap().contains("只支持 JPEG 或 PNG"));
}

#[tokio::test]
async fn oversize_upload_is_rejected() {
    let app = spawn_app().await;
    app.login().await;
    let mut blob = b"\xff\xd8\xff".to_vec();
    blob.extend(vec![b'x'; 80]);
    let response = app
        .client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new().part(
                "image",
                reqwest::multipart::Part::bytes(blob)
                    .file_name("big.jpg")
                    .mime_str("image/jpeg")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 400);
    assert!(response.text().await.unwrap().contains("10 MB"));
}

#[tokio::test]
async fn force_popup_toggle_persists_without_bumping_revision() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new().part(
                "image",
                reqwest::multipart::Part::bytes(JPEG.to_vec())
                    .file_name("a.jpg")
                    .mime_str("image/jpeg")
                    .unwrap(),
            ),
        )
        .send()
        .await
        .unwrap();
    app.client
        .post(app.url("/force-popup"))
        .form(&[("force_popup", "on")])
        .send()
        .await
        .unwrap();
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["force_popup"], true);
    assert_eq!(status["revision"], 1);
    app.client
        .post(app.url("/force-popup"))
        .form(&[("force_popup", "off")])
        .send()
        .await
        .unwrap();
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["force_popup"], false);
}

#[tokio::test]
async fn logout_returns_to_login() {
    let app = spawn_app().await;
    app.login().await;
    let response = app.client.post(app.url("/logout")).send().await.unwrap();
    assert_eq!(response.status(), 303);
    let page = app.client.get(app.url("/")).send().await.unwrap();
    assert_eq!(page.status(), 303);
    assert_eq!(page.headers().get("location").unwrap(), "/login");
}
