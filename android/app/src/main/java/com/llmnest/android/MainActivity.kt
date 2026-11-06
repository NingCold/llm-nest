package com.llmnest.android

import android.app.Activity
import android.content.Intent
import android.content.res.Configuration
import android.graphics.Color
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.View
import android.view.WindowInsetsController
import android.view.WindowManager
import kotlinx.coroutines.*
import org.json.JSONArray
import org.json.JSONObject
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.Executors

private fun json(vararg pairs: Pair<String, Any?>) = JSONObject().apply {
    pairs.forEach { (key, value) -> put(key, value ?: JSONObject.NULL) }
}
private fun JSONArray.objects() = (0 until length()).asSequence().map(::getJSONObject)
private suspend fun CryptoHost.perform(op: String, args: JSONObject = JSONObject()): JSONObject =
    suspendCancellableCoroutine { continuation ->
        call(op, args) { if (continuation.isActive) continuation.resumeWith(it) }
    }

/** Login is process-local; a Matrix device and unsent drafts outlive it, scoped to the server and person. */
class MainActivity : Activity(), ChatScreens.Actions {
    private lateinit var ui: ChatScreens
    private val prefs by lazy { getSharedPreferences("connection", MODE_PRIVATE) }
    private var scope = MainScope()
    private val draftIO = Executors.newSingleThreadExecutor().asCoroutineDispatcher()
    private val disk = CoroutineScope(SupervisorJob() + draftIO)
    private var lastWrite: Deferred<DraftRecord>? = null
    private var store: DraftStore? = null
    private var cloud: CloudClient? = null
    private var host: CryptoHost? = null
    private var me: JSONObject? = null
    private var device: JSONObject? = null
    private var snapshot: JSONObject? = null
    private var room: String? = null
    private var accountPage = false
    private var contacts = false
    private var results: JSONArray? = null
    private var page = JSONObject()
    private var draft: DraftRecord? = null
    private var text = ""
    private var error: String? = null
    private var sendingError: String? = null
    private var draftError: String? = null
    private var busy = false
    private var foreground = false
    private var gateLocked = false
    private var changingGate = false
    private var route = 0L
    private var snapshotJob: Job? = null
    private var snapshotQueued = false
    private var historyJob: Job? = null
    private var searchJob: Job? = null
    private var syncJob: Job? = null
    private var readJob: Job? = null
    private var reconnectJob: Job? = null
    private var stream: Closeable? = null
    private var streamGeneration = 0L
    private var depth = 1
    private var historyBusy = false

