package com.llmnest.android

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CryptoRoutesTest {
    @Test fun onlySharedSdkAndActivationRoutesCrossTheBridge() {
        listOf("/crypto/backup", "/crypto/backup/keys", "/crypto/recovery", "/chat/init", "/crypto/sync?deviceId=a&after=b%2Fc").forEach {
            assertTrue(it, CryptoRoutes.allowed("GET", it))
        }
        assertTrue(CryptoRoutes.allowed("POST", "/crypto/requests"))
        assertTrue(CryptoRoutes.allowed("POST", "/crypto/devices"))
        assertTrue(CryptoRoutes.allowed("PUT", "/crypto/recovery"))
        assertTrue(CryptoRoutes.allowed("POST", "/chat/conversations/abc-123/encryption"))
        listOf("https://other.invalid/crypto/backup", "/crypto/backup?redirect=x", "/crypto/backup#fragment", "/crypto/../auth/login", "/crypto/%2e%2e/auth/login", "/crypto\\backup", "/auth/logout", "/me").forEach {
            assertFalse(it, CryptoRoutes.allowed("GET", it))
        }
        assertFalse(CryptoRoutes.allowed("POST", "/chat/conversations/a/messages"))
        assertFalse(CryptoRoutes.allowed("DELETE", "/crypto/recovery"))
    }
}
