package com.llmnest.android

import android.app.Activity
import android.content.Intent
import android.text.Editable
import android.view.View
import android.view.ViewGroup
import android.view.inputmethod.BaseInputConnection
import android.view.inputmethod.EditorInfo
import android.widget.EditText
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.runner.lifecycle.ActivityLifecycleMonitorRegistry
import androidx.test.runner.lifecycle.Stage
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Assume.assumeTrue
import org.junit.Test
import org.junit.runner.RunWith
import java.util.concurrent.atomic.AtomicReference

/** Fixture-free regression. Open the QA app from its launcher after WAIT_LAUNCH_QA. */
@RunWith(AndroidJUnit4::class)
class ChatComposerRefreshTest {
    private data class EditorState(val editable: Editable, val selectionStart: Int, val selectionEnd: Int,
                                   val composingStart: Int, val composingEnd: Int, val draftWrites: Int)
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
    private fun editor(ui: ChatScreens) = views(ui.root).filterIsInstance<EditText>().single()
    private fun message(position: Int) = JSONObject().apply {
        put("id", "regression-message-$position"); put("senderId", "peer"); put("senderName", "测试联系人")
        put("text", "历史刷新 $position"); put("createdAt", "2026-11-06T10:00:00Z")
        put("status", "committed"); put("attachments", JSONArray())
    }

