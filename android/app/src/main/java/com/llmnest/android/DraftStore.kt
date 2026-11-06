package com.llmnest.android

import android.content.Context
import android.util.AtomicFile
import org.json.JSONObject
import java.io.File
import java.io.FileNotFoundException

class DraftError(message: String) : Exception(message)

/** A fresh JSON object is returned each time; callers cannot mutate the persisted command. */
class PreparedCommand internal constructor(
    val requestId: String,
    val draftRevision: Long,
    val text: String,
    internal val bodyJson: String,
) {
    val body: JSONObject get() = JSONObject(bodyJson)
}

data class DraftRecord(val text: String, val revision: Long, val pending: PreparedCommand?)

internal interface DraftFile {
    fun read(): String?
    fun write(value: String)
}

private class AndroidDraftFile(context: Context) : DraftFile {
    // Local plaintext drafts must not enter Android's cloud/device backup.
    private val file = AtomicFile(File(context.noBackupFilesDir, "llm-nest-social-drafts-v1.json"))

    override fun read(): String? = try {
        file.openRead().bufferedReader(Charsets.UTF_8).use { it.readText() }
    } catch (error: FileNotFoundException) {
        if (file.baseFile.exists() || File(file.baseFile.path + ".bak").exists()) throw error
        null
    }

    override fun write(value: String) {
        val output = file.startWrite()
        try {
            output.write(value.toByteArray(Charsets.UTF_8))
            output.fd.sync()
            file.finishWrite(output)
        } catch (error: Exception) {
            file.failWrite(output)
            throw error
        }
        // AtomicFile reports some rename errors only to logcat; fail closed on those too.
        if (read() != value) throw DraftError("草稿未能确认保存，请检查设备存储空间")
    }
}

/** All writes are synchronous. An unknown send result is retried only by an explicit caller action. */
class DraftStore internal constructor(
    origin: String,
    accountId: String,
    private val file: DraftFile,
) {
    constructor(context: Context, origin: String, accountId: String) :
        this(origin, accountId, AndroidDraftFile(context.applicationContext))

    private val owner = "$origin\n$accountId"

    init {
        require(origin.isNotBlank() && accountId.isNotBlank() && '\n' !in origin && '\n' !in accountId)
    }

    fun load(conversationId: String): DraftRecord = synchronized(lock) {
        record(records(readRoot()), conversationId)
    }

    fun saveText(conversationId: String, text: String): DraftRecord = change(conversationId) { old ->
        if (old.text == text) old else old.copy(text = text, revision = old.revision + 1)
    }

    /** Persist the complete, already-encrypted command before the first network attempt. */
    fun prepare(conversationId: String, body: JSONObject): PreparedCommand {
        val result = change(conversationId) { old ->
            if (old.pending != null) throw DraftError("此会话有结果未确认的发送，请先重试或放弃该发送")
            val requestId = body.optString("requestId")
            if (requestId.isBlank()) throw DraftError("发送命令缺少请求标识")
            old.copy(pending = PreparedCommand(requestId, old.revision, old.text, body.toString()))
        }
        return result.pending!!
    }

    fun acknowledge(conversationId: String, requestId: String): DraftRecord = change(conversationId) { old ->
        val pending = old.pending
        if (pending == null || pending.requestId != requestId) old
        else if (old.revision == pending.draftRevision) DraftRecord("", old.revision + 1, null)
        else old.copy(pending = null)
    }

    /** Discard only the retry record, not the editable text; delivery may already have happened. */
    fun discard(conversationId: String, requestId: String): DraftRecord = change(conversationId) { old ->
        if (old.pending?.requestId == requestId) old.copy(pending = null) else old
    }

    private fun change(conversationId: String, update: (DraftRecord) -> DraftRecord): DraftRecord = synchronized(lock) {
        require(conversationId.isNotBlank())
        val root = readRoot()
        val records = records(root) ?: JSONObject()
        val result = update(record(records, conversationId))
        records.put(conversationId, encode(result))
        root.put(owner, records)
        try {
            file.write(root.toString())
        } catch (_: Exception) {
            throw DraftError("草稿保存失败，请保留当前页面并检查设备存储空间后重试")
        }
        result
    }

    private fun readRoot(): JSONObject = try {
        file.read()?.let(::JSONObject) ?: JSONObject()
    } catch (_: Exception) {
        throw DraftError("无法读取本地草稿，原数据已保留")
    }

    private fun decode(value: JSONObject?): DraftRecord {
        if (value == null) return DraftRecord("", 0, null)
        try {
            val pending = (if (value.isNull("pending")) null else value.getJSONObject("pending"))?.let {
                val body = it.getString("body")
                PreparedCommand(JSONObject(body).getString("requestId"), it.getLong("draftRevision"), it.getString("text"), body)
            }
            return DraftRecord(value.getString("text"), value.getLong("revision"), pending)
        } catch (_: Exception) {
            throw DraftError("本地草稿格式无法读取，原数据已保留")
        }
    }

    private fun records(root: JSONObject): JSONObject? = try {
        if (root.has(owner)) root.getJSONObject(owner) else null
    } catch (_: Exception) { throw DraftError("本地草稿格式无法读取，原数据已保留") }

    private fun record(records: JSONObject?, conversationId: String): DraftRecord = try {
        decode(if (records != null && records.has(conversationId)) records.getJSONObject(conversationId) else null)
    } catch (_: Exception) { throw DraftError("本地草稿格式无法读取，原数据已保留") }

    private fun encode(record: DraftRecord) = JSONObject()
        .put("text", record.text).put("revision", record.revision)
        .put("pending", record.pending?.let {
            JSONObject().put("body", it.bodyJson).put("draftRevision", it.draftRevision).put("text", it.text)
        } ?: JSONObject.NULL)

    private companion object {
        // Stores opened by a new login and late callbacks from the old login share the same lock.
        val lock = Any()
    }
}
