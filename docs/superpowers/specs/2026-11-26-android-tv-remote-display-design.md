# 遥显设计说明

日期：2026-11-26

## 目标

一个人在手机上打开管理页，上传一张图片和可选文字，客厅里那一台 Android 电视全屏显示出来。树莓派已经有公网 IPv6，服务直接听在这台机器上。

## 不做的事

- 不把内容推到微信或其他聊天软件
- 不同时服务多台电视。新的电视连接会替换旧的
- 不上架应用商店。安装包侧载
- 不使用 Cloudflare Tunnel，也不使用其他内网穿透
- 不使用 Google Play 服务或 Firebase
- 不把真实密码写进仓库或日志

## 角色

- 管理页：手机浏览器。登录、上传、预览、看电视是否在线、开关强制弹出
- 服务：FastAPI。保存当前内容，通过 WebSocket 推给电视
- 电视：Kotlin，Jetpack Compose for TV，minSdk 24。全屏显示，前台服务保持连接

## 内容

当前内容只有一份，存在磁盘上（`content.json` 加 `image.jpg` 或 `image.png`）：

- 图片必填，只接受 JPEG 或 PNG 的文件头，最大 10 MB（10 × 1024 × 1024 字节）。不看扩展名
- 文字可选，去掉首尾空白后最长 4000 字
- `force_popup` 布尔值，单独修改时不增加修订号
- `revision` 从 0 开始，每成功发布一张新图加 1
- `updated_at` 为 UTC 时间

服务重启后从磁盘读回，电视重连时拿到的就是这份内容。

## 管理页

简体中文，手机优先，服务端渲染。

- `GET /login`、`POST /login`、`POST /logout`
- `GET /` 预览当前图片和文字、电视在线或离线、强制弹出是开还是关
- `POST /content` 上传图片、可选文字、强制弹出勾选
- `POST /force-popup` 只改开关。已有图片时把同一修订号再推一次，电视不因此抢焦点
- `GET /media/current` 给预览用，要登录
- `GET /api/status` 返回 `tv_online`、`force_popup`、`has_image`、`revision`、`text`，要登录。页面每 3 秒拉一次，用来刷新在线状态
- `GET /health` 返回 `{"ok": true}`，不登录，给部署检查用

密码来自环境变量 `ADMIN_PASSWORD`。空密码和示例值（包括 `change-me`）拒绝启动，退出信息里不带密码本身。

登录成功后发名为 `tvdisplay_session` 的 HttpOnly、SameSite=Lax Cookie。Cookie 的值是密码的 HMAC，不是密码原文。比较密码和比较令牌都用恒定时间比较。

浏览器如果带了 `Origin`，必须和 `Host` 一致，否则 POST 返回 403。

校验失败时管理页直接显示中文原因，例如「请上传图片」「只支持 JPEG 或 PNG」「图片不能超过 10 MB」。不回显用户刚输入的密码。

关闭 `/docs`、`/redoc` 和 OpenAPI，避免在公网 IPv6 上把接口形状摊开。

## 电视连接

`WS /ws/tv`。密码不放在 URL 里，避免进访问日志。连接建立后，客户端先发一条文本帧：

```json
{"type": "auth", "password": "<管理密码>"}
```

密码不对、正文不是 JSON、或超过 `auth_timeout_seconds`（默认 10 秒）没有这条消息，服务用关闭码 1008 断开，并且不把这台连接算作在线。

通过之后：

1. 若已经有一台电视，用关闭码 4000 断开它，只保留最新的连接
2. 立刻下发当前内容，让刚才离线的电视补上
3. 管理页之后每发布一次，向这一台连接推一条同样形状的消息

消息：

```json
{
  "type": "content",
  "revision": 1,
  "text": "可选文字",
  "force_popup": true,
  "image_mime": "image/jpeg",
  "image_base64": "<标准 base64，没有图片时为 null>",
  "updated_at": "2026-11-26T06:00:00+00:00"
}
```

还没有任何图片时，`revision` 为 0，`image_base64` 为 null。

在线状态就是「当前有没有这一条 WebSocket」。

## 日志

服务日志可以写「登录失败」「电视已连接」「已发布」。不可以写密码、Cookie、认证帧正文，也不写用户的说明文字。进程启动时给 `tvdisplay` 和 uvicorn 日志装上替换过滤器，万一密码被拼进日志，也替换成 `***`。

## Android 电视

包名 `com.tvdisplay.remote`。Jetpack Compose for TV。minSdk 24，targetSdk 35。调试 APK 侧载。清单里声明 Leanback 启动入口，不要求触屏。允许明文 HTTP，因为没有域名时地址就是 `http://[IPv6]:8080`。

用户在电视上填写：

- 服务器地址。必须接受 `http://[2001:db8::1]:8080` 这种带方括号的 IPv6，也接受普通主机名和 IPv4。只允许 `http` 或 `https`。密码不能写在地址里
- 管理密码。存在应用私有偏好里，不打印到 logcat

保存后启动前台服务（`dataSync`）。服务用 OkHttp 连接 `ws://` 或 `wss://` 主机的 `/ws/tv`，查询串里没有密码，连上后发送认证帧。

断线后重试。第 `attempt` 次（从 0 开始）等待 `min(1000 << attempt, 30000)` 毫秒，attempt 超过 5 仍按 30 秒封顶。连上后把次数清零。

收到内容后更新界面上的图和文字。是否抢焦点只由这一条规则决定：

```text
force_popup 为真，且这次的 revision 大于本机已经展示过的 revision
```

为真：发一条全屏意图通知，并尝试把 `MainActivity` 调到前台。为假：只更新内存里的内容；应用已经开着时画面会换，但不会调用把应用拉到前台的代码。已经展示过的修订号写在偏好里，所以同一次内容的重连不会反复弹出。

界面文案全部是简体中文。没有内容时说明还没发布。连接状态用「已连接 / 正在重试 / 密码不正确」这类短句，不包含密码。

## 部署

见 `docs/deploy-raspberry-pi.md`。

- Compose 使用 `network_mode: host`
- 默认 `HOST=::`，`PORT=8080`
- 数据卷挂到 `/data`
- 没有域名：HTTP，文档说明明文密码风险
- 有域名且 AAAA 指向树莓派：可选的 `docker-compose.https.yml` 用 Caddy 申请 Let’s Encrypt，服务改听 127.0.0.1
- 镜像基于 `python:3.12-slim`，树莓派 64 位可直接构建

## 测试

服务用 pytest：密码比较、示例密码被拒绝、图片类型和 10 MB 上限、磁盘往返、登录与中文页面、越权与跨站、WebSocket 补发、推送、后来的连接替换先前的、日志里没有密码。

电视端用 JVM 单元测试，不需要模拟器：IPv6 地址解析、退避时间、抢焦点规则、认证帧脱敏、内容 JSON 解析。调试 APK 用 `assembleDebug` 打出。
