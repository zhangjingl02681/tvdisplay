# 在树莓派上部署遥显

遥显是一个 Rust 进程。图片和文字都可以发到一台 Android 电视上，至少要有一样。飞书是可选的第二种控制方式：在指定群里发图或文字，点卡片确认后才会送到电视。树莓派需要一块已经能从公网访问的 IPv6 地址。服务直接听在这台机器上，默认端口 8080。

本方案不使用 Cloudflare Tunnel，也不使用其他内网穿透。Docker Compose 使用 `network_mode: host`，让进程绑定宿主机的 IPv6，而不是只绑在容器的私网里。

## 你需要准备

- 树莓派 4 或更新的型号，64 位系统（Raspberry Pi OS 或其他 Debian 系）。内存只有 1 GB 时，不要在派上编译 Rust
- 已安装 Docker，以及 Docker Compose 插件
- 一块公网 IPv6。没有域名时，路由器或防火墙放行入站 TCP 8080
- 一台 Android 7（API 24）或更高版本的电视，用来侧载安装包
- 一台手机，用来打开管理页
- 一台内存更大的电脑，用来构建 `linux/arm64` 镜像

先在树莓派上确认 IPv6 地址：

```bash
ip -6 addr show scope global
```

下面用 `2001:db8::1` 做示例。地址里的字母按你机器上的实际地址替换。浏览器和电视里都要把 IPv6 写在方括号中，例如 `http://[2001:db8::1]:8080`。

## 1. 安装 Docker

```bash
sudo apt-get update
sudo apt-get install -y ca-certificates curl
curl -fsSL https://get.docker.com | sudo sh
sudo usermod -aG docker "$USER"
```

重新登录后执行 `docker compose version`，能打出版本号即可。

## 2. 在其它机器上构建镜像

1 GB 的树莓派上不要执行 `docker compose build`。Rust 编译容易把内存打满。请在内存充足的机器上交叉构建，再把镜像拷过去。改过代码后，在这台构建机器上先确认测试通过：

```bash
cd server
cargo test
```

构建并导出镜像：

```bash
# 在内存充足的机器上（已安装 Docker Buildx）
git clone <你的仓库地址> tvdisplay
cd tvdisplay
docker buildx create --use --name tvdisplay-builder || true
docker buildx build --platform linux/arm64 -t tvdisplay:arm64 --load .
docker save tvdisplay:arm64 | gzip > tvdisplay-arm64.tar.gz
```

把 `tvdisplay-arm64.tar.gz` 拷到树莓派后：

```bash
gunzip -c tvdisplay-arm64.tar.gz | docker load
```

然后在树莓派的 `docker-compose.yml` 里给 `tvdisplay` 服务加上 `image: tvdisplay:arm64`，或执行 `docker tag tvdisplay:arm64 tvdisplay:latest`。启动时用 `docker compose up -d`，不要再加 `--build`。文件里如果仍写着 `build: .`，加上 `image: tvdisplay:arm64` 并避免再次 `--build`，否则 Compose 会在派上重新编译。

如果有人坚持在树莓派上编译：需要很大的 swap，耗时长，可能仍会 OOM，不推荐。

## 3. 取得代码并设置密码

```bash
git clone <你的仓库地址> tvdisplay
cd tvdisplay
cp .env.example .env
python3 -c "import secrets; print(secrets.token_urlsafe(24))"
```

把打印出来的字符串写进 `.env` 的 `ADMIN_PASSWORD=`。这是你自己的密码，不要提交 `.env`，不要写进文档，也不要贴到聊天里。`.env.example` 里的 `change-me` 只是占位，服务器会拒绝用它启动。

`.env` 里保持：

```text
HOST=::
PORT=8080
DATA_DIR=/data
```

`HOST=::` 表示同时接受 IPv6。在 Linux 上这也通常接受 IPv4 映射连接。

## 4. 启动

镜像已经按上一节载入时：

```bash
docker compose up -d
docker compose logs -f tvdisplay
```

