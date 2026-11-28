# 遥显飞书机器人与 Rust 后端 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 用一个 Rust 进程替换现有 Python 后端，继续提供网页后台和电视 WebSocket，并在指定飞书群里用长连接接收图片或文字，经卡片确认后推到 Android 电视。

**Architecture:** 树莓派上只跑一个 Tokio + Axum 进程：内容落在 `/data` 的 `content.json` 与可选图片上，WebSocket 只服务最后连上的一台电视，飞书通过官方长连接协议向外建 WebSocket（不提供回调 URL）。管理页与飞书写入同一份当前内容；电视应用做最小改动以支持无图纯文字全屏显示。

**Tech Stack:** Rust 2021、Tokio 1.54、Axum 0.8、tower-http 0.7、minijinja 3、reqwest 0.13、tokio-tungstenite 0.30、prost 0.14、tracing、Docker Compose（`network_mode: host`）、Kotlin / Jetpack Compose for TV（minSdk 24）。

## Global Constraints

- 单一 Rust 进程同时提供网页后台、内容存储、电视 WebSocket 和飞书长连接；不保留 Python 进程，不做 sidecar，不使用飞书 Webhook 回调
- 图片和文字至少要有一样；图片只接受 JPEG 或 PNG 文件头，最大 10 MB（10 × 1024 × 1024 字节）；文字去掉首尾空白后最长 4000 字，去掉空白后为空视为没有文字
- `force_popup` 单独修改时不增加 `revision`；每成功发布一条新内容（包括只改文字、只改图）`revision` 加 1
- 现有磁盘上的 `content.json` 与 `image.jpg` / `image.png` 必须仍能读回；`content.json` 字段保持兼容：`text`、`force_popup`、`mime`、`revision`、`updated_at`、`image_name`
- 管理页路径保持：`GET`/`POST /login`、`POST /logout`、`GET /`、`POST /content`、`POST /force-popup`、`GET /media/current`（无图时 404）、`GET /api/status`（增加 `has_text`）、`GET /health` 返回 `{"ok": true}`
- Cookie 名 `tvdisplay_session`，值是密码的 HMAC-SHA256，不是密码原文；比较密码和令牌都用恒定时间比较；HMAC 消息为 `tvdisplay-admin-session-v1`，与现有 Python 实现一致，已登录浏览器会话在切换后仍有效
- 空密码和示例值（包括 `change-me`）拒绝启动，退出信息里不带密码本身；浏览器若带了 `Origin`，必须和 `Host` 一致，否则 POST 返回 403；关闭 `/docs`、`/redoc` 和 OpenAPI
- `WS /ws/tv` 认证帧为 `{"type":"auth","password":"..."}`，默认 10 秒超时，失败关闭码 1008；新电视连接用关闭码 4000 替换旧连接；连上后立即下发当前内容；消息字段形状不变
- 飞书环境变量为 `FEISHU_APP_ID`、`FEISHU_APP_SECRET`、`FEISHU_ALLOWED_CHAT_IDS`（逗号分隔，去首尾空白，完全匹配）；三项都未配置时网页和电视照常工作，日志写「飞书未配置」；三项都配置但连接失败时按 `min(1000 << attempt, 30000)` 毫秒退避重试，网页和电视不受影响；只配一部分则拒绝启动，说明缺了哪一项，不打印密钥
- 只处理白名单群的群聊消息，不必 @；忽略其他群、私聊和系统消息，不回复、不记消息正文；校验通过后先发交互卡片（「强制弹出」「普通显示」），点击后才发布；重复点击提示「这条已经发送过了」；待确认 10 分钟过期；最多 5 条，超出丢掉最早的（被丢掉的卡片按过期处理）
- 日志可以写「登录失败」「电视已连接」「已发布」「飞书已连接」「飞书未配置」「飞书连接已断开」；不可以写密码、Cookie、认证帧正文、飞书密钥、用户说明文字或群消息正文；过滤器把密钥替换成 `***`
- 部署：Compose 使用 `network_mode: host`，默认 `HOST=::`、`PORT=8080`，数据卷挂到 `/data`；没有域名时用 HTTP 并说明明文风险；有域名且 AAAA 指向树莓派时可用 Caddy Let’s Encrypt；不使用 Cloudflare Tunnel 或其他内网穿透
- Android 包名 `com.tvdisplay.remote`，minSdk 24，侧载调试 APK，不使用 Google Play 服务或 Firebase；服务器地址必须接受 `http://[2001:db8::1]:8080`；界面文案全部是简体中文；抢焦点仍只取决于 `force_popup && revision > lastShownRevision`
- 仓库、`.env.example` 和日志里都不得出现真实密码或飞书凭证

---

## File Structure

创建：

- `server/Cargo.toml` — 包名 `tvdisplay`，锁定下方依赖版本
- `server/Cargo.lock` — 由第一次 `cargo test` 生成并提交
- `server/src/lib.rs` — 库入口，供二进制和 `server/tests/*` 引用
- `server/src/main.rs` — 进程入口；无参数启动服务，`tvdisplay health` 做 Docker HEALTHCHECK
- `server/src/config.rs` — 从环境变量读取 `Settings` 和飞书配置
- `server/src/auth.rs` — 恒定时间比密码、HMAC Cookie、Origin/Host CSRF
- `server/src/content.rs` — 磁盘存储、校验、修订号、WebSocket 载荷
- `server/src/hub.rs` — 同一时间只保留一台电视连接
- `server/src/logging.rs` — 把密码和飞书密钥替换为 `***`
- `server/src/http.rs` — Axum 路由和 HTML 页面
- `server/src/health.rs` — 健康检查 URL 与探测，供 `tvdisplay health` 使用
- `server/templates/base.html`、`login.html`、`admin.html` — 简体中文管理页，支持纯文字
- `server/src/feishu/mod.rs` — 飞书模块入口，把长连接事件接到内容存储和电视推送
- `server/src/feishu/client.rs` — `tenant_access_token`、下载资源、回复消息、长连接与退避
- `server/src/feishu/allowlist.rs` — 允许的群 ID
- `server/src/feishu/drafts.rs` — 待确认项（10 分钟、最多 5 条、丢掉最早的）
- `server/src/feishu/cards.rs` — 交互卡片 JSON 与按钮处理
- `server/tests/common/mod.rs` — 集成测试共用夹具
- `server/tests/config.rs`、`auth.rs`、`content.rs`、`logging.rs`、`hub.rs`、`admin.rs`、`websocket.rs`、`feishu_allowlist.rs`、`feishu_drafts.rs`、`feishu_cards.rs`、`feishu_client.rs`、`feishu_connection.rs`、`feishu_bot.rs`、`deploy.rs`

修改：

- `Dockerfile` — 多阶段 Rust 构建；HEALTHCHECK 改为 `tvdisplay health`
- `docker-compose.yml`、`docker-compose.https.yml` — 仅在命令或健康检查变化时调整
- `.env.example` — 增加飞书三项占位符
- `.dockerignore`、`.gitignore` — 适配 Rust，去掉对已删除 Python 文件的依赖
- `docs/deploy-raspberry-pi.md`、`README.md`
- Android 最小改动：`ContentMessage.kt`、`Poster.kt`（如无需改则保持）、`TvApp.kt`、`strings.xml`，以及 `ContentMessageTest.kt` 与新增的发布/空白规则测试
- `FocusPolicy.kt` 不改逻辑

删除（靠后的专门任务，Rust 服务测试通过之后再做）：

- `server/tvdisplay/` 全部 Python 源码与模板
- `server/pyproject.toml`
- `server/tests/` 下的 Python 测试（`conftest.py`、`test_*.py`、`__init__.py`）

不新增平行的第二套后端目录。飞书长连接的 protobuf 帧类型放在 `server/src/feishu/client.rs`，不另起 crate 或协议目录。

### 依赖版本（已核对 docs.rs，2026-11-28）

`server/Cargo.toml` 使用这些版本。`hmac 0.13` 需同时导入 `KeyInit` 与 `Mac`。`reqwest 0.13` 的 TLS feature 名为 `rustls`（不再使用 `rustls-tls`）。`tokio-tungstenite 0.30` 使用 `rustls-tls-native-roots`。不引入官方未维护的 `feishu-ws 0.1.0`，按飞书官方 Go/Python SDK 的 HTTP/WebSocket 协议自行实现小客户端。

### 飞书协议要点（实现时按此，不要再改成 Webhook）

官方文档只把长连接作为 Go/Python/Java/Node SDK 的能力提供，不单独给出 Rust SDK。本计划按官方 SDK 源码实现同一套协议：

1. 取 token（最新 OAuth，企业自建应用）：`POST https://accounts.feishu.cn/oauth/v3/token`，见 [自建应用获取 tenant_access_token](https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/authentication-management/access-token/tenant_access_token)。请求体用 `application/x-www-form-urlencoded`：`grant_type=client_credentials&client_id=<APP_ID>&client_secret=<APP_SECRET>`。成功响应从 `access_token` 读取（不要读历史字段 `tenant_access_token`），`expires_in` 为秒，调用业务 API 时用 `Authorization: Bearer <access_token>`。凭证缓存到即将过期前再刷新（剩余不足 300 秒时重取）。
2. 取长连接地址（官方 SDK `GenEndpointUri`）：`POST https://open.feishu.cn/callback/ws/endpoint`，请求头 `locale: zh`，JSON `{"AppID":"<id>","AppSecret":"<secret>"}`。成功：`{"code":0,"msg":"...","data":{"URL":"wss://...","ClientConfig":{"ReconnectCount":-1,"ReconnectInterval":120,"ReconnectNonce":30,"PingInterval":120}}}`。`data.URL` 的查询串含 `device_id`、`service_id`。然后对 `data.URL` 做 WebSocket 握手。参考：[使用长连接接收事件](https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case?lang=zh-CN)、[使用长连接接收回调](https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address?lang=zh-CN)，以及 `larksuite/oapi-sdk-go` 的 `ws/const.go`、`ws/model.go`、`ws/client_message.go` 与 `larksuite/oapi-sdk-python` 的 `lark_oapi/ws/`。
3. 二进制帧为 protobuf2 消息 `pbbp2.Frame`：字段 `SeqID=1`、`LogID=2`、`service=3`、`method=4`（0=CONTROL，1=DATA）、`headers=5`（`Header.key`/`Header.value`）、`payload_encoding=6`、`payload_type=7`、`payload=8`、`LogIDNew=9`。头键：`type`、`message_id`、`sum`、`seq`、`trace_id`、`biz_rt`。`type` 取值：`event`、`card`、`ping`、`pong`。客户端按 `PingInterval`（默认 120 秒）发 CONTROL/`ping`，`service` 取 URL 里的 `service_id`。PONG 的 payload 可能是新的 `ClientConfig`。DATA 帧若 `sum>1` 则按 `message_id` 拼包。处理完后把同一帧的 payload 改写成 `{"code":200,"headers":null,"data":"<base64>"}`（`data` 为业务 JSON 的标准 Base64；事件通常无 `data`），并加上 `biz_rt`（毫秒），再发回。必须在 3 秒内完成应答。
4. 事件 [im.message.receive_v1](https://open.feishu.cn/document/server-docs/im-v1/message/events/receive?lang=zh-CN) 的 payload 为 schema 2.0 JSON；群 ID 在 `event.message.chat_id`，类型在 `event.message.chat_type`（`group` / `p2p`），`event.message.message_type` 与 `event.message.content`（JSON 字符串），`event.message.message_id` 用于回复与去重，`event.sender.sender_type` 为 `user` 或 `bot`。
5. 下载消息中的图片：[获取消息中的资源文件](https://open.feishu.cn/document/server-docs/im-v1/message/get-2?lang=zh-CN) `GET https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type=image`，成功时响应体是图片二进制。不要用只能下载机器人自己上传图片的 `/open-apis/im/v1/images/{image_key}`。
6. 回复：[回复消息](https://open.feishu.cn/document/server-docs/im-v1/message/reply?lang=zh-CN) `POST https://open.feishu.cn/open-apis/im/v1/messages/{message_id}/reply`，`msg_type` 为 `text` 或 `interactive`，`content` 是序列化后的 JSON 字符串。
7. 交互卡片用 JSON 2.0，按钮 `behaviors: [{"type":"callback","value":{"draft_id":"...","force_popup":true/false}}]`。点击后走 [card.action.trigger](https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN)。在长连接里 3 秒内把 toast + 更新后的卡片作为应答 `data` 返回。`event.action.value` 为按钮自定义数据，`event.context.open_chat_id` 为群 ID。

产品规格中的重连退避 `min(1000 << attempt, 30000)` 毫秒优先于平台下发的 `ClientConfig.ReconnectInterval`。心跳仍使用平台下发的 `PingInterval`（没有则 120 秒）。

官方 Python SDK 当前对 `type=card` 的帧直接 return、既不处理也不应答（已知问题）。Go SDK 近期也只自动分发 `type=event`。本实现同时接受 `type=event` 与 `type=card` 的 DATA 帧，一律按 payload 里的 `header.event_type` 分发并应答，这样 `im.message.receive_v1` 与 `card.action.trigger` 都能在长连接上工作。

---

### Task 1: 创建可编译的 Rust 工程并写下第一批失败测试

**Files:**
- Create: `server/Cargo.toml`
- Create: `server/src/lib.rs`
- Create: `server/src/main.rs`
- Create: `server/src/config.rs`（空模块，仅 `pub struct Settings` 占位，尚无 `load_settings`）
- Create: `server/src/health.rs`（空模块占位）
- Test: `server/tests/config.rs`

**Interfaces:**
- Consumes: 无
- Produces: 包名 `tvdisplay` 的库 + 二进制 `tvdisplay`；本任务结束时工程能编译，但 `load_settings` / `health_urls` 尚未实现，配置测试失败

- [ ] **Step 1: Write the failing test**

创建 `server/tests/config.rs`：

```rust
use std::collections::HashMap;
use std::path::PathBuf;
use tvdisplay::config::{load_settings, PLACEHOLDER_PASSWORDS};
use tvdisplay::health::health_urls;

#[test]
fn load_settings_reads_host_port_and_directory() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("HOST".into(), "::".into());
    env.insert("PORT".into(), "8080".into());
    env.insert("DATA_DIR".into(), "/tmp/tvdisplay-data".into());
    let settings = load_settings(&env).expect("settings should load");
    assert_eq!(settings.host, "::");
    assert_eq!(settings.port, 8080);
    assert_eq!(settings.data_dir, PathBuf::from("/tmp/tvdisplay-data"));
    assert_eq!(settings.admin_password, "a-long-random-secret");
    assert_eq!(settings.auth_timeout_seconds, 10.0);
    assert_eq!(settings.max_image_bytes, 10 * 1024 * 1024);
    assert_eq!(settings.text_max_chars, 4000);
}

#[test]
fn missing_password_exits_without_leaking_a_value() {
    let err = load_settings(&HashMap::new()).expect_err("must refuse");
    assert!(err.message.contains("ADMIN_PASSWORD"));
    assert!(!err.message.contains("change-me"));
}

#[test]
fn placeholder_passwords_are_rejected() {
    for placeholder in PLACEHOLDER_PASSWORDS {
        let mut env = HashMap::new();
        env.insert("ADMIN_PASSWORD".into(), (*placeholder).into());
        env.insert("DATA_DIR".into(), "/tmp/x".into());
        let err = load_settings(&env).expect_err(placeholder);
        assert!(
            !err.message.contains(placeholder),
            "exit text leaked placeholder {placeholder}"
        );
    }
}

#[test]
fn bad_port_is_rejected() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("PORT".into(), "nope".into());
    assert!(load_settings(&env).is_err());
}

#[test]
fn health_urls_cover_ipv6_and_localhost() {
    assert_eq!(
        health_urls("::", "8080"),
        vec![
            "http://[::1]:8080/health".to_string(),
            "http://127.0.0.1:8080/health".to_string(),
        ]
    );
    assert_eq!(
        health_urls("127.0.0.1", "8080"),
        vec!["http://127.0.0.1:8080/health".to_string()]
    );
    assert_eq!(
        health_urls("2001:db8::1", "9090"),
        vec!["http://[2001:db8::1]:9090/health".to_string()]
    );
}

#[test]
fn all_feishu_vars_missing_disables_feishu() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    let settings = load_settings(&env).expect("web and tv must still start");
    assert_eq!(settings.feishu, tvdisplay::config::FeishuMode::Disabled);
}

#[test]
fn partial_feishu_config_is_rejected_without_leaking_secret() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("FEISHU_APP_ID".into(), "cli_test".into());
    env.insert("FEISHU_APP_SECRET".into(), "super-secret-value".into());
    let err = load_settings(&env).expect_err("partial feishu config");
    assert!(err.message.contains("FEISHU_ALLOWED_CHAT_IDS"));
    assert!(!err.message.contains("super-secret-value"));
}

#[test]
fn complete_feishu_config_trims_chat_ids() {
    let mut env = HashMap::new();
    env.insert("ADMIN_PASSWORD".into(), "a-long-random-secret".into());
    env.insert("FEISHU_APP_ID".into(), "cli_test".into());
    env.insert("FEISHU_APP_SECRET".into(), "super-secret-value".into());
    env.insert(
        "FEISHU_ALLOWED_CHAT_IDS".into(),
        " oc_aaa ,oc_bbb ".into(),
    );
    let settings = load_settings(&env).unwrap();
    match settings.feishu {
        tvdisplay::config::FeishuMode::Enabled(feishu) => {
            assert_eq!(feishu.app_id, "cli_test");
            assert_eq!(feishu.app_secret, "super-secret-value");
            assert_eq!(feishu.allowed_chat_ids, vec!["oc_aaa", "oc_bbb"]);
        }
        tvdisplay::config::FeishuMode::Disabled => panic!("expected enabled"),
    }
}
```

- [ ] **Step 2: 写入可编译的工程骨架（尚无 `load_settings`）**

创建 `server/Cargo.toml`：

```toml
[package]
name = "tvdisplay"
version = "1.0.0"
edition = "2021"
description = "Push an image and/or text to one Android TV, with an optional Feishu bot"

[[bin]]
name = "tvdisplay"
path = "src/main.rs"

[dependencies]
axum = { version = "0.8.9", features = ["ws", "multipart", "form"] }
axum-extra = { version = "0.12.6", features = ["cookie"] }
base64 = "0.23.1"
bytes = "1.12.2"
cookie = "0.18.3"
futures-util = "0.3.34"
hex = "0.4.3"
hmac = "0.13.0"
http = "1.5.0"
minijinja = { version = "3.1.0", features = ["loader"] }
prost = "0.14.6"
reqwest = { version = "0.13.5", default-features = false, features = ["charset", "http2", "json", "stream", "rustls"] }
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.154"
sha2 = "0.11.2"
subtle = "2.6.1"
thiserror = "2.0.21"
time = { version = "0.3.55", features = ["formatting", "parsing"] }
tokio = { version = "1.54.1", features = ["full", "test-util"] }
tokio-tungstenite = { version = "0.30.0", features = ["rustls-tls-native-roots"] }
tower = "0.5.3"
tower-http = { version = "0.7.1", features = ["trace"] }
tracing = "0.1.44"
tracing-subscriber = { version = "0.3.23", features = ["env-filter", "fmt"] }
url = "2.5.9"
uuid = { version = "1.29.3", features = ["v4"] }

[dev-dependencies]
http-body-util = "0.1.5"
tempfile = "3.27.0"
```

创建 `server/src/lib.rs`：

```rust
pub mod auth;
pub mod config;
pub mod content;
pub mod feishu;
pub mod health;
pub mod http;
pub mod hub;
pub mod logging;
```

本任务先只让 `config` 与 `health` 存在，其余模块在后续任务创建。为了现在就能编译，暂时把 `lib.rs` 写成：

```rust
pub mod config;
pub mod health;
```

后续任务每增加一个模块，就在 `lib.rs` 里加一行 `pub mod`。不要提前放空的 `feishu` 目录。

创建 `server/src/config.rs`（仅类型占位，没有 `load_settings`）：

```rust
use std::path::PathBuf;

pub const PLACEHOLDER_PASSWORDS: &[&str] = &[
    "change-me",
    "password",
    "admin",
    "请改成一长串随机密码",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeishuSettings {
    pub app_id: String,
    pub app_secret: String,
    pub allowed_chat_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FeishuMode {
    Disabled,
    Enabled(FeishuSettings),
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub admin_password: String,
    pub data_dir: PathBuf,
    pub host: String,
    pub port: u16,
    pub auth_timeout_seconds: f64,
    pub max_image_bytes: usize,
    pub text_max_chars: usize,
    pub feishu: FeishuMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigError {
    pub message: String,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ConfigError {}
```

创建 `server/src/health.rs`：

```rust
// health_urls 在 Task 2 实现
```

创建 `server/src/main.rs`：

```rust
fn main() {
    let mut args = std::env::args();
    let _exe = args.next();
    match args.next().as_deref() {
        Some("health") => {
            eprintln!("health subcommand is not implemented yet");
            std::process::exit(2);
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            std::process::exit(2);
        }
        None => {
            eprintln!("server entry is not implemented yet");
            std::process::exit(2);
        }
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

```bash
cd server && cargo test --test config -- --nocapture
```

Expected: 编译可通过，但测试失败，错误信息含 `cannot find function load_settings in crate tvdisplay::config` 或同类未找到符号。若 `Cargo.lock` 尚未生成，第一次会联网解析依赖并写出 `server/Cargo.lock`，把它留在仓库里。

- [ ] **Step 4: Commit**

```bash
git add server/Cargo.toml server/Cargo.lock server/src/lib.rs server/src/main.rs server/src/config.rs server/src/health.rs server/tests/config.rs
git commit -m "test: add failing Rust config and health URL tests"
```

---

### Task 2: 实现进程配置与健康检查地址

**Files:**
- Modify: `server/src/config.rs`
- Modify: `server/src/health.rs`
- Test: `server/tests/config.rs`（Task 1 已写）

**Interfaces:**
- Consumes: Task 1 中的 `Settings`、`FeishuMode`、`FeishuSettings`、`ConfigError`、`PLACEHOLDER_PASSWORDS`
- Produces: `load_settings(environ: &HashMap<String, String>) -> Result<Settings, ConfigError>`
- Produces: `load_settings_from_env() -> Result<Settings, ConfigError>`，内部读 `std::env::vars()` 再调用 `load_settings`
- Produces: `health_urls(host: &str, port: &str) -> Vec<String>`，规则与现有 Python `health_urls` 相同：`::` / `0.0.0.0` / 空字符串返回 `http://[::1]:{port}/health` 与 `http://127.0.0.1:{port}/health`；主机含 `:` 时返回 `http://[{host}]:{port}/health`；否则 `http://{host}:{port}/health`

配置规则：

- `ADMIN_PASSWORD` 去掉首尾空白后为空：`ConfigError { message: "请设置环境变量 ADMIN_PASSWORD" }`
- 密码属于 `PLACEHOLDER_PASSWORDS`：`ConfigError { message: "ADMIN_PASSWORD 还是示例值，请换成你自己的随机密码" }`，消息里不得出现该示例值本身
- `HOST` 缺省或空字符串则为 `"::"`
- `PORT` 缺省 `"8080"`；非整数或范围不在 1..=65535：`PORT 必须是整数` 或 `PORT 必须在 1 到 65535 之间`
- `DATA_DIR` 缺省 `"data"`
- `auth_timeout_seconds` 固定默认 `10.0`，`max_image_bytes` 固定 `10 * 1024 * 1024`，`text_max_chars` 固定 `4000`（不从环境读）
- 飞书三项（`FEISHU_APP_ID`、`FEISHU_APP_SECRET`、`FEISHU_ALLOWED_CHAT_IDS`）均缺失或去掉空白后为空：`feishu = FeishuMode::Disabled`
- 三项都有值：`FeishuMode::Enabled`，`allowed_chat_ids` 按逗号拆分、去掉每段首尾空白、丢掉拆完为空的段
- 只配了其中一部分：拒绝启动，消息为 `飞书配置不完整，缺少 FEISHU_XXX`（多项用顿号连接，按 APP_ID、APP_SECRET、ALLOWED_CHAT_IDS 的顺序），不得出现密钥原文

- [ ] **Step 1: 实现 `health_urls` 与 `load_settings`**

把 `server/src/health.rs` 写成：

```rust
pub fn health_urls(host: &str, port: &str) -> Vec<String> {
    if matches!(host, "::" | "0.0.0.0" | "") {
        return vec![
            format!("http://[::1]:{port}/health"),
            format!("http://127.0.0.1:{port}/health"),
        ];
    }
    if host.contains(':') {
        vec![format!("http://[{host}]:{port}/health")]
    } else {
        vec![format!("http://{host}:{port}/health")]
    }
}
```

在 `server/src/config.rs` 增加：

```rust
use std::collections::HashMap;
use std::path::PathBuf;

// 保留 Task 1 已有的类型定义，然后追加：

const FEISHU_KEYS: [&str; 3] = [
    "FEISHU_APP_ID",
    "FEISHU_APP_SECRET",
    "FEISHU_ALLOWED_CHAT_IDS",
];

pub fn load_settings_from_env() -> Result<Settings, ConfigError> {
    let environ: HashMap<String, String> = std::env::vars().collect();
    load_settings(&environ)
}

pub fn load_settings(environ: &HashMap<String, String>) -> Result<Settings, ConfigError> {
    let password = environ.get("ADMIN_PASSWORD").cloned().unwrap_or_default();
    if password.trim().is_empty() {
        return Err(ConfigError {
            message: "请设置环境变量 ADMIN_PASSWORD".into(),
        });
    }
    if PLACEHOLDER_PASSWORDS.contains(&password.as_str()) {
        return Err(ConfigError {
            message: "ADMIN_PASSWORD 还是示例值，请换成你自己的随机密码".into(),
        });
    }

    let host = environ
        .get("HOST")
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .unwrap_or("::")
        .to_string();

    let raw_port = environ.get("PORT").map(String::as_str).unwrap_or("8080");
    let port: u16 = raw_port.parse().map_err(|_| ConfigError {
        message: "PORT 必须是整数".into(),
    })?;
    if !(1..=65535).contains(&port) {
        return Err(ConfigError {
            message: "PORT 必须在 1 到 65535 之间".into(),
        });
    }

    let data_dir = PathBuf::from(environ.get("DATA_DIR").map(String::as_str).unwrap_or("data"));
    let feishu = load_feishu(environ)?;

    Ok(Settings {
        admin_password: password,
        data_dir,
        host,
        port,
        auth_timeout_seconds: 10.0,
        max_image_bytes: 10 * 1024 * 1024,
        text_max_chars: 4000,
        feishu,
    })
}

fn env_nonempty(environ: &HashMap<String, String>, key: &str) -> Option<String> {
    environ
        .get(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn load_feishu(environ: &HashMap<String, String>) -> Result<FeishuMode, ConfigError> {
    let present: Vec<bool> = FEISHU_KEYS
        .iter()
        .map(|key| env_nonempty(environ, key).is_some())
        .collect();
    if present.iter().all(|flag| !flag) {
        return Ok(FeishuMode::Disabled);
    }
    if present.iter().any(|flag| !flag) {
        let missing: Vec<&str> = FEISHU_KEYS
            .iter()
            .zip(present)
            .filter(|(_, present)| !present)
            .map(|(key, _)| *key)
            .collect();
        return Err(ConfigError {
            message: format!("飞书配置不完整，缺少 {}", missing.join("、")),
        });
    }
    let allowed_chat_ids = env_nonempty(environ, "FEISHU_ALLOWED_CHAT_IDS")
        .unwrap()
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    if allowed_chat_ids.is_empty() {
        return Err(ConfigError {
            message: "飞书配置不完整，缺少 FEISHU_ALLOWED_CHAT_IDS".into(),
        });
    }
    Ok(FeishuMode::Enabled(FeishuSettings {
        app_id: env_nonempty(environ, "FEISHU_APP_ID").unwrap(),
        app_secret: env_nonempty(environ, "FEISHU_APP_SECRET").unwrap(),
        allowed_chat_ids,
    }))
}
```

注意：`u16` 的解析已经把大于 65535 的值排除了，`1..=65535` 的下限检查仍然保留 0 端口。`PORT=0` 必须失败。

- [ ] **Step 2: Run test to verify it passes**

```bash
cd server && cargo test --test config -- --nocapture
```

Expected: 全部 8 个测试 `ok`。

- [ ] **Step 3: Commit**

```bash
git add server/src/config.rs server/src/health.rs
git commit -m "feat: load server settings and Feishu config from the environment"
```

---

### Task 3: 恒定时间密码比较与兼容现有 HMAC 会话

**Files:**
- Create: `server/src/auth.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod auth;`）
- Test: `server/tests/auth.rs`

**Interfaces:**
- Consumes: 无（纯函数）
- Produces: `COOKIE_NAME: &str = "tvdisplay_session"`
- Produces: `passwords_match(provided: &str, expected: &str) -> bool` — 任一侧为空则 `false`；使用 `subtle::ConstantTimeEq`
- Produces: `issue_session_token(password: &str) -> String` — `HMAC-SHA256(key=password_bytes, message=b"tvdisplay-admin-session-v1")` 的小写十六进制，与现有 Python `hmac.new(password.encode("utf-8"), b"tvdisplay-admin-session-v1", hashlib.sha256).hexdigest()` 一致
- Produces: `session_token_valid(token: &str, password: &str) -> bool` — 先算期望令牌，再用恒定时间比较；任一侧为空则 `false`
- Produces: `origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool` — 无 `Origin` 放行；有 `Origin` 时必须是 `http` 或 `https` 且 `url::Url::authority()`（或 `host:port` 形式的 netloc）与 `Host` 完全相等；有 `Origin` 但没有 `Host` 则拒绝

已知兼容向量（密码 `unit-test-password`）：`31ab44cf12e015ceb7436a0b696dad9e2725da70819a6ee62f373dc7c5612d53`。切到 Rust 后，已登录管理页的浏览器不必重新登录。

- [ ] **Step 1: Write the failing test**

创建 `server/tests/auth.rs`：

```rust
use tvdisplay::auth::{
    issue_session_token, origin_allowed, passwords_match, session_token_valid, COOKIE_NAME,
};

const SECRET: &str = "unit-test-password";
const PYTHON_TOKEN: &str = "31ab44cf12e015ceb7436a0b696dad9e2725da70819a6ee62f373dc7c5612d53";

#[test]
fn cookie_name_matches_existing_service() {
    assert_eq!(COOKIE_NAME, "tvdisplay_session");
}

#[test]
fn matching_passwords() {
    assert!(passwords_match(SECRET, SECRET));
    assert!(!passwords_match("other-value", SECRET));
    assert!(!passwords_match("", SECRET));
    assert!(!passwords_match(SECRET, ""));
}

#[test]
fn session_token_matches_existing_python_hmac_and_is_not_the_password() {
    let token = issue_session_token(SECRET);
    assert_eq!(token, PYTHON_TOKEN);
    assert_ne!(token, SECRET);
    assert!(!token.contains(SECRET));
    assert!(session_token_valid(&token, SECRET));
    assert!(!session_token_valid(&token, "other-value"));
    assert!(!session_token_valid("short", SECRET));
    assert!(!session_token_valid("", SECRET));
}

#[test]
fn origin_must_match_host_when_present() {
    assert!(origin_allowed(None, Some("localhost")));
    assert!(origin_allowed(
        Some("http://[2001:db8::1]:8080"),
        Some("[2001:db8::1]:8080"),
    ));
    assert!(!origin_allowed(
        Some("https://evil.example"),
        Some("[2001:db8::1]:8080"),
    ));
    assert!(!origin_allowed(Some("http://evil.example"), None));
    assert!(!origin_allowed(Some("ftp://localhost"), Some("localhost")));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test auth -- --nocapture
```

Expected: FAIL，因为还没有 `tvdisplay::auth` 模块。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/lib.rs` 增加 `pub mod auth;`。

创建 `server/src/auth.rs`：

```rust
use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use url::Url;

pub const COOKIE_NAME: &str = "tvdisplay_session";
const SESSION_LABEL: &[u8] = b"tvdisplay-admin-session-v1";

type HmacSha256 = Hmac<Sha256>;

pub fn passwords_match(provided: &str, expected: &str) -> bool {
    if provided.is_empty() || expected.is_empty() {
        return false;
    }
    let left = provided.as_bytes();
    let right = expected.as_bytes();
    if left.len() != right.len() {
        return false;
    }
    left.ct_eq(right).into()
}

pub fn issue_session_token(password: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(password.as_bytes())
        .expect("HMAC-SHA256 accepts a key of any size");
    mac.update(SESSION_LABEL);
    hex::encode(mac.finalize().into_bytes())
}

pub fn session_token_valid(token: &str, password: &str) -> bool {
    if token.is_empty() || password.is_empty() {
        return false;
    }
    let expected = issue_session_token(password);
    if token.len() != expected.len() {
        return false;
    }
    token.as_bytes().ct_eq(expected.as_bytes()).into()
}

pub fn origin_allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    let Some(origin) = origin else {
        return true;
    };
    let Some(host) = host else {
        return false;
    };
    let Ok(parsed) = Url::parse(origin) else {
        return false;
    };
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return false;
    }
    let Some(authority) = origin_authority(&parsed) else {
        return false;
    };
    authority == host
}

fn origin_authority(parsed: &Url) -> Option<String> {
    let host = parsed.host_str()?;
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_string()
    };
    Some(match parsed.port() {
        Some(port) => format!("{host}:{port}"),
        None => host,
    })
}
```

`url::Url::host_str()` 对 IPv6 返回不带方括号的地址，所以要自行加上方括号，才能与请求头 `Host: [2001:db8::1]:8080` 比较。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test auth --test config -- --nocapture
```

