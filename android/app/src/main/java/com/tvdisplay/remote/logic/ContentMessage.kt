package com.tvdisplay.remote.logic

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put

private val json = Json { ignoreUnknownKeys = true }

@Serializable
data class ContentMessage(
    val type: String = "",
    val revision: Long = 0,
    val text: String = "",
    @SerialName("force_popup") val forcePopup: Boolean = false,
    @SerialName("image_mime") val imageMime: String? = null,
    @SerialName("image_base64") val imageBase64: String? = null,
    @SerialName("updated_at") val updatedAt: String? = null,
)

fun parseServerMessage(raw: String): ContentMessage = json.decodeFromString(raw)

fun authPayload(password: String): String = buildJsonObject {
    put("type", "auth")
    put("password", password)
}.toString()

fun redactForLog(payload: String, password: String): String {
    if (password.isEmpty()) return payload
    return payload.replace(password, "***")
}

fun ContentMessage.isPublishedTextOnly(): Boolean =
    revision > 0 && imageBase64.isNullOrBlank() && text.isNotBlank()

fun ContentMessage.shouldShowUnpublishedPlaceholder(): Boolean =
    imageBase64.isNullOrBlank() && text.isBlank()

