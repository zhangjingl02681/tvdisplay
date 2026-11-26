package com.tvdisplay.remote.logic

import java.net.URI

data class ParsedServer(
    val httpBase: String,
    val webSocketUrl: String,
)

fun parseServerUrl(input: String): ParsedServer {
    val trimmed = input.trim()
    if (trimmed.isEmpty()) {
        throw IllegalArgumentException("请填写服务器地址")
    }
    val uri = try {
        URI(trimmed)
    } catch (_: Exception) {
        throw IllegalArgumentException("地址格式不正确。IPv6 要写成 http://[2001:db8::1]:8080")
    }
    val scheme = uri.scheme?.lowercase()
    if (scheme != "http" && scheme != "https") {
        throw IllegalArgumentException("地址必须以 http:// 或 https:// 开头")
    }
    if (!uri.userInfo.isNullOrEmpty()) {
        throw IllegalArgumentException("请不要把密码写在地址里")
    }
    val authority = uri.rawAuthority
    if (authority.isNullOrBlank() || uri.host.isNullOrBlank()) {
        throw IllegalArgumentException("地址缺少主机名")
    }
    val wsScheme = if (scheme == "https") "wss" else "ws"
    return ParsedServer(
        httpBase = "$scheme://$authority",
        webSocketUrl = "$wsScheme://$authority/ws/tv",
    )
}