Expected: 全部通过。若 `issue_session_token` 与 `PYTHON_TOKEN` 不一致，先核对 `KeyInit`/`Mac` 的用法和是否误用了大写十六进制，不要改向量。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/auth.rs server/tests/auth.rs
git commit -m "feat: check the admin password in constant time and keep the existing session cookie"
```

---

### Task 4: 内容校验与磁盘存储（含纯文字和旧 content.json）

**Files:**
- Create: `server/src/content.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod content;`）
- Test: `server/tests/content.rs`

**Interfaces:**
- Consumes: 无
- Produces: `MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024`，`TEXT_MAX_CHARS: usize = 4000`
- Produces: `ContentValidationError { message: String }`
- Produces: `DisplayContent { text: String, force_popup: bool, mime: Option<String>, revision: i64, updated_at: Option<String>, image: Option<Vec<u8>> }`，方法 `has_image(&self) -> bool`、`has_text(&self) -> bool`
- Produces: `sniff_image_mime(data: &[u8]) -> Option<&'static str>` — PNG 魔数 `\x89PNG\r\n\x1a\n` 为 `image/png`，JPEG 魔数 `\xff\xd8\xff` 为 `image/jpeg`，不看扩展名
- Produces: `validate_image(data: &[u8], max_bytes: usize) -> Result<&'static str, ContentValidationError>` — 空数据返回「请发送图片或文字」；超长返回「图片不能超过 10 MB」；非 JPEG/PNG 返回「只支持 JPEG 或 PNG」
- Produces: `normalize_text(text: Option<&str>, max_chars: usize) -> Result<String, ContentValidationError>` — 把 `\r\n` 换成 `\n` 再 `trim`；超长返回「说明文字不能超过 4000 字」
- Produces: `parse_force_popup(value: Option<&str>) -> bool` — 去掉空白并转小写后属于 `1`/`true`/`on`/`yes` 则为真
- Produces: `WsContentMessage`（serde 字段：`type`、`revision`、`text`、`force_popup`、`image_mime`、`image_base64`、`updated_at`）；没有图片时后两个字段序列化为 JSON `null`，不得省略
- Produces: `ContentStore::new(root: PathBuf) -> Self`（必要时创建目录）
- Produces: `ContentStore::load(&self) -> DisplayContent`
- Produces: `ContentStore::publish(&self, image: Option<&[u8]>, text: &str, force_popup: bool, max_bytes: usize) -> Result<DisplayContent, ContentValidationError>` — 图片和文字（已规范化且非空）至少要有一样，否则「请发送图片或文字」；成功则 `revision += 1`，`updated_at` 为 UTC、格式与 Python `datetime.now(timezone.utc).isoformat(timespec="seconds")` 相同，例如 `2026-11-28T12:00:00+00:00`；只发文字时删除遗留的 `image.jpg`/`image.png`，并把 `mime`、`image_name` 写成 `null`；只发图或图文都有时按 mime 写入对应文件并删除另一种扩展名
- Produces: `ContentStore::set_force_popup(&self, force_popup: bool) -> DisplayContent` — 不增加 `revision`
- Produces: `ContentStore::message(&self) -> WsContentMessage`

`load` 必须能读现有 Python 写下的 `content.json`。`image_name` 指向的文件不存在时，把 `mime` 视为没有图片（与现有 Python 一致）。损坏的 JSON 当作空元数据。写入使用 `*.tmp` 再 `rename` 替换。

- [ ] **Step 1: Write the failing test**

创建 `server/tests/content.rs`：

```rust
use base64::Engine;
use serde_json::json;
use tempfile::tempdir;
use tvdisplay::content::{
    normalize_text, parse_force_popup, validate_image, ContentStore, ContentValidationError,
    MAX_IMAGE_BYTES,
};

const JPEG: &[u8] = b"\xff\xd8\xff\xd9";
const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest";

#[test]
fn default_image_limit_is_ten_mebibytes() {
    assert_eq!(MAX_IMAGE_BYTES, 10 * 1024 * 1024);
}

#[test]
fn jpeg_and_png_magic_are_accepted_regardless_of_name() {
    assert_eq!(validate_image(JPEG, MAX_IMAGE_BYTES).unwrap(), "image/jpeg");
    assert_eq!(validate_image(PNG, MAX_IMAGE_BYTES).unwrap(), "image/png");
}

#[test]
fn empty_gif_and_oversize_are_rejected() {
    let empty = validate_image(b"", MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(empty.message, "请发送图片或文字");
    let gif = validate_image(b"GIF89a-not-a-photo", MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(gif.message, "只支持 JPEG 或 PNG");
    let huge = validate_image(b"\xff\xd8\xffxxxxxxxx", 10).unwrap_err();
    assert!(huge.message.contains("10 MB"));
}

#[test]
fn text_is_optional_and_bounded() {
    assert_eq!(normalize_text(None, 4000).unwrap(), "");
    assert_eq!(normalize_text(Some("  你好 \r\n"), 4000).unwrap(), "你好");
    let err = normalize_text(Some(&"字".repeat(12)), 8).unwrap_err();
    assert!(err.message.contains("4000") || err.message.contains("8") || err.message.contains("说明文字"));
}

#[test]
fn force_popup_parser() {
    assert!(parse_force_popup(Some("on")));
    assert!(parse_force_popup(Some("true")));
    assert!(!parse_force_popup(None));
    assert!(!parse_force_popup(Some("off")));
}

#[test]
fn store_roundtrip_keeps_image_text_and_toggle() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    let saved = store.publish(Some(JPEG), "今晚菜单", true, MAX_IMAGE_BYTES).unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.text, "今晚菜单");
    assert!(saved.force_popup);
    assert_eq!(saved.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(saved.image.as_deref(), Some(JPEG));

    let reloaded = ContentStore::new(dir.path().to_path_buf()).load();
    assert_eq!(reloaded.image.as_deref(), Some(JPEG));
    assert_eq!(reloaded.text, "今晚菜单");
    assert!(reloaded.force_popup);
    assert_eq!(reloaded.revision, 1);
}

#[test]
fn replacing_png_removes_previous_jpeg() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "", false, MAX_IMAGE_BYTES).unwrap();
    store.publish(Some(PNG), "第二张", false, MAX_IMAGE_BYTES).unwrap();
    assert!(!dir.path().join("image.jpg").exists());
    assert_eq!(std::fs::read(dir.path().join("image.png")).unwrap(), PNG);
    assert_eq!(store.load().revision, 2);
}

#[test]
fn text_only_publish_removes_leftover_images() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "旧图", false, MAX_IMAGE_BYTES).unwrap();
    let saved = store.publish(None, "只有文字", true, MAX_IMAGE_BYTES).unwrap();
    assert_eq!(saved.revision, 2);
    assert_eq!(saved.text, "只有文字");
    assert!(saved.image.is_none());
    assert!(saved.mime.is_none());
    assert!(!dir.path().join("image.jpg").exists());
    assert!(!dir.path().join("image.png").exists());
    let message = store.message();
    assert_eq!(message.revision, 2);
    assert_eq!(message.text, "只有文字");
    assert!(message.image_mime.is_none());
    assert!(message.image_base64.is_none());
}

#[test]
fn publish_rejects_when_both_image_and_text_are_missing() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    let err: ContentValidationError = store.publish(None, "", false, MAX_IMAGE_BYTES).unwrap_err();
    assert_eq!(err.message, "请发送图片或文字");
}

#[test]
fn toggle_persists_without_bumping_revision() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(JPEG), "字幕", false, MAX_IMAGE_BYTES).unwrap();
    let updated = store.set_force_popup(true);
    assert!(updated.force_popup);
    assert_eq!(updated.revision, 1);
    assert!(ContentStore::new(dir.path().to_path_buf()).load().force_popup);
}

#[test]
fn toggle_can_be_saved_before_any_content() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.set_force_popup(true);
    let loaded = store.load();
    assert!(loaded.force_popup);
    assert!(!loaded.has_image());
    assert_eq!(loaded.revision, 0);
}

#[test]
fn message_includes_base64_image() {
    let dir = tempdir().unwrap();
    let store = ContentStore::new(dir.path().to_path_buf());
    store.publish(Some(PNG), "", true, MAX_IMAGE_BYTES).unwrap();
    let message = store.message();
    assert_eq!(message.type_, "content");
    assert!(message.force_popup);
    assert_eq!(message.image_mime.as_deref(), Some("image/png"));
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(message.image_base64.as_ref().unwrap())
        .unwrap();
    assert_eq!(decoded, PNG);
}

#[test]
fn empty_message_when_nothing_published() {
    let dir = tempdir().unwrap();
    let message = ContentStore::new(dir.path().to_path_buf()).message();
    assert_eq!(message.revision, 0);
    assert_eq!(message.text, "");
    assert!(message.image_base64.is_none());
    assert!(message.image_mime.is_none());
    assert!(!message.force_popup);
}

#[test]
fn load_reads_existing_python_content_json() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("image.jpg"), JPEG).unwrap();
    std::fs::write(
        dir.path().join("content.json"),
        serde_json::to_vec_pretty(&json!({
            "text": "旧服务写下的内容",
            "force_popup": true,
            "mime": "image/jpeg",
            "revision": 4,
            "updated_at": "2026-11-26T06:00:00+00:00",
            "image_name": "image.jpg"
        }))
        .unwrap(),
    )
    .unwrap();
    let loaded = ContentStore::new(dir.path().to_path_buf()).load();
    assert_eq!(loaded.revision, 4);
    assert_eq!(loaded.text, "旧服务写下的内容");
    assert!(loaded.force_popup);
    assert_eq!(loaded.mime.as_deref(), Some("image/jpeg"));
    assert_eq!(loaded.image.as_deref(), Some(JPEG));
    assert_eq!(loaded.updated_at.as_deref(), Some("2026-11-26T06:00:00+00:00"));
}
```

上面 `normalize_text` 超长断言允许消息里出现「4000」或「说明文字」，实现时固定为「说明文字不能超过 4000 字」（即使用了更小的 `max_chars` 也用这句，与现有 Python 测试一致）。

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test content -- --nocapture
```

Expected: FAIL，缺少 `tvdisplay::content`。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/lib.rs` 增加 `pub mod content;`。

创建 `server/src/content.rs`（完整文件）：