    @Test fun historyRefreshKeepsAttachedComposingEditor() {
        val autoLaunch = InstrumentationRegistry.getArguments().getString("autoLaunch") == "true"
        if (!autoLaunch) assumeTrue("Run this assisted test explicitly on the QA package",
            instrumentation.targetContext.packageName == "com.llmnest.android.debug" &&
                InstrumentationRegistry.getArguments().getString("class")?.substringBefore('#') == javaClass.name)
        android.util.Log.i("LLMNComposer", "WAIT_LAUNCH_QA package=${instrumentation.targetContext.packageName}")
        var resumed: Activity? = if (autoLaunch) instrumentation.startActivitySync(
            android.content.Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) else null
        val launchDeadline = System.currentTimeMillis() + 60_000
        while (resumed == null && System.currentTimeMillis() < launchDeadline) {
            resumed = main {
                ActivityLifecycleMonitorRegistry.getInstance().getActivitiesInStage(Stage.RESUMED)
                    .filterIsInstance<MainActivity>().singleOrNull {
                        it.packageName == instrumentation.targetContext.packageName && it.hasWindowFocus() && !it.isFinishing
                    }
            }
            if (resumed == null) Thread.sleep(200)
        }
        val activity = requireNotNull(resumed) { "Launch the QA MainActivity from its launcher while ChatComposerRefreshTest waits" }
        var draft = ""
        var draftWrites = 0
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
            override fun refresh() {}
            override fun logout() {}
            override fun back() {}
            override fun send(text: String) {}
            override fun draftChanged(text: String) { draft = text; draftWrites++ }
            override fun retryDraftSave() {}
            override fun loadOlder() {}
            override fun retryPending() {}
            override fun discardPending() {}
            override fun visibleLatest() {}
        }
        val ui = main { ChatScreens(activity, actions) }
        val content = main { activity.findViewById<ViewGroup>(android.R.id.content) }
        val previous = main { (0 until content.childCount).map { content.getChildAt(it) to content.getChildAt(it).visibility } }
        val conversation = JSONObject().apply { put("id", "composer-regression"); put("name", "测试聊天"); put("kind", "direct") }
        val messages = JSONArray().apply { repeat(30) { put(message(it)) } }
        try {
            main {
                assertTrue("QA Activity must have window focus", activity.hasWindowFocus())
                previous.forEach { (view, _) -> view.visibility = View.GONE }
                content.addView(ui.root, ViewGroup.LayoutParams(-1, -1))
                ui.conversation(conversation, "self", messages, hasMore = true, draft = draft)
            }
            instrumentation.waitForIdleSync() // Let the initial history's posted positioning finish first.
            val input = main { editor(ui) }
            var detaches = 0
            var focusLosses = 0
            val detachObserver = object : View.OnAttachStateChangeListener {
                override fun onViewAttachedToWindow(v: View) {}
                override fun onViewDetachedFromWindow(v: View) { detaches++ }
            }
            val connection = main {
                assertTrue(input.requestFocus())
                val connection = input.onCreateInputConnection(EditorInfo())
                assertNotNull(connection)
                input.addOnAttachStateChangeListener(detachObserver)
                input.onFocusChangeListener = View.OnFocusChangeListener { _, focused -> if (!focused) focusLosses++ }
                connection!!
            }
            val token = main { input.windowToken }
            val inputParent = main { input.parent }
            repeat(3) { iteration ->
                val before = main {
                    when (iteration) {
                        0 -> assertTrue(connection.setComposingText("nihao", 1))
                        1 -> {
                            assertTrue(connection.finishComposingText())
                            input.setText("正在输入拼音")
                            input.setSelection(1, 4)
                        }
                        2 -> input.setSelection(4, 1)
                    }
                    val state = EditorState(input.text, input.selectionStart, input.selectionEnd,
                        BaseInputConnection.getComposingSpanStart(input.text),
                        BaseInputConnection.getComposingSpanEnd(input.text), draftWrites)
                    // Some handset editors finish composition when a range is selected. Exercise
                    // real composition and forward/reverse range selection as independent states.
                    if (iteration == 0) {
                        assertTrue("setComposingText establishes an observable composing region", state.composingStart >= 0)
                        assertTrue(state.composingEnd > state.composingStart)
                    } else {
                        assertEquals(-1, state.composingStart)
                        assertEquals(-1, state.composingEnd)
                        assertNotEquals(state.selectionStart, state.selectionEnd)
                    }
                    messages.put(message(30 + iteration))
                    ui.conversation(conversation, "self", messages, hasMore = true, draft = draft,
                        error = if (iteration == 1) "实时连接正在恢复" else null)
                    state
                }
                instrumentation.waitForIdleSync() // Also catches deferred fullScroll stealing focus.
                main {
                    assertSame("Refresh keeps the original editor", input, editor(ui))
                    assertEquals("Refresh must never detach/re-attach an active editor", 0, detaches)
                    assertEquals("Refresh must not move focus away from the editor", 0, focusLosses)
                    assertTrue(input.isAttachedToWindow); assertTrue(input.hasFocus())
                    assertSame(inputParent, input.parent); assertSame(token, input.windowToken)
                    assertSame("Refresh must retain the Editable, not copy its text", before.editable, input.text)
                    assertEquals(before.selectionStart, input.selectionStart); assertEquals(before.selectionEnd, input.selectionEnd)
                    assertEquals(before.composingStart, BaseInputConnection.getComposingSpanStart(input.text))
                    assertEquals(before.composingEnd, BaseInputConnection.getComposingSpanEnd(input.text))
                    assertEquals("Binding history does not resave/replace the draft", before.draftWrites, draftWrites)
                }
            }
            main {
                // Busy/pending still freeze edits. Acknowledgement may clear text/composition.
                ui.conversation(conversation, "self", messages, true, busy = true, draft = draft)
                assertSame(input, editor(ui)); assertFalse(input.isEnabled)
                ui.conversation(conversation, "self", messages, true, draft = draft, pending = ChatScreens.PendingSend(draft, true))
                assertFalse(input.isEnabled)
                draft = ""
                ui.conversation(conversation, "self", messages, true, draft = draft)
                assertTrue(input.isEnabled); assertEquals("", input.text.toString())
                assertEquals(-1, BaseInputConnection.getComposingSpanStart(input.text))
                input.removeOnAttachStateChangeListener(detachObserver)
                input.onFocusChangeListener = null
                // Read-only/navigation/locking must still unload the editor, unlike normal refresh.
                conversation.put("readOnly", true)
                ui.conversation(conversation, "self", messages, true)
                assertFalse(input.isAttachedToWindow)
                assertTrue(views(ui.root).filterIsInstance<EditText>().none())
                conversation.put("readOnly", false)
                ui.conversation(conversation, "self", messages, true)
                val replacement = editor(ui)
                assertNotSame(input, replacement)
                ui.gate("测试账号", busy = false, locked = true, error = null)
                assertFalse(replacement.isAttachedToWindow)
                assertTrue(views(ui.root).filterIsInstance<EditText>().none())
            }
        } finally {
            main {
                content.removeView(ui.root)
                previous.forEach { (view, visibility) -> view.visibility = visibility }
                if (autoLaunch) activity.finish()
            }
        }
    }
}
