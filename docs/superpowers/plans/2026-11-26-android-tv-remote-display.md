# 遥显 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 手机管理页上传一张 JPEG/PNG 和可选文字，树莓派上的服务推给一台 Android 电视全屏显示。

**Architecture:** FastAPI 把当前内容存在磁盘上，只用一条 WebSocket 推向最后连上的电视。电视应用是 Kotlin + Jetpack Compose for TV，前台服务负责重连；强制弹出开着且修订号变大时才把应用调到前台。树莓派用 Docker Compose 的 host 网络直接听公网 IPv6。

**Tech Stack:** Python 3.12、FastAPI、uvicorn、Jinja2、pytest、Docker Compose、Kotlin、Jetpack Compose for TV、OkHttp、minSdk 24。

## Global Constraints

- 管理页、电视界面、README、树莓派部署文档全部用简体中文
- 图片必填，只收 JPEG/PNG 文件头，最大 10 MB；文字可选
- 强制弹出开关和当前内容一起落盘
- WebSocket 同一时间只有一台电视，新连接替换旧连接；连上时先下发当前内容
- 密码不出现在仓库、URL 查询串或日志里
- 没有域名时默认 HTTP，部署文档写明明文密码风险；有域名和 AAAA 时文档说明可选的 Let’s Encrypt
- 监听 IPv6，默认端口 8080；Compose 使用 `network_mode: host`
- 不使用 Cloudflare Tunnel 或任何内网穿透
- Android minSdk 24，侧载调试 APK，不用 Google Play 服务，不用 Firebase
- 服务器地址必须接受 `http://[2001:db8::1]:8080`
- 强制弹出关着时不抢焦点
- 不做微信推送、不多电视并发、不上架

---

### Task 1: 设计说明与计划

**Files:**
- Create: `docs/superpowers/specs/2026-11-26-android-tv-remote-display-design.md`
- Create: `docs/superpowers/plans/2026-11-26-android-tv-remote-display.md`

- [ ] **Step 1: Commit**

```bash
git add docs/superpowers
git commit -m "docs: add remote display design spec and plan"
```

### Task 2: 服务配置

**Files:**
- Create: `server/pyproject.toml`
- Create: `server/tvdisplay/config.py`
- Create: `server/tvdisplay/healthcheck.py`
- Test: `server/tests/test_config.py`

**Interfaces:**
- Produces: `Settings(admin_password, data_dir, host="::", port=8080, auth_timeout_seconds=10.0, max_image_bytes=10*1024*1024, text_max_chars=4000)`
- Produces: `load_settings(environ) -> Settings`，缺密码或密码是 `change-me` 时 `SystemExit`，退出文本不含密码
- Produces: `health_urls(host, port) -> list[str]`

- [ ] **Step 1: Write the failing test** `test_load_settings_reads_host_port_and_directory`、`test_placeholder_passwords_are_rejected`、`test_health_urls_cover_ipv6_and_localhost`
- [ ] **Step 2: Run** `pytest tests/test_config.py -v`，确认因缺少模块失败
- [ ] **Step 3: Implement**
- [ ] **Step 4: Run** 同上，期望通过
- [ ] **Step 5: Commit**

```bash
git commit -m "feat: load server settings without logging the password"
```

### Task 3: 恒定时间密码与会话

**Files:**
- Create: `server/tvdisplay/auth.py`
- Test: `server/tests/test_auth.py`

**Interfaces:**
- Produces: `passwords_match(provided: str, expected: str) -> bool`
- Produces: `issue_session_token(password: str) -> str`（结果不含密码原文）
- Produces: `session_token_valid(token: str, password: str) -> bool`
- Produces: `origin_allowed(origin: str | None, host: str | None) -> bool`
- Produces: `COOKIE_NAME = "tvdisplay_session"`

- [ ] **Step 1–4: 测试先行，再实现**
- [ ] **Step 5: Commit**

```bash
git commit -m "feat: check the admin password in constant time"
```

### Task 4: 内容落盘

**Files:**
- Create: `server/tvdisplay/content.py`
- Test: `server/tests/test_content.py`

**Interfaces:**
- Produces: `MAX_IMAGE_BYTES = 10 * 1024 * 1024`
- Produces: `validate_image(data, max_bytes=MAX_IMAGE_BYTES) -> mime`，失败抛 `ContentValidationError`，消息为「请上传图片」「只支持 JPEG 或 PNG」或「图片不能超过 10 MB」
- Produces: `ContentStore.save_upload(image, text, force_popup) -> DisplayContent`
- Produces: `ContentStore.set_force_popup(force_popup) -> DisplayContent`（不增加 revision）
- Produces: `ContentStore.message() -> dict`，键为 `type, revision, text, force_popup, image_mime, image_base64, updated_at`

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: persist the current picture, caption, and popup flag"
```

### Task 5: 管理页

**Files:**
- Create: `server/tvdisplay/app.py`
- Create: `server/tvdisplay/templates/base.html`
- Create: `server/tvdisplay/templates/login.html`
- Create: `server/tvdisplay/templates/admin.html`
- Test: `server/tests/test_admin.py`

**Interfaces:**
- Consumes: Task 2–4
- Produces: `create_app(settings) -> FastAPI`
- 页面文案包含「发布到电视」「强制弹出」「电视状态」「尚未发布内容」「密码不正确」
- 未登录访问 `/` 返回 303 到 `/login`；`/api/status` 与 `/media/current` 返回 401

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: add the Chinese mobile admin page"
```

### Task 6: 单台电视的 WebSocket

**Files:**
- Create: `server/tvdisplay/hub.py`
- Modify: `server/tvdisplay/app.py`（`/ws/tv`）
- Test: `server/tests/test_websocket.py`

