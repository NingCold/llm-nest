package com.llmnest.android

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.atomic.AtomicReference

/** A real native client flow against explicitly supplied, disposable HTTPS fixture accounts. */
@RunWith(AndroidJUnit4::class)
class AndroidChatUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: Activity
    private fun <T> main(work: () -> T): T {
        val result = AtomicReference<Result<T>>()
        instrumentation.runOnMainSync { result.set(runCatching(work)) }
        return result.get().getOrThrow()
    }
    private fun descendants(view: View): Sequence<View> = sequence {
        yield(view)
        if (view is ViewGroup) for (i in 0 until view.childCount) yieldAll(descendants(view.getChildAt(i)))
    }
    private fun root(): View = activity.findViewById<ViewGroup>(android.R.id.content).getChildAt(0)
    private fun visible(label: String): Boolean = main {
        descendants(root()).filterIsInstance<TextView>().any { it !is EditText && it.text.toString() == label && it.isShown }
    }
    private fun waitFor(label: String, timeout: Long = 90_000, predicate: () -> Boolean) {
        val until = System.currentTimeMillis() + timeout
        while (System.currentTimeMillis() < until) {
            if (predicate()) return
            Thread.sleep(200)
        }
        fail("Timed out waiting for $label")
    }
    private fun edit(description: String, value: String) = main {
        descendants(root()).filterIsInstance<EditText>().single { it.contentDescription?.toString() == description }.apply {
            assertTrue("Input must be enabled", isEnabled)
            setText(value)
        }
    }
    private fun tap(label: String) = main {
        var target: View = descendants(root()).filterIsInstance<TextView>().first {
            it !is EditText && (it.text.toString() == label || it.contentDescription?.toString() == label) && it.isShown
        }
        while (!target.isClickable) target = target.parent as? View ?: error("No clickable ancestor for $label")
        assertTrue("Control $label is enabled", target.isEnabled)
        assertTrue("Click $label", target.performClick())
    }
    private fun capture(name: String) = main {
        val view = root()
        assertTrue("Phone width must be usable", view.width >= 320 * activity.resources.displayMetrics.density)
        assertFalse("Chat screens are native, not a WebView", descendants(view).any { it is android.webkit.WebView })
        val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
        view.draw(Canvas(bitmap))
        File(instrumentation.targetContext.cacheDir, "native-ui-$name.png").outputStream().use {
            check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it))
        }
        bitmap.recycle()
    }
    private fun json(vararg fields: Pair<String, Any?>) = JSONObject().apply { fields.forEach { put(it.first, it.second ?: JSONObject.NULL) } }
    private fun messages(client: CloudClient, room: String): List<JSONObject> {
        val rows = client.request("GET", "/chat/conversations/$room/messages")!!.getJSONArray("messages")
        return (0 until rows.length()).map(rows::getJSONObject)
    }
    private fun send(client: CloudClient, room: String, text: String) = client.request("POST", "/chat/conversations/$room/messages", json("requestId" to UUID.randomUUID().toString(), "text" to text, "mentions" to JSONArray(), "replyTo" to null))

    @Test fun existingAccountUsesOriginalConversationAndReceivesRealtimeText() {
        val args = InstrumentationRegistry.getArguments()
        fun arg(key: String) = requireNotNull(args.getString(key)) { "Supply disposable fixture argument $key" }
        val origin = arg("origin")
        val a = CloudClient(origin)
        val b = CloudClient(origin)
        try {
            a.login(arg("emailA"), arg("passwordA"))
            val peer = b.login(arg("emailB"), arg("passwordB"))
            assertEquals("Android B", peer.getString("name"))
            val originalRoom = a.request("POST", "/chat/direct", json("accountId" to peer.getString("id")))!!.getString("id")
            val suffix = UUID.randomUUID().toString().take(8)
            val originalText = "原有聊天记录 · $suffix"
            send(b, originalRoom, originalText)
            activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            waitFor("login") { visible("回到你的聊天") }
            instrumentation.waitForIdleSync()
            capture("login") // Capture before entering even synthetic credentials.
            edit("服务器地址", origin)
            edit("邮箱", arg("emailA"))
            edit("密码", arg("passwordA"))
            tap("登录")
            waitFor("authenticated native inbox") { visible("账号") && visible("通讯录") }
            instrumentation.waitForIdleSync()
            capture("inbox")
            tap("通讯录")
            waitFor("directory search") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "搜索名称或账号 ID" } } }
            edit("搜索名称或账号 ID", "Android B")
            tap("搜索")
            waitFor("directory result") { visible("Android B") }
            tap("Android B")
            waitFor("original conversation history") { visible(originalText) }
            waitFor("loaded composer") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" && it.isEnabled } } }
            val outbound = "安卓原生发送：你好，世界。$suffix"
            edit("写消息…", outbound)
            waitFor("enabled send") { main { descendants(root()).filterIsInstance<Button>().any { it.text.toString() == "发送" && it.isEnabled } } }
            tap("发送")
            waitFor("remote peer receives native send") { messages(b, originalRoom).any { it.optString("text") == outbound } }
            waitFor("server acknowledged composer") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" && it.isEnabled && it.text.isEmpty() } } }
            val inbound = "实时回复：原会话已收到。$suffix"
            send(b, originalRoom, inbound)
            // No refresh action: this visible update must arrive through the native SSE subscription.
            waitFor("SSE inbound message") { visible(inbound) }
            instrumentation.waitForIdleSync()
            capture("chat")
            val draft = "未发送的本机草稿 $suffix"
            edit("写消息…", draft)
            tap("返回")
            waitFor("back to inbox") { visible("账号") }
            tap("Android B")
            waitFor("reopened original conversation") { visible(originalText) && visible(inbound) }
            waitFor("persisted draft") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" && it.text.toString() == draft } } }
            assertEquals(1, messages(b, originalRoom).count { it.optString("text") == outbound })
            assertFalse("Draft must not be sent implicitly", messages(b, originalRoom).any { it.optString("text") == draft })
            edit("写消息…", "")
            tap("返回")
            waitFor("inbox before account") { visible("账号") }
            tap("账号")
            waitFor("account encryption access") { visible("设备与加密设置") }
            capture("account")
            tap("设备与加密设置")
            waitFor("crypto settings dialog") { main {
                val hostField = MainActivity::class.java.getDeclaredField("host").apply { isAccessible = true }
                val host = hostField.get(activity) as CryptoHost
                val dialogField = CryptoHost::class.java.getDeclaredField("dialog").apply { isAccessible = true }
                (dialogField.get(host) as? android.app.Dialog)?.let { it.isShowing && (it.window?.decorView?.width ?: 0) > 0 } == true
            } }
            // Opening settings creates no identity; A/B remain fresh for the independent crypto test.
            main {
                val hostField = MainActivity::class.java.getDeclaredField("host").apply { isAccessible = true }
                val host = hostField.get(activity) as CryptoHost
                val dialogField = CryptoHost::class.java.getDeclaredField("dialog").apply { isAccessible = true }
                (dialogField.get(host) as android.app.Dialog).dismiss()
            }
        } finally {
            if (::activity.isInitialized) main { activity.finish() }
            a.close(); b.close()
        }
    }
}
