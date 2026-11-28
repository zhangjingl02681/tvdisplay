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