**Interfaces:**
- Produces: `TvHub.replace(websocket) -> previous | None`
- Produces: `TvHub.release(websocket)`，只在当前连接仍是这一条时清除
- Produces: `TvHub.push(message) -> bool`
- Produces: `TvHub.online`
- 认证帧 `{"type":"auth","password":...}`；查询串里的密码不能代替认证帧
- 新连接关闭码 4000 替换旧连接；失败关闭码 1008
- 发布和开关变更都 `push` 当前 `message()`

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: push content to the latest TV websocket"
```

### Task 7: 日志脱敏与进程入口

**Files:**
- Create: `server/tvdisplay/logging_redact.py`
- Create: `server/tvdisplay/__main__.py`
- Test: `server/tests/test_logging.py`

**Interfaces:**
- Produces: `RedactingFilter(secret)` 把日志消息和参数里的密码换成 `***`
- Produces: `install_redaction(secret)` 装到 `tvdisplay`、`uvicorn`、`uvicorn.access`、`uvicorn.error`
- `python -m tvdisplay` 用 `settings.host` 和 `settings.port` 启动 uvicorn

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: redact the admin password from server logs"
```

### Task 8: 树莓派 Compose 与中文部署文档

**Files:**
- Create: `Dockerfile`
- Create: `docker-compose.yml`
- Create: `docker-compose.https.yml`
- Create: `deploy/Caddyfile`
- Create: `.env.example`
- Create: `docs/deploy-raspberry-pi.md`
- Create: `README.md`
- Test: `server/tests/test_deploy.py`

**Interfaces:**
- `docker-compose.yml` 含 `network_mode: host`，不含 cloudflare、ngrok、tailscale
- 部署文档含「明文」「AAAA」「Let’s Encrypt」「不使用 Cloudflare Tunnel」「方括号中的 IPv6」
- Caddyfile 把 `{$DOMAIN}` 反代到 `127.0.0.1:8080`

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: serve on the Pi public IPv6 with host networking"
```

### Task 9: 电视端地址解析

**Files:**
- Create: `android/` Gradle 工程（AGP 8.7.3、Kotlin 2.0.21、compileSdk 35、minSdk 24）
- Create: `android/app/src/main/java/com/tvdisplay/remote/logic/ServerUrl.kt`
- Test: `android/app/src/test/java/com/tvdisplay/remote/logic/ServerUrlTest.kt`

**Interfaces:**
- Produces: `parseServerUrl(input: String): ParsedServer`
- `ParsedServer.httpBase` 无末尾斜杠
- `parseServerUrl("http://[2001:db8::1]:8080").webSocketUrl == "ws://[2001:db8::1]:8080/ws/tv"`
- `https` 主机变成 `wss`，查询串为空
- 空串、`ftp://`、把密码写进 userinfo，都抛 `IllegalArgumentException`

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: accept an IPv6 server address on Android TV"
```

### Task 10: 退避、抢焦点与消息

**Files:**
- Create: `android/app/src/main/java/com/tvdisplay/remote/logic/Backoff.kt`
- Create: `android/app/src/main/java/com/tvdisplay/remote/logic/FocusPolicy.kt`
- Create: `android/app/src/main/java/com/tvdisplay/remote/logic/ContentMessage.kt`
- Test: `BackoffTest.kt`、`FocusPolicyTest.kt`、`ContentMessageTest.kt`

**Interfaces:**
- Produces: `backoffMillis(attempt: Int): Long`，attempt 0 → 1000，1 → 2000，4 → 16000，5 及以上 → 30000
- Produces: `shouldBringToForeground(forcePopup: Boolean, incomingRevision: Long, lastShownRevision: Long): Boolean`
  - `(true, 2, 1)` 为真
  - `(false, 2, 1)` 为假
  - `(true, 2, 2)` 为假
- Produces: `authPayload(password)`、`redactForLog(payload, password)`（结果不含密码）、`parseServerMessage(json)`
- `parseServerMessage` 读 `force_popup`、`image_base64`、`image_mime`、`revision`、`text`

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: decide reconnect delay and whether to take focus"
```

### Task 11: 全屏界面与前台服务

**Files:**
- Create: `MainActivity.kt`、`service/ConnectionService.kt`、`data/Prefs.kt`、`ui/TvApp.kt`
- Create: `AndroidManifest.xml`、简体中文 `strings.xml`、通知图标与 TV banner

**Interfaces:**
- Consumes: Task 9 和 Task 10 的纯函数。服务是否调用「调到前台」只看 `shouldBringToForeground`
- 前台服务类型 `dataSync`，通知文案为中文
- `force_popup` 为假时不调用把 Activity 拉到前台的函数
- 认证帧只通过 WebSocket 文本发送，logcat 不写密码

- [ ] **Step 5: Commit**

```bash
git commit -m "feat: show the picture on Android TV from a foreground service"
```

### Task 12: 调试 APK

**Files:**
- Build: `android/app/build/outputs/apk/debug/app-debug.apk`
- Copy: `dist/tvdisplay-debug.apk`

- [ ] **Step 1: Run** `./gradlew :app:testDebugUnitTest :app:assembleDebug`
- [ ] **Step 2: Copy the APK into `dist/`**
- [ ] **Step 3: Commit**

```bash
git commit -m "chore: add the sideload debug APK"
```

### Task 13: 收尾核对

- [ ] **Step 1: Run** `server/.venv/bin/pytest`
- [ ] **Step 2: Run** Android 单元测试（Task 12 已包含则确认结果仍是通过）
- [ ] **Step 3: 确认 README 与部署文档覆盖本地运行、pytest、APK、IPv6 HTTP 和可选 HTTPS**
- [ ] **Step 4: Commit** 仅当文档或测试还有缺口

```bash
git commit -m "docs: finish the Chinese README and deploy notes"
```
