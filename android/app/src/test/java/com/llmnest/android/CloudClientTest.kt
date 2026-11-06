package com.llmnest.android

import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.net.UnknownHostException
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference
import java.util.concurrent.atomic.AtomicInteger

class CloudClientTest {
    private fun http(block: (Interceptor.Chain) -> Response) = OkHttpClient.Builder().addInterceptor(block).build()
    private fun response(chain: Interceptor.Chain, code: Int = 200, body: String = "{}", cookie: String? = null): Response =
        Response.Builder().request(chain.request()).protocol(Protocol.HTTP_1_1).code(code).message("test")
            .body(body.toResponseBody("application/json".toMediaType()))
            .apply { cookie?.let { header("Set-Cookie", it) } }.build()

    @Test fun pathsAreExactOriginAndAuthenticationCookieIsPerInstance() {
        val seen = mutableListOf<okhttp3.Request>()
        val network = http { chain ->
            seen.add(chain.request())
            if (chain.request().url.encodedPath == "/api/auth/login")
                response(chain, body = "{\"account\":{\"id\":\"alice\"}}", cookie = "nest_session=private; Secure; HttpOnly; Path=/")
            else response(chain)
        }
        val a = CloudClient("https://HOST.test:443/", network)
        val b = CloudClient("https://host.test", network)
        assertEquals("https://host.test", a.origin)
        assertEquals("alice", a.login("mail", "password").getString("id"))
        a.request("GET", "/me?filter=a%20b")
        b.request("GET", "/me")
        assertEquals("/api/me", seen[1].url.encodedPath)
        assertEquals("a b", seen[1].url.queryParameter("filter"))
        assertEquals("nest_session=private", seen[1].header("Cookie"))
        assertNull(seen[2].header("Cookie"))
        assertEquals(a.origin, seen[0].header("Origin"))
        assertEquals("1", seen[0].header("X-LLMN-Client"))
        assertTrue(seen[0].body!!.isOneShot())
        assertFalse(seen[0].body!!.let { val buffer = okio.Buffer(); it.writeTo(buffer); buffer.readUtf8() }.contains("\"rememberLogin\":true"))
    }

    @Test fun rejectsInsecureAddressesAndEscapingPathsBeforeNetwork() {
        val client = CloudClient("https://host.test", http { error("must not call") })
        for (address in listOf("http://host.test", "https://user:pass@host.test", "https://host.test/api", "https://host.test?secret=yes", "https://host.test#x")) {
            try { CloudClient(address); fail(address) } catch (error: CloudError) { assertEquals("invalid_address", error.code) }
        }
        for (path in listOf("https://evil.test/me", "//evil.test/me", "/../me", "/%2e%2e/me", "/x%2Fy", "/x%5cy", "/me#fragment", "/api/me")) {
            try { client.request("GET", path); fail(path) } catch (error: CloudError) { assertEquals("invalid_request", error.code) }
        }
    }

    @Test fun supportsArraysAndEmptyResponsesWithoutRetryingOrLeakingServerErrors() {
        var requests = 0
        val client = CloudClient("https://host.test", http { chain ->
            requests++
            when (chain.request().url.encodedPath) {
                "/api/list" -> response(chain, body = "[{}]")
                "/api/empty" -> response(chain, code = 204, body = "")
                "/api/error" -> response(chain, code = 409, body = "{\"code\":\"audience_changed\",\"message\":\"private credential\"}")
                else -> throw UnknownHostException("private DNS detail")
            }
        })
        assertTrue(client.exchange("GET", "/list") is JSONArray)
        assertNull(client.request("POST", "/empty"))
        try { client.request("POST", "/error"); fail() } catch (error: CloudError) {
            assertEquals(409, error.status); assertEquals("audience_changed", error.code)
            assertFalse(error.toString().contains("private")); assertNull(error.cause)
        }
        try { client.request("GET", "/dns"); fail() } catch (error: CloudError) {
            assertEquals("dns_error", error.code); assertFalse(error.toString().contains("private"))
        }
        assertEquals(4, requests)
    }

    @Test fun unauthorizedClearsCookieAndRetains401ForCaller() {
        var sentCookie: String? = "unset"
        val client = CloudClient("https://host.test", http { chain ->
            when (chain.request().url.encodedPath) {
                "/api/auth/login" -> response(chain, body = "{\"account\":{\"id\":\"a\"}}", cookie = "nest_session=old; Secure; Path=/")
                "/api/fail" -> response(chain, code = 401, body = "<html>private</html>")
                else -> { sentCookie = chain.request().header("Cookie"); response(chain) }
            }
        })
        client.login("a", "b")
        try { client.request("GET", "/fail"); fail() } catch (error: CloudError) { assertEquals(401, error.status) }
        client.request("GET", "/me")
        assertNull(sentCookie)
    }

    @Test fun lateLoginCannotResurrectClosedOrLoggedOutSession() {
        for (close in listOf(false, true)) {
            val entered = CountDownLatch(1)
            val release = CountDownLatch(1)
            val result = AtomicReference<Throwable>()
            val client = CloudClient("https://host.test", http { chain ->
                entered.countDown(); assertTrue(release.await(5, TimeUnit.SECONDS))
                response(chain, body = "{\"account\":{\"id\":\"a\"}}", cookie = "nest_session=late; Secure; Path=/")
            })
            val thread = Thread {
                try { client.login("a", "b"); result.set(AssertionError("late login succeeded")) }
                catch (error: Throwable) { result.set(error) }
            }
            thread.start(); assertTrue(entered.await(5, TimeUnit.SECONDS))
            if (close) client.close() else client.logout()
            release.countDown(); thread.join(5000)
            assertTrue(result.get() is CloudError)
            assertEquals("session_changed", (result.get() as CloudError).code)
        }
    }

    @Test fun eventNotificationsIgnorePayloadAndEncodeCursorWithoutTreatingIdsAsState() {
        val changes = AtomicInteger()
        val ended = CountDownLatch(1)
        val failure = AtomicReference<Throwable?>()
        var cursor: String? = null
        val client = CloudClient("https://host.test", http { chain ->
            cursor = chain.request().url.queryParameter("after")
            response(chain, body = "id: not-a-cursor\nevent: change\ndata: private content ignored\n\nevent: keepalive\ndata: ignored\n\n")
                .newBuilder().header("Content-Type", "text/event-stream; charset=utf-8").build()
        })
        val subscription = client.events("cursor&other=bad", { changes.incrementAndGet() }, { failure.set(it); ended.countDown() })
        assertTrue(ended.await(5, TimeUnit.SECONDS))
        assertEquals("cursor&other=bad", cursor)
        assertEquals(1, changes.get())
        assertNull(failure.get())
        subscription.close(); client.close()
    }

    @Test fun redirectIsAnErrorAndNeverBecomesAnotherRequest() {
        var requests = 0
        val client = CloudClient("https://host.test", http { chain ->
            requests++; response(chain, code = 302, body = "private redirect body")
                .newBuilder().header("Location", "https://evil.test/").build()
        })
        try { client.request("GET", "/me"); fail() } catch (error: CloudError) {
            assertEquals(302, error.status); assertFalse(error.toString().contains("private"))
        }
        assertEquals(1, requests)
    }
}
