package com.llmnest.android

import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import java.util.concurrent.Executors
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit

class ChatFlowTest {
    private class MemoryFile : DraftFile {
        var value: String? = null
        var writes = 0
        var fail = false
        override fun read() = value
        override fun write(value: String) { writes++; if (fail) error("disk full"); this.value = value }
    }

    @Test fun pendingTypingCollapsesButFlushSealsAndAccountsNeverMerge() = runBlocking {
        val dispatcher = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
        val scope = CoroutineScope(SupervisorJob() + dispatcher)
        val blocked = CountDownLatch(1)
        val started = CountDownLatch(1)
        scope.launch { started.countDown(); blocked.await(5, TimeUnit.SECONDS) }
        assertTrue(started.await(5, TimeUnit.SECONDS))
        try {
            val file = MemoryFile()
            val a = DraftStore("https://test", "a", file)
            val b = DraftStore("https://test", "b", file)
            val queue = DraftTextWrites(scope)
            val first = queue.save(a, "room", "one", true)
            assertSame(first, queue.save(a, "room", "two", true))
            queue.barrier(first)
            val next = queue.save(a, "room", "three", true)
            assertNotSame(first, next)
            val other = queue.save(b, "room", "other", true)
            blocked.countDown()
            assertEquals("two", first.await().text)
            assertEquals("three", next.await().text)
            assertEquals("other", other.await().text)
            assertEquals(3, file.writes)
            assertEquals("three", a.load("room").text)
        } finally { blocked.countDown(); scope.cancel(); dispatcher.close() }
    }

    @Test fun failedBatchCanBeRetriedAndPendingWritesAreNotCollapsed() = runBlocking {
        val dispatcher = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
        val scope = CoroutineScope(SupervisorJob() + dispatcher)
        try {
            val file = MemoryFile().apply { fail = true }
            val store = DraftStore("https://test", "a", file)
            val queue = DraftTextWrites(scope)
            try { queue.save(store, "room", "unsaved", true).await(); fail("expected storage failure") } catch (_: DraftError) { }
            file.fail = false
            queue.save(store, "room", "unsaved", true).await()
            store.prepare("room", JSONObject().put("requestId", "original"))
            val one = queue.save(store, "room", "new", false)
            val two = queue.save(store, "room", "newer", false)
            assertNotSame(one, two)
            one.await(); two.await()
            assertEquals("original", store.load("room").pending!!.requestId)
            assertEquals("unsaved", store.load("room").pending!!.text)
        } finally { scope.cancel(); dispatcher.close() }
    }

    @Test fun historyDeduplicatesOrderedDecimalPositionsAndKeepsUpdatedRows() {
        fun row(id: String, position: String, text: String) = JSONObject().put("id", id).put("position", position).put("text", text)
        val result = mergeHistory(listOf(JSONArray().put(row("a", "2", "old")).put(row("b", "10", "b")), JSONArray().put(row("a", "2", "edited"))))
        assertEquals(2, result.length())
        assertEquals("edited", result.getJSONObject(0).getString("text"))
        assertEquals("b", result.getJSONObject(1).getString("id"))
    }
    @Test fun olderFetchesOnePageAndRefreshReprojectsEverySeenPosition() = runBlocking {
        fun page(vararg positions: Int, more: Boolean = true) = JSONObject()
            .put("messages", JSONArray(positions.map { JSONObject().put("id", "m$it").put("position", "$it").put("text", "edited-$it") }))
            .put("hasMore", more).put("latestPosition", "12")
        val seen = page(7, 8, 9, 10)
        val calls = ArrayList<String>()
        val older = readHistory(true, 5, seen) { before -> calls.add(before); page(5, 6) }
        assertEquals(listOf("7"), calls)
        assertEquals(6, older.getJSONArray("messages").length())
        assertEquals("12", older.getString("latestPosition"))
        calls.clear()
        val refreshed = readHistory(false, 2, older) { before ->
            calls.add(before)
            when (before) {
                "" -> page(11, 12)
                "11" -> page(9, 10)
                "9" -> page(7, 8)
                "7" -> page(5, 6)
                else -> error("Unexpected cursor $before")
            }
        }
        assertEquals(listOf("", "11", "9", "7"), calls)
        assertEquals("edited-5", refreshed.getJSONArray("messages").getJSONObject(0).getString("text"))
        assertEquals(8, refreshed.getJSONArray("messages").length())
    }

    @Test fun stalledOrReversedCursorFailsWithoutReplacingTheExistingPage() = runBlocking {
        fun page(position: Int) = JSONObject()
            .put("messages", JSONArray().put(JSONObject().put("id", "m$position").put("position", "$position")))
            .put("hasMore", true).put("latestPosition", "12")
        val previous = page(2)
        val original = previous.toString()
        for (invalidNext in listOf(10, 11)) {
            val calls = ArrayList<String>()
            var mounted = previous
            try {
                mounted = readHistory(false, 1, previous) { before ->
                    calls.add(before)
                    check(calls.size <= 2) { "Paging must fail before another request" }
                    if (before.isEmpty()) page(10) else page(invalidNext)
                }
                fail("Expected non-advancing page cursor to fail")
            } catch (e: IllegalStateException) {
                assertEquals("聊天历史分页位置未向前推进，请刷新后重试。", e.message)
            }
            assertEquals(listOf("", "10"), calls)
            assertSame(previous, mounted)
            assertEquals(original, previous.toString())
        }
    }

}
