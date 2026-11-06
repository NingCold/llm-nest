package com.llmnest.android

import android.app.Activity
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.Canvas
import android.view.View
import android.view.ViewGroup
import android.webkit.WebView
import android.widget.EditText
import android.widget.TextView
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.json.JSONObject
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import java.io.File
import java.util.UUID
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

/** Explicit disposable backend only. No account discovery, personal fixtures or model requests. */
@RunWith(AndroidJUnit4::class)
class CryptoInteropTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()
    private val hosts = mutableListOf<CryptoHost>()
    private val clients = mutableListOf<CloudClient>()
    private val files = mutableListOf<File>()
    private lateinit var activity: Activity

    private fun json(vararg fields: Pair<String, Any?>) = JSONObject().apply { fields.forEach { put(it.first, it.second ?: JSONObject.NULL) } }
    private fun <T> main(work: () -> T): T {
        val result = AtomicReference<Result<T>>()
        instrumentation.runOnMainSync { result.set(runCatching(work)) }
        return result.get().getOrThrow()
    }
    private fun invoke(host: CryptoHost, op: String, args: JSONObject = JSONObject()): JSONObject {
        val result = AtomicReference<Result<JSONObject>>()
        val done = CountDownLatch(1)
        android.util.Log.i("CryptoInterop", "Starting operation $op")
        main { host.call(op, args) { result.set(it); done.countDown() } }
        if (!done.await(10, TimeUnit.SECONDS)) main {
            val field = CryptoHost::class.java.getDeclaredField("web").apply { isAccessible = true }
            val web = field.get(host) as android.webkit.WebView
            android.util.Log.i("CryptoInterop", "Pending $op; attached=${web.isAttachedToWindow}")
            // Fixed stage names/durations only. No account IDs, keys, phrases or request bodies.
            web.evaluateJavascript("JSON.stringify({secure:isSecureContext,hidden:document.hidden,ready:document.readyState,measures:performance.getEntriesByType('measure').map(m=>({name:m.name,duration:m.duration}))})") { diagnostic -> android.util.Log.i("CryptoInterop", "Runtime diagnostic: $diagnostic") }
        }
        assertTrue("Crypto operation $op timed out", done.await(120, TimeUnit.SECONDS))
        return result.get().getOrElse { throw IllegalStateException("Crypto operation failed: $op", it) }
    }
    private fun host(client: CloudClient, account: String) = main { CryptoHost(activity, client, account) }.also { hosts.add(it) }
    private fun client(origin: String, email: String, password: String): Pair<CloudClient, String> {
        val client = CloudClient(origin).also { clients.add(it) }
        return client to client.login(email, password).getString("id")
    }
    private fun newIdentity(host: CryptoHost, passphrase: String): JSONObject {
        val before = invoke(host, "state")
        assertTrue("Use fresh disposable fixture accounts", before.getBoolean("locked"))
        assertFalse("Fixture must not reuse this app's old crypto device", before.getBoolean("hasDevice"))
        invoke(host, "unlock", json("passphrase" to passphrase, "create" to true))
        assertFalse(invoke(host, "state").getBoolean("trusted"))
        invoke(host, "initialize")
        val state = invoke(host, "state")
        assertTrue(state.getBoolean("trusted"))
        assertFalse(state.getBoolean("recoveryConfirmed"))
        return state
    }
    private fun saveRecovery(host: CryptoHost): JSONObject {
        val recovery = invoke(host, "createRecovery")
        assertFalse("Creating a recovery file alone never confirms it", invoke(host, "state").getBoolean("recoveryConfirmed"))
        val file = File.createTempFile("llmn-disposable-recovery-", ".json", instrumentation.targetContext.cacheDir).also { files.add(it) }
        file.writeText(recovery.toString(), Charsets.UTF_8)
        val readback = JSONObject(file.readText(Charsets.UTF_8))
        assertEquals(recovery.getString("version"), readback.getString("version"))
        invoke(host, "commitRecovery", json("file" to readback))
        assertTrue(invoke(host, "state").getBoolean("recoveryConfirmed"))
        return readback
    }
    private fun verify(host: CryptoHost, peer: String, independent: JSONObject) {
        val devices = invoke(host, "devices", json("account" to peer)).getJSONArray("devices")
        val device = (0 until devices.length()).map { devices.getJSONObject(it) }.single { it.getString("deviceId") == independent.getString("deviceId") }
        // The expected values come from the other independently initialized local SDK, not this directory response.
        for (key in listOf("master", "fingerprint", "curve")) assertEquals("Independent $key verification", independent.getString(key), device.getString(key))
        invoke(host, "trust", json("account" to peer, "deviceId" to independent.getString("deviceId"), "master" to independent.getString("master"), "fingerprint" to independent.getString("fingerprint"), "curve" to independent.getString("curve")))
    }
    private fun conversation(client: CloudClient, id: String): JSONObject {
        val conversations = client.request("GET", "/chat/init")!!.getJSONArray("conversations")
        return (0 until conversations.length()).map { conversations.getJSONObject(it) }.single { it.getString("id") == id }
    }
    private fun descendants(view: View): Sequence<View> = sequence {
        yield(view)
        if (view is ViewGroup) for (index in 0 until view.childCount) yieldAll(descendants(view.getChildAt(index)))
    }
    private fun root() = activity.findViewById<ViewGroup>(android.R.id.content).getChildAt(0)
    private fun visible(label: String) = main { descendants(root()).filterIsInstance<TextView>().any { it !is EditText && it.text.toString() == label && it.isShown } }
    private fun waitFor(label: String, condition: () -> Boolean) {
        val deadline = System.currentTimeMillis() + 90_000
        while (System.currentTimeMillis() < deadline) { if (condition()) return; Thread.sleep(150) }
        fail("Timed out waiting for $label")
    }
    private fun edit(description: String, value: String) = main {
        descendants(root()).filterIsInstance<EditText>().single { it.contentDescription?.toString() == description }.setText(value)
    }
    private fun tap(label: String) = main {
        var view: View = descendants(root()).filterIsInstance<TextView>().first { it !is EditText && it.text.toString() == label && it.isShown }
        while (!view.isClickable) view = view.parent as View
        check(view.isEnabled && view.performClick()) { "Control $label is not actionable" }
    }
    private fun mainHost() = main { MainActivity::class.java.getDeclaredField("host").apply { isAccessible = true }.get(activity) as CryptoHost }
    private fun evaluate(host: CryptoHost, script: String): String {
        val result = AtomicReference<String>()
        val latch = CountDownLatch(1)
        main {
            val web = CryptoHost::class.java.getDeclaredField("web").apply { isAccessible = true }.get(host) as WebView
            web.evaluateJavascript(script) { result.set(it); latch.countDown() }
        }
        check(latch.await(10, TimeUnit.SECONDS)) { "Local setup UI did not respond" }
        return result.get()
    }
    private fun messages(client: CloudClient, id: String): List<JSONObject> {
        val rows = client.request("GET", "/chat/conversations/$id/messages")!!.getJSONArray("messages")
        return (0 until rows.length()).map { rows.getJSONObject(it) }
    }

    @Test fun officialMatrixWasmInteroperatesAcrossTrustedDevicesAndRecovery() {
        val args = InstrumentationRegistry.getArguments()
        fun arg(key: String) = requireNotNull(args.getString(key)) { "Supply disposable fixture argument $key" }
        val origin = arg("origin")
        val a = client(origin, arg("emailA"), arg("passwordA"))
        val b = client(origin, arg("emailB"), arg("passwordB"))
        activity = instrumentation.startActivitySync(Intent(instrumentation.targetContext, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        try {
            val passA = "disposable-A-${UUID.randomUUID()}"
            val passB = "disposable-B-${UUID.randomUUID()}"
            val hostA = host(a.first, a.second)
            val hostB = host(b.first, b.second)
            val stateA = newIdentity(hostA, passA)
            val stateB = newIdentity(hostB, passB)
            val recoveryA = saveRecovery(hostA)
            val recoveryB = saveRecovery(hostB)
            assertEquals(a.second, recoveryA.getString("accountId"))
            val direct = a.first.request("POST", "/chat/direct", json("accountId" to b.second))!!
            val id = direct.getString("id")
            verify(hostA, b.second, stateB)
            verify(hostB, a.second, stateA)

            invoke(hostA, "enable", json("conversation" to direct))
            val encrypted = conversation(a.first, id)
            assertEquals(1, encrypted.getJSONObject("encryption").getInt("version"))
            val plaintext = "disposable Android official SDK message ${UUID.randomUUID()}"
            val command = invoke(hostA, "prepareSend", json("conversation" to encrypted, "text" to plaintext, "requestId" to UUID.randomUUID().toString()))
            assertEquals("", command.getString("text"))
            assertFalse("Plaintext must never occur in the server command", command.toString().contains(plaintext))
            assertEquals("m.megolm.v1.aes-sha2", command.getJSONObject("sealed").getJSONObject("content").getString("algorithm"))
            a.first.request("POST", "/chat/conversations/$id/messages", command)
            invoke(hostB, "sync")
            val raw = b.first.request("GET", "/chat/conversations/$id/messages")!!
            val projected = invoke(hostB, "projectPage", json("page" to raw, "conversation" to conversation(b.first, id)))
            val messageId = command.getString("messageId")
            val readable = projected.getJSONArray("messages").let { messages -> (0 until messages.length()).map { messages.getJSONObject(it) }.single { it.getString("id") == messageId } }
            assertEquals(plaintext, readable.getString("text"))
            assertTrue(readable.getBoolean("_decrypted"))
            assertFalse(readable.has("cryptoError"))

            val forged = JSONObject(raw.toString())
            val injected = forged.getJSONArray("messages").getJSONObject(forged.getJSONArray("messages").length() - 1)
            injected.remove("sealed"); injected.put("text", "forged plaintext")
            val rejected = invoke(hostB, "projectPage", json("page" to forged, "conversation" to encrypted)).getJSONArray("messages")
            assertEquals("", rejected.getJSONObject(rejected.length() - 1).getString("text"))
            assertTrue(rejected.getJSONObject(rejected.length() - 1).getString("cryptoError").contains("明文回退"))
            val tampered = JSONObject(raw.toString())
            val last = tampered.getJSONArray("messages").getJSONObject(tampered.getJSONArray("messages").length() - 1)
            last.getJSONObject("sealed").getJSONObject("content").put("ciphertext", "tampered")
            val invalid = invoke(hostB, "projectPage", json("page" to tampered, "conversation" to encrypted)).getJSONArray("messages")
            assertEquals("", invalid.getJSONObject(invalid.length() - 1).getString("text"))
            assertTrue(invalid.getJSONObject(invalid.length() - 1).getString("cryptoError").isNotBlank())

            // Back up the newly received Megolm room key, then restore it into an explicitly new device.
            invoke(hostB, "sync")
            main { hostB.close() }
            val restored = host(b.first, b.second)
            assertTrue(invoke(restored, "state").getBoolean("locked"))
            invoke(restored, "unlock", json("passphrase" to passB, "create" to "new-device"))
            assertFalse(invoke(restored, "state").getBoolean("trusted"))
            invoke(restored, "restoreRecovery", json("file" to recoveryB))
            assertTrue(invoke(restored, "state").getBoolean("trusted"))
            verify(restored, a.second, stateA)
            val history = invoke(restored, "projectPage", json("page" to raw, "conversation" to encrypted)).getJSONArray("messages")
            assertEquals(plaintext, (0 until history.length()).map { history.getJSONObject(it) }.single { it.getString("id") == messageId }.getString("text"))
            invoke(restored, "lock")
            assertTrue(invoke(restored, "state").getBoolean("locked"))
            val failed = runCatching { invoke(restored, "unlock", json("passphrase" to "incorrect-passphrase")) }
            assertTrue(failed.isFailure)
            assertTrue(invoke(restored, "state").getBoolean("locked"))
            invoke(restored, "unlock", json("passphrase" to passB))
            assertTrue(invoke(restored, "state").getBoolean("trusted"))

            // Exercise the actual native login, manual device-unlock form, composer, projection and SSE reader.
            // A has exactly one active SDK host: the temporary one is closed before Main opens its own.
            main { hostA.close() }
            waitFor("native login") { visible("回到你的聊天") }
            edit("服务器地址", origin); edit("邮箱", arg("emailA")); edit("密码", arg("passwordA")); tap("登录")
            waitFor("manual crypto gate") { visible("聊天已锁定") }
            tap("解锁加密设备")
            val native = mainHost()
            evaluate(native, "document.querySelector('input[type=password]').value=${JSONObject.quote(passA)}; Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='解锁本机资料').click()")
            waitFor("actual setup form unlock") { evaluate(native, "document.body.textContent.includes('本机设备 · 已授权')") == "true" }
            evaluate(native, "Array.from(document.querySelectorAll('button')).find(b=>b.textContent==='完成，返回聊天').click()")
            val peerName = conversation(a.first, id).getJSONObject("peer").getString("name")
            waitFor("native decrypted inbox") { visible("账号") && visible(peerName) }
            tap(peerName)
            waitFor("native encrypted history") { visible(plaintext) }
            val nativeText = "安卓原生密文发送 ${UUID.randomUUID()}"
            val before = messages(b.first, id).map { it.getString("id") }.toSet()
            edit("写消息…", nativeText)
            waitFor("native enabled send") { main { descendants(root()).filterIsInstance<TextView>().any { it.text.toString() == "发送" && it.isEnabled } } }
            tap("发送")
            waitFor("native sealed server command") { messages(b.first, id).any { it.getString("id") !in before && it.optString("senderId") == a.second } }
            val sentByNative = messages(b.first, id).single { it.getString("id") !in before && it.optString("senderId") == a.second }
            assertEquals("", sentByNative.getString("text")); assertTrue(sentByNative.has("sealed"))
            assertFalse(sentByNative.toString().contains(nativeText))
            invoke(restored, "sync")
            val nativePage = b.first.request("GET", "/chat/conversations/$id/messages")!!
            val receivedByPeer = invoke(restored, "projectPage", json("page" to nativePage, "conversation" to encrypted)).getJSONArray("messages")
            assertEquals(nativeText, (0 until receivedByPeer.length()).map { receivedByPeer.getJSONObject(it) }.single { it.getString("id") == sentByNative.getString("id") }.getString("text"))
            waitFor("native own decrypted acknowledgement") { visible(nativeText) }
            val reply = "加密实时回复 ${UUID.randomUUID()}"
            val response = invoke(restored, "prepareSend", json("conversation" to conversation(b.first, id), "text" to reply, "requestId" to UUID.randomUUID().toString()))
            b.first.request("POST", "/chat/conversations/$id/messages", response)
            waitFor("native encrypted SSE read without refresh") { visible(reply) }
            main {
                val view = root()
                assertFalse("Native chat remains native after decryption", descendants(view).any { it is WebView })
                val bitmap = Bitmap.createBitmap(view.width, view.height, Bitmap.Config.ARGB_8888)
                view.draw(Canvas(bitmap))
                File(instrumentation.targetContext.cacheDir, "native-encrypted-chat.png").outputStream().use { bitmap.compress(Bitmap.CompressFormat.PNG, 100, it) }
                bitmap.recycle()
            }
            main { restored.close() }
            assertTrue(runCatching { invoke(restored, "state") }.isFailure)
        } finally {
            main { hosts.forEach { it.close() }; activity.finish() }
            clients.forEach { it.close() }
            // Complete cleanup list is only this test's exact, newly-created private cache files.
            val cache = instrumentation.targetContext.cacheDir.canonicalFile
            val cleanup = files.map { it.canonicalFile }
            cleanup.forEach { require(it.parentFile == cache && it.name.startsWith("llmn-disposable-recovery-")) }
            cleanup.forEach { if (it.exists()) check(it.delete()) { "Could not remove disposable recovery fixture" } }
            assertTrue(cleanup.none { it.exists() })
        }
    }
}
