use crate::auth::{
    issue_session_token, origin_allowed, passwords_match, session_token_valid, COOKIE_NAME,
};
use crate::config::Settings;
use crate::content::{
    normalize_text, parse_force_popup, ContentStore, ContentValidationError,
};
use crate::hub::TvHub;
use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, Multipart, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::{Form, Json, Router};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use minijinja::{context, Environment};
use serde::Deserialize;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct AppState {
    pub settings: Settings,
    pub store: Arc<ContentStore>,
    pub hub: TvHub,
    pub templates: Environment<'static>,
}

pub fn build_state(settings: Settings) -> AppState {
    let store = Arc::new(ContentStore::new(settings.data_dir.clone()));
    AppState {
        settings,
        store,
        hub: TvHub::new(),
        templates: template_env(),
    }
}

pub fn template_env() -> Environment<'static> {
    let mut env = Environment::new();
    env.add_template("base.html", include_str!("../templates/base.html"))
        .expect("base.html");
    env.add_template("login.html", include_str!("../templates/login.html"))
        .expect("login.html");
    env.add_template("admin.html", include_str!("../templates/admin.html"))
        .expect("admin.html");
    env
}

pub fn router(state: AppState) -> Router {
    // Axum 默认只收 2 MB。放宽到配置上限再加 1 MB，10 MB 图片才能进到中文校验。
    let body_limit = state.settings.max_image_bytes.saturating_add(1024 * 1024);
    Router::new()
        .route("/health", get(health))
        .route("/login", get(login_form).post(login))
        .route("/logout", post(logout))
        .route("/", get(admin))
        .route("/content", post(publish))
        .route("/force-popup", post(update_force_popup))
        .route("/api/status", get(status))
        .route("/media/current", get(media))
        .layer(DefaultBodyLimit::max(body_limit))
        .with_state(state)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"ok": true}))
}

fn is_admin(jar: &CookieJar, settings: &Settings) -> bool {
    jar.get(COOKIE_NAME)
        .map(|cookie| session_token_valid(cookie.value(), &settings.admin_password))
        .unwrap_or(false)
}

fn csrf_ok(headers: &HeaderMap) -> bool {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok());
    origin_allowed(origin, host)
}

fn forbidden() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(serde_json::json!({"detail": "跨站请求已被拒绝"})),
    )
        .into_response()
}

fn unauthorized() -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(serde_json::json!({"detail": "未登录"})),
    )
        .into_response()
}

fn render(state: &AppState, name: &str, ctx: minijinja::value::Value, status: StatusCode) -> Response {
    let template = state.templates.get_template(name).expect("template");
    let html = template.render(ctx).expect("render");
    (status, Html(html)).into_response()
}

async fn login_form(State(state): State<AppState>, jar: CookieJar) -> Response {
    if is_admin(&jar, &state.settings) {
        return Redirect::to("/").into_response();
    }
    render(
        &state,
        "login.html",
        context! { error => minijinja::value::Serde(Option::<String>::None) },
        StatusCode::OK,
    )
}

#[derive(Deserialize)]
struct PasswordForm {
    password: Option<String>,
}

async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<PasswordForm>,
) -> Response {
    if !csrf_ok(&headers) {
        return forbidden();
    }
    let provided = form.password.unwrap_or_default();
    if !passwords_match(&provided, &state.settings.admin_password) {
        info!("登录失败");
        return render(
            &state,
            "login.html",
            context! { error => "密码不正确" },
            StatusCode::UNAUTHORIZED,
        );
    }
    let token = issue_session_token(&state.settings.admin_password);
    let cookie = Cookie::build((COOKIE_NAME, token))
        .http_only(true)
        .same_site(SameSite::Lax)
        .path("/")
        .max_age(cookie::time::Duration::days(30))
        .build();
    (jar.add(cookie), Redirect::to("/")).into_response()
}

async fn logout(headers: HeaderMap, jar: CookieJar) -> Response {
    if !csrf_ok(&headers) {
        return forbidden();
    }
    let mut removal = Cookie::from(COOKIE_NAME);
    removal.set_path("/");
    let jar = jar.remove(removal);
    (jar, Redirect::to("/login")).into_response()
}

