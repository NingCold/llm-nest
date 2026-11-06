package com.llmnest.android

import android.app.Activity
import android.app.AlertDialog
import android.app.Instrumentation
import android.content.Intent
import android.graphics.Rect
import android.os.Build
import android.util.Log
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.WindowManager
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.view.inputmethod.InputMethodManager
import android.webkit.WebView
import android.widget.EditText
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * Opt-in, assisted physical-phone acceptance; not part of the emulator-only runner contract.
 *
 * Run only on the isolated QA package with unused fixture A/B identities. Observe LLMNHandset:
 * WAIT_IME_SEND: use the real keyboard to enter exactly 你好 and tap Send. If Chinese input is
 * unavailable, type LLMN_IME_FALLBACK without sending; the explicitly classified fallback exercises
 * composing spans through the composer's Android InputConnection, then WAIT_FALLBACK_SEND asks
 * the operator to tap Send. This fallback does not claim acceptance of a Chinese handset IME.
 * WAIT_PICKER_CANCEL: cancel the actual system create-document picker.
 * WAIT_PICKER_SAVE filename=...: select local Downloads and use that NEW exact filename, then Save.
 * WAIT_RECOVERY_CONFIRM: inspect the native confirmation and tap 我已安全保存.
 * WAIT_BACKGROUND_HOME: press Home; wait for BACKGROUND_LOCK_VERIFIED before launching QA again.
 * WAIT_UNLOCK_OPEN: inspect the locked UI, then tap 解锁加密设备. The test supplies its in-memory
 * disposable passphrase to the real production dialog and unlocks using its normal controls.
 * COMPLETE reports the synthetic document filename retained for exact, operator-led cleanup.
 * No system UI is intercepted or automated; no document contents or credentials enter logcat.
 */
