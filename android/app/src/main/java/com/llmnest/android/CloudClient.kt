package com.llmnest.android

import okhttp3.Call
import okhttp3.Callback
import okhttp3.Cookie
import okhttp3.CookieJar
import okhttp3.HttpUrl
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody
import okhttp3.RequestBody.Companion.toRequestBody
import okhttp3.Response
import org.json.JSONArray
import org.json.JSONObject
import org.json.JSONTokener
import java.io.Closeable
import java.io.IOException
import java.net.ConnectException
import java.net.SocketTimeoutException
import java.net.URI
import java.net.UnknownHostException
import java.util.concurrent.TimeUnit
import javax.net.ssl.SSLException

class CloudError(val status: Int, message: String, val code: String? = null) : IOException(message)

/** One server and one memory-only login. All synchronous operations must run off the UI thread. */
class CloudClient internal constructor(address: String, private val http: OkHttpClient) : Closeable {
    constructor(address: String) : this(address, newHttpClient())

    val origin: String = normalizeOrigin(address)
    private val lock = Any()
    private var generation = 0L
    private var cookie: String? = null
    private var closed = false
    private val calls = mutableSetOf<Call>()
    private data class Context(val generation: Long, val cookie: String?)

    fun login(email: String, password: String): JSONObject {
        val response = request("POST", "/auth/login", JSONObject()
            .put("email", email).put("password", password).put("rememberLogin", false))
        return response?.optJSONObject("account")
            ?: throw CloudError(0, "登录响应无效，请重新登录", "invalid_response")
    }

    fun request(method: String, path: String, body: JSONObject? = null): JSONObject? {
        val value = exchange(method, path, body)
        if (value == null || value is JSONObject) return value as JSONObject?
        throw CloudError(0, "服务器响应格式不正确", "invalid_response")
    }

    fun exchange(method: String, path: String, body: JSONObject? = null): Any? {
        val url = target(path)
        if (url.encodedPath == "/api/auth/logout") {
            if (method != "POST") throw CloudError(0, "请求方法不正确", "invalid_request")
            logout()
            return null
        }
        val authChange = url.encodedPath == "/api/auth/login" || url.encodedPath == "/api/auth/verify"
        val context = synchronized(lock) {
            ensureOpen()
            if (authChange) invalidate()
            Context(generation, cookie)
        }
        val call = register(requestBuilder(method, url, context, body).build(), context)
        try {
            call.execute().use { response ->
                val value = parseResponse(response, context)
                synchronized(lock) {
                    checkCurrent(context)
                    if (authChange) {
                        if (value !is JSONObject || value.optJSONObject("account") == null) {
                            throw CloudError(0, "登录响应无效，请重新登录", "invalid_response")
                        }
                        cookie = response.headers("Set-Cookie").asSequence()
                            .mapNotNull { Cookie.parse(url, it) }
                            .firstOrNull { it.name == "nest_session" && it.matches(url) && it.value.isNotEmpty() && it.expiresAt > System.currentTimeMillis() }
                            ?.let { "${it.name}=${it.value}" }
                            ?: throw CloudError(0, "服务器未建立登录会话，请重新登录", "invalid_response")
                    }
                    return value
                }
            }
        } catch (error: IOException) {
            if (error is CloudError) throw error
            synchronized(lock) { checkCurrent(context) }
            throw safeNetworkError(error)
        } finally {
            synchronized(lock) { calls.remove(call) }
        }
    }

    /** Local invalidation happens before the network request, even if the server is unreachable. */
    fun logout() {
        val context = synchronized(lock) {
            ensureOpen()
            val oldCookie = cookie
            invalidate()
            Context(generation, oldCookie)
        }
        if (context.cookie == null) return
        val call = register(requestBuilder("POST", target("/auth/logout"), context, null).build(), context)
        try {
            call.execute().use { parseResponse(it, context) }
        } catch (error: IOException) {
            throw safeNetworkError(error)
        } finally {
            synchronized(lock) { calls.remove(call) }
        }
    }

