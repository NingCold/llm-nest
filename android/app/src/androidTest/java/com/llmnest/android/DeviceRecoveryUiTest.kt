package com.llmnest.android

import android.app.Activity
import android.app.AlertDialog
import android.app.Instrumentation
import android.content.Intent
import android.net.Uri
import android.view.View
import android.view.ViewGroup
import android.view.WindowManager
import android.webkit.WebView
import android.widget.EditText
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import kotlinx.coroutines.CompletableDeferred
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** Real settings UI and export writer; picker results are deliberately intercepted, not a release hook. */
@RunWith(AndroidJUnit4::class)
class DeviceRecoveryUiTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private lateinit var activity: Activity
    private lateinit var host: CryptoHost
    private lateinit var web: WebView
    private fun <T> main(work: () -> T): T {
        if (android.os.Looper.myLooper() == android.os.Looper.getMainLooper()) return work()
        val result = AtomicReference<Result<T>>()
        instrumentation.runOnMainSync { result.set(runCatching(work)) }
        return result.get().getOrThrow()
    }
    private fun views(view: View): Sequence<View> = sequence {
        yield(view)
        if (view is ViewGroup) for (i in 0 until view.childCount) yieldAll(views(view.getChildAt(i)))
    }
    private fun root() = activity.findViewById<ViewGroup>(android.R.id.content).getChildAt(0)
    private fun stage(label: String) { android.util.Log.i("LLMNRecoveryUi", label) }
    private fun waitFor(label: String, timeout: Long = 140_000, check: () -> Boolean) {
        val end = System.currentTimeMillis() + timeout
        while (System.currentTimeMillis() < end) { if (check()) return; Thread.sleep(200) }
        fail("Timed out waiting for $label")
    }
    private fun tapNative(label: String) {
        waitFor("enabled native control $label") {
            main {
                views(root()).filter { view ->
                    view !is EditText && view.isShown &&
                        ((view as? TextView)?.text?.toString() == label || view.contentDescription?.toString() == label)
                }.mapNotNull { match ->
                    generateSequence(match) { it.parent as? View }.firstOrNull { it.isClickable }
                }.firstOrNull { it.isShown && it.isEnabled }?.performClick() == true
            }
        }
    }
    private fun editNative(label: String, value: String) = main {
        views(root()).filterIsInstance<EditText>().single { it.contentDescription?.toString() == label }.setText(value)
    }
    private fun hasNative(label: String) = main {
        views(root()).filterIsInstance<TextView>().any { it !is EditText && it.isShown && it.text.toString() == label }
    }
    private fun eval(script: String): String {
        val result = AtomicReference<String>()
        val done = CountDownLatch(1)
        main { web.evaluateJavascript(script) { result.set(it); done.countDown() } }
        assertTrue("Settings script callback", done.await(10, TimeUnit.SECONDS))
        return result.get()
    }
    private fun hasButton(label: String): Boolean = eval("[...document.querySelectorAll('button')].some(b=>b.textContent===${JSONObject.quote(label)}&&!b.disabled)") == "true"
    private fun click(label: String) {
        waitFor("settings control $label") { hasButton(label) }
        assertEquals("true", eval("(()=>{const b=[...document.querySelectorAll('button')].find(b=>b.textContent===${JSONObject.quote(label)}&&!b.disabled);if(!b)return false;b.click();return true})()"))
    }
    private fun state(): JSONObject {
        val result = AtomicReference<Result<JSONObject>>()
        val done = CountDownLatch(1)
        main { host.call("state", JSONObject()) { result.set(it); done.countDown() } }
        assertTrue("Official SDK state query", done.await(130, TimeUnit.SECONDS))
        return result.get().getOrThrow()
    }
    private fun confirmation(): AlertDialog? = main {
        val alerts = CryptoHost::class.java.getDeclaredField("alerts").apply { isAccessible = true }.get(host) as List<*>
        alerts.filterIsInstance<AlertDialog>().firstOrNull { it.isShowing && it.getButton(AlertDialog.BUTTON_POSITIVE)?.text.toString() == "我已安全保存" }
    }
    private class PickerResult(private val result: Int, private val uri: Uri? = null) : Instrumentation.ActivityMonitor() {
        val captured = AtomicReference<Intent?>()
        override fun onStartActivity(intent: Intent): Instrumentation.ActivityResult? {
            if (intent.action != Intent.ACTION_CREATE_DOCUMENT) return null
            captured.set(Intent(intent))
            return Instrumentation.ActivityResult(result, uri?.let { Intent().setData(it) })
        }
    }

    private fun checkBackgroundDrafts(args: android.os.Bundle, client: CloudClient, account: String, passphrase: String) {
        // Continue with the same unlocked identity, so slow/failed lifecycle saves exercise
        // the real production device gate rather than a no-device auto-open path.
        click("完成，返回聊天")
        waitFor("account page") { hasNative("设备与加密设置") }
        tapNative("返回")
        val peerClient = CloudClient(requireNotNull(args.getString("origin")))
        try {
            val peer = peerClient.login(requireNotNull(args.getString("emailB")) { "backgroundDraftCheck requires emailB" }, requireNotNull(args.getString("passwordB")) { "backgroundDraftCheck requires passwordB" })
            val room = client.request("POST", "/chat/direct", JSONObject().put("accountId", peer.getString("id")))!!.getString("id")
            tapNative("通讯录")
            editNative("搜索名称或账号 ID", peer.getString("id")); tapNative("搜索")
            waitFor("fixture peer") { hasNative(peer.getString("name")) }; tapNative(peer.getString("name"))
            fun field(name: String) = MainActivity::class.java.getDeclaredField(name).apply { isAccessible = true }
            fun mountedDraft() = main { views(root()).filterIsInstance<EditText>().firstOrNull { it.contentDescription?.toString() == "写消息…" } }
            waitFor("loaded draft") { main { field("draft").get(activity) != null } && mountedDraft()?.isEnabled == true }
            val journal = DraftStore(instrumentation.targetContext, client.origin, account)
            val baseline = "durable-before-lock-${UUID.randomUUID()}"
            editNative("写消息…", baseline)
            waitFor("baseline disk write") { journal.load(room).text == baseline }

            fun returnAndUnlock(write: CompletableDeferred<DraftRecord>? = null, succeed: Boolean = true, expected: String) {
                val previousHost = main { field("host").get(activity) }
                main { assertTrue("Actual Android task can background", activity.moveTaskToBack(true)) }
                waitFor("background gate") { main { field("foreground").get(activity) == false && field("gateLocked").get(activity) == true } }
                main {
                    assertNull("No composer behind the gate", mountedDraft())
                    assertNull("Subscription stopped", field("stream").get(activity))
                }
                instrumentation.targetContext.startActivity(Intent(instrumentation.targetContext, MainActivity::class.java)
                    .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_REORDER_TO_FRONT))
                waitFor("actual foreground return") { main { field("foreground").get(activity) == true && activity.hasWindowFocus() } }
                if (write != null) {
                    main {
                        assertSame("Slow save has not closed keys yet", previousHost, field("host").get(activity))
                        assertEquals(true, field("gateLocked").get(activity))
                        assertEquals(true, field("changingGate").get(activity))
                        assertNull("Fast return cannot expose plaintext", mountedDraft())
                    }
                    if (succeed) write.complete(journal.saveText(room, expected))
                    else write.completeExceptionally(DraftError("Injected disk failure"))
                }
                waitFor("replacement locked host") { main { field("host").get(activity) !== previousHost && field("host").get(activity) != null } && hasNative("聊天已锁定") }
                host = main { field("host").get(activity) as CryptoHost }
                web = main { CryptoHost::class.java.getDeclaredField("web").apply { isAccessible = true }.get(host) as WebView }
                assertTrue("Device stays locked after late completion", state().getBoolean("locked"))
                tapNative("解锁加密设备")
                waitFor("unlock dialog") { hasButton("解锁本机资料") }
                assertEquals("true", eval("(()=>{const p=document.querySelector('input[type=password]');if(!p)return false;p.value=${JSONObject.quote(passphrase)};return true})()"))
                click("解锁本机资料")
                waitFor("unlocked settings") { hasButton("锁定本机资料") }; click("完成，返回聊天")
                waitFor("restored original draft") { main { mountedDraft()?.text?.toString() == expected && mountedDraft()?.isEnabled == true } }
            }
            fun injectUnsaved(value: String): CompletableDeferred<DraftRecord> = main {
                val write = CompletableDeferred<DraftRecord>()
                // Deterministic storage boundary injection: no plaintext is persisted by this
                // seam; all lifecycle, locking, SDK unlock, loading and retry code is real.
                field("text").set(activity, value); field("lastWrite").set(activity, write)
                MainActivity::class.java.getDeclaredMethod("render").apply { isAccessible = true }.invoke(activity)
                write
            }
            val slow = "slow-save-${UUID.randomUUID()}"
            returnAndUnlock(injectUnsaved(slow), expected = slow)
            assertEquals(slow, journal.load(room).text)
            val failed = "failed-save-${UUID.randomUUID()}"
            returnAndUnlock(injectUnsaved(failed), succeed = false, expected = failed)
            assertEquals("Failed save did not replace disk content", slow, journal.load(room).text)
            assertNotNull("Recovery requires an explicit retry", main { field("draftError").get(activity) })
            // No lastWrite now: a second Home/return must not replace the recovery with empty
            // or old disk text, and must not clear its retry state.
            returnAndUnlock(expected = failed)
            assertNotNull(main { field("draftError").get(activity) })
            main { (activity as MainActivity).retryDraftSave() }
            waitFor("production retry is durable") { journal.load(room).text == failed && main { field("draftError").get(activity) == null } }
            returnAndUnlock(expected = failed)
            stage("SLOW_AND_FAILED_DRAFT_BACKGROUND_LOCK_VERIFIED retry-durable=true")
        } finally { peerClient.close() }
    }

    @Test fun exportCancelAndWriteRequireSeparateExplicitRecoveryConfirmation() {
        val args = InstrumentationRegistry.getArguments()
        fun arg(key: String) = requireNotNull(args.getString(key)) { "Supply isolated fixture argument $key" }
        // The caller must pass an unused account pair. No registrations or discovery happen here.
        val client = CloudClient(arg("origin"))
        val account = client.login(arg("emailA"), arg("passwordA")).getString("id")
        val cache = instrumentation.targetContext.cacheDir.canonicalFile
        val file = File(cache, "device-recovery-ui-${UUID.randomUUID()}.json").canonicalFile
        val cleanup = listOf(file)
        cleanup.forEach { require(it.parentFile == cache && it.name.startsWith("device-recovery-ui-")); assertFalse(it.exists()) }
        var monitor: PickerResult? = null
        try {
            activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
            waitFor("login") { hasNative("回到你的聊天") }
            editNative("服务器地址", arg("origin")); editNative("邮箱", arg("emailA")); editNative("密码", arg("passwordA")); tapNative("登录")
            waitFor("account access") { hasNative("账号") }
            stage("Authenticated isolated recovery-test account")
            tapNative("账号"); waitFor("encryption settings") { hasNative("设备与加密设置") }; tapNative("设备与加密设置")
            host = main { MainActivity::class.java.getDeclaredField("host").apply { isAccessible = true }.get(activity) as CryptoHost }
            web = main { CryptoHost::class.java.getDeclaredField("web").apply { isAccessible = true }.get(host) as WebView }
            assertFalse("Use a fresh isolated recovery-test identity", state().getBoolean("hasDevice"))
            waitFor("create local device") { hasButton("确认创建独立设备") }
            val passphrase = "disposable-recovery-${UUID.randomUUID()}"
            assertEquals("true", eval("(()=>{const p=document.querySelector('input[type=password]');if(!p)return false;p.value=${JSONObject.quote(passphrase)};return true})()"))
            click("确认创建独立设备")
            click("确认建立首次加密身份")
            waitFor("recovery export available") { hasButton("生成并保存恢复文件") }
            stage("Device identity initialized through settings UI")
            assertTrue(state().getBoolean("trusted")); assertFalse(state().getBoolean("recoveryConfirmed"))

            val canceledMonitor = PickerResult(Activity.RESULT_CANCELED)
            monitor = canceledMonitor
            instrumentation.addMonitor(canceledMonitor)
            click("生成并保存恢复文件")
            waitFor("canceled create-document result") { canceledMonitor.captured.get() != null && !main { host.isPickingDocument() } }
            val canceled = canceledMonitor.captured.get()!!
            assertEquals("application/json", canceled.type)
            assertTrue(canceled.categories?.contains(Intent.CATEGORY_OPENABLE) == true)
            assertFalse("Cancel creates no output file", file.exists())
            assertFalse("Cancel must not confirm recovery", state().getBoolean("recoveryConfirmed"))
            waitFor("cancel feedback") { eval("document.body.textContent.includes('已取消文件操作，恢复状态未确认')") == "true" }
            stage("Canceled picker result left recovery unconfirmed")
            instrumentation.removeMonitor(canceledMonitor); monitor = null

            val successfulMonitor = PickerResult(Activity.RESULT_OK, Uri.fromFile(file))
            monitor = successfulMonitor
            instrumentation.addMonitor(successfulMonitor)
            click("生成并保存恢复文件")
            waitFor("successful private file write") { file.exists() && file.length() > 0 && confirmation() != null }
            assertNotNull(successfulMonitor.captured.get())
            val bytes = file.readBytes()
            try {
                val recovery = JSONObject(String(bytes, Charsets.UTF_8))
                assertEquals("llmn-recovery-v1", recovery.getString("format"))
                assertEquals(client.origin, recovery.getString("server"))
                assertEquals(account, recovery.getString("accountId"))
                assertTrue(recovery.getString("version").isNotBlank())
                assertTrue(recovery.getString("mediaEncryptionInfo").isNotBlank())
                assertTrue("Recovery file contains encrypted attachment metadata", JSONObject(recovery.getString("mediaEncryptionInfo")).length() > 0)
            } finally { bytes.fill(0) }
            assertFalse("A successful write alone must not confirm recovery", state().getBoolean("recoveryConfirmed"))
            stage("Private file written and validated, still awaiting explicit confirmation")
            val alert = confirmation()!!
            main {
                assertTrue("Recovery confirmation remains secure", alert.window!!.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE != 0)
                alert.getButton(AlertDialog.BUTTON_POSITIVE).performClick()
            }
            waitFor("explicitly confirmed recovery") { state().getBoolean("recoveryConfirmed") }
            waitFor("ready settings feedback") { eval("document.body.textContent.includes('恢复文件 · 已确认保存')") == "true" }
            stage("Explicit native confirmation made recovery ready")
            if (args.getString("backgroundDraftCheck") == "true") {
                instrumentation.removeMonitor(successfulMonitor); monitor = null
                checkBackgroundDrafts(args, client, account, passphrase)
            }
        } finally {
            monitor?.let { instrumentation.removeMonitor(it) }
            if (::activity.isInitialized) main { activity.finish() }
            client.close()
            // Only the exact private file allocated and checked by this test is removed.
            cleanup.forEach { if (it.exists()) check(it.delete()) { "Unable to remove isolated recovery test file" } }
            assertTrue(cleanup.none { it.exists() })
        }
    }
}
