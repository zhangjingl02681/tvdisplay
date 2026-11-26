# 遥显

手机打开管理页，上传一张 JPEG 或 PNG，电视上的 Android 应用全屏显示这张图和可选的说明文字。家里的树莓派有公网 IPv6，服务直接听在上面，不经过内网穿透。

管理页和部署说明都是简体中文。仓库里没有真实密码。

## 本地运行管理页

```bash
cd server
python3 -m venv .venv
.venv/bin/pip install -e ".[dev]"
ADMIN_PASSWORD='换成你自己的长随机密码' DATA_DIR=../data HOST=127.0.0.1 PORT=8080 .venv/bin/python -m tvdisplay
```

浏览器打开 `http://127.0.0.1:8080`。`ADMIN_PASSWORD` 不要写进仓库，也不要使用 `.env.example` 里的 `change-me`，服务会拒绝这个示例值。

## 测试

```bash
cd server
.venv/bin/pytest
```

Android 单元测试和调试安装包：

```bash
export ANDROID_HOME="$HOME/android-sdk"
cd android
./gradlew :app:testDebugUnitTest :app:assembleDebug
```

安装包在 `android/app/build/outputs/apk/debug/app-debug.apk`，并复制一份到 `dist/tvdisplay-debug.apk`。

## 部署到树莓派

见 [docs/deploy-raspberry-pi.md](docs/deploy-raspberry-pi.md)。默认 HTTP、端口 8080、`network_mode: host`，直接使用树莓派的公网 IPv6。

## 设计与计划

- [设计说明](docs/superpowers/specs/2026-11-26-android-tv-remote-display-design.md)
- [实施计划](docs/superpowers/plans/2026-11-26-android-tv-remote-display.md)
