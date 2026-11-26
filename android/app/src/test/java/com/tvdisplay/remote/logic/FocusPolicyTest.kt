package com.tvdisplay.remote.logic

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class FocusPolicyTest {
    @Test
    fun bringsForwardOnlyForNewContentWhenEnabled() {
        assertTrue(shouldBringToForeground(forcePopup = true, incomingRevision = 2, lastShownRevision = 1))
        assertFalse(shouldBringToForeground(forcePopup = false, incomingRevision = 2, lastShownRevision = 1))
        assertFalse(shouldBringToForeground(forcePopup = true, incomingRevision = 2, lastShownRevision = 2))
        assertFalse(shouldBringToForeground(forcePopup = true, incomingRevision = 1, lastShownRevision = 2))
        assertTrue(shouldBringToForeground(forcePopup = true, incomingRevision = 1, lastShownRevision = 0))
    }
}
