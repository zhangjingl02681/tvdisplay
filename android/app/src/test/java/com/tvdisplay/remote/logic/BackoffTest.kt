package com.tvdisplay.remote.logic

import org.junit.Assert.assertEquals
import org.junit.Test

class BackoffTest {
    @Test
    fun doublesUntilThirtySeconds() {
        assertEquals(1_000L, backoffMillis(0))
        assertEquals(2_000L, backoffMillis(1))
        assertEquals(4_000L, backoffMillis(2))
        assertEquals(8_000L, backoffMillis(3))
        assertEquals(16_000L, backoffMillis(4))
        assertEquals(30_000L, backoffMillis(5))
        assertEquals(30_000L, backoffMillis(8))
    }
}
