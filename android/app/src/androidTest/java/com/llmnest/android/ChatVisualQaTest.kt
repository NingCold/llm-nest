package com.llmnest.android

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import android.view.ViewGroup
import android.accessibilityservice.AccessibilityServiceInfo
import android.view.accessibility.AccessibilityNodeInfo
import android.view.WindowManager
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
import java.util.concurrent.atomic.AtomicReference

/** Credential-free visual fixture. Draws only this test's synthetic ChatScreens, never a user window. */
@RunWith(AndroidJUnit4::class)
class ChatVisualQaTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private fun <T> main(work: () -> T): T {
        val result = AtomicReference<Result<T>>()
        instrumentation.runOnMainSync { result.set(runCatching(work)) }
        return result.get().getOrThrow()
    }
    private fun views(view: View): Sequence<View> = sequence {
        yield(view)
        if (view is ViewGroup) for (i in 0 until view.childCount) yieldAll(views(view.getChildAt(i)))
    }
    @Test fun syntheticConversationVisualAndRecovery() {
        val activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        var retries = 0
        var refreshes = 0
        val actions = object : ChatScreens.Actions {
            override fun login(origin: String, email: String, password: String) {}
            override fun selectTab(contacts: Boolean) {}
            override fun openConversation(id: String) {}
            override fun openContact(id: String) {}
            override fun searchContacts(query: String) {}
            override fun account() {}
            override fun crypto() {}
            override fun unlock() {}
            override fun lock() {}
            override fun enableEncryption() {}
            override fun refresh() { refreshes++ }
            override fun logout() {}
            override fun back() {}
            override fun send(text: String) {}
            override fun draftChanged(text: String) {}
            override fun retryDraftSave() { retries++ }
            override fun loadOlder() {}
            override fun retryPending() { retries++ }
            override fun discardPending() {}
            override fun visibleLatest() {}
        }
        val ui = main { ChatScreens(activity, actions) }
        val conversation = JSONObject().put("id", "synthetic-visual").put("name", "林澈").put("kind", "direct")
        val messages = JSONArray()
        fun message(sender: String, text: String, time: String) {
            messages.put(JSONObject().put("id", "visual-${messages.length()}").put("senderId", sender)
                .put("senderName", if (sender == "self") "我" else "林澈").put("text", text)
                .put("createdAt", "2026-11-07T${time}:00Z").put("status", "committed").put("attachments", JSONArray()))
        }
        message("peer", "周六一起去旧书店吗？", "10:00")
        message("peer", "就是河边那家。", "10:01")
        message("self", "好呀，想去很久了。", "10:02")
        message("self", "几点集合？", "10:03")
        message("peer", "下午两点吧。我们先在地铁口碰面，沿河走过去。那边新到了一批旧画册，逛完还能去旁边喝杯咖啡，不用赶时间。", "10:04")
        message("self", "听起来很棒，我带个帆布袋。", "10:05")
        message("peer", "那就说定了，周六见。", "10:06")
        fun render(draft: String = "", pending: ChatScreens.PendingSend? = null, draftError: String? = null) = main {
            ui.conversation(conversation, "self", messages, false, draft = draft, pending = pending, draftSaveError = draftError)
        }
        fun capture(name: String) {
            instrumentation.waitForIdleSync()
            main {
                assertTrue(activity.window.attributes.flags and WindowManager.LayoutParams.FLAG_SECURE != 0)
                assertTrue(ui.root.width > 0 && ui.root.height > 0)
                val bitmap = Bitmap.createBitmap(ui.root.width, ui.root.height, Bitmap.Config.ARGB_8888)
                ui.root.draw(Canvas(bitmap))
                File(instrumentation.targetContext.cacheDir, "$name.png").outputStream().use { assertTrue(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
                bitmap.recycle()
            }
        }
        try {
            main { activity.setContentView(ui.root) }
            render()
            capture("chat-visual-light")
            main {
                val labels = views(ui.root).filterIsInstance<TextView>().map { it.text.toString() }.toList()
                assertEquals("Adjacent peer messages share sender attribution", 3, labels.count { it.startsWith("林澈  ·  ") })
                assertEquals("Adjacent own messages share sender attribution", 2, labels.count { it.startsWith("我  ·  ") })
            }
            message("peer", "晚一点再确认路线。", "10:20")
            message("peer", "出门前联系。", "10:21")
            messages.getJSONObject(messages.length() - 1).put("createdAt", "2026-11-08T10:21:00Z")
            render()
            main {
                assertEquals("Long gaps and a new local day each start a group", 5,
                    views(ui.root).filterIsInstance<TextView>().count { it.text.toString().startsWith("林澈  ·  ") })
            }
            messages.remove(messages.length() - 1)
            messages.remove(messages.length() - 1)
            render()
            val input = main { views(ui.root).filterIsInstance<EditText>().single() }
            main { input.setText("我会提前十分钟到。"); input.setSelection(3) }
            val automation = instrumentation.uiAutomation
            automation.serviceInfo = automation.serviceInfo.apply {
                flags = flags or AccessibilityServiceInfo.FLAG_RETRIEVE_INTERACTIVE_WINDOWS
            }
            main { views(ui.root).single { it.contentDescription?.toString() == "更多操作" }.performClick() }
            instrumentation.waitForIdleSync()
            var refreshNode: AccessibilityNodeInfo? = null
            val deadline = System.currentTimeMillis() + 5000
            while (refreshNode == null && System.currentTimeMillis() < deadline) {
                refreshNode = automation.windows.asSequence().mapNotNull { it.root }
                    .flatMap { it.findAccessibilityNodeInfosByText("刷新").asSequence() }
                    .firstOrNull { it.isVisibleToUser && it.text?.toString() == "刷新" }
                if (refreshNode == null) Thread.sleep(100)
            }
            var clickNode = requireNotNull(refreshNode) { "Native refresh menu item must be visible" }
            while (!clickNode.isClickable) clickNode = requireNotNull(clickNode.parent)
            assertTrue(clickNode.performAction(AccessibilityNodeInfo.ACTION_CLICK))
            instrumentation.waitForIdleSync()
            assertEquals(1, refreshes)
            render("我会提前十分钟到。")
            instrumentation.waitForIdleSync()
            main {
                assertSame(input, views(ui.root).filterIsInstance<EditText>().single())
                assertEquals(3, input.selectionStart)
                assertEquals("我会提前十分钟到。", input.text.toString())
                assertTrue(views(ui.root).any { it.contentDescription?.toString() == "发送" })
            }
            render("我会提前十分钟到。", ChatScreens.PendingSend("我会提前十分钟到。", true))
            main { views(ui.root).filterIsInstance<TextView>().single { it.text.toString() == "重试原发送" }.performClick() }
            assertEquals(1, retries)
            capture("chat-visual-pending")
            render("我会提前十分钟到。", draftError = "草稿暂未保存，请重试。")
            main { views(ui.root).filterIsInstance<TextView>().single { it.text.toString() == "重试保存草稿" }.performClick() }
            assertEquals(2, retries)
            capture("chat-visual-draft-error")
        } finally { main { activity.finish() } }
    }
}