async fn render_admin(
    state: &AppState,
    error: Option<String>,
    draft_text: String,
    status: StatusCode,
) -> Response {
    let content = state.store.load();
    let preview_url = if content.has_image() {
        Some(format!("/media/current?v={}", content.revision))
    } else {
        None
    };
    let tv_online = state.hub.online().await;
    render(
        state,
        "admin.html",
        context! {
            error => minijinja::value::Serde(error),
            content => minijinja::value::Serde(content),
            preview_url => minijinja::value::Serde(preview_url),
            draft_text,
            tv_online,
        },
        status,
    )
}

async fn admin(State(state): State<AppState>, jar: CookieJar) -> Response {
    if !is_admin(&jar, &state.settings) {
        return Redirect::to("/login").into_response();
    }
    let content = state.store.load();
    render_admin(&state, None, content.text, StatusCode::OK).await
}

async fn status(State(state): State<AppState>, jar: CookieJar) -> Response {
    if !is_admin(&jar, &state.settings) {
        return unauthorized();
    }
    let content = state.store.load();
    Json(serde_json::json!({
        "tv_online": state.hub.online().await,
        "force_popup": content.force_popup,
        "has_image": content.has_image(),
        "has_text": content.has_text(),
        "revision": content.revision,
        "text": content.text,
    }))
    .into_response()
}

async fn media(State(state): State<AppState>, jar: CookieJar) -> Response {
    if !is_admin(&jar, &state.settings) {
        return unauthorized();
    }
    let content = state.store.load();
    match (content.image, content.mime) {
        (Some(bytes), Some(mime)) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "no-store".into()),
            ],
            bytes,
        )
            .into_response(),
        _ => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"detail": "还没有图片"})),
        )
            .into_response(),
    }
}

async fn publish(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Response {
    if !csrf_ok(&headers) {
        return forbidden();
    }
    if !is_admin(&jar, &state.settings) {
        return Redirect::to("/login").into_response();
    }
    let mut draft_text = String::new();
    let mut image: Option<Bytes> = None;
    let mut force_raw: Option<String> = None;
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        match name.as_str() {
            "text" => {
                draft_text = field.text().await.unwrap_or_default();
            }
            "force_popup" => {
                force_raw = Some(field.text().await.unwrap_or_default());
            }
            "image" => {
                let file_name = field.file_name().map(ToOwned::to_owned);
                let bytes = field.bytes().await.unwrap_or_default();
                if file_name.as_deref().is_some_and(|name| !name.is_empty()) && !bytes.is_empty() {
                    image = Some(bytes);
                }
            }
            _ => {}
        }
    }
    let result = (|| {
        let text = normalize_text(Some(&draft_text), state.settings.text_max_chars)?;
        if let Some(bytes) = &image {
            if bytes.len() > state.settings.max_image_bytes {
                return Err(ContentValidationError {
                    message: "图片不能超过 10 MB".into(),
                });
            }
        }
        let force_popup = parse_force_popup(force_raw.as_deref());
        state.store.publish(
            image.as_deref(),
            &text,
            force_popup,
            state.settings.max_image_bytes,
        )
    })();
    match result {
        Ok(_) => {
            let payload =
                serde_json::to_string(&state.store.message()).expect("ws message is serializable");
            let _ = state.hub.push(&payload).await;
            info!("已发布");
            Redirect::to("/").into_response()
        }
        Err(err) => {
            render_admin(&state, Some(err.message), draft_text, StatusCode::BAD_REQUEST).await
        }
    }
}

#[derive(Deserialize)]
struct ForcePopupForm {
    force_popup: Option<String>,
}

async fn update_force_popup(
    State(state): State<AppState>,
    jar: CookieJar,
    headers: HeaderMap,
    Form(form): Form<ForcePopupForm>,
) -> Response {
    if !csrf_ok(&headers) {
        return forbidden();
    }
    if !is_admin(&jar, &state.settings) {
        return Redirect::to("/login").into_response();
    }
    let content = state.store.set_force_popup(parse_force_popup(form.force_popup.as_deref()));
    if content.revision > 0 {
        let payload = serde_json::to_string(&state.store.message()).expect("ws message");
        let _ = state.hub.push(&payload).await;
    }
    Redirect::to("/").into_response()
}