    /** Only notifications are consumed; callers obtain authoritative state and cursor via snapshots. */
    fun events(after: String, onChange: () -> Unit, onEnd: (Throwable?) -> Unit): Closeable {
        val context = synchronized(lock) { ensureOpen(); Context(generation, cookie) }
        val url = target("/chat/events").newBuilder().addQueryParameter("after", after).build()
        val request = requestBuilder("GET", url, context, null).header("Accept", "text/event-stream").build()
        val streamHttp = http.newBuilder().readTimeout(0, TimeUnit.MILLISECONDS).callTimeout(0, TimeUnit.MILLISECONDS).build()
        val call = streamHttp.newCall(request)
        synchronized(lock) { checkCurrent(context); calls.add(call) }
        val subscription = object : Closeable {
            @Volatile var cancelled = false
            override fun close() { cancelled = true; call.cancel(); synchronized(lock) { calls.remove(call) } }
        }
        call.enqueue(object : Callback {
            override fun onFailure(call: Call, error: IOException) {
                synchronized(lock) { calls.remove(call) }
                if (!subscription.cancelled && isCurrent(context)) onEnd(safeNetworkError(error))
            }

            override fun onResponse(call: Call, response: Response) {
                var failure: Throwable? = null
                try {
                    response.use {
                        checkStatus(response, context)
                        if (response.header("Content-Type")?.substringBefore(';')?.trim() != "text/event-stream") {
                            throw CloudError(0, "服务器事件流格式不正确", "invalid_response")
                        }
                        val source = response.body?.source()
                            ?: throw CloudError(0, "服务器事件流为空", "invalid_response")
                        var event = ""
                        while (!subscription.cancelled && isCurrent(context)) {
                            if (source.exhausted()) break
                            val line = source.readUtf8LineStrict(64 * 1024)
                            if (line.isEmpty()) {
                                if (event == "change" && isCurrent(context)) onChange()
                                event = ""
                            } else if (line.startsWith("event:")) event = line.substring(6).trim()
                        }
                    }
                } catch (error: Exception) {
                    failure = if (error is IOException) safeNetworkError(error)
                    else CloudError(0, "无法处理服务器更新，请刷新会话", "invalid_response")
                } finally {
                    synchronized(lock) { calls.remove(call) }
                    val unauthorized = failure is CloudError && failure.status == 401
                    if (!subscription.cancelled && (isCurrent(context) || unauthorized)) onEnd(failure)
                }
            }
        })
        return subscription
    }

    override fun close() {
        val pending = synchronized(lock) {
            if (closed) return
            closed = true
            generation++
            cookie = null
            calls.toList().also { calls.clear() }
        }
        // Activity teardown runs on main; TLS socket close may send close_notify packets.
        // Invalidate synchronously, then release sockets without doing network I/O on that thread.
        Thread({
            try {
                pending.forEach { it.cancel() }
                http.connectionPool.evictAll()
            } finally {
                http.dispatcher.executorService.shutdown()
            }
        }, "llmn-cloud-close").apply { isDaemon = true }.start()
    }

    private fun requestBuilder(method: String, url: HttpUrl, context: Context, body: JSONObject?): Request.Builder {
        if (method !in setOf("GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS") ||
            (body != null && method in setOf("GET", "HEAD"))) {
            throw CloudError(0, "请求方法不正确", "invalid_request")
        }
        val content = body?.toString()?.toRequestBody(JSON_MEDIA)
            ?: if (method in setOf("POST", "PUT", "PATCH", "DELETE")) "".toRequestBody(JSON_MEDIA) else null
        // A follow-up must never replay a mutating request, including HTTP 503 Retry-After: 0.
        val payload = content?.let { object : RequestBody() {
            override fun contentType() = it.contentType()
            override fun contentLength() = it.contentLength()
            override fun writeTo(sink: okio.BufferedSink) = it.writeTo(sink)
            override fun isOneShot() = true
        } }
        return Request.Builder().url(url).header("Accept", "application/json")
            .header("Origin", origin).header("X-LLMN-Client", "1")
            .apply { context.cookie?.let { header("Cookie", it) } }.method(method, payload)
    }

    private fun register(request: Request, context: Context): Call = synchronized(lock) {
        checkCurrent(context)
        http.newCall(request).also { calls.add(it) }
    }

    private fun parseResponse(response: Response, context: Context): Any? {
        checkStatus(response, context)
        if (response.code == 204 || response.code == 205) return null
        val body = response.body ?: return null
        if (body.contentLength() > MAX_JSON_BYTES) throw CloudError(0, "服务器响应过大", "invalid_response")
        val source = body.source()
        source.request(MAX_JSON_BYTES + 1)
        val bytes = source.buffer.readByteArray()
        if (bytes.size > MAX_JSON_BYTES) throw CloudError(0, "服务器响应过大", "invalid_response")
        if (bytes.isEmpty()) return null
        try {
            val parser = JSONTokener(bytes.toString(Charsets.UTF_8))
            val value = parser.nextValue()
            if ((value !is JSONObject && value !is JSONArray) || parser.nextClean() != '\u0000') throw Exception()
            return value
        } catch (_: Exception) {
            throw CloudError(0, "服务器响应格式不正确", "invalid_response")
        }
    }