```rust
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use time::format_description::well_known::Rfc3339;
use time::{OffsetDateTime, UtcOffset};

pub const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
pub const TEXT_MAX_CHARS: usize = 4000;

const JPEG_MAGIC: &[u8] = b"\xff\xd8\xff";
const PNG_MAGIC: &[u8] = b"\x89PNG\r\n\x1a\n";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentValidationError {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayContent {
    pub text: String,
    pub force_popup: bool,
    pub mime: Option<String>,
    pub revision: i64,
    pub updated_at: Option<String>,
    pub image: Option<Vec<u8>>,
}

impl DisplayContent {
    pub fn has_image(&self) -> bool {
        self.image.as_ref().is_some_and(|bytes| !bytes.is_empty())
    }

    pub fn has_text(&self) -> bool {
        !self.text.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WsContentMessage {
    #[serde(rename = "type")]
    pub type_: String,
    pub revision: i64,
    pub text: String,
    pub force_popup: bool,
    pub image_mime: Option<String>,
    pub image_base64: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContentMeta {
    text: String,
    force_popup: bool,
    mime: Option<String>,
    revision: i64,
    updated_at: Option<String>,
    image_name: Option<String>,
}

pub struct ContentStore {
    root: PathBuf,
    meta_path: PathBuf,
    lock: Mutex<()>,
}

impl ContentStore {
    pub fn new(root: PathBuf) -> Self {
        fs::create_dir_all(&root).expect("create content data directory");
        let meta_path = root.join("content.json");
        Self {
            root,
            meta_path,
            lock: Mutex::new(()),
        }
    }

    pub fn load(&self) -> DisplayContent {
        let _guard = self.lock.lock().expect("content lock");
        self.load_unlocked()
    }

    pub fn publish(
        &self,
        image: Option<&[u8]>,
        text: &str,
        force_popup: bool,
        max_bytes: usize,
    ) -> Result<DisplayContent, ContentValidationError> {
        let _guard = self.lock.lock().expect("content lock");
        let has_text = !text.is_empty();
        let mime = match image {
            Some(bytes) => Some(validate_image(bytes, max_bytes)?),
            None => None,
        };
        if mime.is_none() && !has_text {
            return Err(ContentValidationError {
                message: "请发送图片或文字".into(),
            });
        }
        let current = self.load_unlocked();
        let revision = current.revision + 1;
        let updated_at = now_utc_seconds();
        match mime {
            Some(mime) => {
                let name = image_name(mime);
                write_bytes(&self.root.join(name), image.unwrap());
                let other = if name == "image.png" {
                    "image.jpg"
                } else {
                    "image.png"
                };
                let leftover = self.root.join(other);
                if leftover.exists() {
                    let _ = fs::remove_file(leftover);
                }
                self.write_meta(&ContentMeta {
                    text: text.to_string(),
                    force_popup,
                    mime: Some(mime.to_string()),
                    revision,
                    updated_at: Some(updated_at),
                    image_name: Some(name.to_string()),
                });
            }
            None => {
                for name in ["image.jpg", "image.png"] {
                    let leftover = self.root.join(name);
                    if leftover.exists() {
                        let _ = fs::remove_file(leftover);
                    }
                }
                self.write_meta(&ContentMeta {
                    text: text.to_string(),
                    force_popup,
                    mime: None,
                    revision,
                    updated_at: Some(updated_at),
                    image_name: None,
                });
            }
        }
        Ok(self.load_unlocked())
    }

    pub fn set_force_popup(&self, force_popup: bool) -> DisplayContent {
        let _guard = self.lock.lock().expect("content lock");
        let current = self.load_unlocked();
        let mut meta = self.read_meta();
        meta.text = current.text;
        meta.force_popup = force_popup;
        meta.mime = current.mime;
        meta.revision = current.revision;
        meta.updated_at = current.updated_at;
        meta.image_name = if current.has_image() {
            meta.image_name
        } else {
            None
        };
        self.write_meta(&meta);
        self.load_unlocked()
    }

    pub fn message(&self) -> WsContentMessage {
        let content = self.load();
        let image_base64 = content.image.as_ref().map(|bytes| {
            base64::engine::general_purpose::STANDARD.encode(bytes)
        });
        WsContentMessage {
            type_: "content".into(),
            revision: content.revision,
            text: content.text,
            force_popup: content.force_popup,
            image_mime: content.mime,
            image_base64,
            updated_at: content.updated_at,
        }
    }

    fn load_unlocked(&self) -> DisplayContent {
        let meta = self.read_meta();
        let mut mime = meta.mime.filter(|value| !value.is_empty());
        let mut image = None;
        if let Some(name) = meta.image_name.as_deref().filter(|name| !name.is_empty()) {
            let path = self.root.join(name);
            if path.is_file() {
                image = fs::read(path).ok();
            } else {
                mime = None;
            }
        }
        DisplayContent {
            text: meta.text,
            force_popup: meta.force_popup,
            mime,
            revision: meta.revision,
            updated_at: meta.updated_at,
            image,
        }
    }

    fn read_meta(&self) -> ContentMeta {
        let Ok(raw) = fs::read_to_string(&self.meta_path) else {
            return ContentMeta::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    fn write_meta(&self, meta: &ContentMeta) {
        let payload = serde_json::to_vec_pretty(meta).expect("content meta is serializable");
        write_bytes(&self.meta_path, &payload);
    }
}

impl Default for ContentMeta {
    fn default() -> Self {
        Self {
            text: String::new(),
            force_popup: false,
            mime: None,
            revision: 0,
            updated_at: None,
            image_name: None,
        }
    }
}

pub fn sniff_image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(PNG_MAGIC) {
        Some("image/png")
    } else if data.starts_with(JPEG_MAGIC) {
        Some("image/jpeg")
    } else {
        None
    }
}

pub fn validate_image(data: &[u8], max_bytes: usize) -> Result<&'static str, ContentValidationError> {
    if data.is_empty() {
        return Err(ContentValidationError {
            message: "请发送图片或文字".into(),
        });
    }
    if data.len() > max_bytes {
        return Err(ContentValidationError {
            message: "图片不能超过 10 MB".into(),
        });
    }
    sniff_image_mime(data).ok_or(ContentValidationError {
        message: "只支持 JPEG 或 PNG".into(),
    })
}

pub fn normalize_text(text: Option<&str>, max_chars: usize) -> Result<String, ContentValidationError> {
    let value = text.unwrap_or("").replace("\r\n", "\n");
    let value = value.trim().to_string();
    if value.chars().count() > max_chars {
        return Err(ContentValidationError {
            message: "说明文字不能超过 4000 字".into(),
        });
    }
    Ok(value)
}

pub fn parse_force_popup(value: Option<&str>) -> bool {
    matches!(
        value.map(str::trim).map(str::to_ascii_lowercase).as_deref(),
        Some("1" | "true" | "on" | "yes")
    )
}

fn image_name(mime: &str) -> &'static str {
    if mime == "image/png" {
        "image.png"
    } else {
        "image.jpg"
    }
}

fn write_bytes(path: &Path, data: &[u8]) {
    let temporary = PathBuf::from(format!("{}.tmp", path.display()));
    fs::write(&temporary, data).expect("write temp content file");
    fs::rename(&temporary, path).expect("replace content file");
}

fn now_utc_seconds() -> String {
    let now = OffsetDateTime::now_utc().replace_offset(UtcOffset::UTC);
    // Python timespec=seconds 不带小数，偏移是 +00:00 而不是 Z。
    let format = time::format_description::parse(
        "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_minute]",
    )
    .expect("timestamp format");
    now.format(&format).unwrap_or_else(|_| {
        now.format(&Rfc3339).unwrap_or_default().replace('Z', "+00:00")
    })
}
```

`set_force_popup` 在还没有任何内容时也要能写出 `content.json`。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test content --test config --test auth -- --nocapture
```

Expected: 全部通过。纯文字发布后目录里不能再有旧图片；`load_reads_existing_python_content_json` 必须通过。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/content.rs server/tests/content.rs
git commit -m "feat: persist image and text-only content, including existing content.json"
```

---

### Task 5: 日志脱敏

**Files:**
- Create: `server/src/logging.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod logging;`）
- Test: `server/tests/logging.rs`

**Interfaces:**
- Consumes: 无
- Produces: `REDACTED: &str = "***"`
- Produces: `redact(text: &str, secrets: &[String]) -> String` — 把每一个非空 secret 的所有出现替换成 `***`
- Produces: `install_redaction(secrets: &[String])` — 在进程里给 `tracing` 订阅者装上过滤器；之后通过 `tracing` 或 `log` 兼容层写出的日志，消息和字段里的密码、飞书 App Secret 都会变成 `***`。空字符串不作为 secret。本任务先保证 `redact` 的单元测试以及一个用 `tracing_subscriber` 捕获输出的测试

不要把 secret 再写进日志配置或调试输出。

- [ ] **Step 1: Write the failing test**

创建 `server/tests/logging.rs`：

```rust
use tvdisplay::logging::{install_redaction, redact, REDACTED};

#[test]
fn redact_replaces_every_secret() {
    let secrets = vec!["unit-test-password".into(), "feishu-app-secret".into()];
    let rendered = redact(
        "rejected unit-test-password feishu-app-secret",
        &secrets,
    );
    assert!(!rendered.contains("unit-test-password"));
    assert!(!rendered.contains("feishu-app-secret"));
    assert!(rendered.contains(REDACTED));
}

#[test]
fn empty_secret_is_ignored() {
    assert_eq!(redact("hello", &["".into()]), "hello");
}

#[test]
fn install_redaction_filters_tracing_output() {
    let password = "unit-test-password".to_string();
    let buf = std::sync::Arc::new(std::sync::Mutex::new(Vec::<u8>::new()));
    let writer = Writer(buf.clone());
    install_redaction_with_writer(&[password.clone()], writer);
    tracing::info!("rejected {}", password);
    let rendered = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
    assert!(!rendered.contains(&password));
    assert!(rendered.contains("***"));
}

use std::io::Write;
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct Writer(Arc<Mutex<Vec<u8>>>);

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Writer {
    type Writer = Writer;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

// 测试里调用的辅助函数在 logging 模块中导出，见下面实现。
use tvdisplay::logging::install_redaction_with_writer;
```

把 `use tvdisplay::logging::install_redaction_with_writer;` 移到文件顶部，避免重复 use。`install_redaction` 在生产里写到 stderr；测试用 `install_redaction_with_writer`。

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test logging -- --nocapture
```

Expected: FAIL，缺少 `tvdisplay::logging`。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/lib.rs` 增加 `pub mod logging;`。

创建 `server/src/logging.rs`：

```rust
use std::io::{self, Write};
use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, Registry};

pub const REDACTED: &str = "***";

#[derive(Clone, Debug)]
pub struct Secrets(pub Vec<String>);

pub fn redact(text: &str, secrets: &[String]) -> String {
    let mut output = text.to_string();
    for secret in secrets {
        if secret.is_empty() {
            continue;
        }
        output = output.replace(secret, REDACTED);
    }
    output
}

pub fn install_redaction(secrets: &[String]) {
    install_redaction_with_writer(secrets, io::stderr);
}

pub fn install_redaction_with_writer<W>(secrets: &[String], writer: W)
where
    W: for<'a> MakeWriter<'a> + Send + Sync + 'static,
{
    let secrets = secrets
        .iter()
        .filter(|secret| !secret.is_empty())
        .cloned()
        .collect::<Vec<_>>();
    let writer = RedactingWriter {
        secrets: secrets.clone(),
        inner: writer,
    };
    let _ = Registry::default()
        .with(LevelFilter::INFO)
        .with(fmt::layer().with_writer(writer).with_target(false))
        .try_init();
}

struct RedactingWriter<W> {
    secrets: Vec<String>,
    inner: W,
}

impl<'a, W> MakeWriter<'a> for RedactingWriter<W>
where
    W: for<'b> MakeWriter<'b> + Send + Sync + 'static,
{
    type Writer = RedactingIo<<W as MakeWriter<'a>>::Writer>;

    fn make_writer(&'a self) -> Self::Writer {
        RedactingIo {
            secrets: self.secrets.clone(),
            inner: self.inner.make_writer(),
        }
    }
}

struct RedactingIo<W> {
    secrets: Vec<String>,
    inner: W,
}

impl<W: Write> Write for RedactingIo<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let redacted = redact(&text, &self.secrets);
        self.inner.write_all(redacted.as_bytes())?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
```

注意：`write` 返回的长度必须是原始 `buf.len()`，这样上层不会把已脱敏的较短字符串当成部分写入而重试。`try_init` 允许测试重复安装时不 panic（第二次安装会被忽略）。`install_redaction_with_writer` 的测试必须是该进程里第一次初始化 subscriber；本文件里先跑捕获测试即可。若与别的测试并行冲突，给这个测试加 `serial` 或放到单独的 `[[test]]` 二进制（已经是 `server/tests/logging.rs`，默认单独进程）。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test logging -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/logging.rs server/tests/logging.rs
git commit -m "feat: redact passwords and Feishu secrets from process logs"
```

---

### Task 6: 单台电视 Hub

**Files:**
- Create: `server/src/hub.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod hub;`）
- Test: `server/tests/hub.rs`（只测 Hub，不启动 HTTP）。完整 `/ws/tv` 协议在 Task 8 的 `server/tests/websocket.rs` 中覆盖。

**Interfaces:**
- Consumes: `WsContentMessage`（来自 `content`）只在后续 push 序列化时用；Hub 本身推的是已经序列化的 `String`
- Produces: `#[derive(Clone)] pub struct TvHub`
- Produces: `TvHub::new() -> Self`
- Produces: `TvHub::online(&self) -> bool`（异步：`pub async fn online(&self) -> bool`）
- Produces: `pub async fn replace(&self, outgoing: tokio::sync::mpsc::Sender<HubEvent>) -> Option<tokio::sync::mpsc::Sender<HubEvent>>` — 放入新连接，返回旧连接的 sender（若有）
- Produces: `pub async fn release(&self, connection_id: u64)` — 只在当前连接仍是这一条时清除
- Produces: `pub async fn push(&self, payload: &str) -> bool` — 没有连接返回 `false`；发送失败则释放并返回 `false`
- Produces: `#[derive(Debug, Clone)] pub enum HubEvent { Push(String), Close(u16) }`
- Produces: `pub async fn attach(&self, outgoing: tokio::sync::mpsc::Sender<HubEvent>) -> (u64, Option<tokio::sync::mpsc::Sender<HubEvent>>)` — 分配递增的 `connection_id`，便于 `release`

关闭码约定由调用方执行：拿到旧 sender 后对其发送 `HubEvent::Close(4000)`。Hub 不直接操作 WebSocket。

- [ ] **Step 1: Write the failing test**

创建 `server/tests/hub.rs`：

```rust
use tokio::sync::mpsc;
use tvdisplay::hub::{HubEvent, TvHub};

#[tokio::test]
async fn starts_offline_and_comes_online_after_attach() {
    let hub = TvHub::new();
    assert!(!hub.online().await);
    let (tx, _rx) = mpsc::channel(4);
    let (_id, previous) = hub.attach(tx).await;
    assert!(previous.is_none());
    assert!(hub.online().await);
}

#[tokio::test]
async fn replace_returns_the_previous_sender() {
    let hub = TvHub::new();
    let (first_tx, mut first_rx) = mpsc::channel(4);
    let (second_tx, _second_rx) = mpsc::channel(4);
    let (first_id, _) = hub.attach(first_tx).await;
    let (_second_id, previous) = hub.attach(second_tx).await;
    let previous = previous.expect("previous tv");
    previous.send(HubEvent::Close(4000)).await.unwrap();
    match first_rx.recv().await {
        Some(HubEvent::Close(4000)) => {}
        other => panic!("unexpected {other:?}"),
    }
    hub.release(first_id).await;
    assert!(hub.online().await);
}

#[tokio::test]
async fn release_only_clears_if_it_is_still_current() {
    let hub = TvHub::new();
    let (first_tx, _first_rx) = mpsc::channel(4);
    let (second_tx, _second_rx) = mpsc::channel(4);
    let (first_id, _) = hub.attach(first_tx).await;
    let (_second_id, _) = hub.attach(second_tx).await;
    hub.release(first_id).await;
    assert!(hub.online().await);
}

#[tokio::test]
async fn push_delivers_to_the_current_connection() {
    let hub = TvHub::new();
    let (tx, mut rx) = mpsc::channel(4);
    let _ = hub.attach(tx).await;
    assert!(hub.push(r#"{"type":"content","revision":1}"#).await);
    match rx.recv().await {
        Some(HubEvent::Push(body)) => assert!(body.contains("revision"));
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn push_without_a_tv_returns_false() {
    let hub = TvHub::new();
    assert!(!hub.push("{}").await);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test hub -- --nocapture
```

Expected: FAIL，缺少 `tvdisplay::hub`。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/lib.rs` 增加 `pub mod hub;`。

创建 `server/src/hub.rs`：

```rust
use tokio::sync::mpsc;
use tokio::sync::Mutex;

#[derive(Debug, Clone)]
pub enum HubEvent {
    Push(String),
    Close(u16),
}

#[derive(Clone)]
pub struct TvHub {
    inner: std::sync::Arc<Mutex<Inner>>,
}

struct Inner {
    next_id: u64,
    current: Option<Slot>,
}

struct Slot {
    id: u64,
    outgoing: mpsc::Sender<HubEvent>,
}

impl TvHub {
    pub fn new() -> Self {
        Self {
            inner: std::sync::Arc::new(Mutex::new(Inner {
                next_id: 1,
                current: None,
            })),
        }
    }

    pub async fn online(&self) -> bool {
        self.inner.lock().await.current.is_some()
    }

    pub async fn attach(
        &self,
        outgoing: mpsc::Sender<HubEvent>,
    ) -> (u64, Option<mpsc::Sender<HubEvent>>) {
        let mut inner = self.inner.lock().await;
        let id = inner.next_id;
        inner.next_id += 1;
        let previous = inner.current.take().map(|slot| slot.outgoing);
        inner.current = Some(Slot { id, outgoing });
        (id, previous)
    }

    pub async fn release(&self, connection_id: u64) {
        let mut inner = self.inner.lock().await;
        if inner.current.as_ref().is_some_and(|slot| slot.id == connection_id) {
            inner.current = None;
        }
    }

    pub async fn push(&self, payload: &str) -> bool {
        let outgoing = {
            let inner = self.inner.lock().await;
            inner.current.as_ref().map(|slot| slot.outgoing.clone())
        };
        let Some(outgoing) = outgoing else {
            return false;
        };
        match outgoing.send(HubEvent::Push(payload.to_string())).await {
            Ok(()) => true,
            Err(_) => {
                let mut inner = self.inner.lock().await;
                if inner
                    .current
                    .as_ref()
                    .is_some_and(|slot| slot.outgoing.same_channel(&outgoing))
                {
                    inner.current = None;
                }
                false
            }
        }
    }
}

impl Default for TvHub {
    fn default() -> Self {
        Self::new()
    }
}
```

`mpsc::Sender::same_channel` 在 Tokio 1.54 可用。若编译器提示没有该方法，改为在 `Slot` 中比较 `id`：`push` 失败时记下当时的 `id` 再 `release(id)`。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test hub -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/hub.rs server/tests/hub.rs
git commit -m "feat: keep a single TV connection in the in-process hub"
```

---

### Task 7: HTTP 管理页、状态接口与健康检查

**Files:**
- Create: `server/src/http.rs`
- Create: `server/templates/base.html`
- Create: `server/templates/login.html`
- Create: `server/templates/admin.html`
- Create: `server/tests/common/mod.rs`
- Test: `server/tests/admin.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod http;`）

**Interfaces:**
- Consumes: `Settings`、`ContentStore`、`TvHub`、`passwords_match`、`issue_session_token`、`session_token_valid`、`origin_allowed`、`COOKIE_NAME`、`normalize_text`、`parse_force_popup`、`ContentValidationError`
- Produces: `#[derive(Clone)] pub struct AppState { pub settings: Settings, pub store: std::sync::Arc<ContentStore>, pub hub: TvHub, pub templates: minijinja::Environment<'static> }`
- Produces: `pub fn build_state(settings: Settings) -> AppState`
- Produces: `pub fn template_env() -> minijinja::Environment<'static>`，用 `include_str!("../templates/...")` 嵌入三份模板
- Produces: `pub fn router(state: AppState) -> axum::Router`，路由：
  - `GET /health` → `{"ok": true}`，无需登录
  - `GET /login`、`POST /login`、`POST /logout`
  - `GET /`、`POST /content`、`POST /force-popup`
  - `GET /api/status`、`GET /media/current`
  - 本任务先不挂 `/ws/tv`（Task 8 加）
- Cookie：`tvdisplay_session`，HttpOnly，SameSite=Lax，`Path=/`，`Max-Age=2592000`（30 天），不设 `Secure`（默认 HTTP 必须能用）
- `POST` 若带 `Origin` 且与 `Host` 不一致：403，JSON `{"detail":"跨站请求已被拒绝"}`
- 未登录访问 `/`：303 到 `/login`；`/api/status` 与 `/media/current`：401 JSON `{"detail":"未登录"}`
- `GET /api/status` 登录后返回 `tv_online`、`force_popup`、`has_image`、`has_text`、`revision`、`text`
- `POST /content`：multipart，字段 `image`（可选）、`text`（可选）、`force_popup`；图片和文字至少一样；失败时 400 并在管理页显示中文原因，不回显密码；成功 303 到 `/`，并 `hub.push` 当前 `message()`（本任务 TV 通常离线，push 返回 false 也可以）
- `POST /force-popup`：只改开关；若 `revision > 0`（已有发布内容，含纯文字）则再 push 同一修订号
- 日志：登录失败写「登录失败」，成功发布写「已发布」；不得写密码或正文
- 不提供 `/docs`、`/redoc`、OpenAPI（Axum 默认没有，不要加）

- [ ] **Step 1: Write the failing test and test helpers**

创建 `server/tests/common/mod.rs`：

```rust
use std::collections::HashMap;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tvdisplay::config::{FeishuMode, Settings};
use tvdisplay::http::{build_state, router, AppState};

pub const PASSWORD: &str = "unit-test-password";
pub const JPEG: &[u8] = b"\xff\xd8\xff\xd9";
pub const PNG: &[u8] = b"\x89PNG\r\n\x1a\nrest-of-png";

pub struct TestApp {
    pub base: String,
    pub state: AppState,
    pub client: reqwest::Client,
    _dir: TempDir,
}

pub fn test_settings(dir: &std::path::Path) -> Settings {
    Settings {
        admin_password: PASSWORD.to_string(),
        data_dir: dir.to_path_buf(),
        host: "127.0.0.1".into(),
        port: 0,
        auth_timeout_seconds: 0.4,
        max_image_bytes: 64,
        text_max_chars: 20,
        feishu: FeishuMode::Disabled,
    }
}

pub async fn spawn_app() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let settings = test_settings(dir.path());
    let state = build_state(settings);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let app = router(state.clone());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .cookie_store(true)
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap();
    TestApp {
        base: format!("http://127.0.0.1:{}", addr.port()),
        state,
        client,
        _dir: dir,
    }
}

impl TestApp {
    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    pub async fn login(&self) -> reqwest::Response {
        self.client
            .post(self.url("/login"))
            .form(&HashMap::from([("password", PASSWORD)]))
            .send()
            .await
            .unwrap()
    }
}
```

`reqwest 0.13` 的 cookie store 需要 feature `cookies`。若编译提示没有 `cookie_store`，在 `Cargo.toml` 的 `[dev-dependencies]` 增加：

```toml
reqwest = { version = "0.13.5", default-features = false, features = ["charset", "http2", "json", "stream", "rustls", "cookies", "multipart"] }
```