@RunWith(AndroidJUnit4::class)
class HandsetAcceptanceUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: Activity
    private lateinit var host: CryptoHost
    private lateinit var web: WebView
    private var manualTimeout = 600_000L

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
    private fun field(name: String): Any? = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }.get(activity)
    private fun edit(description: String): EditText = descendants(root()).filterIsInstance<EditText>().single { it.contentDescription?.toString() == description }
    private fun visible(label: String) = main {
        descendants(root()).filterIsInstance<TextView>().any { it !is EditText && it.isShown && it.text.toString() == label }
    }

    private fun tap(label: String) = main {
        var view: View = descendants(root()).filterIsInstance<TextView>().first { it !is EditText && it.isShown && it.text.toString() == label }
        while (!view.isClickable) view = view.parent as View
        assertTrue("Enabled native control", view.isEnabled)
        assertTrue("Native click", view.performClick())
    }

    private fun waitFor(label: String, timeout: Long = 140_000, check: () -> Boolean) {
        val end = System.currentTimeMillis() + timeout
        while (System.currentTimeMillis() < end) {
            if (check()) return
            Thread.sleep(200)
        }
        fail("Timed out waiting for $label")
    }

    private fun stage(label: String) { Log.i("LLMNHandset", label) }

    private fun attachHost() = main {
        host = field("host") as CryptoHost
        web = CryptoHost::class.java.getDeclaredField("web").apply { isAccessible = true }.get(host) as WebView
    }

    private fun eval(script: String): String {
        val result = AtomicReference<String>()
        val done = CountDownLatch(1)
        main { web.evaluateJavascript(script) { result.set(it); done.countDown() } }
        assertTrue("Bundled settings callback", done.await(10, TimeUnit.SECONDS))
        return result.get()
    }

    private fun hasButton(label: String) = eval("[...document.querySelectorAll('button')].some(b=>b.textContent===${JSONObject.quote(label)}&&!b.disabled)") == "true"
    private fun click(label: String) {
        waitFor("settings control") { hasButton(label) }
        assertEquals("true", eval("(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent===${JSONObject.quote(label)}&&!b.disabled);if(!b)return false;b.click();return true})()"))
    }

    private fun fillPassphrase(passphrase: String) {
        assertEquals("true", eval("(()=>{const p=document.querySelector('input[type=password]');if(!p)return false;p.value=${JSONObject.quote(passphrase)};return true})()"))
    }

    private fun state(): JSONObject {
        val result = AtomicReference<Result<JSONObject>>()
        val done = CountDownLatch(1)
        main { host.call("state", JSONObject()) { result.set(it); done.countDown() } }
        assertTrue("Official SDK state", done.await(130, TimeUnit.SECONDS))
        return result.get().getOrThrow()
    }

    private fun recoveryConfirmation(): AlertDialog? = main {
        val alerts = CryptoHost::class.java.getDeclaredField("alerts").apply { isAccessible = true }.get(host) as List<*>
        alerts.filterIsInstance<AlertDialog>().firstOrNull { it.isShowing && it.getButton(AlertDialog.BUTTON_POSITIVE)?.text.toString() == "我已安全保存" }
    }

    /** Observe only: returning null leaves Android to launch the actual picker without substitution. */
    private class DocumentLaunchObserver : Instrumentation.ActivityMonitor() {
        val count = AtomicInteger()
        val captured = AtomicReference<Intent?>()
        override fun onStartActivity(intent: Intent): Instrumentation.ActivityResult? {
            if (intent.action == Intent.ACTION_CREATE_DOCUMENT) {
                captured.set(Intent(intent))
                count.incrementAndGet()
            }
            return null
        }
    }

    private fun imeVisible(input: EditText): Boolean {
        if (Build.VERSION.SDK_INT >= 30) return input.rootWindowInsets?.isVisible(WindowInsets.Type.ime()) == true
        val visible = Rect()
        input.getWindowVisibleDisplayFrame(visible)
        return input.rootView.height - visible.bottom > 100 * activity.resources.displayMetrics.density
    }

    private fun composeFallback() = main {
        val input = edit("写消息…")
        assertTrue("Real composer remains focused", input.hasFocus())
        val connection = requireNotNull(input.onCreateInputConnection(EditorInfo()))
        assertTrue(connection.beginBatchEdit())
        try {
            assertTrue(connection.setSelection(0, input.length()))
            assertTrue(connection.setComposingText("你", 1))
            assertTrue("Chinese composing span starts", BaseInputConnection.getComposingSpanStart(input.text) >= 0)
            assertTrue(connection.setComposingText("你好", 1))
            assertEquals("你好", input.text.toString())
            assertEquals("Chinese composing span covers text", 2, BaseInputConnection.getComposingSpanEnd(input.text) - BaseInputConnection.getComposingSpanStart(input.text))
            assertTrue(connection.finishComposingText())
            assertEquals(-1, BaseInputConnection.getComposingSpanStart(input.text))
        } finally { connection.endBatchEdit(); connection.closeConnection() }
    }

    private fun json(vararg fields: Pair<String, Any?>) = JSONObject().apply { fields.forEach { put(it.first, it.second ?: JSONObject.NULL) } }
    private fun messages(client: CloudClient, room: String): List<JSONObject> {
        val rows = client.request("GET", "/chat/conversations/$room/messages")!!.getJSONArray("messages")
        return (0 until rows.length()).map(rows::getJSONObject)
    }

    @Test fun realImeRecoveryPickerAndBackgroundLock() {
        val args = InstrumentationRegistry.getArguments()
        assumeTrue("Assisted handset flow requires an explicit operator", args.getString("assistedHandset") == "true")
        assertEquals("Only the dedicated QA application is authorized", "com.llmnest.android.debug", instrumentation.targetContext.packageName)
        manualTimeout = args.getString("handsetTimeoutMs")?.toLong() ?: 600_000L
        require(manualTimeout in 60_000L..900_000L)
        fun arg(key: String) = requireNotNull(args.getString(key)) { "Supply isolated fixture argument $key" }
        val a = CloudClient(arg("origin"))
        val b = CloudClient(arg("origin"))
        val observer = DocumentLaunchObserver()
        val filename = "llmn-handset-${UUID.randomUUID()}.json"
        // Cleanup plan: retain exactly this synthetic document for the operator; do not enumerate,
        // read, or delete personal Downloads. The operator chooses its location through real SAF.
        stage("CLEANUP_PLAN retain-only-new-synthetic-document filename=$filename")
        var monitorAdded = false
        try {
            val account = a.login(arg("emailA"), arg("passwordA")).getString("id")
            val peer = b.login(arg("emailB"), arg("passwordB"))
            val conversation = a.request("POST", "/chat/direct", json("accountId" to peer.getString("id")))!!
            val room = conversation.getString("id")
            assertNull("Use the original unencrypted direct conversation", conversation.optJSONObject("encryption"))
            val marker = "手机验收原会话 ${UUID.randomUUID().toString().take(8)}"
            b.request("POST", "/chat/conversations/$room/messages", json("requestId" to UUID.randomUUID().toString(), "text" to marker, "mentions" to JSONArray(), "replyTo" to null))
            val chineseBefore = messages(b, room).count { it.optString("text") == "你好" }
            activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            waitFor("login screen") { visible("回到你的聊天") }
            main {
                edit("服务器地址").setText(arg("origin"))
                edit("邮箱").setText(arg("emailA"))
                edit("密码").setText(arg("passwordA"))
            }
            tap("登录")
            waitFor("authenticated inbox") { visible("通讯录") && visible("账号") }
            attachHost()
            assertFalse("A must have a fresh local identity", state().getBoolean("hasDevice"))
            tap("通讯录")
            waitFor("contact search") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "搜索名称或账号 ID" } } }
            main { edit("搜索名称或账号 ID").setText(peer.getString("id")) }
            tap("搜索")
            waitFor("fixture contact") { visible(peer.getString("name")) }
            tap(peer.getString("name"))
            waitFor("original direct history") { visible(marker) }
            waitFor("enabled real composer") { main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" && it.isEnabled } } }
            main {
                val input = edit("写消息…")
                assertEquals("Fresh QA conversation draft", "", input.text.toString())
                assertTrue(input.requestFocus())
                (activity.getSystemService(Activity.INPUT_METHOD_SERVICE) as InputMethodManager).showSoftInput(input, InputMethodManager.SHOW_IMPLICIT)
            }
            waitFor("actual handset IME visible") { main { imeVisible(edit("写消息…")) } }
            stage("WAIT_IME_SEND text=你好 fallback-token=LLMN_IME_FALLBACK")
            var fallback = false
            waitFor("handset Chinese input and explicit Send", manualTimeout) {
                if (!fallback && main { edit("写消息…").text.toString() == "LLMN_IME_FALLBACK" }) {
                    stage("IME_INPUT_CONNECTION_FALLBACK chinese-handset-ime-not-accepted")
                    composeFallback()
                    fallback = true
                    stage("WAIT_FALLBACK_SEND text=你好 composition-verified=true")
                }
                messages(b, room).count { it.optString("text") == "你好" } == chineseBefore + 1
            }
            waitFor("acknowledged send") { main { edit("写消息…").text.isEmpty() && edit("写消息…").isEnabled } }
            stage(if (fallback) "IME_INPUT_CONNECTION_VERIFIED" else "IME_HANDSET_VERIFIED")
            assertFalse("Fallback command must never be sent", messages(b, room).any { it.optString("text") == "LLMN_IME_FALLBACK" })
            main { (activity.getSystemService(Activity.INPUT_METHOD_SERVICE) as InputMethodManager).hideSoftInputFromWindow(edit("写消息…").windowToken, 0) }

            tap("返回")
            waitFor("inbox") { visible("账号") }
            tap("账号")
            tap("设备与加密设置")
            val passphrase = "disposable-handset-${UUID.randomUUID()}"
            waitFor("fresh device control") { hasButton("确认创建独立设备") }
            fillPassphrase(passphrase)
            click("确认创建独立设备")
            click("确认建立首次加密身份")
            waitFor("recovery export") { hasButton("生成并保存恢复文件") }
            assertTrue(state().getBoolean("trusted"))
            assertFalse(state().getBoolean("recoveryConfirmed"))
            stage("IDENTITY_CREATED_VIA_SETTINGS")

            instrumentation.addMonitor(observer); monitorAdded = true
            stage("WAIT_PICKER_CANCEL")
            click("生成并保存恢复文件")
            waitFor("actual cancel picker launch") { observer.count.get() == 1 && main { host.isPickingDocument() } }
            val intent = observer.captured.get()!!
            assertEquals("application/json", intent.type)
            assertTrue(intent.categories?.contains(Intent.CATEGORY_OPENABLE) == true)
            waitFor("operator canceled actual picker", manualTimeout) { !main { host.isPickingDocument() } }
            assertFalse(state().getBoolean("recoveryConfirmed"))
            assertNull(recoveryConfirmation())
            waitFor("cancel feedback") { eval("document.body.textContent.includes('已取消文件操作，恢复状态未确认')") == "true" }
            stage("PICKER_CANCEL_VERIFIED")

            stage("WAIT_PICKER_SAVE filename=$filename location=local-Downloads rename-before-save=true")
            click("生成并保存恢复文件")
            waitFor("actual save picker launch") { observer.count.get() == 2 && main { host.isPickingDocument() } }
            waitFor("real picker write and explicit confirmation", manualTimeout) { recoveryConfirmation() != null }
            assertFalse("Writing alone cannot confirm recovery", state().getBoolean("recoveryConfirmed"))
            val confirmation = recoveryConfirmation()!!
            main { assertTrue("Confirmation is secure", confirmation.window!!.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE != 0) }
            stage("WAIT_RECOVERY_CONFIRM")
            waitFor("operator explicitly confirms recovery", manualTimeout) { state().getBoolean("recoveryConfirmed") }
            val recovery = a.request("GET", "/crypto/recovery")!!
            assertTrue("Production commit registered recovery version", recovery.getString("version").isNotBlank())
            assertTrue("Production commit registered encrypted recovery", recovery.getString("ciphertext").isNotBlank())
            stage("RECOVERY_CONFIRMED")
            click("完成，返回聊天")
            waitFor("account page after setup") { visible("账号") && visible("此设备已获信任，恢复文件已确认") }
            tap("返回")
            waitFor("inbox after account") { visible("通讯录") }
            tap(peer.getString("name"))
            waitFor("same original conversation") { visible(marker) }
            val draft = "手机后台锁定草稿 ${UUID.randomUUID().toString().take(8)}"
            main { edit("写消息…").setText(draft) }
            val store = DraftStore(instrumentation.targetContext, a.origin, account)
            waitFor("draft durably saved") { store.load(room).text == draft }
            stage("WAIT_BACKGROUND_HOME")
            waitFor("operator Home and production background lock", manualTimeout) { main { field("foreground") == false && field("host") == null && field("gateLocked") == true } }
            main {
                assertFalse("OS actually removed window focus", activity.hasWindowFocus())
                assertNull("Snapshot unmounted", field("snapshot"))
                assertEquals("Message page removed", 0, (field("page") as JSONObject).length())
                assertNull("Draft removed from mounted state", field("draft"))
                assertEquals("Draft plaintext removed from mounted state", "", field("text"))
                assertNull("Realtime subscription stopped", field("stream"))
                assertFalse("Message UI removed", descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" })
                assertFalse("Original message UI removed", descendants(root()).filterIsInstance<TextView>().any { it.text.toString() == marker })
            }
            stage("BACKGROUND_LOCK_VERIFIED")
            waitFor("operator returns through actual OS", manualTimeout) { main { field("foreground") == true && activity.hasWindowFocus() } && visible("聊天已锁定") }
            attachHost()
            assertTrue(state().getBoolean("locked"))
            assertTrue(state().getBoolean("hasDevice"))
            assertFalse("No composer while locked", main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" } })
            stage("WAIT_UNLOCK_OPEN")
            waitFor("operator opens production unlock dialog", manualTimeout) { main { web.isShown } && hasButton("解锁本机资料") }
            fillPassphrase(passphrase)
            click("解锁本机资料")
            waitFor("manual passphrase unlock") { !state().getBoolean("locked") }
            click("完成，返回聊天")
            waitFor("original chat and restored draft") { visible(marker) && main { descendants(root()).filterIsInstance<EditText>().any { it.contentDescription?.toString() == "写消息…" && it.text.toString() == draft } } }
            assertFalse("Draft never sent implicitly", messages(b, room).any { it.optString("text") == draft })
            assertEquals("Exactly one Chinese send", chineseBefore + 1, messages(b, room).count { it.optString("text") == "你好" })
            stage("UNLOCK_PRODUCTION_DIALOG_VERIFIED draft-restored=true")
            stage("COMPLETE ime=${if (fallback) "input-connection-fallback" else "handset"} retained-document=$filename")
        } finally {
            if (monitorAdded) instrumentation.removeMonitor(observer)
            if (::activity.isInitialized) main { activity.finish() }
            a.close(); b.close()
            stage("CLEANUP_HANDOFF retained-only-new-synthetic-document filename=$filename")
        }
    }
}
