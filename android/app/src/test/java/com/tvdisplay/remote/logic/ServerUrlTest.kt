package com.tvdisplay.remote.logic

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Test

class ServerUrlTest {
    @Test
    fun acceptsBracketedIpv6() {
        val parsed = parseServerUrl("http://[2001:db8::1]:8080")
        assertEquals("http://[2001:db8::1]:8080", parsed.httpBase)
        assertEquals("ws://[2001:db8::1]:8080/ws/tv", parsed.webSocketUrl)
        assertFalse(parsed.webSocketUrl.contains("?"))
    }

    @Test
    fun trimsSlashAndUsesWss() {
        val parsed = parseServerUrl("https://tv.example.com/")
        assertEquals("https://tv.example.com", parsed.httpBase)
        assertEquals("wss://tv.example.com/ws/tv", parsed.webSocketUrl)
    }

    @Test
    fun acceptsIpv4WithPort() {
        val parsed = parseServerUrl("  http://192.168.1.8:8080/admin  ")
        assertEquals("http://192.168.1.8:8080", parsed.httpBase)
        assertEquals("ws://192.168.1.8:8080/ws/tv", parsed.webSocketUrl)
    }

    @Test
    fun dropsPasswordQuery() {
        val parsed = parseServerUrl("http://[2001:db8::1]:8080/path?password=secret-value")
        assertFalse(parsed.httpBase.contains("secret-value"))
        assertFalse(parsed.webSocketUrl.contains("secret-value"))
        assertFalse(parsed.webSocketUrl.contains("?"))
    }

    @Test
    fun rejectsEmptyFtpUserinfoAndBareIpv6() {
        assertThrows(IllegalArgumentException::class.java) { parseServerUrl("   ") }
        assertThrows(IllegalArgumentException::class.java) { parseServerUrl("ftp://example.com") }
        assertThrows(IllegalArgumentException::class.java) {
            parseServerUrl("http://user:secret@[2001:db8::1]:8080")
        }
        assertThrows(IllegalArgumentException::class.java) {
            parseServerUrl("http://2001:db8::1:8080")
        }
    }
}
