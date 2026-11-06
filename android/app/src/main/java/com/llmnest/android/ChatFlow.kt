package com.llmnest.android

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Deferred
import kotlinx.coroutines.launch
import org.json.JSONArray

/** Only not-yet-started plain text writes may collapse; barriers seal the current batch. */
internal class DraftTextWrites(private val scope: CoroutineScope) {
    private class Batch(val store: DraftStore, val room: String, var text: String) {
        val result = CompletableDeferred<DraftRecord>()
    }
    private var tail: Batch? = null

    @Synchronized
    fun save(store: DraftStore, room: String, text: String, coalesce: Boolean): Deferred<DraftRecord> {
        tail?.takeIf { coalesce && it.store === store && it.room == room }?.let {
            it.text = text
            return it.result
        }
        val batch = Batch(store, room, text)
        tail = if (coalesce) batch else null
        scope.launch {
            val input = synchronized(this@DraftTextWrites) {
                if (tail === batch) tail = null
                batch.text
            }
            try { batch.result.complete(store.saveText(room, input)) }
            catch (e: Exception) { batch.result.completeExceptionally(e) }
        }
        return batch.result
    }

    @Synchronized
    fun barrier(write: Deferred<DraftRecord>?): Deferred<DraftRecord>? {
        tail = null
        return write
    }
}

/** Pages arrive oldest first in server position order; duplicates retain their original slot. */
internal fun mergeHistory(pages: List<JSONArray>): JSONArray {
    val rows = LinkedHashMap<String, org.json.JSONObject>()
    pages.forEach { page -> repeat(page.length()) { i -> page.getJSONObject(i).let { rows[it.getString("id")] = it } } }
    return JSONArray(rows.values.toList())
}

/** Older requests append exactly one page; refresh reprojects the entire already-seen range. */
internal suspend fun readHistory(
    older: Boolean,
    depth: Int,
    previous: org.json.JSONObject,
    fetch: suspend (before: String) -> org.json.JSONObject,
): org.json.JSONObject {
    val existing = previous.optJSONArray("messages") ?: JSONArray()
    val oldest = existing.optJSONObject(0)?.optString("position")?.toBigInteger()
    var before = if (older) existing.optJSONObject(0)?.optString("position").orEmpty() else ""
    val chunks = ArrayList<JSONArray>()
    var more = true
    var latest = ""
    val count = if (older) 1 else depth
    while (more && (chunks.size < count || (!older && oldest != null && before.isNotEmpty() && before.toBigInteger() > oldest))) {
        val projected = fetch(before)
        val messages = projected.getJSONArray("messages")
        if (chunks.isEmpty()) latest = projected.getString("latestPosition")
        chunks.add(messages)
        more = projected.optBoolean("hasMore") && messages.length() > 0
        if (messages.length() > 0) {
            val next = messages.getJSONObject(0).getString("position")
            check(before.isEmpty() || next.toBigInteger() < before.toBigInteger()) {
                "聊天历史分页位置未向前推进，请刷新后重试。"
            }
            if (more) before = next
        }
    }
    return org.json.JSONObject()
        .put("messages", mergeHistory(chunks.asReversed() + if (older) listOf(existing) else emptyList()))
        .put("hasMore", more)
        .put("latestPosition", if (older) previous.optString("latestPosition") else latest)
}
