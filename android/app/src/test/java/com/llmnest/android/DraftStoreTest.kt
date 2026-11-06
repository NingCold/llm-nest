package com.llmnest.android

import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test

class DraftStoreTest {
    private class MemoryFile : DraftFile {
        var value: String? = null
        var fail = false
        override fun read() = value
        override fun write(value: String) { if (fail) error("disk failure with private content"); this.value = value }
    }

    @Test fun pendingCommandSurvivesReloginWithoutMutatingPreparedPayload() {
        val file = MemoryFile()
        val store = DraftStore("https://a.test", "alice", file)
        store.saveText("room", "draft")
        val input = JSONObject().put("requestId", "fixed-request").put("sealed", JSONObject().put("ciphertext", "exact"))
        val prepared = store.prepare("room", input)
        input.put("requestId", "changed")
        prepared.body.put("requestId", "also-changed")
        val loaded = DraftStore("https://a.test", "alice", file).load("room")
        assertEquals("draft", loaded.text)
        assertEquals(prepared.bodyJson, loaded.pending!!.bodyJson)
        assertEquals("fixed-request", loaded.pending.body.getString("requestId"))
    }

    @Test fun lateAcknowledgementPreservesNewerTextAndDifferentPendingRequest() {
        val store = DraftStore("https://a.test", "alice", MemoryFile())
        store.saveText("room", "old")
        store.prepare("room", JSONObject().put("requestId", "one"))
        store.saveText("room", "new")
        assertEquals("old", store.load("room").pending!!.text)
        assertEquals("new", store.acknowledge("room", "one").text)
        store.prepare("room", JSONObject().put("requestId", "two"))
        assertEquals("two", store.acknowledge("room", "one").pending!!.requestId)
        assertEquals("new", store.discard("room", "two").text)
        assertNull(store.load("room").pending)
    }

    @Test fun unchangedDraftClearsOnlyAfterAcknowledgementAndUnresolvedSendCannotBeReplaced() {
        val store = DraftStore("https://a.test", "alice", MemoryFile())
        store.saveText("room", "old")
        store.prepare("room", JSONObject().put("requestId", "one"))
        try {
            store.prepare("room", JSONObject().put("requestId", "two"))
            fail("unresolved sends cannot be replaced")
        } catch (_: DraftError) { }
        assertEquals("", store.acknowledge("room", "one").text)
        assertNull(store.load("room").pending)
    }

    @Test fun serverAccountAndConversationIsolation() {
        val file = MemoryFile()
        val alice = DraftStore("https://a.test", "alice", file)
        alice.saveText("room", "private")
        assertEquals("", DraftStore("https://b.test", "alice", file).load("room").text)
        assertEquals("", DraftStore("https://a.test", "bob", file).load("room").text)
        assertEquals("", alice.load("another-room").text)
    }

    @Test fun writeFailurePreventsPreparedCommandAndCorruptionIsNotOverwritten() {
        val file = MemoryFile()
        val store = DraftStore("https://a.test", "alice", file)
        store.saveText("room", "private")
        file.fail = true
        try { store.prepare("room", JSONObject().put("requestId", "one")); fail("must fail") }
        catch (error: DraftError) { assertFalse(error.toString().contains("private")) }
        assertNull(store.load("room").pending)
        file.fail = false
        file.value = "not-json"
        try { store.saveText("room", "new"); fail("must preserve corruption") } catch (_: DraftError) { }
        assertEquals("not-json", file.value)
    }
}
