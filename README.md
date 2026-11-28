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