创建 `server/tests/admin.rs`：

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test admin -- --nocapture
```

Expected: FAIL，缺少 `tvdisplay::http`。

- [ ] **Step 3: Write templates and HTTP implementation**

把下面完整内容写入 `server/templates/base.html`。minijinja 3 支持 `{% extends %}` 与 `{% block %}`。不要改 CSS 变量名和 class。

```html
<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <meta name="robots" content="noindex">
  <title>{% block title %}遥显{% endblock %}</title>
  <style>
    :root {
      --bg: #efe7dc;
      --ink: #1c1814;
      --muted: #5f564c;
      --card: #fffdf8;
      --line: #e4d8c8;
      --green: #0b6b4a;
      --green-ink: #08543a;
      --green-soft: #e4f5ec;
      --danger: #8f2d2a;
      --danger-soft: #fdeeee;
      --bezel: #16130f;
    }
    * { box-sizing: border-box; }
    body {
      margin: 0;
      color: var(--ink);
      background:
        radial-gradient(circle at top left, #f7f1e6 0, transparent 28rem),
        var(--bg);
      font-family: "PingFang SC", "Hiragino Sans GB", "Noto Sans SC", "Microsoft YaHei", sans-serif;
      font-size: 16px;
      line-height: 1.5;
    }
    .wrap { width: min(100%, 28rem); margin: 0 auto; padding: 1.25rem 1rem 3rem; }
    h1 { margin: 0; font-size: 1.7rem; letter-spacing: 0.06em; }
    .lede { margin: 0.3rem 0 1rem; color: var(--muted); }
    .card {
      background: var(--card);
      border: 1px solid var(--line);
      border-radius: 18px;
      padding: 1rem;
      margin: 0 0 0.85rem;
    }
    label { display: block; font-weight: 650; margin: 0.7rem 0 0.35rem; }
    input[type="text"], input[type="password"], textarea {
      width: 100%;
      font: inherit;
      padding: 0.75rem 0.8rem;
      border-radius: 12px;
      border: 1px solid #d5c7b4;
      background: white;
    }
    textarea { min-height: 6rem; resize: vertical; }
    button {
      width: 100%;
      margin-top: 0.9rem;
      border: 0;
      border-radius: 12px;
      padding: 0.85rem 1rem;
      font: inherit;
      font-weight: 750;
      background: var(--green);
      color: white;
    }
    button.ghost {
      width: auto;
      margin: 0;
      padding: 0.4rem 0.7rem;
      background: transparent;
      color: var(--ink);
      border: 1px solid var(--line);
    }
    .top { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
    .status-line { display: flex; align-items: center; justify-content: space-between; gap: 0.75rem; }
    .pill {
      display: inline-flex;
      align-items: center;
      gap: 0.4rem;
      border-radius: 999px;
      padding: 0.15rem 0.65rem;
      font-weight: 750;
    }
    .pill.on { background: var(--green-soft); color: var(--green-ink); }
    .pill.off { background: #ece4d8; color: var(--muted); }
    .dot { width: 0.55rem; height: 0.55rem; border-radius: 50%; background: currentColor; }
    .bezel {
      background: var(--bezel);
      color: white;
      border-radius: 16px;
      min-height: 11rem;
      overflow: hidden;
      display: flex;
      flex-direction: column;
    }
    .bezel img { display: block; width: 100%; height: auto; }
    .bezel .empty { margin: auto; padding: 2.2rem 1rem; color: #c9bfb2; text-align: center; }
    .caption { margin: 0; padding: 0.8rem 1rem 1rem; font-size: 1.05rem; }
    .error {
      background: var(--danger-soft);
      color: var(--danger);
      border-radius: 12px;
      padding: 0.7rem 0.8rem;
      margin-bottom: 0.4rem;
    }
    .check { display: flex; gap: 0.65rem; align-items: flex-start; font-weight: 500; margin-top: 0.85rem; }
    .check input { width: 1.25rem; height: 1.25rem; margin-top: 0.15rem; }
    .hint { color: var(--muted); font-size: 0.92rem; margin: 0.35rem 0 0; }
    .file { position: relative; display: block; font-weight: 500; }
    .file input {
      position: absolute;
      width: 1px;
      height: 1px;
      overflow: hidden;
      clip: rect(0 0 0 0);
    }
    .file span {
      display: block;
      padding: 0.8rem 0.85rem;
      border: 1px dashed #c9b79f;
      border-radius: 12px;
      background: white;
    }
    .meta { color: #d9d0c4; font-size: 0.92rem; padding: 0 1rem; }
    @media (min-width: 840px) {
      .wrap { width: min(100%, 42rem); padding-top: 2.5rem; }
    }
  </style>
</head>
<body>
  <main class="wrap">
    {% block body %}{% endblock %}
  </main>
</body>
</html>
```

`server/templates/login.html`：

```html
{% extends "base.html" %}
{% block title %}登录 · 遥显{% endblock %}
{% block body %}
  <h1>遥显</h1>
  <p class="lede">把图片或文字发到客厅的电视上。</p>
  <section class="card">
    {% if error %}
      <p class="error">{{ error }}</p>
    {% endif %}
    <form method="post" action="/login">
      <label for="password">管理密码</label>
      <input id="password" name="password" type="password" autocomplete="current-password" required>
      <button type="submit">进入</button>
    </form>
    <p class="hint">密码会送到你自己的服务器。没有域名、只用 http 时，路径上的人能看到这串密码。</p>
  </section>
{% endblock %}
```

`server/templates/admin.html`：在现有管理页基础上做这些修改——预览区在没有图片但有文字时显示文字而不是「尚未发布内容」；图片输入去掉 `required`；提示改为「JPEG 或 PNG，最大 10 MB。图片和文字至少填写一样。」；lede 改为「手机上选好图片或文字，电视上全屏显示。」其余状态 pill、强制弹出表单、3 秒刷新 `/api/status` 的脚本保持不变。完整文件：

```html
{% extends "base.html" %}
{% block title %}管理 · 遥显{% endblock %}
{% block body %}
  <div class="top">
    <h1>遥显</h1>
    <form method="post" action="/logout">
      <button class="ghost" type="submit">退出登录</button>
    </form>
  </div>
  <p class="lede">手机上选好图片或文字，电视上全屏显示。</p>

  <section class="card">
    <div class="status-line">
      <strong>电视状态</strong>
      <span id="tv-status" class="pill {{ 'on' if tv_online else 'off' }}">
        <span class="dot"></span>
        <span id="tv-status-text">{{ '在线' if tv_online else '离线' }}</span>
      </span>
    </div>
    <p class="hint">同一时间只连一台电视。后打开的那台会替换先打开的。</p>
  </section>

  <section class="card">
    <strong>当前内容</strong>
    <div class="bezel" style="margin-top: 0.7rem">
      {% if preview_url %}
        <img src="{{ preview_url }}" alt="当前发布到电视的图片">
        {% if content.text %}
          <p class="caption">{{ content.text }}</p>
        {% endif %}
      {% elif content.text %}
        <p class="caption">{{ content.text }}</p>
      {% else %}
        <p class="empty">尚未发布内容</p>
      {% endif %}
    </div>
    <p class="hint">强制弹出：{{ '开' if content.force_popup else '关' }}。开着时，新内容到达会把电视上的遥显调到前台；关着时不抢焦点。</p>
    <form method="post" action="/force-popup">
      <input type="hidden" name="force_popup" value="{{ 'off' if content.force_popup else 'on' }}">
      <button type="submit">{{ '关闭强制弹出' if content.force_popup else '开启强制弹出' }}</button>
    </form>
  </section>

  <section class="card">
    <strong>发布新内容</strong>
    {% if error %}
      <p class="error">{{ error }}</p>
    {% endif %}
    <form method="post" action="/content" enctype="multipart/form-data">
      <label class="file">
        图片
        <input id="image" name="image" type="file" accept="image/jpeg,image/png">
        <span id="file-name">选择 JPEG 或 PNG</span>
      </label>
      <p class="hint">JPEG 或 PNG，最大 10 MB。图片和文字至少填写一样。</p>
      <label for="text">说明文字</label>
      <textarea id="text" name="text" maxlength="4000" placeholder="可选。可以只发文字，或显示在图片下方。">{{ draft_text }}</textarea>
      <label class="check">
        <input type="checkbox" name="force_popup" value="on" {% if content.force_popup %}checked{% endif %}>
        <span>新内容到达时强制弹出</span>
      </label>
      <button type="submit">发布到电视</button>
    </form>
  </section>
  <script>
    async function refreshTvStatus() {
      const response = await fetch("/api/status", {headers: {Accept: "application/json"}});
      if (!response.ok) return;
      const data = await response.json();
      const pill = document.getElementById("tv-status");
      const text = document.getElementById("tv-status-text");
      pill.className = data.tv_online ? "pill on" : "pill off";
      text.textContent = data.tv_online ? "在线" : "离线";
    }
    const imageInput = document.getElementById("image");
    imageInput.addEventListener("change", () => {
      const file = imageInput.files && imageInput.files[0];
      document.getElementById("file-name").textContent = file ? file.name : "选择 JPEG 或 PNG";
    });
    refreshTvStatus();
    setInterval(refreshTvStatus, 3000);
  </script>
{% endblock %}
```


创建 `server/src/http.rs`。关键实现如下（完整写入该文件）：

```rust
use crate::auth::{
    issue_session_token, origin_allowed, passwords_match, session_token_valid, COOKIE_NAME,
};
use crate::config::Settings;
use crate::content::{
    normalize_text, parse_force_popup, ContentStore, ContentValidationError,
};
use crate::hub::TvHub;
use axum::body::Bytes;
use axum::extract::{Multipart, State};
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
    Router::new()
        .route("/health", get(health))
        .route("/login", get(login_form).post(login))
        .route("/logout", post(logout))
        .route("/", get(admin))
        .route("/content", post(publish))
        .route("/force-popup", post(update_force_popup))
        .route("/api/status", get(status))
        .route("/media/current", get(media))
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
    render(&state, "login.html", context! { error => Option::<String>::None }, StatusCode::OK)
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
    let jar = jar.remove(Cookie::from(COOKIE_NAME).into_owned());
    // Cookie::from 只给名字时 path 可能不匹配，显式指定 path=/
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
            error,
            content,
            preview_url,
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
```

`DisplayContent` 要能放进 minijinja context，给它 derive 或实现一个可序列化的视图。最简单：为 `DisplayContent` 加上 `#[derive(serde::Serialize)]`（在 `content.rs`）。模板用 `content.text`、`content.force_popup`、`content.revision`。

`logout` 里只保留一次 `jar.remove`，cookie 必须带 `path=/`。`cookie` crate 与 `axum-extra` 都再导出 `Cookie`；统一用 `axum_extra::extract::cookie::Cookie`。`Cookie::from(COOKIE_NAME)` 之后 `set_path("/")` 再 `remove`。

`media` 的 `CONTENT_TYPE` 头类型是 `HeaderValue`，把 `mime` 转成 `HeaderValue::from_str(&mime).unwrap()`。上面元组写法若类型不匹配，改成手动 `Response::builder()`。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test admin -- --nocapture
```

Expected: 全部通过。特别确认纯文字发布 303、`has_text=true`、`/media/current` 为 404，管理页能看到正文。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/http.rs server/src/content.rs server/templates/base.html server/templates/login.html server/templates/admin.html server/tests/common/mod.rs server/tests/admin.rs server/Cargo.toml
git commit -m "feat: add the Chinese admin site with text-only publishing"
```

---

### Task 8: 电视 WebSocket `/ws/tv`

**Files:**
- Modify: `server/src/http.rs`（增加 `/ws/tv`）
- Test: `server/tests/websocket.rs`

**Interfaces:**
- Consumes: `AppState`、`TvHub::attach` / `release` / `push`、`HubEvent`、`passwords_match`、`ContentStore::message`、`settings.admin_password`、`settings.auth_timeout_seconds`
- Produces: `router` 增加 `route("/ws/tv", get(tv_socket))`
- 认证：连接后必须在 `auth_timeout_seconds` 内收到一条文本帧，JSON 为 `{"type":"auth","password":"..."}`。密码不对、不是 JSON、缺字段、或超时：关闭码 1008，且不把该连接算作在线。查询串里的密码不能代替认证帧。
- 认证通过后：`attach` 新连接；若有旧连接，向其发送 `HubEvent::Close(4000)`；立即把 `store.message()` 作为文本帧下发；日志写「电视已连接」。连接结束后 `release(id)`。
- 不得把认证帧正文或密码写入日志。

- [ ] **Step 1: Write the failing test**

创建 `server/tests/websocket.rs`：

```rust
mod common;
use common::{spawn_app, JPEG, PASSWORD};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::time::{timeout, Duration};
use tokio_tungstenite::tungstenite::{protocol::CloseFrame, Message};

fn ws_url(http_base: &str) -> String {
    format!("{}/ws/tv", http_base.replacen("http://", "ws://", 1))
}

async fn connect(url: &str) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let (stream, _) = tokio_tungstenite::connect_async(url).await.expect("ws connect");
    stream
}

async fn auth_and_recv(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
    password: &str,
) -> Value {
    stream
        .send(Message::Text(json!({"type":"auth","password": password}).to_string().into()))
        .await
        .unwrap();
    let msg = stream.next().await.unwrap().unwrap();
    serde_json::from_str(msg.to_text().unwrap()).unwrap()
}

#[tokio::test]
async fn bad_password_is_rejected() {
    let app = spawn_app().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    stream
        .send(Message::Text(json!({"type":"auth","password":"wrong-password"}).to_string().into()))
        .await
        .unwrap();
    let close = wait_close(&mut stream).await;
    assert_eq!(close.code, 1008.into());
}

#[tokio::test]
async fn query_string_password_does_not_authenticate() {
    let app = spawn_app().await;
    let url = format!("{}?password={PASSWORD}", ws_url(&app.base));
    let mut stream = connect(&url).await;
    let close = timeout(Duration::from_secs(2), wait_close(&mut stream)).await;
    assert!(close.is_ok(), "connection without an auth frame must be closed");
    assert_eq!(close.unwrap().code, 1008.into());
}

#[tokio::test]
async fn connect_receives_current_content_after_offline() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("text", "补上的内容")
                .text("force_popup", "on")
                .part(
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
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["type"], "content");
    assert_eq!(message["text"], "补上的内容");
    assert_eq!(message["force_popup"], true);
    assert_eq!(message["revision"], 1);
    let decoded = base64::Engine::decode(
        &base64::engine::general_purpose::STANDARD,
        message["image_base64"].as_str().unwrap(),
    )
    .unwrap();
    assert_eq!(decoded, JPEG);
}

#[tokio::test]
async fn empty_catalog_is_sent_when_nothing_published() {
    let app = spawn_app().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["revision"], 0);
    assert!(message["image_base64"].is_null());
    assert!(message["image_mime"].is_null());
    assert_eq!(message["text"], "");
}

#[tokio::test]
async fn text_only_content_has_null_image_fields() {
    let app = spawn_app().await;
    app.login().await;
    app.client
        .post(app.url("/content"))
        .multipart(reqwest::multipart::Form::new().text("text", "纯文字"))
        .send()
        .await
        .unwrap();
    let mut stream = connect(&ws_url(&app.base)).await;
    let message = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(message["revision"], 1);
    assert_eq!(message["text"], "纯文字");
    assert!(message["image_base64"].is_null());
    assert!(message["image_mime"].is_null());
}

#[tokio::test]
async fn publish_pushes_to_the_connected_tv() {
    let app = spawn_app().await;
    app.login().await;
    let mut stream = connect(&ws_url(&app.base)).await;
    let first = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(first["revision"], 0);
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["tv_online"], true);
    app.client
        .post(app.url("/content"))
        .multipart(
            reqwest::multipart::Form::new()
                .text("text", "新海报")
                .part(
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
    let pushed = next_json(&mut stream).await;
    assert_eq!(pushed["text"], "新海报");
}

#[tokio::test]
async fn latest_connection_replaces_the_previous_one() {
    let app = spawn_app().await;
    let mut first = connect(&ws_url(&app.base)).await;
    let _ = auth_and_recv(&mut first, PASSWORD).await;
    let mut second = connect(&ws_url(&app.base)).await;
    let _ = auth_and_recv(&mut second, PASSWORD).await;
    let close = wait_close(&mut first).await;
    assert_eq!(close.code, 4000.into());
    app.login().await;
    let status: Value = app.client.get(app.url("/api/status")).send().await.unwrap().json().await.unwrap();
    assert_eq!(status["tv_online"], true);
}

#[tokio::test]
async fn toggle_push_keeps_revision() {
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
    let mut stream = connect(&ws_url(&app.base)).await;
    let first = auth_and_recv(&mut stream, PASSWORD).await;
    assert_eq!(first["revision"], 1);
    app.client
        .post(app.url("/force-popup"))
        .form(&[("force_popup", "on")])
        .send()
        .await
        .unwrap();
    let pushed = next_json(&mut stream).await;
    assert_eq!(pushed["revision"], 1);
    assert_eq!(pushed["force_popup"], true);
}

async fn next_json(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> Value {
    let msg = timeout(Duration::from_secs(3), stream.next())
        .await
        .expect("timeout")
        .unwrap()
        .unwrap();
    serde_json::from_str(msg.to_text().unwrap()).unwrap()
}

async fn wait_close(
    stream: &mut tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>,
) -> CloseFrame {
    loop {
        match timeout(Duration::from_secs(3), stream.next()).await {
            Ok(Some(Ok(Message::Close(Some(frame))))) => return frame,
            Ok(Some(Ok(Message::Close(None)))) => {
                return CloseFrame {
                    code: 1008.into(),
                    reason: "".into(),
                }
            }
            Ok(Some(Ok(_))) => continue,
            Ok(Some(Err(_))) | Ok(None) | Err(_) => {
                panic!("expected a close frame")
            }
        }
    }
}
```

在文件顶部加 `use base64::Engine;`。若 `CloseCode` 比较用 `u16` 更稳：`assert_eq!(u16::from(close.code), 1008);`。

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test websocket -- --nocapture
```

Expected: 连接失败或超时，因为还没有 `/ws/tv`。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/http.rs` 的 `router` 增加 `.route("/ws/tv", get(tv_socket))`，并加入：

```rust
use crate::hub::HubEvent;
use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket, WebSocketUpgrade};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};

async fn tv_socket(ws: WebSocketUpgrade, State(state): State<AppState>) -> Response {
    ws.on_upgrade(move |socket| handle_tv_socket(socket, state))
        .into_response()
}

async fn handle_tv_socket(mut socket: WebSocket, state: AppState) {
    let timeout_secs = state.settings.auth_timeout_seconds;
    let first = timeout(Duration::from_secs_f64(timeout_secs), socket.recv()).await;
    let Ok(Some(Ok(Message::Text(text)))) = first else {
        close_with(&mut socket, 1008).await;
        return;
    };
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(text.as_str());
    let authed = match parsed {
        Ok(value) => {
            let typ = value.get("type").and_then(serde_json::Value::as_str);
            let password = value.get("password").and_then(serde_json::Value::as_str);
            typ == Some("auth")
                && password.is_some_and(|provided| {
                    passwords_match(provided, &state.settings.admin_password)
                })
        }
        Err(_) => false,
    };
    drop(text);
    if !authed {
        close_with(&mut socket, 1008).await;
        return;
    }

    let (outgoing_tx, mut outgoing_rx) = mpsc::channel::<HubEvent>(8);
    let (connection_id, previous) = state.hub.attach(outgoing_tx).await;
    if let Some(previous) = previous {
        let _ = previous.send(HubEvent::Close(4000)).await;
    }
    let payload = serde_json::to_string(&state.store.message()).expect("ws message");
    if socket.send(Message::Text(Utf8Bytes::from(payload))).await.is_err() {
        state.hub.release(connection_id).await;
        return;
    }
    info!("电视已连接");

    loop {
        tokio::select! {
            event = outgoing_rx.recv() => {
                match event {
                    Some(HubEvent::Push(body)) => {
                        if socket.send(Message::Text(Utf8Bytes::from(body))).await.is_err() {
                            break;
                        }
                    }
                    Some(HubEvent::Close(code)) => {
                        close_with(&mut socket, code).await;
                        break;
                    }
                    None => break,
                }
            }
            incoming = socket.recv() => {
                match incoming {
                    Some(Ok(Message::Close(_))) | None => break,
                    Some(Ok(_)) => {}
                    Some(Err(_)) => break,
                }
            }
        }
    }
    state.hub.release(connection_id).await;
}

async fn close_with(socket: &mut WebSocket, code: u16) {
    let _ = socket
        .send(Message::Close(Some(CloseFrame {
            code: code.into(),
            reason: Utf8Bytes::default(),
        })))
        .await;
    let _ = socket.close().await;
}
```

Axum 0.8 的 `Message::Text` 使用 `Utf8Bytes`。若当前 API 是 `Message::Text(String)`，改成 `Message::Text(payload.into())`。`CloseFrame.code` 用 `axum::extract::ws::close_code` 或 `code.into()`。认证失败不要写密码；不要 `info` 认证帧。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test websocket --test admin -- --nocapture
```

Expected: 全部通过。查询串带密码但不能在超时前完成认证的连接必须以 1008 关闭。新连接必须用 4000 关掉旧连接。

- [ ] **Step 5: Commit**

```bash
git add server/src/http.rs server/tests/websocket.rs
git commit -m "feat: push content to the latest TV websocket"
```

---

### Task 9: 飞书群白名单

**Files:**
- Create: `server/src/feishu/mod.rs`
- Create: `server/src/feishu/allowlist.rs`
- Modify: `server/src/lib.rs`（增加 `pub mod feishu;`）
- Test: `server/tests/feishu_allowlist.rs`

**Interfaces:**
- Consumes: 无
- Produces: `parse_allowed_chat_ids(raw: &str) -> Vec<String>` — 逗号拆分、去空白、丢掉空段
- Produces: `chat_is_allowed(chat_id: &str, allowed: &[String]) -> bool` — 完全匹配，不去掉 `chat_id` 内部空白
- Produces: `is_allowed_group(chat_id: &str, chat_type: &str, allowed: &[String]) -> bool` — 仅当 `chat_type == "group"` 且 `chat_is_allowed` 为真。私聊（`p2p`）即使 ID 碰巧在列表里也拒绝

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_allowlist.rs`：

```rust
use tvdisplay::feishu::allowlist::{chat_is_allowed, is_allowed_group, parse_allowed_chat_ids};

#[test]
fn parse_trims_and_drops_empty_segments() {
    assert_eq!(
        parse_allowed_chat_ids(" oc_aaa , ,oc_bbb"),
        vec!["oc_aaa", "oc_bbb"]
    );
}

#[test]
fn exact_match_only() {
    let allowed = parse_allowed_chat_ids("oc_aaa,oc_bbb");
    assert!(chat_is_allowed("oc_aaa", &allowed));
    assert!(!chat_is_allowed("oc_aa", &allowed));
    assert!(!chat_is_allowed(" oc_aaa", &allowed));
}

#[test]
fn private_chats_are_never_allowed() {
    let allowed = parse_allowed_chat_ids("oc_aaa");
    assert!(is_allowed_group("oc_aaa", "group", &allowed));
    assert!(!is_allowed_group("oc_aaa", "p2p", &allowed));
    assert!(!is_allowed_group("oc_zzz", "group", &allowed));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_allowlist -- --nocapture
```

Expected: FAIL，缺少模块。

- [ ] **Step 3: Write minimal implementation**

`server/src/feishu/mod.rs`：

```rust
pub mod allowlist;
```

`server/src/feishu/allowlist.rs`：

```rust
pub fn parse_allowed_chat_ids(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

pub fn chat_is_allowed(chat_id: &str, allowed: &[String]) -> bool {
    allowed.iter().any(|item| item == chat_id)
}

pub fn is_allowed_group(chat_id: &str, chat_type: &str, allowed: &[String]) -> bool {
    chat_type == "group" && chat_is_allowed(chat_id, allowed)
}
```

`server/src/lib.rs` 增加 `pub mod feishu;`。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_allowlist -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/feishu/mod.rs server/src/feishu/allowlist.rs server/tests/feishu_allowlist.rs
git commit -m "feat: match Feishu group chats against an exact allowlist"
```

---

### Task 10: 飞书待确认草稿

**Files:**
- Create: `server/src/feishu/drafts.rs`
- Modify: `server/src/feishu/mod.rs`（增加 `pub mod drafts;`）
- Test: `server/tests/feishu_drafts.rs`

**Interfaces:**
- Consumes: 无
- Produces: `MAX_DRAFTS: usize = 5`，`DRAFT_TTL: Duration = Duration::from_secs(600)`
- Produces: `pub struct Draft { pub id: String, pub text: String, pub image: Option<Vec<u8>>, pub created_at: Instant }`
- Produces: `pub enum DraftError { AlreadySent, ExpiredOrMissing }`
- Produces: `DraftStore::new() -> Self`
- Produces: `insert(&mut self, draft: Draft, now: Instant) -> Option<String>` — 若待确认（尚未发送）数量已达 5，丢掉最早的一条并返回其 `id`；被丢掉的条目之后按过期处理，不记为已发送
- Produces: `take_for_publish(&mut self, id: &str, now: Instant) -> Result<Draft, DraftError>` — 未过期的待确认项取出后标记为已发送（释放图片只发生在调用方拿到所有权之后；store 内不再保留图片字节）；同一 id 再次调用返回 `AlreadySent`（只要仍在原 10 分钟窗口内）；超过 10 分钟、被淘汰或不存在返回 `ExpiredOrMissing`
- 已发送记录不计入 5 条上限，但保留到该草稿的原始过期时间，以便区分「已经发送过了」和「已过期」
- 测试用注入的 `now: Instant`，不要在函数里直接读系统时钟

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_drafts.rs`：

```rust
use std::time::{Duration, Instant};
use tvdisplay::feishu::drafts::{Draft, DraftError, DraftStore, DRAFT_TTL, MAX_DRAFTS};

fn draft(id: &str, at: Instant) -> Draft {
    Draft {
        id: id.into(),
        text: format!("text-{id}"),
        image: Some(b"\xff\xd8\xff".to_vec()),
        created_at: at,
    }
}

#[test]
fn limits_and_ttl_constants() {
    assert_eq!(MAX_DRAFTS, 5);
    assert_eq!(DRAFT_TTL, Duration::from_secs(600));
}

#[test]
fn take_returns_the_pending_draft_once() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    let taken = store.take_for_publish("d1", now).unwrap();
    assert_eq!(taken.id, "d1");
    assert_eq!(taken.image.as_deref(), Some(b"\xff\xd8\xff".as_ref()));
    match store.take_for_publish("d1", now) {
        Err(DraftError::AlreadySent) => {}
        other => panic!("expected AlreadySent, got {other:?}"),
    }
}

#[test]
fn expired_draft_cannot_be_published() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    let later = now + DRAFT_TTL + Duration::from_secs(1);
    match store.take_for_publish("d1", later) {
        Err(DraftError::ExpiredOrMissing) => {}
        other => panic!("expected ExpiredOrMissing, got {other:?}"),
    }
}

#[test]
fn sixth_pending_draft_evicts_the_oldest() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    for index in 1..=5 {
        assert!(store.insert(draft(&format!("d{index}"), now), now).is_none());
    }
    let evicted = store.insert(draft("d6", now), now);
    assert_eq!(evicted.as_deref(), Some("d1"));
    match store.take_for_publish("d1", now) {
        Err(DraftError::ExpiredOrMissing) => {}
        other => panic!("evicted draft must be expired, got {other:?}"),
    }
    assert!(store.take_for_publish("d6", now).is_ok());
}

#[test]
fn sent_drafts_do_not_count_toward_the_cap() {
    let mut store = DraftStore::new();
    let now = Instant::now();
    store.insert(draft("d1", now), now);
    store.take_for_publish("d1", now).unwrap();
    for index in 2..=6 {
        assert!(
            store.insert(draft(&format!("d{index}"), now), now).is_none(),
            "already-sent drafts must not force eviction"
        );
    }
    match store.take_for_publish("d1", now) {
        Err(DraftError::AlreadySent) => {}
        other => panic!("sent marker should remain, got {other:?}"),
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_drafts -- --nocapture
```

Expected: FAIL，缺少 `drafts` 模块。

- [ ] **Step 3: Write minimal implementation**

`server/src/feishu/mod.rs` 增加 `pub mod drafts;`。

创建 `server/src/feishu/drafts.rs`：

```rust
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_DRAFTS: usize = 5;
pub const DRAFT_TTL: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone)]
pub struct Draft {
    pub id: String,
    pub text: String,
    pub image: Option<Vec<u8>>,
    pub created_at: Instant,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DraftError {
    AlreadySent,
    ExpiredOrMissing,
}

struct SentMarker {
    expires_at: Instant,
}

pub struct DraftStore {
    pending: Vec<Draft>,
    sent: HashMap<String, SentMarker>,
}

impl DraftStore {
    pub fn new() -> Self {
        Self {
            pending: Vec::new(),
            sent: HashMap::new(),
        }
    }

    pub fn insert(&mut self, draft: Draft, now: Instant) -> Option<String> {
        self.gc_sent(now);
        let mut evicted = None;
        if self.pending.len() >= MAX_DRAFTS {
            evicted = self.pending.first().map(|draft| draft.id.clone());
            self.pending.remove(0);
        }
        self.pending.push(draft);
        evicted
    }

    pub fn take_for_publish(&mut self, id: &str, now: Instant) -> Result<Draft, DraftError> {
        self.gc_sent(now);
        if self.sent.contains_key(id) {
            return Err(DraftError::AlreadySent);
        }
        let position = self.pending.iter().position(|draft| draft.id == id);
        let Some(position) = position else {
            return Err(DraftError::ExpiredOrMissing);
        };
        if now.duration_since(self.pending[position].created_at) >= DRAFT_TTL {
            self.pending.remove(position);
            return Err(DraftError::ExpiredOrMissing);
        }
        let draft = self.pending.remove(position);
        self.sent.insert(
            draft.id.clone(),
            SentMarker {
                expires_at: draft.created_at + DRAFT_TTL,
            },
        );
        Ok(draft)
    }

    fn gc_sent(&mut self, now: Instant) {
        self.sent.retain(|_, marker| marker.expires_at > now);
        self.pending
            .retain(|draft| now.duration_since(draft.created_at) < DRAFT_TTL);
    }
}

impl Default for DraftStore {
    fn default() -> Self {
        Self::new()
    }
}
```

`insert` 在淘汰最早条目时不要把它写入 `sent`。`take_for_publish` 把图片所有权交给调用方，store 不再持有那份字节。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_drafts -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add server/src/feishu/mod.rs server/src/feishu/drafts.rs server/tests/feishu_drafts.rs
git commit -m "feat: keep at most five Feishu drafts for ten minutes"
```

---

### Task 11: 飞书交互卡片与按钮处理

**Files:**
- Create: `server/src/feishu/cards.rs`
- Modify: `server/src/feishu/mod.rs`（增加 `pub mod cards;`）
- Test: `server/tests/feishu_cards.rs`

**Interfaces:**
- Consumes: 无
- Produces: `confirmation_card(draft_id: &str, text: &str, has_image: bool) -> serde_json::Value` — JSON 2.0 交互卡片，正文展示文字摘要（若有，超过 80 字截断加省略号）以及「带图片：是/否」；两个按钮文案为「强制弹出」和「普通显示」，`behaviors` 为 `callback`，`value` 分别为 `{"draft_id":"...","force_popup":true}` 与 `{"draft_id":"...","force_popup":false}`
- Produces: `result_card(text: &str, has_image: bool, status: &str) -> serde_json::Value` — 无按钮，用于点击后更新卡片，`status` 为「发送成功」或「已保存，但电视当前不在线」等
- Produces: `parse_button_value(value: &serde_json::Value) -> Option<ButtonAction>`，`ButtonAction { draft_id: String, force_popup: bool }`
- Produces: `callback_response(toast_type: &str, toast: &str, card: Option<serde_json::Value>) -> serde_json::Value` — 按官方 [card.action.trigger 响应](https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN) 生成 `toast` + 可选 `card: { "type": "raw", "data": ... }`

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_cards.rs`：

```rust
use serde_json::json;
use tvdisplay::feishu::cards::{
    callback_response, confirmation_card, parse_button_value, result_card,
};

#[test]
fn confirmation_card_has_two_callback_buttons() {
    let card = confirmation_card("draft-1", "今晚菜单", true);
    assert_eq!(card["schema"], "2.0");
    let rendered = card.to_string();
    assert!(rendered.contains("今晚菜单"));
    assert!(rendered.contains("带图片：是"));
    assert!(rendered.contains("强制弹出"));
    assert!(rendered.contains("普通显示"));
    let buttons: Vec<_> = card["body"]["elements"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|element| element["tag"] == "button")
        .collect();
    assert_eq!(buttons.len(), 2);
    let values: Vec<_> = buttons
        .iter()
        .map(|button| button["behaviors"][0]["value"].clone())
        .collect();
    assert!(values.contains(&json!({"draft_id":"draft-1","force_popup":true})));
    assert!(values.contains(&json!({"draft_id":"draft-1","force_popup":false})));
}

#[test]
fn parse_button_value_reads_force_popup() {
    let action = parse_button_value(&json!({"draft_id":"d1","force_popup":false})).unwrap();
    assert_eq!(action.draft_id, "d1");
    assert!(!action.force_popup);
}

#[test]
fn callback_response_includes_toast_and_raw_card() {
    let card = result_card("今晚菜单", true, "发送成功");
    let response = callback_response("success", "发送成功", Some(card.clone()));
    assert_eq!(response["toast"]["type"], "success");
    assert_eq!(response["toast"]["content"], "发送成功");
    assert_eq!(response["card"]["type"], "raw");
    assert_eq!(response["card"]["data"]["schema"], "2.0");
    assert!(response["card"]["data"].to_string().contains("发送成功"));
    assert!(!response["card"]["data"].to_string().contains("强制弹出"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_cards -- --nocapture
```

Expected: FAIL，缺少 `cards` 模块。

- [ ] **Step 3: Write minimal implementation**

`server/src/feishu/mod.rs` 增加 `pub mod cards;`。

创建 `server/src/feishu/cards.rs`：

```rust
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ButtonAction {
    pub draft_id: String,
    pub force_popup: bool,
}

pub fn confirmation_card(draft_id: &str, text: &str, has_image: bool) -> Value {
    let summary = if text.is_empty() {
        "（没有文字）".to_string()
    } else {
        truncate_chars(text, 80)
    };
    let image_line = if has_image { "带图片：是" } else { "带图片：否" };
    json!({
        "schema": "2.0",
        "header": {
            "title": { "tag": "plain_text", "content": "发布到电视" },
            "template": "blue"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": { "tag": "plain_text", "content": format!("{summary}\n{image_line}") }
                },
                button("强制弹出", "primary", draft_id, true),
                button("普通显示", "default", draft_id, false)
            ]
        }
    })
}

pub fn result_card(text: &str, has_image: bool, status: &str) -> Value {
    let summary = if text.is_empty() {
        "（没有文字）".to_string()
    } else {
        truncate_chars(text, 80)
    };
    let image_line = if has_image { "带图片：是" } else { "带图片：否" };
    json!({
        "schema": "2.0",
        "header": {
            "title": { "tag": "plain_text", "content": "发布到电视" },
            "template": "green"
        },
        "body": {
            "elements": [
                {
                    "tag": "div",
                    "text": { "tag": "plain_text", "content": format!("{summary}\n{image_line}\n{status}") }
                }
            ]
        }
    })
}

pub fn parse_button_value(value: &Value) -> Option<ButtonAction> {
    Some(ButtonAction {
        draft_id: value.get("draft_id")?.as_str()?.to_string(),
        force_popup: value.get("force_popup")?.as_bool()?,
    })
}

pub fn callback_response(toast_type: &str, toast: &str, card: Option<Value>) -> Value {
    let mut body = json!({
        "toast": { "type": toast_type, "content": toast }
    });
    if let Some(card) = card {
        body["card"] = json!({ "type": "raw", "data": card });
    }
    body
}

fn button(label: &str, typ: &str, draft_id: &str, force_popup: bool) -> Value {
    json!({
        "tag": "button",
        "type": typ,
        "text": { "tag": "plain_text", "content": label },
        "behaviors": [{
            "type": "callback",
            "value": { "draft_id": draft_id, "force_popup": force_popup }
        }]
    })
}

fn truncate_chars(text: &str, max: usize) -> String {
    let count = text.chars().count();
    if count <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_cards -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add server/src/feishu/mod.rs server/src/feishu/cards.rs server/tests/feishu_cards.rs
git commit -m "feat: build Feishu confirmation cards with popup and normal buttons"
```

---

### Task 12: 飞书 HTTP API 客户端（token、下载图片、回复）

**Files:**
- Create: `server/src/feishu/client.rs` 的 HTTP 部分（长连接放到 Task 13，本任务先实现 REST）
- Modify: `server/src/feishu/mod.rs`（增加 `pub mod client;`）
- Test: `server/tests/feishu_client.rs`

**Interfaces:**
- Consumes: 无飞书业务模块之外的类型；下载上限使用调用方传入的 `max_bytes`
- Produces: `pub struct FeishuApi { ... }`，`FeishuApi::new(app_id: String, app_secret: String, open_base: String, accounts_base: String) -> Self`。生产用 `open_base = "https://open.feishu.cn"`，`accounts_base = "https://accounts.feishu.cn"`；测试传入本地 mock 地址
- Produces: `pub async fn tenant_access_token(&self) -> Result<String, FeishuError>` — `POST {accounts_base}/oauth/v3/token`，`Content-Type: application/x-www-form-urlencoded`，body 为 `grant_type=client_credentials&client_id=...&client_secret=...`。成功时读 `code == 0` 的 `access_token` 与 `expires_in`，缓存到剩余不足 300 秒再刷新。失败映射为 `FeishuError::Token`
- Produces: `pub async fn download_message_image(&self, message_id: &str, file_key: &str, max_bytes: usize) -> Result<Vec<u8>, FeishuError>` — `GET {open_base}/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type=image`，头 `Authorization: Bearer <token>`。流式读取，超过 `max_bytes` 返回 `FeishuError::ImageTooLarge`（对应「图片不能超过 10 MB」）；HTTP 非 200 或网络失败返回 `FeishuError::Download`（对应「无法下载图片，请重试」）
- Produces: `pub async fn reply_text(&self, message_id: &str, text: &str) -> Result<(), FeishuError>` — `POST {open_base}/open-apis/im/v1/messages/{message_id}/reply`，JSON `{"msg_type":"text","content":"{\"text\":\"...\"}"}`（`content` 是序列化后的字符串）
- Produces: `pub async fn reply_card(&self, message_id: &str, card: &serde_json::Value) -> Result<(), FeishuError>` — 同样的 reply URL，`msg_type` 为 `interactive`，`content` 为卡片 JSON 的字符串
- Produces: `#[derive(Debug)] pub enum FeishuError { Token(String), Download, ImageTooLarge, Reply(String) }`，`Display` 不包含 token 或 app_secret
- 错误或调试日志不得写出 `app_secret`、token 或用户正文

官方文档：
- Token：https://open.feishu.cn/document/uAjLw4CM/ukTMukTMukTM/authentication-management/access-token/tenant_access_token
- 下载：https://open.feishu.cn/document/server-docs/im-v1/message/get-2?lang=zh-CN
- 回复：https://open.feishu.cn/document/server-docs/im-v1/message/reply?lang=zh-CN

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_client.rs`，用 Axum 在本地模拟飞书 HTTP：

```rust
use axum::body::Bytes;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, StatusCode};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tvdisplay::feishu::client::{FeishuApi, FeishuError};

#[derive(Clone, Default)]
struct Mock {
    token_calls: Arc<Mutex<u32>>,
    replies: Arc<Mutex<Vec<Value>>>,
}

#[derive(Deserialize)]
struct ResourceQuery { r#type: String }

async fn token(State(mock): State<Mock>, body: String) -> Json<Value> {
    *mock.token_calls.lock().unwrap() += 1;
    assert!(body.contains("grant_type=client_credentials"));
    assert!(body.contains("client_id=cli_test"));
    assert!(body.contains("client_secret=app-secret"));
    Json(json!({
        "code": 0,
        "access_token": "t-test-token",
        "expires_in": 7200,
        "token_type": "Bearer"
    }))
}

async fn resource(
    State(_mock): State<Mock>,
    Path((message_id, file_key)): Path<(String, String)>,
    Query(query): Query<ResourceQuery>,
    headers: HeaderMap,
) -> Result<Bytes, StatusCode> {
    assert_eq!(query.r#type, "image");
    assert_eq!(headers.get("authorization").unwrap(), "Bearer t-test-token");
    if message_id == "om_ok" && file_key == "img_ok" {
        return Ok(Bytes::from_static(b"\xff\xd8\xff\xd9"));
    }
    Err(StatusCode::BAD_REQUEST)
}

async fn reply(State(mock): State<Mock>, Path(message_id): Path<String>, Json(body): Json<Value>) -> Json<Value> {
    assert_eq!(message_id, "om_ok");
    mock.replies.lock().unwrap().push(body);
    Json(json!({"code": 0, "msg": "ok", "data": {"message_id": "om_reply"}}))
}

async fn spawn_mock() -> (String, Mock) {
    let mock = Mock::default();
    let app = Router::new()
        .route("/oauth/v3/token", post(token))
        .route("/open-apis/im/v1/messages/{message_id}/resources/{file_key}", get(resource))
        .route("/open-apis/im/v1/messages/{message_id}/reply", post(reply))
        .with_state(mock.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    (format!("http://127.0.0.1:{}", addr.port()), mock)
}

fn api(base: &str) -> FeishuApi {
    FeishuApi::new(
        "cli_test".into(),
        "app-secret".into(),
        base.to_string(),
        base.to_string(),
    )
}

#[tokio::test]
async fn token_is_cached_and_used_for_download_and_reply() {
    let (base, mock) = spawn_mock().await;
    let api = api(&base);
    let image = api.download_message_image("om_ok", "img_ok", 1024).await.unwrap();
    assert_eq!(image, b"\xff\xd8\xff\xd9");
    api.reply_text("om_ok", "请发送图片或文字").await.unwrap();
    api.reply_card("om_ok", &json!({"schema":"2.0"})).await.unwrap();
    assert_eq!(*mock.token_calls.lock().unwrap(), 1);
    let replies = mock.replies.lock().unwrap().clone();
    assert_eq!(replies[0]["msg_type"], "text");
    let content: Value = serde_json::from_str(replies[0]["content"].as_str().unwrap()).unwrap();
    assert_eq!(content["text"], "请发送图片或文字");
    assert_eq!(replies[1]["msg_type"], "interactive");
}

#[tokio::test]
async fn download_failure_maps_to_download_error() {
    let (base, _) = spawn_mock().await;
    let api = api(&base);
    let err = api.download_message_image("om_missing", "img_x", 1024).await.unwrap_err();
    assert!(matches!(err, FeishuError::Download));
    assert!(!format!("{err}").contains("app-secret"));
}

#[tokio::test]
async fn oversize_download_is_image_too_large() {
    let (base, _) = spawn_mock().await;
    let api = api(&base);
    let err = api.download_message_image("om_ok", "img_ok", 1).await.unwrap_err();
    assert!(matches!(err, FeishuError::ImageTooLarge));
}
```

Axum 0.8 的路径参数语法是 `/messages/{message_id}`。若 mock 启动失败，检查 feature。测试不得断言 token 以外的密钥出现在请求日志中。

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_client -- --nocapture
```

Expected: FAIL，缺少 `FeishuApi`。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/feishu/client.rs` 写入 HTTP 客户端（先不要写 WebSocket 长连接）。`mod.rs` 增加 `pub mod client;`。

```rust
use reqwest::Client;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;

#[derive(Debug, Error)]
pub enum FeishuError {
    #[error("获取飞书访问凭证失败")]
    Token(String),
    #[error("无法下载图片，请重试")]
    Download,
    #[error("图片不能超过 10 MB")]
    ImageTooLarge,
    #[error("回复飞书消息失败")]
    Reply(String),
}

struct CachedToken {
    value: String,
    expires_at: Instant,
}

pub struct FeishuApi {
    app_id: String,
    app_secret: String,
    open_base: String,
    accounts_base: String,
    http: Client,
    token: Mutex<Option<CachedToken>>,
}

impl FeishuApi {
    pub fn new(app_id: String, app_secret: String, open_base: String, accounts_base: String) -> Self {
        Self {
            app_id,
            app_secret,
            open_base: open_base.trim_end_matches('/').to_string(),
            accounts_base: accounts_base.trim_end_matches('/').to_string(),
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .expect("reqwest client"),
            token: Mutex::new(None),
        }
    }

    pub async fn tenant_access_token(&self) -> Result<String, FeishuError> {
        if let Some(cached) = self.token.lock().await.as_ref() {
            if cached.expires_at > Instant::now() + Duration::from_secs(300) {
                return Ok(cached.value.clone());
            }
        }
        let url = format!("{}/oauth/v3/token", self.accounts_base);
        let response = self
            .http
            .post(url)
            .header("content-type", "application/x-www-form-urlencoded")
            .body(format!(
                "grant_type=client_credentials&client_id={}&client_secret={}",
                urlencoding(&self.app_id),
                urlencoding(&self.app_secret)
            ))
            .send()
            .await
            .map_err(|err| FeishuError::Token(err.to_string()))?;
        let body: TokenResponse = response
            .json()
            .await
            .map_err(|err| FeishuError::Token(err.to_string()))?;
        if body.code.unwrap_or(1) != 0 || body.access_token.is_none() {
            return Err(FeishuError::Token(body.error_description.unwrap_or_default()));
        }
        let value = body.access_token.unwrap();
        let expires_in = body.expires_in.unwrap_or(7200);
        *self.token.lock().await = Some(CachedToken {
            value: value.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in as u64),
        });
        Ok(value)
    }

    pub async fn download_message_image(
        &self,
        message_id: &str,
        file_key: &str,
        max_bytes: usize,
    ) -> Result<Vec<u8>, FeishuError> {
        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages/{message_id}/resources/{file_key}?type=image",
            self.open_base
        );
        let response = self
            .http
            .get(url)
            .header("Authorization", format!("Bearer {token}"))
            .send()
            .await
            .map_err(|_| FeishuError::Download)?;
        if !response.status().is_success() {
            return Err(FeishuError::Download);
        }
        let mut bytes = Vec::new();
        let mut stream = response.bytes_stream();
        use futures_util::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| FeishuError::Download)?;
            if bytes.len() + chunk.len() > max_bytes {
                return Err(FeishuError::ImageTooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub async fn reply_text(&self, message_id: &str, text: &str) -> Result<(), FeishuError> {
        let content = serde_json::to_string(&json!({ "text": text })).unwrap();
        self.reply(message_id, "text", content).await
    }

    pub async fn reply_card(&self, message_id: &str, card: &Value) -> Result<(), FeishuError> {
        let content = serde_json::to_string(card).unwrap();
        self.reply(message_id, "interactive", content).await
    }

    async fn reply(&self, message_id: &str, msg_type: &str, content: String) -> Result<(), FeishuError> {
        let token = self.tenant_access_token().await?;
        let url = format!(
            "{}/open-apis/im/v1/messages/{message_id}/reply",
            self.open_base
        );
        let response = self
            .http
            .post(url)
            .header("Authorization", format!("Bearer {token}"))
            .json(&json!({ "msg_type": msg_type, "content": content }))
            .send()
            .await
            .map_err(|err| FeishuError::Reply(err.to_string()))?;
        let body: Value = response.json().await.map_err(|err| FeishuError::Reply(err.to_string()))?;
        if body.get("code").and_then(Value::as_i64) != Some(0) {
            return Err(FeishuError::Reply("non-zero code".into()));
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    code: Option<i64>,
    access_token: Option<String>,
    expires_in: Option<i64>,
    error_description: Option<String>,
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}
```

`FeishuError::Token` 的 `Display` 已固定为「获取飞书访问凭证失败」，不要把内部 `String` 打进 `Display`，以免偶然含有密钥。测试里 `format!("{err}")` 对 Download/ImageTooLarge 已足够。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_client -- --nocapture
```

Expected: PASS。第二次下载不得再次打 token 接口。超大下载走 `ImageTooLarge`。

- [ ] **Step 5: Commit**

```bash
git add server/src/feishu/mod.rs server/src/feishu/client.rs server/tests/feishu_client.rs
git commit -m "feat: call Feishu token, image download, and message reply APIs"
```

---

### Task 13: 飞书长连接客户端与重连退避

**Files:**
- Modify: `server/src/feishu/client.rs`（追加 protobuf 帧、取 endpoint、收发循环）
- Test: `server/tests/feishu_connection.rs`

**Interfaces:**
- Consumes: `FeishuApi` 的 `app_id` / `app_secret` / `open_base` / `http`
- Produces: `pub fn reconnect_backoff_ms(attempt: u32) -> u64` — `min(1000u64 << attempt.min(20), 30_000)`，`attempt` 从 0 开始，与规格一致
- Produces: `#[derive(Debug, Clone)] pub struct WsEndpoint { pub url: String, pub ping_interval: Duration }`
- Produces: `impl FeishuApi { pub async fn fetch_ws_endpoint(&self) -> Result<WsEndpoint, FeishuError> }` — `POST {open_base}/callback/ws/endpoint`，头 `locale: zh`，JSON `{"AppID":"...","AppSecret":"..."}`。`code==0` 时读 `data.URL`；`ClientConfig.PingInterval` 若有则用作秒，否则 120 秒。不要用平台的 `ReconnectInterval` 覆盖产品退避
- Produces: protobuf 类型 `Frame` / `Header`（prost，字段编号见本计划「飞书协议要点」）以及 `encode_frame` / `decode_frame`
- Produces: `pub fn ping_frame(service_id: i32) -> Frame` — `method=0`，头 `type=ping`
- Produces: `pub fn ack_frame(mut frame: Frame, biz_rt_ms: i64, body: Option<&Value>) -> Frame` — 把 payload 写成 `{"code":200,"data":<optional base64>}`，追加头 `biz_rt`
- Produces: `pub async fn run_session<H>(endpoint: WsEndpoint, handler: H) -> Result<(), FeishuError> where H: LongConnectionHandler`
- Produces: `#[async_trait]` 或手动 future：`pub trait LongConnectionHandler: Send + Sync { async fn on_event(&self, event_type: &str, payload: Value) -> Option<Value>; }`
- 会话行为：连接 `endpoint.url`；从 URL 查询串读 `service_id`；循环读二进制帧；CONTROL/`pong` 可忽略（若 payload 是 ClientConfig，只更新 ping 间隔）；DATA 且 `type` 为 `event` 或 `card` 时，如 `sum>1` 则按 `message_id`/`seq` 拼包；解析 payload JSON 的 `header.event_type`，调用 `on_event`；事件在调用 handler 之前就可以先准备 ACK（但卡片必须带上 handler 返回的 JSON）；3 秒内写出 ACK。为避免超过 3 秒，Task 14 会把耗时的下载放到 ACK 之后；本任务的 handler 可视为已很快
- 本任务的 `run_session` 在连接断开时返回错误，不在内部重连。重连循环在 Task 14 使用 `reconnect_backoff_ms`

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_connection.rs`：

```rust
use serde_json::json;
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};
use tvdisplay::feishu::client::{
    ack_frame, decode_frame, encode_frame, ping_frame, reconnect_backoff_ms, FeishuApi, Frame,
    Header, LongConnectionHandler,
};

#[test]
fn backoff_matches_the_spec() {
    assert_eq!(reconnect_backoff_ms(0), 1000);
    assert_eq!(reconnect_backoff_ms(1), 2000);
    assert_eq!(reconnect_backoff_ms(2), 4000);
    assert_eq!(reconnect_backoff_ms(4), 16000);
    assert_eq!(reconnect_backoff_ms(5), 30000);
    assert_eq!(reconnect_backoff_ms(20), 30000);
}

#[test]
fn ping_and_ack_frames_roundtrip() {
    let encoded = encode_frame(&ping_frame(42));
    let decoded = decode_frame(&encoded).unwrap();
    assert_eq!(decoded.method, 0);
    assert_eq!(decoded.service, 42);
    assert_eq!(header(&decoded, "type").as_deref(), Some("ping"));

    let mut event = Frame {
        seq_id: 1,
        log_id: 2,
        service: 42,
        method: 1,
        headers: vec![
            Header { key: "type".into(), value: "event".into() },
            Header { key: "message_id".into(), value: "m1".into() },
            Header { key: "sum".into(), value: "1".into() },
            Header { key: "seq".into(), value: "0".into() },
            Header { key: "trace_id".into(), value: "t1".into() },
        ],
        payload_encoding: None,
        payload_type: None,
        payload: Some(br#"{"header":{"event_type":"im.message.receive_v1"}}"#.to_vec()),
        log_id_new: None,
    };
    event = ack_frame(event, 3, Some(&json!({"toast":{"content":"ok"}})));
    let payload: serde_json::Value = serde_json::from_slice(event.payload.as_ref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
    assert!(payload["data"].as_str().is_some());
    assert_eq!(header(&event, "biz_rt").as_deref(), Some("3"));
}

fn header(frame: &Frame, key: &str) -> Option<String> {
    frame.headers.iter().find(|item| item.key == key).map(|item| item.value.clone())
}

struct RecordingHandler {
    received: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl LongConnectionHandler for RecordingHandler {
    fn on_event(
        &self,
        event_type: String,
        _payload: serde_json::Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<serde_json::Value>> + Send + '_>>
    {
        let received = self.received.clone();
        Box::pin(async move {
            received.lock().unwrap().push(event_type);
            None
        })
    }
}

#[tokio::test]
async fn fetch_endpoint_reads_url_and_ping_interval() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/callback/ws/endpoint",
            axum::routing::post(|| async {
                axum::Json(json!({
                    "code": 0,
                    "msg": "ok",
                    "data": {
                        "URL": "wss://example.invalid/ws?device_id=dev&service_id=9",
                        "ClientConfig": { "PingInterval": 30, "ReconnectInterval": 999 }
                    }
                }))
            }),
        );
        axum::serve(listener, app).await.unwrap();
    });
    let api = FeishuApi::new(
        "cli_test".into(),
        "app-secret".into(),
        format!("http://{addr}"),
        format!("http://{addr}"),
    );
    let endpoint = api.fetch_ws_endpoint().await.unwrap();
    assert!(endpoint.url.contains("device_id=dev"));
    assert_eq!(endpoint.ping_interval, Duration::from_secs(30));
}

#[tokio::test]
async fn session_acks_event_frames() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel::<()>();
    let (ack_tx, ack_rx) = tokio::sync::oneshot::channel::<Vec<u8>>();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = accept_async(stream).await.unwrap();
        let mut frame = Frame {
            seq_id: 1,
            log_id: 1,
            service: 9,
            method: 1,
            headers: vec![
                Header { key: "type".into(), value: "event".into() },
                Header { key: "message_id".into(), value: "m-event".into() },
                Header { key: "sum".into(), value: "1".into() },
                Header { key: "seq".into(), value: "0".into() },
                Header { key: "trace_id".into(), value: "tr".into() },
            ],
            payload_encoding: None,
            payload_type: None,
            payload: Some(json!({"schema":"2.0","header":{"event_type":"im.message.receive_v1"},"event":{}}).to_string().into_bytes()),
            log_id_new: None,
        };
        ws.send(Message::Binary(encode_frame(&frame).into())).await.unwrap();
        let reply = loop {
            match ws.next().await {
                Some(Ok(Message::Binary(bytes))) => break bytes,
                Some(Ok(_)) => continue,
                other => panic!("expected ack, got {other:?}"),
            }
        };
        let _ = ack_tx.send(reply.to_vec());
        let _ = ready_tx.send(());
    });

    // 等待服务器就绪：上面 accept 在连接后才运行，所以先连
    let url = format!("ws://{addr}/?device_id=dev&service_id=9");
    let received = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let handler = RecordingHandler { received: received.clone() };
    let session = tokio::spawn(async move {
        tvdisplay::feishu::client::run_session(
            tvdisplay::feishu::client::WsEndpoint {
                url,
                ping_interval: Duration::from_secs(120),
            },
            handler,
        )
        .await
    });
    let ack_bytes = tokio::time::timeout(Duration::from_secs(3), ack_rx)
        .await
        .expect("ack")
        .unwrap();
    let ack = decode_frame(&ack_bytes).unwrap();
    let payload: serde_json::Value = serde_json::from_slice(ack.payload.as_ref().unwrap()).unwrap();
    assert_eq!(payload["code"], 200);
    assert!(received.lock().unwrap().contains(&"im.message.receive_v1".to_string()));
    session.abort();
    let _ = ready_rx.await;
}
```

在测试文件顶部增加 `use futures_util::{SinkExt, StreamExt};`。`LongConnectionHandler` 的具体形状以实现为准，但测试必须覆盖：退避数字、帧编解码、endpoint 解析（忽略 999 秒重连间隔）、以及收到 event 帧后写回 `code=200` 的 ACK。若 async trait 用了 `async-trait` crate，不要新增依赖，改用关联 future 或 `impl LongConnectionHandler for &H` 的手动 Box future，如上面所示。

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_connection -- --nocapture
```

Expected: FAIL，缺少 `reconnect_backoff_ms` / `run_session` 等符号。

- [ ] **Step 3: Write minimal implementation**

在 `server/src/feishu/client.rs` 追加（保留 Task 12 的 HTTP 代码）：

```rust
use prost::Message as ProstMessage;
use std::pin::Pin;
use tokio_tungstenite::connect_async;
use tokio_tungstenite::tungstenite::Message as WsMessage;

pub fn reconnect_backoff_ms(attempt: u32) -> u64 {
    let shift = attempt.min(20);
    (1000u64 << shift).min(30_000)
}

#[derive(Clone, Debug)]
pub struct WsEndpoint {
    pub url: String,
    pub ping_interval: Duration,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Header {
    #[prost(string, required, tag = "1")]
    pub key: String,
    #[prost(string, required, tag = "2")]
    pub value: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Frame {
    #[prost(uint64, required, tag = "1")]
    pub seq_id: u64,
    #[prost(uint64, required, tag = "2")]
    pub log_id: u64,
    #[prost(int32, required, tag = "3")]
    pub service: i32,
    #[prost(int32, required, tag = "4")]
    pub method: i32,
    #[prost(message, repeated, tag = "5")]
    pub headers: Vec<Header>,
    #[prost(string, optional, tag = "6")]
    pub payload_encoding: Option<String>,
    #[prost(string, optional, tag = "7")]
    pub payload_type: Option<String>,
    #[prost(bytes = "vec", optional, tag = "8")]
    pub payload: Option<Vec<u8>>,
    #[prost(string, optional, tag = "9")]
    pub log_id_new: Option<String>,
}

pub fn encode_frame(frame: &Frame) -> Vec<u8> {
    frame.encode_to_vec()
}

pub fn decode_frame(bytes: &[u8]) -> Result<Frame, FeishuError> {
    Frame::decode(bytes).map_err(|_| FeishuError::Download)
}

// 为编译错误选一个更合适的变体：给 FeishuError 增加 Connection(&'static str)，Display 为「飞书连接已断开」且不含密钥。

pub fn ping_frame(service_id: i32) -> Frame {
    Frame {
        seq_id: 0,
        log_id: 0,
        service: service_id,
        method: 0,
        headers: vec![Header { key: "type".into(), value: "ping".into() }],
        payload_encoding: None,
        payload_type: None,
        payload: None,
        log_id_new: None,
    }
}

pub fn ack_frame(mut frame: Frame, biz_rt_ms: i64, body: Option<&Value>) -> Frame {
    frame.headers.push(Header {
        key: "biz_rt".into(),
        value: biz_rt_ms.to_string(),
    });
    let mut payload = json!({ "code": 200 });
    if let Some(body) = body {
        let raw = serde_json::to_vec(body).unwrap();
        payload["data"] = Value::String(base64::engine::general_purpose::STANDARD.encode(raw));
    }
    frame.payload = Some(serde_json::to_vec(&payload).unwrap());
    frame
}

pub trait LongConnectionHandler: Send + Sync + 'static {
    fn on_event(
        &self,
        event_type: String,
        payload: Value,
    ) -> Pin<Box<dyn std::future::Future<Output = Option<Value>> + Send + '_>>;
}

impl FeishuApi {
    pub async fn fetch_ws_endpoint(&self) -> Result<WsEndpoint, FeishuError> {
        let url = format!("{}/callback/ws/endpoint", self.open_base);
        let response = self
            .http
            .post(url)
            .header("locale", "zh")
            .json(&json!({ "AppID": self.app_id, "AppSecret": self.app_secret }))
            .send()
            .await
            .map_err(|_| FeishuError::Connection("endpoint"))?;
        let body: Value = response.json().await.map_err(|_| FeishuError::Connection("endpoint"))?;
        if body.get("code").and_then(Value::as_i64) != Some(0) {
            return Err(FeishuError::Connection("endpoint"));
        }
        let ws_url = body["data"]["URL"]
            .as_str()
            .ok_or(FeishuError::Connection("endpoint"))?
            .to_string();
        let ping_interval = body["data"]["ClientConfig"]["PingInterval"]
            .as_u64()
            .filter(|value| *value > 0)
            .unwrap_or(120);
        Ok(WsEndpoint {
            url: ws_url,
            ping_interval: Duration::from_secs(ping_interval),
        })
    }
}

pub async fn run_session<H: LongConnectionHandler>(
    endpoint: WsEndpoint,
    handler: H,
) -> Result<(), FeishuError> {
    let parsed = url::Url::parse(&endpoint.url).map_err(|_| FeishuError::Connection("url"))?;
    let service_id = parsed
        .query_pairs()
        .find(|(key, _)| key == "service_id")
        .map(|(_, value)| value.parse().unwrap_or(0))
        .unwrap_or(0);
    let (ws, _) = connect_async(endpoint.url.as_str())
        .await
        .map_err(|_| FeishuError::Connection("dial"))?;
    let (mut sink, mut stream) = ws.split();
    let mut ping = tokio::time::interval(endpoint.ping_interval);
    loop {
        tokio::select! {
            _ = ping.tick() => {
                let _ = sink.send(WsMessage::Binary(encode_frame(&ping_frame(service_id)).into())).await;
            }
            incoming = stream.next() => {
                let Some(incoming) = incoming else { return Err(FeishuError::Connection("closed")); };
                let Ok(WsMessage::Binary(bytes)) = incoming else { continue; };
                let mut frame = decode_frame(&bytes).map_err(|_| FeishuError::Connection("frame"))?;
                if frame.method == 0 {
                    continue;
                }
                if frame_header(&frame, "sum").and_then(|value| value.parse::<i32>().ok()).unwrap_or(1) > 1 {
                    // 本任务的测试只用 sum=1；拼包在同一函数内用 HashMap<String, Vec<Option<Vec<u8>>>> 实现，
                    // 未齐包则 continue。
                    continue;
                }
                let typ = frame_header(&frame, "type").unwrap_or_default();
                if typ != "event" && typ != "card" {
                    continue;
                }
                let payload: Value = frame
                    .payload
                    .as_ref()
                    .and_then(|bytes| serde_json::from_slice(bytes).ok())
                    .unwrap_or(Value::Null);
                let event_type = payload
                    .pointer("/header/event_type")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let started = Instant::now();
                let response = handler.on_event(event_type, payload).await;
                let biz_rt = started.elapsed().as_millis() as i64;
                frame = ack_frame(frame, biz_rt, response.as_ref());
                sink.send(WsMessage::Binary(encode_frame(&frame).into()))
                    .await
                    .map_err(|_| FeishuError::Connection("ack"))?;
            }
        }
    }
}

fn frame_header(frame: &Frame, key: &str) -> Option<String> {
    frame.headers.iter().find(|item| item.key == key).map(|item| item.value.clone())
}
```

给 `FeishuError` 增加：

```rust
#[error("飞书连接已断开")]
Connection(&'static str),
```

`Display` 固定为「飞书连接已断开」，不要输出 URL 里可能带的密钥。拼包逻辑：用 `message_id` 做键，`sum` 为槽位数，`seq` 为下标，5 秒后丢弃未齐的包（与官方 SDK 一致）。在 `run_session` 开头创建 `let mut chunks: HashMap<String, Vec<Option<Vec<u8>>>> = HashMap::new();`，收到 DATA 帧后用下面这段拼包，齐了再解析 JSON：

```rust
let sum = frame_header(&frame, "sum").and_then(|value| value.parse::<usize>().ok()).unwrap_or(1);
let seq = frame_header(&frame, "seq").and_then(|value| value.parse::<usize>().ok()).unwrap_or(0);
let mut payload_bytes = frame.payload.clone().unwrap_or_default();
if sum > 1 {
    let id = frame_header(&frame, "message_id").unwrap_or_default();
    let complete = chunks.entry(id).or_insert_with(|| vec![None; sum]);
    if seq < complete.len() {
        complete[seq] = Some(payload_bytes);
    }
    if complete.iter().any(|part| part.is_none()) {
        continue;
    }
    payload_bytes = complete.iter().flatten().flatten().copied().collect();
    chunks.remove(&id);
}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_connection -- --nocapture
```

Expected: PASS。`ReconnectInterval: 999` 不得变成 ping 或退避来源。

- [ ] **Step 5: Commit**

```bash
git add server/src/feishu/client.rs server/tests/feishu_connection.rs
git commit -m "feat: speak the official Feishu long-connection websocket protocol"
```

---

### Task 14: 把飞书长连接接到内容存储和电视推送

**Files:**
- Modify: `server/src/feishu/mod.rs`（增加事件分类、处理循环、`run_feishu_bot`）
- Modify: `server/src/http.rs` 的 `AppState`：增加 `pub drafts: Arc<tokio::sync::Mutex<DraftStore>>` 与 `pub feishu_api: Option<Arc<FeishuApi>>`
- Modify: `server/src/main.rs`：加载配置、安装日志脱敏、按需启动飞书任务、监听 HTTP
- Modify: `server/tests/common/mod.rs` 的 `build_state` 用法（补上新字段的默认值）
- Test: `server/tests/feishu_bot.rs`

**Interfaces:**
- Consumes: `FeishuMode`、`FeishuApi`、`DraftStore`、`confirmation_card`、`result_card`、`callback_response`、`parse_button_value`、`is_allowed_group`、`ContentStore::publish`、`TvHub::push`、`normalize_text`、`validate_image`、`reconnect_backoff_ms`、`run_session`、`install_redaction`
- Produces: `pub fn classify_receive_event(payload: &Value) -> ClassifyResult`
  - `ClassifyResult::Ignore`：`sender_type != "user"`、系统消息、非群、或不在白名单。不回复，也不把正文写入日志
  - `ClassifyResult::Unsupported { message_id }`：用户发来的 `media`/`file`/`audio`/`sticker` 以外的明显不支持类型中，`media` 与 `file`（以及非图片资源）回复「请发送图片或文字」；`sticker` 与系统消息走 `Ignore`（规格：不处理表情、系统消息）
  - `ClassifyResult::Valid { message_id, chat_id, text, image_key: Option<String> }`：`message_type=text` 解析 `content` 字符串里的 `text`；`image` 解析 `image_key`；`post` 抽取所有 `text` 标签拼成一段，并取第一张 `img`/`img_key`/`image_key`。文字走 `normalize_text`（调用方做）。图和字都没有则当作 `Unsupported`
- Produces: `pub async fn handle_receive(...)` — 若需下载图片则调用 `download_message_image`；校验失败用 `reply_text` 回复中文原因（「请发送图片或文字」「只支持 JPEG 或 PNG」「图片不能超过 10 MB」「无法下载图片，请重试」）；成功则 `drafts.insert`，`reply_card` 发送 `confirmation_card`。为满足 3 秒 ACK，长连接 handler 在 `im.message.receive_v1` 上立即返回 `None`（先 ACK），再 `tokio::spawn` 执行 `handle_receive`。用 `message_id` 做短期去重
- Produces: `pub async fn handle_card_action(...)` — 读 `event.action.value`；`take_for_publish`；`AlreadySent` 返回 toast「这条已经发送过了」且不增加 revision；`ExpiredOrMissing` 返回 toast「这条内容已过期，请重新发送」；成功则 `store.publish`、`revision += 1`、按按钮设置 `force_popup`，电视在线则 push，toast/卡片为「发送成功」或「已保存，但电视当前不在线」。此路径必须在 ACK 前完成（本地发布，应远小于 3 秒），返回 `callback_response(...)`
- Produces: `pub async fn run_feishu_bot(state: AppState, settings: FeishuSettings)` — 日志在连上时写「飞书已连接」，断开写「飞书连接已断开」，然后 `sleep(reconnect_backoff_ms(attempt))` 再连；`attempt` 成功一次后归零。网页和电视不受影响
- Produces: `main`：无参数时 `load_settings_from_env()`，失败则 `eprintln` 错误并 `exit(1)`；收集 `admin_password` 与（若有）`app_secret` 调用 `install_redaction`；`FeishuMode::Disabled` 时 info「飞书未配置」；`Enabled` 时 `tokio::spawn(run_feishu_bot(...))`；绑定 `bind_addr(&settings.host, settings.port)`（IPv6 主机加方括号）并 `axum::serve`
- `tvdisplay health` 在本任务先保留退出码 2，Task 15 再实现探测

消息类型处理细则：

- `text`：`content` 为 `{"text":"..."}` 字符串
- `image`：`content` 为 `{"image_key":"img_..."}`。官方图片消息没有文档化的 caption 字段，不要臆造。图文都有主要来自 `post` 富文本
- `post`：`content` 为富文本 JSON，遍历 `zh_cn.content` 或首个语言下的二维数组，收集 `tag=text` 的 `text` 与 `tag` 为 `img` / `img` 组件的 `image_key`
- 用户发 `media`（视频）或 `file`：回复「请发送图片或文字」
- 机器人自己的消息（`sender_type=bot`）忽略，避免循环

- [ ] **Step 1: Write the failing test**

创建 `server/tests/feishu_bot.rs`。本任务用纯函数和内存中的 `ContentStore` / `DraftStore` / `TvHub` 覆盖分类与卡片确认，不连真实飞书、不在测试里启动长连接：

1. 单元测试覆盖 `classify_receive_event`（白名单群文字、白名单群图片、post 图文、非白名单群、私聊、bot、视频、表情）
2. 单元测试覆盖 `handle_card_action`：用真实 `ContentStore` + `DraftStore` + `TvHub`，不发 HTTP，直接调用函数

```rust
use serde_json::json;
use std::time::Instant;
use tempfile::tempdir;
use tvdisplay::content::{ContentStore, MAX_IMAGE_BYTES};
use tvdisplay::feishu::drafts::{Draft, DraftStore};
use tvdisplay::feishu::{
    classify_receive_event, handle_card_action, ClassifyResult,
};
use tvdisplay::hub::TvHub;

fn receive_event(chat_id: &str, chat_type: &str, message_type: &str, content: &str, sender: &str) -> serde_json::Value {
    json!({
        "schema": "2.0",
        "header": { "event_type": "im.message.receive_v1" },
        "event": {
            "sender": { "sender_type": sender },
            "message": {
                "message_id": "om_1",
                "chat_id": chat_id,
                "chat_type": chat_type,
                "message_type": message_type,
                "content": content
            }
        }
    })
}

#[test]
fn classifies_allowed_group_text_and_image_and_post() {
    let allowed = vec!["oc_allow".to_string()];
    match classify_receive_event(&receive_event("oc_allow", "group", "text", r#"{"text":"  你好 "}"#, "user"), &allowed) {
        ClassifyResult::Valid { text, image_key, .. } => {
            assert_eq!(text, "  你好 ");
            assert!(image_key.is_none());
        }
        other => panic!("{other:?}"),
    }
    match classify_receive_event(&receive_event("oc_allow", "group", "image", r#"{"image_key":"img_1"}"#, "user"), &allowed) {
        ClassifyResult::Valid { image_key, text, .. } => {
            assert_eq!(image_key.as_deref(), Some("img_1"));
            assert!(text.is_empty());
        }
        other => panic!("{other:?}"),
    }
    let post = r#"{"zh_cn":{"title":"","content":[[{"tag":"text","text":"菜单"}],[{"tag":"img","image_key":"img_2"}]]}}"#;
    match classify_receive_event(&receive_event("oc_allow", "group", "post", post, "user"), &allowed) {
        ClassifyResult::Valid { text, image_key, .. } => {
            assert!(text.contains("菜单"));
            assert_eq!(image_key.as_deref(), Some("img_2"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ignores_private_chats_other_groups_bots_and_stickers() {
    let allowed = vec!["oc_allow".to_string()];
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "p2p", "text", r#"{"text":"hi"}"#, "user"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_other", "group", "text", r#"{"text":"hi"}"#, "user"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "text", r#"{"text":"hi"}"#, "bot"), &allowed), ClassifyResult::Ignore));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "sticker", r#"{}"#, "user"), &allowed), ClassifyResult::Ignore));
}

#[test]
fn videos_and_files_are_unsupported() {
    let allowed = vec!["oc_allow".to_string()];
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "media", r#"{}"#, "user"), &allowed), ClassifyResult::Unsupported { .. }));
    assert!(matches!(classify_receive_event(&receive_event("oc_allow", "group", "file", r#"{}"#, "user"), &allowed), ClassifyResult::Unsupported { .. }));
}

#[tokio::test]
async fn card_click_publishes_once_and_then_reports_already_sent() {
    let dir = tempdir().unwrap();
    let store = std::sync::Arc::new(ContentStore::new(dir.path().to_path_buf()));
    let hub = TvHub::new();
    let mut drafts = DraftStore::new();
    let now = Instant::now();
    drafts.insert(
        Draft { id: "d1".into(), text: "今晚菜单".into(), image: Some(b"\xff\xd8\xff\xd9".to_vec()), created_at: now },
        now,
    );
    let payload = json!({
        "header": { "event_type": "card.action.trigger" },
        "event": { "action": { "value": { "draft_id": "d1", "force_popup": true }, "tag": "button" } }
    });
    let first = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(first["toast"]["content"], "已保存，但电视当前不在线");
    let content = store.load();
    assert_eq!(content.revision, 1);
    assert_eq!(content.text, "今晚菜单");
    assert!(content.force_popup);
    let second = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(second["toast"]["content"], "这条已经发送过了");
    assert_eq!(store.load().revision, 1);
}

#[tokio::test]
async fn expired_card_does_not_publish() {
    let dir = tempdir().unwrap();
    let store = std::sync::Arc::new(ContentStore::new(dir.path().to_path_buf()));
    let hub = TvHub::new();
    let drafts = DraftStore::new();
    let now = Instant::now();
    let payload = json!({
        "header": { "event_type": "card.action.trigger" },
        "event": { "action": { "value": { "draft_id": "missing", "force_popup": false }, "tag": "button" } }
    });
    let mut drafts = drafts;
    let response = handle_card_action(&payload, &mut drafts, now, store.as_ref(), &hub, MAX_IMAGE_BYTES).await;
    assert_eq!(response["toast"]["content"], "这条内容已过期，请重新发送");
    assert_eq!(store.load().revision, 0);
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test feishu_bot -- --nocapture
```

Expected: FAIL，缺少 `classify_receive_event` / `handle_card_action`。

- [ ] **Step 3: Write implementation**

在 `server/src/feishu/mod.rs` 实现 `ClassifyResult`、`classify_receive_event`、`handle_card_action`、`handle_receive`、`run_feishu_bot`。`handle_card_action` 签名必须与测试一致：

```rust
pub async fn handle_card_action(
    payload: &Value,
    drafts: &mut DraftStore,
    now: Instant,
    store: &ContentStore,
    hub: &TvHub,
    max_bytes: usize,
) -> Value
```

完整实现（写入 `server/src/feishu/mod.rs`，不要把用户正文或群消息写进日志）：

```rust
use crate::content::{normalize_text, validate_image, ContentStore, ContentValidationError};
use crate::http::AppState;
use crate::hub::TvHub;
use crate::config::FeishuSettings;
use allowlist::is_allowed_group;
use cards::{callback_response, confirmation_card, parse_button_value, result_card};
use client::{FeishuApi, FeishuError, LongConnectionHandler};
use drafts::{Draft, DraftError, DraftStore};
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClassifyResult {
    Ignore,
    Unsupported { message_id: String },
    Valid {
        message_id: String,
        chat_id: String,
        text: String,
        image_key: Option<String>,
    },
}

pub fn classify_receive_event(payload: &Value, allowed: &[String]) -> ClassifyResult {
    let event = &payload["event"];
    let sender_type = event["sender"]["sender_type"].as_str().unwrap_or("");
    if sender_type != "user" {
        return ClassifyResult::Ignore;
    }
    let message = &event["message"];
    let message_id = message["message_id"].as_str().unwrap_or("").to_string();
    let chat_id = message["chat_id"].as_str().unwrap_or("");
    let chat_type = message["chat_type"].as_str().unwrap_or("");
    if !is_allowed_group(chat_id, chat_type, allowed) {
        return ClassifyResult::Ignore;
    }
    let message_type = message["message_type"].as_str().unwrap_or("");
    if message_type == "sticker" || message_type == "system" {
        return ClassifyResult::Ignore;
    }
    if message_type == "media" || message_type == "file" || message_type == "audio" {
        return ClassifyResult::Unsupported { message_id };
    }
    let content_raw = message["content"].as_str().unwrap_or("{}");
    let content: Value = serde_json::from_str(content_raw).unwrap_or(Value::Null);
    let (text, image_key) = match message_type {
        "text" => (content["text"].as_str().unwrap_or("").to_string(), None),
        "image" => (String::new(), content["image_key"].as_str().map(ToOwned::to_owned)),
        "post" => extract_post(&content),
        _ => return ClassifyResult::Unsupported { message_id },
    };
    if text.trim().is_empty() && image_key.is_none() {
        return ClassifyResult::Unsupported { message_id };
    }
    ClassifyResult::Valid {
        message_id,
        chat_id: chat_id.to_string(),
        text,
        image_key,
    }
}

fn extract_post(content: &Value) -> (String, Option<String>) {
    let body = content.get("zh_cn").or_else(|| content.as_object().and_then(|map| map.values().next()));
    let mut texts = Vec::new();
    let mut image_key = None;
    if let Some(rows) = body.and_then(|value| value.get("content")).and_then(Value::as_array) {
        for row in rows {
            let Some(items) = row.as_array() else { continue };
            for item in items {
                if item["tag"] == "text" {
                    if let Some(text) = item["text"].as_str() {
                        texts.push(text.to_string());
                    }
                }
                if image_key.is_none() {
                    if let Some(key) = item["image_key"].as_str().or_else(|| item["img_key"].as_str()) {
                        image_key = Some(key.to_string());
                    }
                }
            }
        }
    }
    (texts.join(""), image_key)
}

pub async fn handle_card_action(
    payload: &Value,
    drafts: &mut DraftStore,
    now: Instant,
    store: &ContentStore,
    hub: &TvHub,
    max_bytes: usize,
) -> Value {
    let Some(action) = payload.pointer("/event/action/value").and_then(parse_button_value) else {
        return callback_response("error", "请发送图片或文字", None);
    };
    match drafts.take_for_publish(&action.draft_id, now) {
        Err(DraftError::AlreadySent) => callback_response("info", "这条已经发送过了", None),
        Err(DraftError::ExpiredOrMissing) => callback_response("warning", "这条内容已过期，请重新发送", None),
        Ok(draft) => {
            let text = match normalize_text(Some(&draft.text), 4000) {
                Ok(text) => text,
                Err(err) => return callback_response("error", &err.message, None),
            };
            if let Some(image) = draft.image.as_deref() {
                if let Err(err) = validate_image(image, max_bytes) {
                    return callback_response("error", &err.message, None);
                }
            }
            let has_image = draft.image.is_some();
            if let Err(err) = store.publish(draft.image.as_deref(), &text, action.force_popup, max_bytes) {
                return callback_response("error", &err.message, None);
            }
            let payload = serde_json::to_string(&store.message()).expect("ws message");
            let online = hub.push(&payload).await;
            let status = if online {
                "发送成功"
            } else {
                "已保存，但电视当前不在线"
            };
            callback_response("success", status, Some(result_card(&text, has_image, status)))
        }
    }
}

pub async fn handle_receive(
    payload: Value,
    allowed: &[String],
    api: &FeishuApi,
    drafts: &Mutex<DraftStore>,
    max_bytes: usize,
    seen: &Mutex<HashSet<String>>,
) {
    match classify_receive_event(&payload, allowed) {
        ClassifyResult::Ignore => {}
        ClassifyResult::Unsupported { message_id } => {
            let _ = api.reply_text(&message_id, "请发送图片或文字").await;
        }
        ClassifyResult::Valid { message_id, text, image_key, .. } => {
            {
                let mut seen = seen.lock().await;
                if !seen.insert(message_id.clone()) {
                    return;
                }
            }
            let normalized = match normalize_text(Some(&text), 4000) {
                Ok(value) => value,
                Err(err) => {
                    let _ = api.reply_text(&message_id, &err.message).await;
                    return;
                }
            };
            let image = match image_key {
                Some(key) => match api.download_message_image(&message_id, &key, max_bytes).await {
                    Ok(bytes) => match validate_image(&bytes, max_bytes) {
                        Ok(_) => Some(bytes),
                        Err(err) => {
                            let _ = api.reply_text(&message_id, &err.message).await;
                            return;
                        }
                    },
                    Err(FeishuError::ImageTooLarge) => {
                        let _ = api.reply_text(&message_id, "图片不能超过 10 MB").await;
                        return;
                    }
                    Err(_) => {
                        let _ = api.reply_text(&message_id, "无法下载图片，请重试").await;
                        return;
                    }
                },
                None => None,
            };
            if image.is_none() && normalized.is_empty() {
                let _ = api.reply_text(&message_id, "请发送图片或文字").await;
                return;
            }
            let draft = Draft {
                id: Uuid::new_v4().to_string(),
                text: normalized,
                image,
                created_at: Instant::now(),
            };
            let card = confirmation_card(&draft.id, &draft.text, draft.image.is_some());
            {
                let mut store = drafts.lock().await;
                let _ = store.insert(draft, Instant::now());
            }
            let _ = api.reply_card(&message_id, &card).await;
        }
    }
}

struct BotHandler {
    state: AppState,
    allowed: Vec<String>,
    seen: Arc<Mutex<HashSet<String>>>,
}

impl LongConnectionHandler for BotHandler {
    fn on_event(
        &self,
        event_type: String,
        payload: Value,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Value>> + Send + '_>> {
        Box::pin(async move {
            match event_type.as_str() {
                "im.message.receive_v1" => {
                    let state = self.state.clone();
                    let allowed = self.allowed.clone();
                    let seen = self.seen.clone();
                    tokio::spawn(async move {
                        let Some(api) = state.feishu_api.as_ref() else { return };
                        handle_receive(
                            payload,
                            &allowed,
                            api,
                            &state.drafts,
                            state.settings.max_image_bytes,
                            &seen,
                        )
                        .await;
                    });
                    None
                }
                "card.action.trigger" => {
                    let mut drafts = self.state.drafts.lock().await;
                    Some(
                        handle_card_action(
                            &payload,
                            &mut drafts,
                            Instant::now(),
                            self.state.store.as_ref(),
                            &self.state.hub,
                            self.state.settings.max_image_bytes,
                        )
                        .await,
                    )
                }
                _ => None,
            }
        })
    }
}
```

`run_feishu_bot` 创建 `BotHandler` 时带上 `seen: Arc::new(Mutex::new(HashSet::new()))`。

更新 `AppState`：

```rust
pub drafts: Arc<tokio::sync::Mutex<DraftStore>>,
pub feishu_api: Option<Arc<FeishuApi>>,
```

`build_state` 里 `drafts: Arc::new(Mutex::new(DraftStore::new()))`，`feishu_api: None`。`main` 在 Enabled 时创建 `FeishuApi::new(id, secret, "https://open.feishu.cn".into(), "https://accounts.feishu.cn".into())` 放进 state，再 spawn `run_feishu_bot`。

`run_feishu_bot` 完整实现：

```rust
pub async fn run_feishu_bot(state: AppState, feishu: FeishuSettings) {
    let api = state.feishu_api.clone().expect("feishu api");
    let mut attempt: u32 = 0;
    loop {
        match api.fetch_ws_endpoint().await {
            Ok(endpoint) => {
                tracing::info!("飞书已连接");
                attempt = 0;
                let handler = BotHandler { state: state.clone(), allowed: feishu.allowed_chat_ids.clone(), seen: std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashSet::new())) };
                if let Err(_) = crate::feishu::client::run_session(endpoint, handler).await {
                    tracing::info!("飞书连接已断开");
                }
            }
            Err(_) => {
                tracing::info!("飞书连接已断开");
            }
        }
        let delay = crate::feishu::client::reconnect_backoff_ms(attempt);
        attempt = attempt.saturating_add(1);
        tokio::time::sleep(Duration::from_millis(delay)).await;
    }
}
```

`BotHandler` 实现 `LongConnectionHandler`：`im.message.receive_v1` 克隆 state 后 `tokio::spawn(handle_receive(...))` 并返回 `None`；`card.action.trigger` 锁住 `drafts` 调用 `handle_card_action` 并返回 `Some(response)`。其它 event_type 返回 `None`。

`main.rs`：

```rust
#[tokio::main]
async fn main() {
    if std::env::args().nth(1).as_deref() == Some("health") {
        eprintln!("health subcommand is not implemented yet");
        std::process::exit(2);
    }
    match tvdisplay::config::load_settings_from_env() {
        Ok(settings) => {
            if let Err(err) = tvdisplay::run_server(settings).await {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
        Err(err) => {
            eprintln!("{}", err.message);
            std::process::exit(1);
        }
    }
}
```

把 `run_server` 放在 `lib.rs`：

```rust
pub async fn run_server(settings: crate::config::Settings) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut secrets = vec![settings.admin_password.clone()];
    if let crate::config::FeishuMode::Enabled(feishu) = &settings.feishu {
        secrets.push(feishu.app_secret.clone());
    }
    crate::logging::install_redaction(&secrets);

    let mut state = crate::http::build_state(settings.clone());
    match &settings.feishu {
        crate::config::FeishuMode::Disabled => {
            tracing::info!("飞书未配置");
        }
        crate::config::FeishuMode::Enabled(feishu) => {
            let api = std::sync::Arc::new(crate::feishu::client::FeishuApi::new(
                feishu.app_id.clone(),
                feishu.app_secret.clone(),
                "https://open.feishu.cn".into(),
                "https://accounts.feishu.cn".into(),
            ));
            state.feishu_api = Some(api);
            let bot_state = state.clone();
            let feishu = feishu.clone();
            tokio::spawn(async move {
                crate::feishu::run_feishu_bot(bot_state, feishu).await;
            });
        }
    }

    let bind = bind_addr(&settings.host, settings.port);
    tracing::info!("listening on {} port {}", settings.host, settings.port);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    axum::serve(listener, crate::http::router(state)).await?;
    Ok(())
}

pub fn bind_addr(host: &str, port: u16) -> String {
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}
```

修复 `common/mod.rs` 里若 `build_state` 字段变化导致的编译错误。给 `AppState` 新增字段即可，测试不用改调用。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test feishu_bot --test admin --test websocket -- --nocapture
```

Expected: 全部通过。卡片第一次点击使 `revision` 变为 1；第二次保持 1 并提示已发送。

- [ ] **Step 5: Commit**

```bash
git add server/src/lib.rs server/src/main.rs server/src/http.rs server/src/feishu/mod.rs server/tests/feishu_bot.rs server/tests/common/mod.rs
git commit -m "feat: publish Feishu drafts to the TV after a card confirmation"
```

---

### Task 15: Docker、health 子命令与 Compose

**Files:**
- Modify: `server/src/health.rs`、`server/src/main.rs`
- Modify: `Dockerfile`、`docker-compose.yml`、`docker-compose.https.yml`（若命令或 HEALTHCHECK 有变）、`.dockerignore`、`.gitignore`
- Test: 扩展 `server/tests/config.rs` 或新建 `server/tests/health.rs`；`server/tests/deploy.rs` 的文件断言先写一部分，文档正文在 Task 16 补全

**Interfaces:**
- Consumes: `health_urls`
- Produces: `pub async fn probe_health(host: &str, port: &str) -> i32` — 按 `health_urls` 依次 GET，3 秒超时，任一 200 返回 0，否则 1。不读取、不校验 `ADMIN_PASSWORD`
- Produces: `tvdisplay health` 调用 `probe_health`，用环境变量 `HOST`（默认 `::`）和 `PORT`（默认 `8080`）
- Dockerfile HEALTHCHECK：`HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD ["tvdisplay", "health"]`
- 镜像不再基于 `python:3.12-slim`
- 默认构建策略：多阶段 Dockerfile 可在目标架构上编译；**1 GB 树莓派上不要本机 `docker compose build`**（Rust 编译容易把内存打满）。推荐在内存更大的机器上交叉构建 `linux/arm64` 镜像，再拷到树莓派只跑 `docker compose up -d`。Task 16 的部署文档必须写出完整命令

- [ ] **Step 1: Write the failing test**

在 `server/tests/config.rs` 追加（需要把 health 测试改为 async 时可新建 `server/tests/health.rs`）：

```rust
#[tokio::test]
async fn probe_health_returns_zero_on_ok() {
    use tokio::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let app = axum::Router::new().route("/health", axum::routing::get(|| async { axum::Json(serde_json::json!({"ok": true})) }));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap(); });
    let code = tvdisplay::health::probe_health("127.0.0.1", &port.to_string()).await;
    assert_eq!(code, 0);
}

#[tokio::test]
async fn probe_health_returns_one_when_nothing_listens() {
    let code = tvdisplay::health::probe_health("127.0.0.1", "1").await;
    assert_eq!(code, 1);
}
```

再写 `server/tests/deploy.rs` 的第一批断言（文档里的飞书段落在 Task 16 补上后会再扩）：

```rust
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..")
}

#[test]
fn compose_binds_host_network_without_a_tunnel() {
    let compose = std::fs::read_to_string(repo_root().join("docker-compose.yml")).unwrap();
    let lowered = compose.to_lowercase();
    assert!(compose.contains("network_mode: host"));
    assert!(!lowered.contains("cloudflare"));
    assert!(!lowered.contains("ngrok"));
    assert!(!lowered.contains("tailscale"));
    let dockerfile = std::fs::read_to_string(repo_root().join("Dockerfile")).unwrap();
    assert!(dockerfile.contains("8080"));
    assert!(dockerfile.contains("tvdisplay health") || dockerfile.contains("\"tvdisplay\", \"health\""));
    assert!(!dockerfile.to_lowercase().contains("python:3.12"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test deploy -- --nocapture
```

Expected: Dockerfile 仍是 Python 基础镜像，断言失败。`probe_health` 也可能尚不存在。

- [ ] **Step 3: Implement health probe and Docker files**

`server/src/health.rs` 追加：

```rust
use std::time::Duration;

pub async fn probe_health(host: &str, port: &str) -> i32 {
    let client = match reqwest::Client::builder().timeout(Duration::from_secs(3)).build() {
        Ok(client) => client,
        Err(_) => return 1,
    };
    for url in health_urls(host, port) {
        if let Ok(response) = client.get(&url).send().await {
            if response.status() == reqwest::StatusCode::OK {
                return 0;
            }
        }
    }
    1
}
```

`main.rs` 的 health 分支改为：

```rust
if std::env::args().nth(1).as_deref() == Some("health") {
    let host = std::env::var("HOST").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "::".into());
    let port = std::env::var("PORT").ok().filter(|value| !value.is_empty()).unwrap_or_else(|| "8080".into());
    let code = tvdisplay::health::probe_health(&host, &port).await;
    std::process::exit(code);
}
```

把 `main` 整段放进已有的 `#[tokio::main] async fn main()`。

`Dockerfile`（多阶段，运行镜像尽量小）：

```dockerfile
FROM rust:1-bookworm AS builder
WORKDIR /src
COPY server/Cargo.toml server/Cargo.lock ./
COPY server/src ./src
COPY server/templates ./templates
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /src/target/release/tvdisplay /usr/local/bin/tvdisplay
ENV HOST=::
ENV PORT=8080
ENV DATA_DIR=/data
VOLUME ["/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --retries=3 CMD ["tvdisplay", "health"]
CMD ["tvdisplay"]
```

构建上下文仍是仓库根目录（与现有 Compose `build: .` 一致）。如果 `cargo build --locked` 因路径（Cargo.toml 在 `/src` 而包期望 `src/`）失败，保持 `WORKDIR /src` 且 `COPY server/src ./src`，不要把工作目录改成 `/src/server`。

`.dockerignore` 改为：

```
.git
.env
data
android
dist
docs
**/*.md
server/target
server/.venv
**/__pycache__
**/.pytest_cache
```

`.gitignore` 增加：

```
server/target/
```

暂时保留 Python 的忽略项，等 Task 18 再清理。

`docker-compose.yml` 保持 `network_mode: host`、`env_file: .env`、`DATA_DIR: /data`、`ADMIN_PASSWORD`、卷 `./data:/data`。不必改 `command`，镜像 `CMD` 已是 `tvdisplay`。为方便使用预构建镜像，保留 `build: .`，并增加注释（不要把真实密钥写进去）：

```yaml
# 若在 1GB 树莓派上编译 OOM，请改用在其它机器构建好的镜像名称，例如：
# image: tvdisplay:arm64
# 并注释掉 build: .
```

不要默认注释掉 `build`，以免本地开发失效。`docker-compose.https.yml` 仍只改 `HOST=127.0.0.1` 并启动 Caddy，无需改 command。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test deploy --test config -- --nocapture
```

Expected: `probe_health` 两个用例与 Compose/Dockerfile 断言通过。

在本机（非 1GB 设备）可再手动确认：

```bash
docker build -t tvdisplay:local .
```

这一步不是测试强制项；若当前环境没有 Docker，跳过即可。

- [ ] **Step 5: Commit**

```bash
git add Dockerfile docker-compose.yml docker-compose.https.yml .dockerignore .gitignore server/src/health.rs server/src/main.rs server/tests/config.rs server/tests/deploy.rs
git commit -m "feat: run the Rust server in a small Docker image with a health command"
```

---

### Task 16: 部署文档、README 与 .env 示例

**Files:**
- Modify: `.env.example`、`docs/deploy-raspberry-pi.md`、`README.md`
- Modify: `server/tests/deploy.rs`（补上文档与 README 断言）

**Interfaces:**
- `.env.example` 在现有 `ADMIN_PASSWORD`/`HOST`/`PORT`/`DATA_DIR`/`DOMAIN` 后追加占位符，不得填写真实密钥：

```text
# 可选。三项都填齐后才会启用飞书机器人。不要把真实密钥提交到 git。
# FEISHU_APP_ID=cli_xxxxxxxx
# FEISHU_APP_SECRET=替换成应用密钥
# FEISHU_ALLOWED_CHAT_IDS=oc_xxxxxxxx,oc_yyyyyyyy
```

- README 与部署文档全部用简体中文。本地运行改为 `cargo run`，测试改为 `cargo test`，不再把 `pytest` 当作后端测试命令
- 部署文档必须包含：1GB 树莓派上如何构建镜像、如何创建企业自建应用、开通长连接和群消息权限、如何取得群 ID、如何写入 `.env`、现有 `content.json` 可继续用、电视应用需重新侧载

- [ ] **Step 1: Write the failing documentation assertions**

把下面断言并入 `server/tests/deploy.rs`：

```rust
#[test]
fn env_example_has_feishu_placeholders_only() {
    let text = std::fs::read_to_string(repo_root().join(".env.example")).unwrap();
    assert!(text.contains("FEISHU_APP_ID="));
    assert!(text.contains("FEISHU_APP_SECRET="));
    assert!(text.contains("FEISHU_ALLOWED_CHAT_IDS="));
    assert!(text.contains("cli_xxxxxxxx") || text.contains("替换成应用密钥"));
    assert!(!text.contains("cli_a"));
}

#[test]
fn deploy_doc_is_chinese_and_covers_ipv6_http_https_and_feishu() {
    let text = std::fs::read_to_string(repo_root().join("docs/deploy-raspberry-pi.md")).unwrap();
    for needle in [
        "树莓派",
        "IPv6",
        "明文",
        "AAAA",
        "不使用 Cloudflare Tunnel",
        "内网穿透",
        "FEISHU_APP_ID",
        "长连接",
        "企业自建",
        "会话 ID",
        "oc_",
        "im.message.receive_v1",
        "card.action.trigger",
        "linux/arm64",
        "1 GB",
        "cargo test",
    ] {
        assert!(text.contains(needle), "missing: {needle}");
    }
    assert!(text.contains("[2001:db8::1]") || text.contains("方括号"));
    assert!(text.contains("Let’s Encrypt") || text.contains("Let's Encrypt"));
    let readme = std::fs::read_to_string(repo_root().join("README.md")).unwrap();
    assert!(readme.contains("遥显"));
    assert!(readme.contains("cargo test"));
    assert!(!readme.contains("pytest"));
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test deploy -- --nocapture
```

Expected: 文档尚未改完，部分字符串断言失败。

- [ ] **Step 3: Update the files**

`.env.example` 按上面追加飞书三项（注释掉的占位符）。

`README.md` 重写本地运行与测试段落：

```markdown
# 遥显

手机打开管理页，或在指定的飞书群里发送 JPEG/PNG 或文字，客厅里的 Android 电视全屏显示。家里的树莓派有公网 IPv6，服务直接听在上面，不经过内网穿透。

管理页和部署说明都是简体中文。仓库里没有真实密码或飞书密钥。

## 本地运行管理页

```bash
cd server
ADMIN_PASSWORD='换成你自己的长随机密码' DATA_DIR=../data HOST=127.0.0.1 PORT=8080 cargo run
```

浏览器打开 `http://127.0.0.1:8080`。不要使用 `.env.example` 里的 `change-me`，服务会拒绝这个示例值。

可选：在 `.env` 里配置 `FEISHU_APP_ID`、`FEISHU_APP_SECRET`、`FEISHU_ALLOWED_CHAT_IDS` 后，同一进程会作为飞书机器人，用长连接收指定群的消息。

## 测试

```bash
cd server
cargo test
```

Android 单元测试和调试安装包：

```bash
export ANDROID_HOME="$HOME/android-sdk"
cd android
./gradlew :app:testDebugUnitTest :app:assembleDebug
```

安装包在 `android/app/build/outputs/apk/debug/app-debug.apk`，并可复制一份到 `dist/tvdisplay-debug.apk`。

## 部署到树莓派

见 [docs/deploy-raspberry-pi.md](docs/deploy-raspberry-pi.md)。默认 HTTP、端口 8080、`network_mode: host`，直接使用树莓派的公网 IPv6。树莓派内存只有 1 GB 时，请在文档里的「在其它机器上构建镜像」一节编译，不要在派上直接 `docker compose build`。

## 设计与计划

- [原设计说明](docs/superpowers/specs/2026-11-26-android-tv-remote-display-design.md)
- [飞书与 Rust 后端设计说明](docs/superpowers/specs/2026-11-28-feishu-rust-backend-design.md)
- [飞书与 Rust 后端实施计划](docs/superpowers/plans/2026-11-28-feishu-rust-backend.md)
```

更新 `docs/deploy-raspberry-pi.md`，保留现有章节（Docker 安装、IPv6、明文风险、可选 HTTPS、侧载、备份），并做这些修改：

1. 开篇改为：服务是一个 Rust 进程；图片和文字都可以发；飞书是可选的第二种控制方式。
2. 新增「在其它机器上构建镜像」一节，写明不要在 1 GB 设备上编译，并给出完整命令：

```bash
# 在内存充足的机器上（已安装 Docker Buildx）
git clone <你的仓库地址> tvdisplay
cd tvdisplay
docker buildx create --use --name tvdisplay-builder || true
docker buildx build --platform linux/arm64 -t tvdisplay:arm64 --load .
docker save tvdisplay:arm64 | gzip > tvdisplay-arm64.tar.gz
# 把 tvdisplay-arm64.tar.gz 拷到树莓派后：
gunzip -c tvdisplay-arm64.tar.gz | docker load
```

然后在树莓派的 `docker-compose.yml` 里临时加上 `image: tvdisplay:arm64`，或 `docker tag tvdisplay:arm64 tvdisplay:latest` 后执行 `docker compose up -d`（若仍写着 `build: .`，加上 `image: tvdisplay:arm64` 并避免再次 `--build`）。若有人坚持在派上编译，写明风险：需要很大的 swap，耗时长，可能仍会 OOM，不推荐。

3. 新增「配置飞书机器人」一节，按官方文档写出步骤：
   - 打开 [飞书开发者后台](https://open.feishu.cn/app)，创建**企业自建应用**（长连接不支持商店应用）
   - 在「凭证与基础信息」复制 App ID、App Secret，写入 `.env` 的 `FEISHU_APP_ID`、`FEISHU_APP_SECRET`
   - 开启机器人能力
   - 权限：申请「获取群组中所有消息」（`im:message.group_msg`，敏感权限，因为不必 @ 也能收群消息）以及「以应用身份发消息」（`im:message:send_as_bot` 或 `im:message`）
   - 事件：订阅「接收消息」v2.0，事件类型 `im.message.receive_v1`。文档：https://open.feishu.cn/document/server-docs/im-v1/message/events/receive?lang=zh-CN
   - 回调：订阅新版「卡片回传交互」`card.action.trigger`。文档：https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN
   - 事件配置和回调配置的订阅方式都选「使用长连接接收事件/回调」。文档：https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case?lang=zh-CN 与 https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address?lang=zh-CN
   - 注意：后台保存长连接方式时，通常要求已有一条在线连接。所以先在 `.env` 配齐三项并启动服务，确认日志出现「飞书已连接」，再回后台保存订阅方式，然后发布应用版本
   - 取得群 ID：飞书客户端 7.60 及以上，打开目标群 → 右上角 ··· → 群设置，页面底部「会话 ID」，格式为 `oc_` 开头。文档：https://open.feishu.cn/document/server-docs/group/chat/chat-id-description?lang=zh-CN 。把一个或多个 ID 写入 `FEISHU_ALLOWED_CHAT_IDS`，逗号分隔
   - 把机器人拉进这些群
   - 在群里直接发 JPEG/PNG、文字或富文本图文，不必 @。机器人会回一张卡片，点「强制弹出」或「普通显示」后才送到电视
4. 管理页说明改为：图片不再必填，图片和文字至少一样。
5. 说明升级后 `./data` 里已有的 `content.json` 和图片仍然有效；需要重新侧载一版支持纯文字的电视应用。
6. 保留「不使用 Cloudflare Tunnel，也不使用其他内网穿透」、IPv6 方括号示例、明文密码风险、AAAA 与 Let’s Encrypt。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test deploy -- --nocapture
```

Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add .env.example docs/deploy-raspberry-pi.md README.md server/tests/deploy.rs
git commit -m "docs: explain Feishu setup and building the Pi image off-device"
```

---

### Task 17: Android 纯文字全屏展示

**Files:**
- Modify: `android/app/src/main/java/com/tvdisplay/remote/logic/ContentMessage.kt`
- Modify: `android/app/src/main/java/com/tvdisplay/remote/ui/TvApp.kt`
- Modify: `android/app/src/main/res/values/strings.xml`
- Modify: `android/app/src/test/java/com/tvdisplay/remote/logic/ContentMessageTest.kt`
- Create: `android/app/src/test/java/com/tvdisplay/remote/logic/PublishedContentTest.kt`
- 不修改 `FocusPolicy.kt` 的逻辑。`DisplayState.kt` 与 `Poster.kt` 仅在确实需要时改；默认不必改

**Interfaces:**
- Consumes: 现有 `ContentMessage` 字段（`revision`、`text`、`imageBase64`、`forcePopup`），协议字段不变
- Produces: `fun ContentMessage.isPublishedTextOnly(): Boolean` — `revision > 0 && imageBase64.isNullOrBlank() && text.isNotBlank()`
- Produces: `fun ContentMessage.shouldShowUnpublishedPlaceholder(): Boolean` — 无图且文字为空白时为真（`revision == 0` 的空目录走这里；无图无字也走这里）
- `DisplayScreen`：能解码出图片时仍全屏显示图片，文字作为底部说明；`isPublishedTextOnly()` 时全屏居中显示文字，不再显示「还没发布」；`shouldShowUnpublishedPlaceholder()` 时显示空状态文案
- `strings.xml` 的 `empty_content` 改为提及「还没发布」，并提示可以用管理页或飞书群发送图片或文字
- 抢焦点规则不变，已有 `FocusPolicyTest` 必须继续通过

- [ ] **Step 1: Write the failing tests**

在 `ContentMessageTest.kt` 追加：

```kotlin
@Test
fun textOnlyPublishedMessageParsesWithNullImage() {
    val message = parseServerMessage(
        """{"type":"content","revision":2,"text":"纯文字","force_popup":false,"image_mime":null,"image_base64":null,"updated_at":"2026-11-28T12:00:00+00:00"}""",
    )
    assertEquals(2L, message.revision)
    assertEquals("纯文字", message.text)
    assertNull(message.imageBase64)
    assertTrue(message.isPublishedTextOnly())
    assertFalse(message.shouldShowUnpublishedPlaceholder())
}
```

创建 `android/app/src/test/java/com/tvdisplay/remote/logic/PublishedContentTest.kt`：

```kotlin
package com.tvdisplay.remote.logic

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PublishedContentTest {
    @Test
    fun unpublishedCatalogShowsPlaceholder() {
        val message = parseServerMessage(
            """{"type":"content","revision":0,"text":"","force_popup":false,"image_mime":null,"image_base64":null,"updated_at":null}""",
        )
        assertTrue(message.shouldShowUnpublishedPlaceholder())
        assertFalse(message.isPublishedTextOnly())
    }

    @Test
    fun textOnlyPublishedContentDoesNotShowPlaceholder() {
        val message = ContentMessage(
            type = "content",
            revision = 1,
            text = "客厅公告",
            forcePopup = true,
            imageMime = null,
            imageBase64 = null,
            updatedAt = "2026-11-28T12:00:00+00:00",
        )
        assertTrue(message.isPublishedTextOnly())
        assertFalse(message.shouldShowUnpublishedPlaceholder())
    }

    @Test
    fun imageContentIsNotTextOnly() {
        val message = ContentMessage(
            type = "content",
            revision = 1,
            text = "说明",
            forcePopup = false,
            imageMime = "image/jpeg",
            imageBase64 = "YQ==",
            updatedAt = null,
        )
        assertFalse(message.isPublishedTextOnly())
        assertFalse(message.shouldShowUnpublishedPlaceholder())
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
export ANDROID_HOME="$HOME/android-sdk"
cd android && ./gradlew :app:testDebugUnitTest --tests com.tvdisplay.remote.logic.PublishedContentTest --tests com.tvdisplay.remote.logic.ContentMessageTest
```

Expected: FAIL，缺少 `isPublishedTextOnly` / `shouldShowUnpublishedPlaceholder`。若本机没有 Android SDK，先按仓库现有 README 设置 `ANDROID_HOME` 后再跑，不要改 minSdk 或依赖去绕过。

- [ ] **Step 3: Write minimal implementation**

在 `ContentMessage.kt` 末尾追加：

```kotlin
fun ContentMessage.isPublishedTextOnly(): Boolean =
    revision > 0 && imageBase64.isNullOrBlank() && text.isNotBlank()

fun ContentMessage.shouldShowUnpublishedPlaceholder(): Boolean =
    imageBase64.isNullOrBlank() && text.isBlank()
```

修改 `strings.xml` 中的 `empty_content`：

```xml
<string name="empty_content">还没发布。在手机上打开管理页，或在飞书群里发送图片或文字。</string>
```

修改 `TvApp.kt` 的 `DisplayScreen` 中无 bitmap 的分支。把原来的：

```kotlin
        } else {
            Text(
                stringResource(R.string.empty_content),
                color = Muted,
                fontSize = 28.sp,
                modifier = Modifier.align(Alignment.Center).padding(48.dp),
            )
        }
```

换成：

```kotlin
        } else if (message?.isPublishedTextOnly() == true) {
            Text(
                message.text,
                color = Ink,
                fontSize = 42.sp,
                modifier = Modifier.align(Alignment.Center).padding(48.dp),
            )
        } else {
            Text(
                stringResource(R.string.empty_content),
                color = Muted,
                fontSize = 28.sp,
                modifier = Modifier.align(Alignment.Center).padding(48.dp),
            )
        }
```

底部 caption 仍可在「有图且有文字」时显示。纯文字全屏时不要再在底部重复叠一层同一段文字：把 caption 条件改为 `caption.isNotBlank() && current != null`。

需要在 `TvApp.kt` 增加 `import com.tvdisplay.remote.logic.isPublishedTextOnly`。`FocusPolicy.kt` 不要改。

- [ ] **Step 4: Run test to verify it passes**

```bash
export ANDROID_HOME="$HOME/android-sdk"
cd android && ./gradlew :app:testDebugUnitTest
```

Expected: 全部单元测试通过，包括 `FocusPolicyTest`、`PublishedContentTest`、`ContentMessageTest`。

- [ ] **Step 5: Commit**

```bash
git add android/app/src/main/java/com/tvdisplay/remote/logic/ContentMessage.kt android/app/src/main/java/com/tvdisplay/remote/ui/TvApp.kt android/app/src/main/res/values/strings.xml android/app/src/test/java/com/tvdisplay/remote/logic/ContentMessageTest.kt android/app/src/test/java/com/tvdisplay/remote/logic/PublishedContentTest.kt
git commit -m "feat: show full-screen text on the TV when a revision has no image"
```

---

### Task 18: 删除 Python 后端

**Files:**
- Delete: `server/tvdisplay/` 整棵目录（含 `__init__.py`、`__main__.py`、`app.py`、`auth.py`、`config.py`、`content.py`、`healthcheck.py`、`hub.py`、`logging_redact.py`、`templates/`）
- Delete: `server/pyproject.toml`
- Delete: `server/tests/__init__.py`、`server/tests/conftest.py`、`server/tests/test_admin.py`、`server/tests/test_auth.py`、`server/tests/test_config.py`、`server/tests/test_content.py`、`server/tests/test_deploy.py`、`server/tests/test_logging.py`、`server/tests/test_websocket.py`
- Modify: `.gitignore`（可去掉纯 Python 缓存规则，保留无害项也可以）、`.dockerignore`（去掉已无意义的 `**/.venv` 亦可保留）
- 不要删除 `server/src/`、`server/templates/`、`server/tests/*.rs`、`server/Cargo.toml`、`server/Cargo.lock`

前置条件：Task 1–17 的 Rust 与 Android 测试已经通过。本任务之后仓库里只能有这一套后端。

- [ ] **Step 1: Write a failing assertion that Python is gone**

在 `server/tests/deploy.rs` 追加：

```rust
#[test]
fn python_backend_is_removed() {
    let root = repo_root();
    assert!(!root.join("server/pyproject.toml").exists());
    assert!(!root.join("server/tvdisplay/app.py").exists());
    assert!(!root.join("server/tests/test_admin.py").exists());
    assert!(root.join("server/Cargo.toml").exists());
    assert!(root.join("server/src/main.rs").exists());
}
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd server && cargo test --test deploy python_backend_is_removed -- --nocapture
```

Expected: FAIL，因为 Python 文件还在。

- [ ] **Step 3: Delete the Python backend**

```bash
git rm -r server/tvdisplay server/pyproject.toml \
  server/tests/__init__.py server/tests/conftest.py \
  server/tests/test_admin.py server/tests/test_auth.py \
  server/tests/test_config.py server/tests/test_content.py \
  server/tests/test_deploy.py server/tests/test_logging.py \
  server/tests/test_websocket.py
```

若某些路径此前已不存在，对剩余文件执行 `git rm`。不要手动留下第二套 Python 服务「以备回滚」；回滚用 git 历史。

- [ ] **Step 4: Run test to verify it passes**

```bash
cd server && cargo test --test deploy -- --nocapture
```

Expected: 包括 `python_backend_is_removed` 在内全部通过。再跑一次完整后端测试，确认没有测试还在 `use` Python 模块：

```bash
cd server && cargo test
```

Expected: 全部通过。

- [ ] **Step 5: Commit**

```bash
git add -A server .gitignore .dockerignore server/tests/deploy.rs
git commit -m "chore: remove the Python backend after the Rust server is tested"
```

---

### Task 19: 收尾核对

**Files:**
- 如核对中发现文档或测试缺口，只改必要文件，不要再扩大范围

**Interfaces:**
- Consumes: 本计划全部已实现行为
- Produces: 一组已实际跑过的验证命令及其结果（全部通过才算完成）

- [ ] **Step 1: Run the Rust test suite**

```bash
cd server && cargo test
```

Expected: 全部测试通过，包括 config、auth、content、logging、hub、admin、websocket、feishu_*、health、deploy。

- [ ] **Step 2: Run Android unit tests and build the debug APK**

```bash
export ANDROID_HOME="$HOME/android-sdk"
cd android
./gradlew :app:testDebugUnitTest :app:assembleDebug
```

Expected: 测试通过，生成 `android/app/build/outputs/apk/debug/app-debug.apk`。如仓库惯例需要一份拷贝，再执行 `mkdir -p dist && cp android/app/build/outputs/apk/debug/app-debug.apk dist/tvdisplay-debug.apk`。APK 体积大，只有在仓库已跟踪 `dist/tvdisplay-debug.apk` 时才提交更新后的文件。

- [ ] **Step 3: Confirm the remaining requirements by inspection**

逐项确认：

- 仓库中没有 `server/tvdisplay/` 或 `server/pyproject.toml`
- `.env.example` 只有飞书占位符，没有真实密钥
- `GET /health` 为 `{"ok": true}`；Dockerfile `HEALTHCHECK` 使用 `tvdisplay health`，且该子命令不要求 `ADMIN_PASSWORD`
- 没有 `/docs`、`/redoc`、OpenAPI
- 管理页和电视文案是简体中文；纯文字发布在管理页、WebSocket（`image_*` 为 null）和电视全屏文字三条路径上一致
- HMAC Cookie 仍与向量 `31ab44cf12e015ceb7436a0b696dad9e2725da70819a6ee62f373dc7c5612d53` 一致
- 部署文档包含飞书开通步骤、群 ID 获取方法、1 GB 设备上的交叉构建，以及「不使用 Cloudflare Tunnel」
- `FocusPolicy` 仍只根据 `force_popup` 与修订号决定是否抢焦点

- [ ] **Step 4: Commit only if inspection required fixes**

若 Step 3 改了文件：

```bash
git add -A
git commit -m "docs: finish verification notes for the Rust Feishu backend"
```

若没有改动，不要空提交。

---

