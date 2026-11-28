package com.tvdisplay.remote.logic

import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ContentMessageTest {
    @Test
    fun parsesContentFields() {
        val message = parseServerMessage(
            """
            {"type":"content","revision":3,"text":"你好","force_popup":true,
             "image_mime":"image/jpeg","image_base64":"YQ==","updated_at":"2026-11-26T00:00:00+00:00","extra":1}
            """.trimIndent(),
        )
        assertEquals("content", message.type)
        assertEquals(3L, message.revision)
        assertEquals("你好", message.text)
        assertTrue(message.forcePopup)
        assertEquals("image/jpeg", message.imageMime)
        assertEquals("YQ==", message.imageBase64)
    }

    @Test
    fun emptyPictureStaysNull() {
        val message = parseServerMessage(
            """{"type":"content","revision":0,"text":"","force_popup":false,"image_mime":null,"image_base64":null,"updated_at":null}""",
        )
        assertNull(message.imageBase64)
        assertFalse(message.forcePopup)
        assertEquals(0L, message.revision)
    }

    @Test
    fun authPayloadRedactsPassword() {
        val secret = "p@ss-not-real"
        val raw = authPayload(secret)
        val parsed = Json.parseToJsonElement(raw).jsonObject
        assertEquals("auth", parsed.getValue("type").jsonPrimitive.content)
        assertEquals(secret, parsed.getValue("password").jsonPrimitive.content)
        val redacted = redactForLog(raw, secret)
        assertFalse(redacted.contains(secret))
        assertTrue(redacted.contains("***"))
    }

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
}