    private data class Retained(val client: CloudClient, val account: JSONObject, val room: String?, val contacts: Boolean)
    override fun onRetainNonConfigurationInstance(): Any? = me?.let { Retained(cloud!!, it, room, contacts) }
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
        ui = ChatScreens(this, this)
        setContentView(ui.root)
        configureBars()
        val retained = lastNonConfigurationInstance as? Retained
        if (retained != null) {
            cloud = retained.client; me = retained.account; room = retained.room; contacts = retained.contacts
            startDevice()
        }
        render()
        if (Build.VERSION.SDK_INT >= 33) onBackInvokedDispatcher.registerOnBackInvokedCallback(
            android.window.OnBackInvokedDispatcher.PRIORITY_DEFAULT, ::back)
    }

    @Suppress("DEPRECATION")
    private fun configureBars() {
        val light = resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK != Configuration.UI_MODE_NIGHT_YES
        window.statusBarColor = Color.TRANSPARENT
        window.navigationBarColor = Color.TRANSPARENT
        if (Build.VERSION.SDK_INT >= 30) {
            window.setDecorFitsSystemWindows(false)
            val flags = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
            window.insetsController?.setSystemBarsAppearance(if (light) flags else 0, flags)
        } else {
            window.decorView.systemUiVisibility = View.SYSTEM_UI_FLAG_LAYOUT_STABLE or View.SYSTEM_UI_FLAG_LAYOUT_FULLSCREEN or
                View.SYSTEM_UI_FLAG_LAYOUT_HIDE_NAVIGATION or (if (light) View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR or View.SYSTEM_UI_FLAG_LIGHT_NAVIGATION_BAR else 0)
        }
    }

    private fun render() {
        val account = me ?: return ui.login(prefs.getString("origin", "") ?: "", busy, error)
        val data = snapshot
        if (data == null || gateLocked || changingGate) {
            ui.gate(account.optString("name"), busy || changingGate || (data == null && error == null && !gateLocked), gateLocked, error)
            return
        }
        val state = device
        val trusted = state?.optBoolean("trusted") == true
        val confirmed = state?.optBoolean("recoveryConfirmed") == true
        val conversation = selected()
        when {
            accountPage -> ui.account(data, cloud!!.origin, when {
                !trusted -> "尚未准备可信设备"
                !confirmed -> "设备已获信任，请保存并确认恢复文件"
                else -> "此设备已获信任，恢复文件已确认"
            }, busy, error, hasDevice = state?.optBoolean("hasDevice") == true)
            conversation != null -> ui.conversation(conversation, account.getString("id"), page.optJSONArray("messages") ?: JSONArray(),
                page.optBoolean("hasMore"), busy || draft == null || historyBusy && !page.has("messages"), error, text,
                draft?.pending?.let { ChatScreens.PendingSend(it.text, !busy, sendingError) }, trusted && confirmed, data.optBoolean("requireEncryption"), draftSaveError = draftError)
            else -> ui.inbox(data, contacts, busy, error, results = results)
        }
    }
    private fun selected(): JSONObject? = snapshot?.optJSONArray("conversations")?.objects()?.find { it.optString("id") == room }
    private fun job(work: suspend CoroutineScope.() -> Unit): Job = scope.launch {
        try { work() } catch (e: CancellationException) { throw e } catch (e: Exception) { report(e) }
    }
    private fun report(e: Throwable) {
        if (e is CloudError && e.status == 401 && me != null) {
            endSession("登录已失效，请重新登录。")
        } else { error = e.localizedMessage ?: "操作未完成，请重试。"; render() }
    }
    private suspend fun CloudClient.get(path: String) = withContext(Dispatchers.IO) { request("GET", path)!! }

    override fun login(origin: String, email: String, password: String) {
        if (busy || me != null) return
        val client = try { CloudClient(origin.trim()) } catch (e: Exception) { report(e); return }
        cloud?.close(); cloud = client; error = null; busy = true; render()
        job {
            try {
                me = withContext(Dispatchers.IO) { client.login(email.trim(), password) }
                prefs.edit().putString("origin", client.origin).apply()
                busy = false
                startDevice()
            } finally { busy = false; render() }
        }
    }

    private fun startDevice() {
        val client = cloud ?: return
        val id = me!!.getString("id")
        val runtime = CryptoHost(this, client, id)
        runtime.onUnauthorized = { if (host === runtime) endSession("登录已失效，请重新登录。") }
        host = runtime; store = DraftStore(this, client.origin, id)
        changingGate = true; error = null; render()
        job {
            try { device = runtime.perform("state"); gateLocked = device!!.optBoolean("hasDevice") && device!!.optBoolean("locked") }
            finally { changingGate = false; render() }
            if (!gateLocked) refreshFeed()
        }
    }
    override fun unlock() = openDevices()
    override fun crypto() = openDevices()
    private fun openDevices() {
        if (busy || changingGate) return
        val runtime = host ?: return
        runtime.showDevices(selected()?.optJSONObject("peer")?.optString("id")) {
            if (host === runtime) job {
                device = runtime.perform("state")
                gateLocked = device!!.optBoolean("hasDevice") && device!!.optBoolean("locked")
                error = null
                if (gateLocked) clearFeed() else refreshFeed()
                render()
            }
        }
    }
    override fun lock() = afterDraft { lockDevice() }
    private fun lockDevice() {
        if (host == null) return
        val savedRoom = room
        // Remove decrypted lists, draft and subscriptions; the authenticated account is retained.
        stopOperations(); clearFeed(); room = savedRoom
        gateLocked = true; host?.close(); host = null; device = null
        if (foreground) startDevice() else { changingGate = false; render() }
    }

    private fun refreshFeed() {
        if (gateLocked || changingGate || !foreground) return
        if (snapshotJob?.isActive == true) { snapshotQueued = true; return }
        val client = cloud ?: return
        val runtime = host ?: return
        snapshotJob = job {
            do {
                snapshotQueued = false
                val raw = client.get("/chat/init")
                check(raw.getJSONObject("self").getString("id") == me?.getString("id")) { "账号已变化，请重新登录" }
                val updated = runtime.perform("projectSnapshot", json("snapshot" to raw))
                val needsDraft = draft == null
                snapshot = updated
                val selectedId = room
                if (selectedId != null && selected() == null) {
                    changeRoute(); error = "你已无法访问这个聊天。"
                } else if (selectedId != null) {
                    if (needsDraft) loadDraft(selectedId)
                    loadHistory()
                }
                if (stream == null && reconnectJob?.isActive == true) error = null
                subscribe(client, updated.getString("cursor"))
                startSync(runtime)
                render()
            } while (snapshotQueued)
        }
    }
    private fun subscribe(client: CloudClient, cursor: String) {
        if (stream != null || !foreground) return
        val serial = ++streamGeneration
        stream = client.events(cursor,
            { runOnUiThread { if (streamGeneration == serial) refreshFeed() } },
            { failure -> runOnUiThread {
                if (streamGeneration == serial) {
                    stream = null
                    if (failure is CloudError && failure.status == 401) report(failure)
                    else {
                        error = "实时连接已中断，正在重新连接。"; render()
                        reconnectJob?.cancel()
                        reconnectJob = job {
                            while (foreground && stream == null && !gateLocked) {
                                delay(3000)
                                if (stream == null) refreshFeed()
                            }
                        }
                    }
                }
            } })
    }
    private fun startSync(runtime: CryptoHost) {
        if (device?.optBoolean("locked", true) != false || syncJob?.isActive == true) return
        syncJob = job {
            while (foreground) {
                delay(6000)
                try {
                    val outcome = runtime.perform("sync")
                    if (outcome.optBoolean("locked")) { lockDevice(); return@job }
                    if (outcome.optBoolean("changed")) refreshFeed()
                } catch (e: CloudError) {
                    if (e.status == 401) throw e
                    if (e.status == 403 && runtime.perform("state").optBoolean("locked")) {
                        lockDevice(); error = "设备授权已失效，请重新核对设备。"; render(); return@job
                    }
                    error = e.message; render()
                }
            }
        }
    }
    override fun refresh() {
        error = null
        if (gateLocked) { openDevices(); return }
        if (me != null && device == null && !changingGate) {
            host?.close(); startDevice(); return
        }
        refreshFeed(); render()
    }
    private fun stopFeed() {
        streamGeneration++; stream?.close(); stream = null
        reconnectJob?.cancel(); reconnectJob = null
        syncJob?.cancel(); syncJob = null
    }
    private fun clearFeed() {
        stopFeed(); snapshot = null; page = JSONObject(); draft = null; text = ""; depth = 1
        sendingError = null; draftError = null; results = null; lastWrite = null
    }

    private fun changeRoute() {
        route++; searchJob?.cancel(); historyJob?.cancel(); readJob?.cancel(); historyBusy = false
        room = null; accountPage = false; page = JSONObject(); depth = 1
        draft = null; text = ""; lastWrite = null; error = null; sendingError = null; draftError = null
    }
    /** Do not leave a failed write behind as a seemingly saved draft. */
    private fun afterDraft(action: () -> Unit) {
        if (busy) return
        val write = lastWrite
        val view = route
        if (write == null) action() else job { write.await(); if (route == view && write === lastWrite) action() }
    }
    override fun selectTab(contacts: Boolean) = afterDraft {
        changeRoute(); this.contacts = contacts; results = null; render()
    }
    override fun account() = afterDraft { changeRoute(); accountPage = true; render() }
    override fun openConversation(id: String) = afterDraft { enter(id) }
    private fun enter(id: String) {
        changeRoute(); room = id; contacts = false; results = null
        if (selected() == null) { refreshFeed(); return }
        loadDraft(id); loadHistory(); render()
    }
    private fun loadDraft(id: String) {
        val view = route; val journal = store!!
        job {
            val value = try { withContext(draftIO) { journal.load(id) } } catch (e: Exception) {
                if (isActive && view == route) { draftError = "无法读取本机草稿，请重试。"; render() }; return@job
            }
            if (view == route && room == id) { draft = value; text = value.text; draftError = null; render() }
        }
    }
    override fun searchContacts(query: String) {
        val client = cloud ?: return
        searchJob?.cancel(); error = null
        if (query.isBlank()) { results = null; render(); return }
        val view = route
        searchJob = job {
            val found = withContext(Dispatchers.IO) { client.exchange("GET", "/chat/accounts?query=" + Uri.encode(query.trim())) as JSONArray }
            if (route == view) { results = found; render() }
        }
    }
    override fun openContact(id: String) {
        if (busy) return
        val client = cloud ?: return
        busy = true; error = null; render()
        job {
            try {
                val conversation = withContext(Dispatchers.IO) { client.request("POST", "/chat/direct", json("accountId" to id))!! }
                enter(conversation.getString("id")); refreshFeed()
            } finally { busy = false; render() }
        }
    }

    private fun loadHistory() {
        val conversation = selected() ?: return
        val client = cloud!!; val runtime = host!!; val id = conversation.getString("id")
        val count = depth
        historyJob?.cancel()
        historyBusy = true
        historyJob = job {
            try {
                var before = ""
                val chunks = ArrayList<JSONArray>()
                var more = true
                var latest = ""
                while (chunks.size < count && more) {
                    val suffix = if (before.isEmpty()) "" else "?before=" + Uri.encode(before)
                    val raw = client.get("/chat/conversations/$id/messages$suffix")
                    val projected = runtime.perform("projectPage", json("page" to raw, "conversation" to conversation))
                    val messages = projected.getJSONArray("messages")
                    if (chunks.isEmpty()) latest = projected.getString("latestPosition")
                    chunks.add(messages)
                    more = projected.optBoolean("hasMore") && messages.length() > 0
                    if (more) before = messages.getJSONObject(0).getString("position")
                }
                val all = JSONArray()
                chunks.asReversed().forEach { it.objects().forEach(all::put) }
                page = json("messages" to all, "hasMore" to more, "latestPosition" to latest)
            } catch (e: CloudError) {
                if (e.status == 403 || e.status == 404) { page = JSONObject(); render() }
                throw e
            } finally {
                if (isActive) { historyBusy = false; render() }
            }
        }
    }
    override fun loadOlder() {
        if (!historyBusy && page.optBoolean("hasMore")) { depth++; loadHistory() }
    }
    override fun visibleLatest() {
        val conv = selected() ?: return
        if (!foreground || !hasWindowFocus() || !ui.isAtLatest() || accountPage || gateLocked || conv.optInt("unreadCount") == 0 || readJob?.isActive == true) return
        val latest = page.optString("latestPosition")
        val rows = page.optJSONArray("messages") ?: return
        if (latest.isBlank() || latest != conv.optString("latestPosition") || latest != rows.optJSONObject(rows.length()-1)?.optString("position")) return
        val client = cloud!!; val id = conv.getString("id")
        readJob = job {
            try {
                withContext(Dispatchers.IO) {
                    client.request("PUT","/chat/conversations/$id/list",json("requestId" to uuid(),"expectedRevision" to conv.getString("listRevision"),"readPosition" to latest,"manualUnread" to false))
                }
            } catch (e: CloudError) { if (e.status != 409) throw e }
            refreshFeed()
        }
    }
    override fun onWindowFocusChanged(hasFocus: Boolean) { super.onWindowFocusChanged(hasFocus); if (hasFocus && ::ui.isInitialized) visibleLatest() }

    override fun draftChanged(text: String) {
        val id = room ?: return
        if (draft == null || this.text == text) return
        this.text = text
        persistText(id, text)
    }
    override fun retryDraftSave() {
        val id = room ?: return
        if (draft == null) loadDraft(id) else persistText(id, text)
    }
    private fun persistText(id: String, text: String) {
        val journal = store!!; val view = route
        val write = disk.async { journal.saveText(id, text) }
        lastWrite = write
        job {
            try {
                val value = write.await()
                if (route == view && write === lastWrite) {
                    draft = value
                    if (draftError != null) { draftError = null; render() }
                }
            } catch (e: Exception) { if (route == view && write === lastWrite && isActive) { draftError = "草稿未能保存，请重试。"; render() } }
        }
    }
    override fun send(text: String) = sendText(text, false)
    override fun retryPending() = sendText(text, true)
    private fun sendText(input: String, retry: Boolean) {
        val c = selected() ?: return
        val id = c.getString("id"); val journal = store!!; val client = cloud!!; val runtime = host!!
        if (busy || draftError != null || draft == null || c.optBoolean("directBlocked") || c.optBoolean("readOnly")) return
        if (!retry && (input.isBlank() || draft!!.pending != null)) return
        if (!retry && input != text) draftChanged(input)
        val view = route
        busy = true; error = null; sendingError = null; render()
        job {
            var dispatched: PreparedCommand? = null
            try {
                lastWrite?.await()
                val original = withContext(draftIO) { journal.load(id) }
                val cmd = if (retry) original.pending ?: return@job else {
                    check(original.text == input && original.pending == null) { "草稿已改变，请核对后发送" }
                    val requestId = uuid()
                    val body = if (c.optJSONObject("encryption") != null) runtime.perform("prepareSend",
                        json("conversation" to c, "text" to input, "requestId" to requestId))
                    else json("requestId" to requestId, "text" to input, "mentions" to JSONArray(), "replyTo" to null)
                    val latest = selected() ?: error("聊天已关闭")
                    requireAllowed(latest, body)
                    withContext(draftIO) {
                        check(journal.load(id).revision == original.revision) { "草稿已改变，请核对后发送" }
                        journal.prepare(id, body)
                    }
                }
                requireAllowed(selected() ?: error("聊天已关闭"), cmd.body)
                dispatched = cmd
                withContext(Dispatchers.IO) { client.request("POST", "/chat/conversations/$id/messages", cmd.body) }
                val updated = withContext(draftIO) { journal.acknowledge(id, cmd.requestId) }
                if (route == view) { draft = updated; text = updated.text; lastWrite = null }
                refreshFeed()
            } catch (e: CancellationException) { throw e }
            catch (e: Exception) {
                val sent = dispatched
                val updated = withContext(draftIO) {
                    if (sent != null && !retry && e is CloudError && e.status in setOf(400,403,404,409,413,415,422)) journal.discard(id, sent.requestId)
                    else journal.load(id)
                }
                if (view == route) { draft = updated; text = updated.text; sendingError = if (updated.pending != null) e.message else null }
                throw e
            } finally { if (isActive) { busy = false; render() } }
        }
    }
    private fun requireAllowed(c: JSONObject, body: JSONObject) {
        check(!c.optBoolean("directBlocked") && !c.optBoolean("readOnly")) { "当前聊天不可发送" }
        if (snapshot!!.optBoolean("requireEncryption") || c.optJSONObject("encryption") != null)
            check(body.optJSONObject("sealed") != null && body.optString("text").isEmpty()) { "此聊天要求密文，不能重发明文" }
    }
    override fun discardPending() {
        val pending = draft?.pending ?: return
        val id = room!!; val journal = store!!
        afterDraft {
            job {
                val updated = withContext(draftIO) { journal.discard(id, pending.requestId) }
                if (room == id) { draft = updated; text = updated.text; sendingError = null; error = null; render() }
            }
        }
    }
    override fun enableEncryption() {
        val conversation = selected() ?: return
        if (busy) return
        if (device?.optBoolean("trusted") != true || device?.optBoolean("recoveryConfirmed") != true) {
            openDevices(); return
        }
        busy = true; error = null; render()
        job {
            try { host!!.perform("enable", json("conversation" to conversation)); refreshFeed() }
            finally { busy = false; render() }
        }
    }

    override fun logout() = afterDraft {
        val client = cloud ?: return@afterDraft
        // Remove chat and keys immediately. Failed remote revocation is reported on the login screen.
        endSession(null, closeCloud = false)
        busy = true; render()
        job {
            try { withContext(Dispatchers.IO) { client.logout() } }
            catch (e: Exception) { if (e !is CloudError || e.status != 401) error = "已退出本机；服务器未确认退出，可在其他设备撤销会话。" }
            finally { client.close(); busy = false; render() }
        }
    }
    private fun stopOperations() {
        route++; scope.cancel(); scope = MainScope()
        busy = false; historyBusy = false; snapshotJob = null; historyJob = null
    }
    private fun endSession(reason: String?, closeCloud: Boolean = true) {
        stopOperations(); clearFeed(); host?.close(); host = null
        if (closeCloud) cloud?.close()
        cloud = null; me = null; store = null; device = null; room = null; contacts = false; accountPage = false
        gateLocked = false; changingGate = false; error = reason; render()
    }
    override fun back() {
        if (busy) return
        when {
            room != null || accountPage -> selectTab(contacts)
            else -> afterDraft { finish() }
        }
    }
    @android.annotation.SuppressLint("GestureBackNavigation") // API 33+ uses the registered OnBackInvokedCallback.
    @Deprecated("Legacy platform back")
    override fun onBackPressed() = back()
    @Deprecated("Platform document picker")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        super.onActivityResult(requestCode, resultCode, data)
        host?.onActivityResult(requestCode, resultCode, data)
    }
    override fun onStart() {
        super.onStart(); foreground = true
        if (me != null && host == null) startDevice() else if (me != null && device != null) refreshFeed()
    }
    override fun onStop() {
        foreground = false; stopFeed()
        if (host != null && host?.isPickingDocument() != true && !isChangingConfigurations) {
            val write = lastWrite
            if (write == null) lockDevice()
            else job { write.await(); if (!foreground && write === lastWrite) lockDevice() }
        }
        super.onStop()
    }
    override fun onDestroy() {
        stopFeed(); scope.cancel(); host?.close()
        if (!isChangingConfigurations || me == null) cloud?.close()
        // Disk jobs queued by typing continue in FIFO order even if Android destroys this Activity.
        disk.launch { draftIO.close() }
        super.onDestroy()
    }
    private fun uuid() = UUID.randomUUID().toString()
}
