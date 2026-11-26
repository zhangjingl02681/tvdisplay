# 在树莓派上部署遥显

遥显把手机上传的一张图片和可选文字推到一台 Android 电视上。树莓派需要一块已经能从公网访问的 IPv6 地址。服务直接听在这台机器上，默认端口 8080。

本方案不使用 Cloudflare Tunnel，也不使用其他内网穿透。Docker Compose 使用 `network_mode: host`，让进程绑定宿主机的 IPv6，而不是只绑在容器的私网里。

## 你需要准备

- 树莓派 4 或更新的型号，64 位系统（Raspberry Pi OS 或其他 Debian 系）
- 已安装 Docker，以及 Docker Compose 插件
- 一块公网 IPv6。没有域名时，路由器或防火墙放行入站 TCP 8080
- 一台 Android 7（API 24）或更高版本的电视，用来侧载安装包
- 一台手机，用来打开管理页

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

## 2. 取得代码并设置密码

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

## 3. 启动

```bash
docker compose up -d --build
docker compose logs -f tvdisplay
```

日志里应该能看到服务在监听，但不能出现你的密码。如果日志里出现了密码，立刻换成新密码。

确认端口：

```bash
ss -lnt | grep 8080
```

期望看到 `[::]:8080` 或 `*:8080`。

在树莓派上检查健康接口（`-g` 让 curl 不要把方括号当成通配符）：

```bash
curl -g "http://[::1]:8080/health"
```

返回 `{"ok":true}` 即可。从另一台在公网的机器上再试：

```bash
curl -g "http://[2001:db8::1]:8080/health"
```

如果这里不通，先看路由器有没有把入站 TCP 8080 转到树莓派，以及系统防火墙有没有放行。不要改用内网穿透来绕过。

## 4. 用手机发布内容

手机浏览器打开：

```text
http://[2001:db8::1]:8080
```

输入 `.env` 里的管理密码。页面上可以：

- 上传一张 JPEG 或 PNG，最大 10 MB，图片必填
- 写一段可选的说明文字
- 打开或关闭「新内容到达时强制弹出」
- 看到当前内容的预览
- 看到电视是在线还是离线

发布之后，图片和开关会写到树莓派的 `./data` 目录。换机器或重做系统时，把这个目录一起拷走。

## 5. 没有域名时的明文风险

没有域名时就用上面的 HTTP，不要为了加密去套一层隧道。HTTP 不加密。管理密码、图片和文字都会以明文经过你和树莓派之间的网络，路径上能看到流量的人可以读到密码。

请这样做：

- 使用足够长的随机密码，不要复用邮箱、微信或其他网站的密码
- 只在你信任的网络上看这个管理页；公共 Wi-Fi 上尤其不要输入密码
- 密码一旦可能泄露，就改 `.env` 里的 `ADMIN_PASSWORD`，然后 `docker compose up -d` 重启

如果你有自己的域名，并且做好了指向这块 IPv6 的 AAAA 记录，改用下一节的 HTTPS，不要继续用明文 HTTP。

## 6. 有域名时启用 HTTPS（Let’s Encrypt）

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
docker compose -f docker-compose.yml -f docker-compose.https.yml up -d --build
```

这一步会把遥显改成只听 `127.0.0.1:8080`，由 Caddy 在 443 上申请并续期 Let’s Encrypt 证书，再反代到本机。Caddy 的配置在 `deploy/Caddyfile`。证书存在 Docker 卷里。

完成后用：

```text
https://tv.example.com
```

电视里的服务器地址也填这个，不要再填 `http://[IPv6]:8080`。HTTPS 下密码不再以明文传输。

80 端口仍要开着，证书到期前 Caddy 会用它续期。不要再把 8080 暴露到公网：HTTPS 模式下服务只绑在本机。

## 7. 在电视上安装应用

调试安装包是 `dist/tvdisplay-debug.apk`（自己构建时，Gradle 还会在 `android/app/build/outputs/apk/debug/app-debug.apk` 放一份）。应用不依赖 Google Play 服务，也不使用 Firebase，用侧载安装。

用 U 盘：

1. 把 apk 拷到 U 盘，插到电视上
2. 用电视自带的文件管理器打开 apk
3. 如果系统询问未知来源，允许本次安装

或用 adb（电视和电脑在同一网络，电视已打开网络调试）：

```bash
adb connect 192.168.1.20:5555
adb install -r dist/tvdisplay-debug.apk
```

打开「遥显」。服务器地址填：

- 只有 IPv6、没有域名：`http://[2001:db8::1]:8080`
- 已按上一节启用 HTTPS：`https://tv.example.com`

密码填和管理页相同的那一串。保存后，应用会以前台服务保持连接；断线后按 1 秒、2 秒、4 秒这样等待，最长 30 秒再试。

强制弹出开着，并且来的是一张新内容时，应用会把自己调到前台。关着时只更新内容，不抢当前焦点。Android 10 及以后，系统可能拦住后台直接打开界面；如果电视没有自动切到遥显，请在系统设置里允许遥显显示全屏通知，或允许它显示在其他应用上层。电视重启或应用重连时，会先收到服务器上保存的当前内容，不会因为离线而漏掉最后一张图。

同一时间服务器只认一台电视。电视上后打开的连接会替换先打开的，管理页上的「在线」对应的是最后那一台。

## 8. 更新与备份

```bash
git pull
docker compose up -d --build
```

如果启用了 HTTPS，两条 compose 文件都要带上。图片在 `./data`，升级不会删这个目录。不要把 `data/` 或 `.env` 提交进 git。
