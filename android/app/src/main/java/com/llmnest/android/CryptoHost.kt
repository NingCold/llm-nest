package com.llmnest.android

import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
import android.app.Dialog
import android.content.Intent
import android.graphics.Color
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.view.View
import android.view.WindowManager
import android.webkit.JavascriptInterface
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebResourceError
import android.webkit.RenderProcessGoneDetail
import android.webkit.WebView
import android.webkit.WebViewClient
import org.json.JSONObject
import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

/** Local-only Matrix WASM host. It never receives a cookie, login password or arbitrary URL. */
@SuppressLint("SetJavaScriptEnabled")
class CryptoHost(private val activity: Activity, private val cloud: CloudClient, private val accountId: String) {
    private val main = Handler(Looper.getMainLooper())
    private val network = Executors.newSingleThreadExecutor()
    private val closed = AtomicBoolean(false)
    private val pending = mutableMapOf<String, (Result<JSONObject>) -> Unit>()
    private var sequence = 0L
    private var ready = false
    private var failure: Throwable? = null
    private var rendererGone = false
    private val waiting = mutableListOf<() -> Unit>()
    private var dialog: Dialog? = null
    private val alerts = mutableListOf<AlertDialog>()
    private var recovery: JSONObject? = null
    private var choosing = false
    private val origin = cloud.origin
    var onUnauthorized: (() -> Unit)? = null
    private val web = WebView(activity).apply {
        setBackgroundColor(Color.TRANSPARENT)
        importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO_EXCLUDE_DESCENDANTS
        settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            blockNetworkLoads = true
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
            mixedContentMode = android.webkit.WebSettings.MIXED_CONTENT_NEVER_ALLOW
        }
        addJavascriptInterface(Bridge(), "NativeCrypto")
        webViewClient = object : WebViewClient() {
            override fun onRenderProcessGone(view: WebView, detail: RenderProcessGoneDetail): Boolean {
                rendererGone = true
                (view.parent as? ViewGroup)?.removeView(view)
                view.destroy()
                fail(IllegalStateException("本机加密组件已停止，请更新 Android System WebView 后重新打开应用；原加密资料会保留"))
                return true
            }
            override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                if (request.isForMainFrame) fail(IllegalStateException("本机加密组件加载失败，请更新 Android System WebView 后重试"))
            }
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest) = true
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse {
                val uri = request.url
                val name = uri.path?.removePrefix("/crypto/") ?: ""
                if (request.method != "GET" || uri.scheme != "https" || uri.host != LOCAL_HOST || uri.port != -1 || uri.query != null || !uri.path.orEmpty().startsWith("/crypto/") || !name.matches(Regex("[A-Za-z0-9_.-]+"))) return denied()
                return try {
                    val mime = when (name.substringAfterLast('.')) {
                        "html" -> "text/html"; "js" -> "text/javascript"; "css" -> "text/css"; "wasm" -> "application/wasm"; "txt" -> "text/plain"
                        else -> return denied()
                    }
                    WebResourceResponse(mime, if (mime == "application/wasm") null else "UTF-8", 200, "OK", mapOf("Cache-Control" to "no-store", "X-Content-Type-Options" to "nosniff"), activity.assets.open("crypto/$name"))
                } catch (_: java.io.IOException) { denied() }
            }
        }
        loadUrl("https://$LOCAL_HOST/crypto/index.html")
    }

    init { main.postDelayed({ if (!ready && !closed.get()) fail(IllegalStateException("本机加密组件未能启动，请更新 Android System WebView 后重试")) }, 20_000) }

    private fun fail(error: Throwable) {
        failure = error
        waiting.clear()
        val callbacks = pending.values.toList(); pending.clear()
        callbacks.forEach { it(Result.failure(error)) }
        if (dialog != null) showSecure(AlertDialog.Builder(activity).setMessage(error.message).setPositiveButton("确定", null).create())
    }

    private fun showSecure(alert: AlertDialog) {
        alerts.add(alert)
        alert.setOnDismissListener { alerts.remove(alert) }
        alert.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        alert.show()
    }

    private fun denied() = WebResourceResponse("text/plain", "UTF-8", 403, "Blocked", emptyMap(), ByteArrayInputStream(ByteArray(0)))
    private fun js(code: String) { if (!closed.get() && !rendererGone && failure == null) web.evaluateJavascript(code, null) }
    private fun notice(message: String) { js("window.cryptoNotice(${JSONObject.quote(message)})") }

    /** Main-thread, once-settled API. Responses are direct snapshots/pages/commands, not wrappers. */
    fun call(op: String, args: JSONObject, done: (Result<JSONObject>) -> Unit) {
        check(Looper.myLooper() == Looper.getMainLooper())
        if (closed.get()) { done(Result.failure(IllegalStateException("加密会话已关闭"))); return }
        failure?.let { done(Result.failure(it)); return }
        val id = (++sequence).toString()
        pending[id] = done
        main.postDelayed({ pending.remove(id)?.let { callback ->
            js("window.cryptoCall('', 'lock', {})")
            callback(Result.failure(IllegalStateException("加密操作超时，已锁定本机资料；请重新解锁后重试")))
        } }, 120_000)
        val work = { js("window.cryptoCall(${JSONObject.quote(id)},${JSONObject.quote(op)},$args)") }
        if (ready) work() else waiting.add(work)
    }

    /** Device UI stays in a trusted, bundled view; the chat/navigation remain native. */
    fun showDevices(peerAccountId: String? = null, onChanged: () -> Unit = {}) {
        if (closed.get()) return
        failure?.let { showSecure(AlertDialog.Builder(activity).setMessage(it.message).setPositiveButton("确定", null).create()); return }
        if (dialog != null) return
        val window = Dialog(activity)
        window.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        dialog = window
        window.setTitle("可信设备与恢复")
        window.setContentView(web)
        window.setOnDismissListener {
            (web.parent as? ViewGroup)?.removeView(web)
            dialog = null
            if (!closed.get()) onChanged()
        }
        window.show()
        window.window?.setLayout(ViewGroup.LayoutParams.MATCH_PARENT, ViewGroup.LayoutParams.MATCH_PARENT)
        val showPeer = { js("window.cryptoSetPeer(${JSONObject.quote(peerAccountId.orEmpty())})") }
        if (ready) showPeer() else waiting.add(showPeer)
    }

    /** Dismissing setup reports the actual current lock/trust state; it never establishes trust itself. */
    fun unlock(done: (Result<JSONObject>) -> Unit) = showDevices { call("state", JSONObject(), done) }

    fun isPickingDocument(): Boolean = choosing

    fun close() {
        if (!closed.compareAndSet(false, true)) return
        // Queue the SDK close before removing the bridge and destroying its execution context.
        if (!rendererGone) web.evaluateJavascript("window.cryptoCall && window.cryptoCall('', 'lock', {})") {
            web.removeJavascriptInterface("NativeCrypto")
            web.stopLoading(); web.destroy()
        }
        dialog?.dismiss(); dialog = null
        alerts.toList().forEach { it.dismiss() }
        recovery = null; choosing = false
        waiting.clear()
        val callbacks = pending.values.toList(); pending.clear()
        callbacks.forEach { it(Result.failure(IllegalStateException("加密会话已关闭"))) }
        network.shutdownNow()
    }

    private inner class Bridge {
        @JavascriptInterface fun done() { main.post { if (!closed.get() && !choosing) dialog?.dismiss() } }
        @JavascriptInterface fun context() = JSONObject().put("server", origin).put("account", accountId).toString()
        @JavascriptInterface fun ready() { main.post { if (!closed.get() && failure == null) { ready = true; waiting.toList().also { waiting.clear() }.forEach { it() } } } }
        @JavascriptInterface fun result(id: String, result: String) { main.post {
            if (closed.get()) return@post
            val done = pending.remove(id) ?: return@post
            val response = runCatching {
                val value = JSONObject(result)
                if (value.has("error")) {
                    val error = value.getJSONObject("error")
                    throw if (error.has("status")) CloudError(error.getInt("status"), error.getString("message"), error.optString("code").ifEmpty { null }) else IllegalStateException(error.getString("message"))
                }
                value.getJSONObject("value")
            }
            done(response)
        } }
        @JavascriptInterface fun request(id: String, method: String, path: String, body: String) {
            if (closed.get()) return
            network.execute {
                val response = try {
                    check(!closed.get() && cloud.origin == origin) { "账号或服务器已切换" }
                    require(CryptoRoutes.allowed(method, path) && body.length <= 16 * 1024 * 1024) { "加密桥请求超出允许范围" }
                    val value = cloud.exchange(method, path, if (body.isEmpty()) null else JSONObject(body))
                    check(!closed.get()) { "加密会话已关闭" }
                    JSONObject().put("value", value ?: JSONObject.NULL)
                } catch (error: Exception) {
                    val detail = JSONObject().put("message", error.message ?: "加密请求失败")
                    if (error is CloudError) detail.put("status", error.status).put("code", error.code)
                    JSONObject().put("error", detail)
                }
                main.post { if (!closed.get()) {
                    js("window.cryptoReply(${JSONObject.quote(id)},$response)")
                    if (response.optJSONObject("error")?.optInt("status") == 401) onUnauthorized?.invoke()
                } }
            }
        }
        @JavascriptInterface fun recovery(action: String) { main.post {
            if (closed.get() || dialog == null || choosing) return@post
            when (action) {
                "export" -> call("createRecovery", JSONObject()) { result -> result.fold({ file ->
                    recovery = file; choosing = true
                    try { activity.startActivityForResult(Intent(Intent.ACTION_CREATE_DOCUMENT).apply { addCategory(Intent.CATEGORY_OPENABLE); type = "application/json"; putExtra(Intent.EXTRA_TITLE, "llmn-recovery.json") }, EXPORT) }
                    catch (error: Exception) { choosing = false; recovery = null; notice(error.message ?: "无法打开文件保存器") }
                }, { notice(it.message ?: "无法生成恢复文件") }) }
                "import" -> {
                    choosing = true
                    try { activity.startActivityForResult(Intent(Intent.ACTION_OPEN_DOCUMENT).apply { addCategory(Intent.CATEGORY_OPENABLE); type = "application/json" }, IMPORT) }
                    catch (error: Exception) { choosing = false; notice(error.message ?: "无法打开文件选择器") }
                }
                "new-device" -> showSecure(AlertDialog.Builder(activity).setTitle("建立另一独立设备？").setMessage("原本机加密库会保留。新设备必须使用恢复文件或已有可信设备重新授权，不会重置账号身份。").setNegativeButton("取消", null).setPositiveButton("继续") { _, _ -> newDevice() }.create())
            }
        } }
    }

    private fun newDevice() {
        val input = android.widget.EditText(activity).apply { inputType = android.text.InputType.TYPE_CLASS_TEXT or android.text.InputType.TYPE_TEXT_VARIATION_PASSWORD; hint = "至少 8 位的独立本机口令"; importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO }
        val confirmation = AlertDialog.Builder(activity).setTitle("新设备的本机口令").setView(input).setNegativeButton("取消", null).setPositiveButton("确认建立", null).create()
        confirmation.window?.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        confirmation.setOnShowListener { confirmation.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener {
            val value = input.text.toString(); input.text.clear()
            if (value.length < 8) { input.error = "请输入至少 8 位口令"; return@setOnClickListener }
            confirmation.dismiss()
            call("unlock", JSONObject().put("passphrase", value).put("create", "new-device")) { result -> result.fold({ js("window.cryptoRefresh()") }, { notice(it.message ?: "无法建立设备") }) }
        } }
        showSecure(confirmation)
    }

    /** Forward Activity.onActivityResult here before other request-code handlers. */
    fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        if (requestCode != EXPORT && requestCode != IMPORT) return false
        choosing = false
        if (closed.get()) return true
        val uri = data?.data
        if (resultCode != Activity.RESULT_OK || uri == null) { recovery = null; notice("已取消文件操作，恢复状态未确认"); return true }
        if (requestCode == EXPORT) {
            val file = recovery ?: return true
            recovery = null
            network.execute {
                val result = runCatching {
                    activity.contentResolver.openOutputStream(uri, "wt")?.use { it.write(file.toString(2).toByteArray(Charsets.UTF_8)) } ?: error("文件无法写入")
                }
                main.post { if (!closed.get()) result.fold({
                    showSecure(AlertDialog.Builder(activity).setTitle("确认恢复文件已保存").setMessage("文件已写入。请确认它已保存在安全位置，理解平台无法代为恢复。只有确认后才登记这份恢复材料。").setNegativeButton("尚未确认", null).setPositiveButton("我已安全保存") { _, _ ->
                        call("commitRecovery", JSONObject().put("file", file)) { committed -> committed.fold({ js("window.cryptoRefresh()") }, { notice(it.message ?: "恢复文件确认失败") }) }
                    }.create())
                }, { notice(it.message ?: "恢复文件保存失败") }) }
            }
        } else {
            network.execute {
                val result = runCatching {
                    val bytes = activity.contentResolver.openInputStream(uri)?.use { stream ->
                        val output = ByteArrayOutputStream()
                        val buffer = ByteArray(4096)
                        while (output.size() <= MAX_RECOVERY_BYTES) {
                            val count = stream.read(buffer, 0, minOf(buffer.size, MAX_RECOVERY_BYTES + 1 - output.size()))
                            if (count < 0) break
                            output.write(buffer, 0, count)
                        }
                        output.toByteArray()
                    } ?: error("无法读取恢复文件")
                    require(bytes.size <= MAX_RECOVERY_BYTES) { "恢复文件过大" }
                    JSONObject(String(bytes, Charsets.UTF_8)).also { bytes.fill(0) }
                }
                main.post { if (!closed.get()) result.fold({ file ->
                    call("restoreRecovery", JSONObject().put("file", file)) { restored -> restored.fold({ js("window.cryptoRefresh()") }, { notice(it.message ?: "恢复失败") }) }
                }, { notice(it.message ?: "恢复文件格式无效") }) }
            }
        }
        return true
    }

    companion object {
        private const val LOCAL_HOST = "crypto.llmnest.invalid"
        private const val EXPORT = 47121
        private const val IMPORT = 47122
        private const val MAX_RECOVERY_BYTES = 64 * 1024
    }
}

/** Only routes needed by the shared SDK and explicit encryption activation may cross this bridge. */
internal object CryptoRoutes {
    fun allowed(method: String, path: String): Boolean {
        if (path.contains('#') || path.contains('\\') || path.contains('%')) {
            // Percent encoding is permitted only in sync query values, never in route segments.
            if (!path.startsWith("/crypto/sync?") || path.substringBefore('?').contains('%') || path.contains('#') || path.contains('\\')) return false
        }
        val route = path.substringBefore('?')
        return when (method) {
            "GET" -> route in setOf("/crypto/sync", "/crypto/backup", "/crypto/backup/keys", "/crypto/recovery", "/chat/init") && (route == "/crypto/sync" || !path.contains('?'))
            "POST" -> !path.contains('?') && (route in setOf("/crypto/requests", "/crypto/devices") || route.matches(Regex("/chat/conversations/[A-Za-z0-9_-]+/encryption")))
            "PUT" -> path in setOf("/crypto/backup", "/crypto/recovery")
            else -> false
        }
    }
}