    private fun checkStatus(response: Response, context: Context) {
        synchronized(lock) { checkCurrent(context) }
        if (response.isSuccessful) return
        var code: String? = null
        // Only a bounded machine code is retained. Server messages/bodies never enter exceptions or logs.
        try {
            val body = response.peekBody(16 * 1024).string()
            code = JSONObject(body).optString("code").takeIf { it.matches(Regex("[a-z][a-z0-9_]{0,63}")) }
        } catch (_: Exception) { /* Non-JSON error pages use the status-specific safe message. */ }
        if (response.code == 401 && context.cookie != null) {
            synchronized(lock) { if (generation == context.generation) invalidate() }
        }
        throw CloudError(response.code, when (response.code) {
            401 -> "登录已失效或邮箱密码不正确，请重新登录"
            403 -> "服务器拒绝此操作，请检查账号权限和服务器地址"
            404 -> "内容不存在或当前账号无权访问"
            409 -> "服务器状态已变化，请刷新后检查并重试"
            413 -> "请求内容超过服务器限制"
            429 -> "操作频繁，请稍后手动重试"
            in 300..399 -> "服务器返回了重定向；请填写最终的 HTTPS 服务器地址"
            in 500..599 -> "服务器暂时无法完成操作，请稍后手动重试"
            else -> "服务器未接受此请求，请检查输入后重试"
        }, code)
    }

    private fun target(path: String): HttpUrl {
        try {
            val uri = URI(path)
            val segments = uri.path.orEmpty().split('/')
            if (!path.startsWith('/') || path.startsWith("//") || uri.isAbsolute || uri.rawAuthority != null ||
                uri.rawFragment != null || path.contains('\\') || uri.path.contains('\\') ||
                segments.any { it == "." || it == ".." } ||
                Regex("%2f|%5c", RegexOption.IGNORE_CASE).containsMatchIn(uri.rawPath) ||
                uri.path == "/api" || uri.path.startsWith("/api/")) throw Exception()
            val result = (origin + "/api" + path).toHttpUrl()
            if (!result.encodedPath.startsWith("/api/")) throw Exception()
            return result
        } catch (_: Exception) {
            throw CloudError(0, "请求路径无效", "invalid_request")
        }
    }

    private fun ensureOpen() { if (closed) throw CloudError(0, "服务器连接已关闭", "session_changed") }
    private fun checkCurrent(context: Context) {
        ensureOpen()
        if (context.generation != generation) throw CloudError(0, "登录状态已变化，请重新打开会话", "session_changed")
    }
    private fun isCurrent(context: Context): Boolean = synchronized(lock) { !closed && generation == context.generation }
    private fun invalidate() { generation++; cookie = null; calls.forEach { it.cancel() }; calls.clear() }

    companion object {
        private const val MAX_JSON_BYTES = 16L * 1024 * 1024
        private val JSON_MEDIA = "application/json; charset=utf-8".toMediaType()
        internal fun newHttpClient() = OkHttpClient.Builder()
            .cookieJar(CookieJar.NO_COOKIES).followRedirects(false).followSslRedirects(false)
            .retryOnConnectionFailure(false).connectTimeout(15, TimeUnit.SECONDS)
            // OkHttp's 503 follow-up is independent of retryOnConnectionFailure, even for GET.
            .addNetworkInterceptor { chain ->
                val response = chain.proceed(chain.request())
                if (response.code == 503) response.newBuilder().header("Retry-After", "1").build() else response
            }
            .readTimeout(45, TimeUnit.SECONDS).callTimeout(60, TimeUnit.SECONDS).build()

        private fun normalizeOrigin(address: String): String {
            try {
                val uri = URI(address.trim())
                if (!uri.scheme.equals("https", ignoreCase = true) || uri.rawUserInfo != null || uri.rawQuery != null || uri.rawFragment != null ||
                    (uri.rawPath != "" && uri.rawPath != "/") || uri.host.isNullOrBlank() || uri.port == 0) throw Exception()
                val url = address.trim().toHttpUrl()
                return url.newBuilder().encodedPath("/").build().toString().removeSuffix("/")
            } catch (_: Exception) {
                throw CloudError(0, "请输入仅含主机和端口的 HTTPS 服务器地址", "invalid_address")
            }
        }

        private fun safeNetworkError(error: IOException): CloudError = when (error) {
            is CloudError -> error
            is SSLException -> CloudError(0, "TLS 证书验证失败，请检查证书、服务器域名和设备时间", "tls_error")
            is UnknownHostException -> CloudError(0, "无法解析服务器域名，请检查地址和网络 DNS", "dns_error")
            is SocketTimeoutException -> CloudError(0, "连接超时；发送结果可能未知，请仅手动重试原请求", "timeout")
            is ConnectException -> CloudError(0, "无法连接服务器，请检查地址、端口和网络", "connection_error")
            else -> CloudError(0, "连接中断；发送结果可能未知，请仅手动重试原请求", "connection_error")
        }
    }
}