日志里应该能看到服务在监听，但不能出现你的密码或飞书密钥。如果日志里出现了密码，立刻换成新密码。

确认端口：

```bash
ss -lnt | grep 8080
```

期望看到 `[::]:8080` 或 `*:8080`。

在树莓派上检查健康接口（`-g` 让 curl 不要把方括号当成通配符）。健康检查不需要 `ADMIN_PASSWORD`：

```bash
curl -g "http://[::1]:8080/health"
```

返回 `{"ok":true}` 即可。从另一台在公网的机器上再试：

```bash
curl -g "http://[2001:db8::1]:8080/health"
```

如果这里不通，先看路由器有没有把入站 TCP 8080 转到树莓派，以及系统防火墙有没有放行。不要改用内网穿透来绕过。

## 5. 配置飞书机器人

飞书是可选的。三项都留空时，网页和电视照常工作，日志写「飞书未配置」。只填一部分时，服务会拒绝启动，并说明缺了哪一项，不会打印密钥。

1. 打开 [飞书开发者后台](https://open.feishu.cn/app)，创建**企业自建应用**。长连接不支持商店应用。
2. 在「凭证与基础信息」复制 App ID、App Secret，写入 `.env` 的 `FEISHU_APP_ID`、`FEISHU_APP_SECRET`。不要把真实密钥提交到 git。
3. 开启机器人能力。
4. 权限：申请「获取群组中所有消息」（`im:message.group_msg`，敏感权限，因为不必 @ 也能收群消息）以及「以应用身份发消息」（`im:message:send_as_bot` 或 `im:message`）。
5. 事件：订阅「接收消息」v2.0，事件类型 `im.message.receive_v1`。文档：https://open.feishu.cn/document/server-docs/im-v1/message/events/receive?lang=zh-CN
6. 回调：订阅新版「卡片回传交互」`card.action.trigger`。文档：https://open.feishu.cn/document/feishu-cards/card-callback-communication?lang=zh-CN
7. 事件配置和回调配置的订阅方式都选「使用长连接接收事件/回调」。文档：https://open.feishu.cn/document/server-docs/event-subscription-guide/event-subscription-configure-/request-url-configuration-case?lang=zh-CN 与 https://open.feishu.cn/document/event-subscription-guide/callback-subscription/step-1-choose-a-subscription-mode/configure-callback-request-address?lang=zh-CN
8. 后台保存长连接方式时，通常要求已有一条在线连接。所以先在 `.env` 配齐三项并启动服务，确认日志出现「飞书已连接」，再回后台保存订阅方式，然后发布应用版本。
9. 取得群 ID：飞书客户端 7.60 及以上，打开目标群 → 右上角 ··· → 群设置，页面底部「会话 ID」，格式为 `oc_` 开头。文档：https://open.feishu.cn/document/server-docs/group/chat/chat-id-description?lang=zh-CN 。把一个或多个 ID 写入 `FEISHU_ALLOWED_CHAT_IDS`，逗号分隔。
10. 把机器人拉进这些群。
11. 在群里直接发 JPEG/PNG、文字或富文本图文，不必 @。机器人会回一张卡片，点「强制弹出」或「普通显示」后才送到电视。

## 6. 用手机发布内容

手机浏览器打开：

```text
http://[2001:db8::1]:8080
```

输入 `.env` 里的管理密码。页面上可以：

- 上传一张 JPEG 或 PNG，最大 10 MB。图片不再必填
- 写一段说明文字。图片和文字至少一样
- 打开或关闭「新内容到达时强制弹出」
- 看到当前内容的预览
- 看到电视是在线还是离线

发布之后，内容会写到树莓派的 `./data` 目录，包括 `content.json` 和可选图片。升级后，目录里已有的 `content.json` 和图片仍然有效。换机器或重做系统时，把这个目录一起拷走。

支持纯文字全屏的电视应用需要重新侧载一次。旧安装包不会把没有图片的内容显示成全屏文字。

## 7. 没有域名时的明文风险

没有域名时就用上面的 HTTP，不要为了加密去套一层隧道。HTTP 不加密。管理密码、图片和文字都会以明文经过你和树莓派之间的网络，路径上能看到流量的人可以读到密码。

请这样做：

- 使用足够长的随机密码，不要复用邮箱、微信或其他网站的密码
- 只在你信任的网络上看这个管理页；公共 Wi-Fi 上尤其不要输入密码
- 密码一旦可能泄露，就改 `.env` 里的 `ADMIN_PASSWORD`，然后 `docker compose up -d` 重启

如果你有自己的域名，并且做好了指向这块 IPv6 的 AAAA 记录，改用下一节的 HTTPS，不要继续用明文 HTTP。

## 8. 有域名时启用 HTTPS（Let’s Encrypt）

前提：

- 你有一个域名，例如 `tv.example.com`
- 这个名字的 AAAA 记录指向树莓派的公网 IPv6
- 入站 TCP 80 和 443 能到达树莓派（Let’s Encrypt 要验证域名）

在 `.env` 增加：

```text
DOMAIN=tv.example.com
```

然后：

```bash
docker compose -f docker-compose.yml -f docker-compose.https.yml up -d
```

这一步会把遥显改成只听 `127.0.0.1:8080`，由 Caddy 在 443 上申请并续期 Let’s Encrypt 证书，再反代到本机。Caddy 的配置在 `deploy/Caddyfile`。证书存在 Docker 卷里。镜像已经在其它机器构建好时，不要加 `--build`。

完成后用：

```text
https://tv.example.com
```

电视里的服务器地址也填这个，不要再填 `http://[IPv6]:8080`。HTTPS 下密码不再以明文传输。

80 端口仍要开着，证书到期前 Caddy 会用它续期。不要再把 8080 暴露到公网：HTTPS 模式下服务只绑在本机。

## 9. 在电视上安装应用

调试安装包是 `dist/tvdisplay-debug.apk`（自己构建时，Gradle 还会在 `android/app/build/outputs/apk/debug/app-debug.apk` 放一份）。应用不依赖 Google Play 服务，也不使用 Firebase，用侧载安装。升级到支持纯文字的版本后，需要重新侧载一次。

用 U 盘：

1. 把 apk 拷到 U 盘，插到电视上
2. 用电视自带的文件管理器打开 apk
3. 如果系统询问未知来源，允许本次安装

或用 adb（电视和电脑在同一网络，电视已打开网络调试）：

```bash
adb connect 192.0.2.20:5555
adb install -r dist/tvdisplay-debug.apk
```

打开「遥显」。服务器地址填：

- 只有 IPv6、没有域名：`http://[2001:db8::1]:8080`
- 已按上一节启用 HTTPS：`https://tv.example.com`

密码填和管理页相同的那一串。保存后，应用会以前台服务保持连接；断线后按 1 秒、2 秒、4 秒这样等待，最长 30 秒再试。

强制弹出开着，并且来的是一条新内容（修订号比上次显示过的更大）时，应用会把自己调到前台。关着时只更新内容，不抢当前焦点。只改强制弹出开关不会增加修订号。Android 10 及以后，系统可能拦住后台直接打开界面；如果电视没有自动切到遥显，请在系统设置里允许遥显显示全屏通知，或允许它显示在其他应用上层。电视重启或应用重连时，会先收到服务器上保存的当前内容。

同一时间服务器只认一台电视。电视上后打开的连接会替换先打开的，管理页上的「在线」对应的是最后那一台。

## 10. 更新与备份

```bash
git pull
```

在构建机器上重新执行「在其它机器上构建镜像」里的 `docker buildx build`，把新的 `tvdisplay-arm64.tar.gz` 拷到树莓派，`docker load` 之后：

```bash
docker compose up -d
```

如果启用了 HTTPS，两条 compose 文件都要带上。`./data` 里的 `content.json` 和图片在升级时仍然有效。不要把 `data/` 或 `.env` 提交进 git。
