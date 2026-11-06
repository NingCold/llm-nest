package com.llmnest.android

import android.app.Activity
import android.app.AlertDialog
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.graphics.drawable.StateListDrawable
import android.os.Build
import android.text.Editable
import android.text.InputType
import android.text.TextWatcher
import android.view.Gravity
import android.view.View
import android.view.ViewGroup
import android.view.WindowInsets
import android.view.inputmethod.EditorInfo
import android.widget.*
import org.json.JSONArray
import org.json.JSONObject
import java.time.OffsetDateTime
import java.time.ZoneId
import java.time.ZonedDateTime
import java.time.format.DateTimeFormatter
import java.util.Locale

/** Native presentation only. MainActivity owns navigation, requests, drafts and cryptographic state. */
class ChatScreens(private val activity: Activity, private val actions: Actions) {
    interface Actions {
        fun login(origin: String, email: String, password: String)
        fun selectTab(contacts: Boolean)
        fun openConversation(id: String)
        fun openContact(id: String)
        fun searchContacts(query: String)
        fun account()
        fun crypto()
        fun unlock()
        fun lock()
        fun enableEncryption()
        fun refresh()
        fun logout()
        fun back()
        fun send(text: String)
        fun draftChanged(text: String)
        fun retryDraftSave()
        fun loadOlder()
        fun retryPending()
        fun discardPending()
        fun visibleLatest()
    }
    data class PendingSend(val text: String, val unknown: Boolean, val error: String? = null)

    val root = LinearLayout(activity).apply {
        orientation = LinearLayout.VERTICAL
        setBackgroundColor(color(R.color.surface))
        setOnApplyWindowInsetsListener { view, insets ->
            if (Build.VERSION.SDK_INT >= 30) {
                val bars = insets.getInsets(WindowInsets.Type.systemBars() or WindowInsets.Type.displayCutout())
                val ime = insets.getInsets(WindowInsets.Type.ime())
                view.setPadding(bars.left, bars.top, bars.right, maxOf(bars.bottom, ime.bottom))
            } else {
                @Suppress("DEPRECATION")
                view.setPadding(insets.systemWindowInsetLeft, insets.systemWindowInsetTop,
                    insets.systemWindowInsetRight, insets.systemWindowInsetBottom)
            }
            insets
        }
    }
    private var currentPage = ""
    private var originInput: EditText? = null
    private var emailInput: EditText? = null
    private var passwordInput: EditText? = null
    private var composer: EditText? = null
    private var composerWatcher: TextWatcher? = null
    private var history: ScrollView? = null
    private var chatHeader: ConversationBar? = null
    private var chatBody: LinearLayout? = null
    private var chatStatus: LinearLayout? = null
    private var chatComposerRow: LinearLayout? = null
    private var chatSend: Button? = null
    private var composerAllowed = false
    private var chatRenderRevision = 0L
    private var loginFeedback: LinearLayout? = null
    private var loginButton: Button? = null
    private val conversationScroll = mutableMapOf<String, Int>()
    private val savedListScroll = mutableMapOf<String, Int>()
    private var listScroll: ScrollView? = null
    private var searchInput: EditText? = null
    private var searchQuery = ""
    private var firstMessageId = ""

    fun login(origin: String, busy: Boolean = false, error: String? = null) {
        if (currentPage != "login") {
            begin("login")
            val body = column(24)
            scrollBody().addView(body)
            body.addView(text("LLM NEST", 13, secondary = true).apply {
                letterSpacing = .16f; setPadding(0, dp(28), 0, dp(16))
            })
            body.addView(text("回到你的聊天", 30, bold = true, heading = true))
            body.addView(text("登录已有账号，在你的服务器上继续对话。", 16, secondary = true).apply {
                setPadding(0, dp(10), 0, dp(28))
            })
            body.addView(label("服务器地址"))
            originInput = field("https://chat.example.com", InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_URI).apply {
                contentDescription = "服务器地址"; setText(origin); imeOptions = EditorInfo.IME_ACTION_NEXT
            }
            body.addView(originInput)
            body.addView(text("使用 HTTPS，只记住服务器地址。", 13, secondary = true).apply { setPadding(0, dp(6), 0, dp(18)) })
            body.addView(label("邮箱"))
            emailInput = field("你的账号邮箱", InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS).apply { contentDescription = "邮箱"; imeOptions = EditorInfo.IME_ACTION_NEXT }
            body.addView(emailInput)
            body.addView(label("密码").apply { setPadding(0, dp(18), 0, dp(8)) })
            passwordInput = field("账号密码", InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD).apply {
                contentDescription = "密码"
                imeOptions = EditorInfo.IME_ACTION_DONE
                setOnEditorActionListener { _, id, _ ->
                    if (id == EditorInfo.IME_ACTION_DONE) { loginButton?.performClick(); true } else false
                }
            }
            body.addView(passwordInput)
            loginFeedback = column().also { body.addView(it) }
            loginButton = button("登录", primary = true) {
                val server = originInput!!.text.toString().trim()
                val email = emailInput!!.text.toString().trim()
                when {
                    !server.startsWith("https://", ignoreCase = true) -> { originInput!!.error = "请输入 HTTPS 服务器地址"; originInput!!.requestFocus() }
                    email.isBlank() -> { emailInput!!.error = "请输入邮箱"; emailInput!!.requestFocus() }
                    passwordInput!!.text.isEmpty() -> { passwordInput!!.error = "请输入密码"; passwordInput!!.requestFocus() }
                    else -> actions.login(server, email, passwordInput!!.text.toString())
                }
            }.also { body.addView(it, full().apply { topMargin = dp(20) }) }
            body.addView(text("会话仅保留在当前运行期间；退出后需要重新登录。", 13, secondary = true).apply { setPadding(0, dp(16), 0, dp(24)) })
        }
        loginFeedback!!.removeAllViews()
        if (error != null) loginFeedback!!.addView(notice(error, error = true))
        loginButton!!.isEnabled = !busy
        loginButton!!.text = if (busy) "正在登录…" else "登录"
        listOf(originInput, emailInput, passwordInput).forEach { it?.isEnabled = !busy }
    }

    fun gate(accountName: String, busy: Boolean, locked: Boolean, error: String?) {
        begin("gate")
        header("LLM Nest", null, "退出", { actions.logout() })
        val body = column(24)
        scrollBody().addView(body)
        body.addView(empty(if (locked) "聊天已锁定" else if (busy) "正在连接" else "暂时无法读取聊天", accountName, R.drawable.ic_message))
        if (error != null) body.addView(notice(error, true))
        body.addView(button(if (locked) "解锁加密设备" else "重新连接", primary = true) { if (locked) actions.unlock() else actions.refresh() }.apply { isEnabled = !busy }, full())
        body.addView(button("设备与加密设置") { actions.crypto() }, full())
    }

    fun inbox(snapshot: JSONObject, contacts: Boolean = false, busy: Boolean = false, error: String? = null,
              notice: String? = null, results: JSONArray? = null) {
        val key = if (contacts) "contacts" else "messages"
        val restoreScroll = if (currentPage == key) listScroll?.scrollY ?: 0 else savedListScroll[key] ?: 0
        if (currentPage == "contacts") searchQuery = searchInput?.text?.toString() ?: searchQuery
        val oldSearch = if (currentPage == "contacts") searchInput else null
        val searchFocused = oldSearch?.hasFocus() == true
        val searchSelection = oldSearch?.selectionStart ?: searchQuery.length
        begin(key)
        header(activity.getString(if (contacts) R.string.contacts else R.string.messages), null, "账号", { actions.account() })
        val info = row().apply { setPadding(dp(20), 0, dp(12), dp(8)); gravity = Gravity.CENTER_VERTICAL }
        info.addView(text(snapshot.optJSONObject("self")?.optString("name").orEmpty(), 14, secondary = true), weight())
        info.addView(button(if (busy) "刷新中…" else "刷新") { actions.refresh() }.apply { isEnabled = !busy })
        root.addView(info)
        if (contacts) {
            val search = row().apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(16), 0, dp(12), dp(10)) }
            (oldSearch?.parent as? ViewGroup)?.removeView(oldSearch)
            searchInput = (oldSearch ?: field("搜索名称或账号 ID", InputType.TYPE_CLASS_TEXT)).apply {
                if (text.toString() != searchQuery) setText(searchQuery)
                setSelection(searchSelection.coerceIn(0, text.length)); imeOptions = EditorInfo.IME_ACTION_SEARCH
                setOnEditorActionListener { _, action, _ -> if (action == EditorInfo.IME_ACTION_SEARCH && !busy) { actions.searchContacts(text.toString().trim()); true } else false }
            }
            search.addView(searchInput, weight())
            if (searchFocused) searchInput?.requestFocus()
            search.addView(button("搜索") { actions.searchContacts(searchInput!!.text.toString().trim()) }.apply { isEnabled = !busy })
            root.addView(search)
            if (results != null) root.addView(button("返回我的联系人") { searchQuery = ""; searchInput?.setText(""); actions.searchContacts("") })
        }
        if (error != null) root.addView(notice(error, true, "重试") { actions.refresh() })
        if (notice != null) root.addView(notice(notice))
        val scroll = scrollBody().also { listScroll = it }
        val body = column(12).apply { setPadding(dp(12), dp(4), dp(12), dp(24)) }
        scroll.addView(body)
        val items = if (contacts && results != null) results else snapshot.optJSONArray(if (contacts) "contacts" else "conversations") ?: JSONArray()
        if (items.length() == 0) {
            body.addView(empty(if (contacts && results != null) "没有找到账号" else if (contacts) "还没有联系人" else "还没有消息", if (contacts) "搜索服务器上的账号，选择联系人开始聊天。" else "从通讯录选择联系人，开始一段对话。", if (contacts) R.drawable.ic_contacts else R.drawable.ic_message))
        } else for (i in 0 until items.length()) {
            val item = items.getJSONObject(i)
            body.addView(if (contacts) contactRow(item) else conversationRow(item))
        }
        scroll.post { scroll.scrollTo(0, restoreScroll) }
        root.addView(navigation(contacts))
    }

    fun account(snapshot: JSONObject, origin: String, cryptoSummary: String, busy: Boolean = false,
                error: String? = null, notice: String? = null, hasDevice: Boolean = false) {
        begin("account")
        header("账号", "返回", "刷新", { actions.refresh() }, busy)
        val body = column(20)
        scrollBody().addView(body)
        val self = snapshot.getJSONObject("self")
        body.addView(avatar(self.optString("name"), 64), LinearLayout.LayoutParams(dp(64), dp(64)).apply { topMargin = dp(16); bottomMargin = dp(16) })
        body.addView(text(self.optString("name"), 26, bold = true, heading = true))
        if (self.optString("bio").isNotBlank()) body.addView(text(self.optString("bio"), 16, secondary = true).apply { setPadding(0, dp(8), 0, dp(8)) })
        body.addView(label("账号 ID").apply { setPadding(0, dp(24), 0, dp(8)) })
        body.addView(text(self.getString("id"), 15, secondary = true).apply { typeface = Typeface.MONOSPACE; setTextIsSelectable(true) })
        body.addView(button("复制账号 ID") {
            val clipboard = activity.getSystemService(android.content.Context.CLIPBOARD_SERVICE) as android.content.ClipboardManager
            clipboard.setPrimaryClip(android.content.ClipData.newPlainText("LLM Nest 账号 ID", self.getString("id")))
            if (Build.VERSION.SDK_INT < 33) Toast.makeText(activity, "账号 ID 已复制", Toast.LENGTH_SHORT).show()
        }, full())
        body.addView(label("当前服务器").apply { setPadding(0, dp(28), 0, dp(8)) })
        body.addView(text(origin, 15, secondary = true).apply { setTextIsSelectable(true) })
        body.addView(label("端到端加密").apply { setPadding(0, dp(28), 0, dp(8)) })
        body.addView(text(cryptoSummary, 15, secondary = true))
        body.addView(button("设备与加密设置") { actions.crypto() }, full().apply { topMargin = dp(12) })
        if (hasDevice) body.addView(button("锁定聊天") { actions.lock() }, full())
        if (snapshot.optBoolean("requireEncryption")) body.addView(notice("此服务器要求新消息使用端到端加密。"))
        if (error != null) body.addView(notice(error, true))
        if (notice != null) body.addView(notice(notice))
        body.addView(button("退出登录") {
            AlertDialog.Builder(activity).setTitle("退出登录？")
                .setMessage("退出会关闭当前会话。请先处理待确认发送；已发送的消息不会撤回。")
                .setNegativeButton("取消", null).setPositiveButton("退出登录") { _, _ -> actions.logout() }.show()
        }, full().apply { topMargin = dp(32) })
    }
    fun isAtLatest(): Boolean = history?.let { it.getChildAt(0)?.let { child -> child.height - it.height - it.scrollY <= dp(24) } ?: false } ?: false
    fun draft(): String = composer?.text?.toString().orEmpty()

    fun conversation(conversation: JSONObject, selfId: String, messages: JSONArray, hasMore: Boolean,
                     busy: Boolean = false, error: String? = null, draft: String = "", pending: PendingSend? = null,
                     canSendEncrypted: Boolean = false, requireEncryption: Boolean = false, draftSaveError: String? = null) {
        val id = conversation.getString("id")
        val key = "chat:$id"
        val sameChat = currentPage == key
        val oldScroll = if (sameChat) history?.scrollY else conversationScroll[id]
        val atBottom = !sameChat || isAtLatest()
        val previousHeight = if (sameChat) history?.getChildAt(0)?.height ?: 0 else 0
        val nextFirstId = messages.optJSONObject(0)?.optString("id").orEmpty()
        val prepended = sameChat && firstMessageId.isNotEmpty() && nextFirstId != firstMessageId
        firstMessageId = nextFirstId
        if (!sameChat) {
            begin(key)
            chatHeader = ConversationBar().also { root.addView(it) }
            history = scrollBody()
            chatBody = column().apply { setPadding(dp(20), dp(4), dp(20), dp(18)) }.also { history!!.addView(it) }
            rule(root)
            val footer = column().apply { setPadding(dp(12), dp(8), dp(12), dp(8)) }
            chatStatus = column().also { footer.addView(it) }
            root.addView(footer)
        }
        // Ordinary snapshots/history updates must not detach the editor from its window: doing so
        // ends the IME input connection even when the same EditText is subsequently reattached.
        val encrypted = conversation.has("encryption") && !conversation.isNull("encryption")
        val readOnly = conversation.optBoolean("readOnly")
        val blocked = conversation.optBoolean("directBlocked")
        val kind = when {
            readOnly -> "已归档"
            conversation.optString("kind") == "group" -> "群聊"
            conversation.optJSONObject("peer")?.optString("kind") == "ai" -> "AI 私聊"
            else -> "私聊"
        }
        chatHeader!!.bind(conversationName(conversation), kind, encrypted, busy, error,
            verify = !readOnly && !blocked && (encrypted || canSendEncrypted && !requireEncryption),
            enable = !encrypted && !readOnly && !blocked && !requireEncryption, pending = pending != null)
        val scroll = history!!
        val body = chatBody!!
        body.removeAllViews()
        if (hasMore) body.addView(button(if (busy) "正在加载…" else "加载更早消息") { actions.loadOlder() }.apply { isEnabled = !busy }, full())
        if (messages.length() == 0) body.addView(empty(if (busy) "正在加载聊天" else "还没有消息", if (busy) "正在读取服务器上的记录。" else "发送第一条消息，开始对话。", R.drawable.ic_message))
        val messageIndex = (0 until messages.length()).associate { messages.getJSONObject(it).let { row -> row.getString("id") to row } }
        var previousTime: ZonedDateTime? = null
        var previousSender = ""
        for (i in 0 until messages.length()) {
            val message = messages.getJSONObject(i)
            val time = localTime(message.optString("createdAt"))
            val prior = previousTime
            val sameDay = time != null && prior != null && time.toLocalDate() == prior.toLocalDate()
            if (time != null && !sameDay) body.addView(dayDivider(time))
            val sender = message.optString("senderId")
            val continued = sameDay && sender.isNotBlank() && sender == previousSender &&
                time!!.toEpochSecond() - prior!!.toEpochSecond() in 0..300
            body.addView(messageView(message, selfId, encrypted, messageIndex, time?.format(clockFormat).orEmpty(), continued))
            previousTime = time
            previousSender = sender
        }
        val footer = chatStatus!!
        footer.removeAllViews()
        if (draftSaveError != null) footer.addView(chatAlert(draftSaveError, "重试保存草稿") { actions.retryDraftSave() })
        when {
            readOnly -> footer.addView(text("这是旧版导入的只读聊天。你仍可查看历史记录。", 14, secondary = true))
            blocked -> footer.addView(text("双方私聊已屏蔽。历史和草稿保留，解除屏蔽后可发送。", 14, secondary = true))
            encrypted && !canSendEncrypted -> {
                footer.addView(text("解锁加密设备后即可发送。", 14, secondary = true))
                footer.addView(button("设备与加密设置") { actions.crypto() }, full())
            }
            requireEncryption && !encrypted -> {
                footer.addView(text("此服务器要求先开启端到端加密。", 14, secondary = true))
                footer.addView(button("开启此聊天加密", primary = true) { actions.enableEncryption() }.apply { isEnabled = !busy && pending == null }, full())
                footer.addView(button("设备与加密设置") { actions.crypto() }, full())
            }
        }
        if (pending != null) {
            footer.addView(text(if (busy) "正在发送…" else if (pending.unknown) "发送结果待确认" else "消息尚未发出", 15, bold = true))
            footer.addView(text(pending.text, 15).apply { maxLines = 3; setTextIsSelectable(true) })
            if (pending.error != null) footer.addView(text(pending.error, 13, secondary = true))
            if (!busy) {
                footer.addView(text("重试使用同一请求。请勿重复新建消息。", 13, secondary = true))
                val controls = row()
                controls.addView(button("重试原发送", primary = true) { actions.retryPending() }.apply {
                    isEnabled = !readOnly && !blocked && (!encrypted || canSendEncrypted) && (!requireEncryption || encrypted)
                }, weight())
                controls.addView(button("放弃待确认") {
                    AlertDialog.Builder(activity).setTitle("放弃本机待确认发送？")
                        .setMessage("这只移除本机的待确认请求，不会撤回服务器可能已接受的消息。建议先刷新核对聊天记录。")
                        .setNegativeButton("取消", null).setPositiveButton("放弃") { _, _ -> actions.discardPending() }.show()
                }, weight())
                footer.addView(controls)
            }
        }
        if (!readOnly) {
            composerAllowed = !blocked && (!encrypted || canSendEncrypted) && (!requireEncryption || encrypted) && pending == null && !busy && draftSaveError == null
            if (composer == null) {
                val composeRow = row().apply { gravity = Gravity.BOTTOM }
                val input = field("写消息…", InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_FLAG_MULTI_LINE or InputType.TYPE_TEXT_FLAG_CAP_SENTENCES).apply {
                    isSingleLine = false; minLines = 1; maxLines = 5
                    setText(draft); setSelection(text.length)
                    imeOptions = EditorInfo.IME_FLAG_NO_EXTRACT_UI
                    minimumHeight = dp(48)
                    background = StateListDrawable().apply {
                        addState(intArrayOf(android.R.attr.state_focused), rounded(color(R.color.navigation_surface), 20).apply { setStroke(dp(1), color(R.color.chat_action)) })
                        addState(intArrayOf(), rounded(color(R.color.navigation_surface), 20).apply { setStroke(dp(1), color(R.color.chat_rule)) })
                    }
                    setPadding(dp(16), dp(10), dp(16), dp(10))
                }
                composer = input
                composeRow.addView(input, weight().apply { rightMargin = dp(8) })
                val send = button("发送", primary = true) { actions.send(input.text.toString()) }.apply {
                    contentDescription = "发送"
                    stateListAnimator = null; backgroundTintList = null
                    val enabled = intArrayOf(android.R.attr.state_enabled)
                    background = RippleDrawable(ColorStateList.valueOf(color(R.color.divider)), StateListDrawable().apply {
                        addState(enabled, rounded(color(R.color.chat_action), 20))
                        addState(intArrayOf(), rounded(color(R.color.chat_reply), 20))
                    }, null)
                    setTextColor(ColorStateList(arrayOf(enabled, intArrayOf()), intArrayOf(color(R.color.on_accent), color(R.color.chat_muted))))
                    setPadding(dp(18), 0, dp(18), 0)
                }
                chatSend = send
                composeRow.addView(send)
                composerWatcher = object : TextWatcher {
                    override fun beforeTextChanged(s: CharSequence?, start: Int, count: Int, after: Int) {}
                    override fun onTextChanged(s: CharSequence?, start: Int, before: Int, count: Int) { send.isEnabled = composerAllowed && !s.isNullOrBlank(); actions.draftChanged(s.toString()) }
                    override fun afterTextChanged(s: Editable?) {}
                }.also { input.addTextChangedListener(it) }
                chatComposerRow = composeRow
                (footer.parent as LinearLayout).addView(composeRow)
            }
            val input = composer!!
            if (input.text.toString() != draft) {
                // Acknowledgement/explicit draft replacement is authoritative. Equal text needs no
                // setter calls: retain the Editable, range selection and the IME's composing spans.
                val start = input.selectionStart.coerceAtLeast(0)
                val end = input.selectionEnd.coerceAtLeast(0)
                composerWatcher?.let { input.removeTextChangedListener(it) }
                input.setText(draft)
                input.setSelection(start.coerceAtMost(draft.length), end.coerceAtMost(draft.length))
                composerWatcher?.let { input.addTextChangedListener(it) }
            }
            val editable = pending == null && !busy
            if (input.isEnabled != editable) input.isEnabled = editable
            chatSend!!.isEnabled = composerAllowed && input.text.isNotBlank()
        } else {
            composerWatcher?.let { watcher -> composer?.removeTextChangedListener(watcher) }
            (chatComposerRow?.parent as? ViewGroup)?.removeView(chatComposerRow)
            composer = null; composerWatcher = null; chatComposerRow = null; chatSend = null
        }
        val renderRevision = ++chatRenderRevision
        scroll.post {
            if (history !== scroll || renderRevision != chatRenderRevision) return@post
            when {
                oldScroll == null || atBottom -> scroll.scrollTo(0, (body.height - scroll.height).coerceAtLeast(0))
                prepended -> scroll.scrollTo(0, oldScroll + body.height - previousHeight)
                else -> scroll.scrollTo(0, oldScroll)
            }
            if (isAtLatest()) actions.visibleLatest()
        }
        scroll.setOnScrollChangeListener { _: View, _: Int, _: Int, _: Int, _: Int -> if (history === scroll && isAtLatest()) actions.visibleLatest() }
    }

    private fun messageView(message: JSONObject, selfId: String, encrypted: Boolean, all: Map<String, JSONObject>, time: String, continued: Boolean): View {
        val own = message.optString("senderId") == selfId
        val sender = if (own) "我" else message.optString("senderName").ifBlank { "未知联系人" }
        val outer = column().apply { gravity = if (own) Gravity.END else Gravity.START; setPadding(0, dp(if (continued) 4 else 16), 0, 0) }
        val byline = listOf(sender, time).filter(String::isNotEmpty).joinToString("  ·  ")
        if (!continued) outer.addView(text(byline, 13, secondary = true).apply {
            gravity = if (own) Gravity.END else Gravity.START
            setTextColor(color(R.color.chat_muted)); setPadding(0, 0, dp(if (own) 4 else 0), dp(5))
        })
        val bubble = column().apply {
            if (own) {
                val wide = dp(18).toFloat(); val turn = dp(5).toFloat()
                background = rounded(color(R.color.chat_reply), 18).apply {
                    cornerRadii = floatArrayOf(wide, wide, turn, turn, wide, wide, wide, wide)
                }
                setPadding(dp(14), dp(9), dp(14), dp(9))
            }
        }
        val withdrawn = message.optString("status") == "withdrawn"
        val sealed = message.has("sealed") && !message.isNull("sealed")
        val readable = !sealed || message.optBoolean("_decrypted")
        val cryptoError = message.optString("cryptoError").takeUnless { it == "null" }.orEmpty()
        if (withdrawn) bubble.addView(text("消息已撤回", 15, secondary = true))
        else if (!readable || cryptoError.isNotBlank()) {
            bubble.addView(text("加密消息暂不可读", 16, bold = true))
            bubble.addView(text(cryptoError.ifBlank { "请在设备与加密设置中解锁或恢复密钥后刷新。" }, 14, secondary = true))
        } else {
            val reply = message.optString("replyTo").takeUnless { it == "null" }.orEmpty()
            if (reply.isNotEmpty()) {
                val source = all[reply]
                val replyPreview = if (source == null) "回复了较早的消息" else "回复 ${source.optString("senderName")}：${preview(source)}"
                bubble.addView(text(replyPreview, 13, secondary = true).apply { maxLines = 2; setPadding(0, 0, 0, dp(8)) })
            }
            val content = message.optString("text")
            if (content.isNotEmpty()) bubble.addView(text(content, 17).apply {
                setTextColor(color(R.color.chat_ink)); setTextIsSelectable(true); setLineSpacing(dp(3).toFloat(), 1f)
                if (continued) contentDescription = "$byline，$content"
            })
            val attachments = message.optJSONArray("attachments") ?: JSONArray()
            for (i in 0 until attachments.length()) {
                val a = attachments.getJSONObject(i)
                bubble.addView(text("附件 · ${a.optString("name").ifBlank { a.optString("mime", "文件") }} · ${fileSize(a.optLong("byteLength"))}\n此版本暂不支持查看或下载附件", 13, secondary = true).apply { setPadding(0, dp(8), 0, 0) })
            }
            if (encrypted && !sealed) bubble.addView(text("旧明文记录 · 尚未迁移", 12, secondary = true).apply { setPadding(0, dp(8), 0, 0) })
            if (message.has("contextReference") && !message.isNull("contextReference")) bubble.addView(text("包含聊天记录引用", 12, secondary = true))
            val status = when (message.optString("status")) { "generating" -> "正在回复…"; "failed" -> "回复未完成"; "interrupted" -> "回复已中断"; else -> "" }
            if (status.isNotBlank()) bubble.addView(text(status, 13, secondary = true).apply { setPadding(0, dp(8), 0, 0) })
        }
        outer.addView(bubble, LinearLayout.LayoutParams(-2, -2).apply { if (own) leftMargin = dp(32) else rightMargin = dp(32) })
        return outer
    }

    private fun conversationRow(c: JSONObject): View {
        val title = conversationName(c)
        val line = row().apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12), dp(14), dp(12), dp(14)); minimumHeight = dp(88); background = rounded(color(R.color.navigation_surface), 16) }
        line.addView(avatar(title, 48))
        val copy = column().apply { setPadding(dp(12), 0, dp(8), 0) }
        val heading = row().apply { gravity = Gravity.CENTER_VERTICAL }
        heading.addView(text(title, 17, bold = true).apply { maxLines = 1; ellipsize = android.text.TextUtils.TruncateAt.END }, weight())
        val last = c.optJSONObject("lastMessage")
        heading.addView(text(last?.optString("createdAt")?.let { formatTime(it) }.orEmpty(), 12, secondary = true))
        copy.addView(heading)
        copy.addView(text(last?.let { preview(it) } ?: "还没有消息", 14, secondary = true).apply { maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END; setPadding(0, dp(5), 0, 0) })
        if (c.optBoolean("readOnly") || c.optBoolean("directBlocked") || c.optBoolean("pinned")) copy.addView(text(listOfNotNull(if (c.optBoolean("pinned")) "置顶" else null, if (c.optBoolean("readOnly")) "只读" else null, if (c.optBoolean("directBlocked")) "已屏蔽" else null).joinToString(" · "), 12, secondary = true))
        line.addView(copy, weight())
        val count = c.optInt("unreadCount")
        if (count > 0 || c.optBoolean("manualUnread")) line.addView(text(if (count > 99) "99+" else if (count > 0) "$count" else "未读", 12, bold = true).apply { setTextColor(color(R.color.accent)); contentDescription = if (count > 0) "$count 条未读消息" else "标记未读"; setPadding(dp(5), dp(5), dp(5), dp(5)) })
        clickable(line) { actions.openConversation(c.getString("id")) }
        return column().apply { addView(line, full().apply { bottomMargin = dp(8) }) }
    }

    private fun contactRow(c: JSONObject): View {
        val line = row().apply { gravity = Gravity.CENTER_VERTICAL; minimumHeight = dp(80); setPadding(dp(12), dp(12), dp(12), dp(12)); background = rounded(color(R.color.navigation_surface), 16) }
        line.addView(avatar(c.optString("name"), 44))
        val copy = column().apply { setPadding(dp(12), 0, dp(8), 0) }
        copy.addView(text(c.optString("name"), 17, bold = true))
        copy.addView(text(if (c.optString("status") == "disabled") "账号已停用" else if (c.optString("kind") == "ai") "AI 联系人" else c.optString("bio").ifBlank { "联系人" }, 14, secondary = true).apply { maxLines = 2; setPadding(0, dp(5), 0, 0) })
        copy.addView(text(c.optString("id"), 12, secondary = true).apply { typeface = Typeface.MONOSPACE; setTextIsSelectable(true); setPadding(0, dp(5), 0, 0) })
        line.addView(copy, weight())
        if (c.optString("status") != "disabled") {
            line.addView(text("›", 24, secondary = true))
            clickable(line) { actions.openContact(c.getString("id")) }
        }
        return column().apply { addView(line, full().apply { bottomMargin = dp(8) }) }
    }

    private fun preview(m: JSONObject): String = when {
        m.optString("status") == "withdrawn" -> "消息已撤回"
        m.optString("cryptoError").let { it.isNotBlank() && it != "null" } -> "加密消息暂不可读"
        m.has("sealed") && !m.isNull("sealed") && !m.optBoolean("_decrypted") -> "加密消息"
        m.optString("text").isNotBlank() -> m.optString("text").replace('\n', ' ')
        (m.optJSONArray("attachments")?.length() ?: 0) > 0 -> "附件消息"
        m.optString("status") == "generating" -> "正在回复…"
        else -> "消息"
    }
    private fun conversationName(c: JSONObject) = c.optString("name").ifBlank { c.optJSONObject("peer")?.optString("name").orEmpty().ifBlank { "群聊" } }

    private fun begin(page: String) {
        composerWatcher?.let { watcher -> composer?.removeTextChangedListener(watcher) }
        composerWatcher = null
        if (currentPage == "contacts" && page != "contacts") searchQuery = ""
        if (currentPage.startsWith("chat:")) history?.let { conversationScroll[currentPage.removePrefix("chat:")] = it.scrollY }
        if (currentPage == "messages" || currentPage == "contacts") listScroll?.let { savedListScroll[currentPage] = it.scrollY }
        if (currentPage == "login" && page != "login") {
            passwordInput?.text?.clear(); originInput = null; emailInput = null; passwordInput = null; loginFeedback = null; loginButton = null
        }
        if (page == "login" || page == "gate") {
            conversationScroll.clear(); savedListScroll.clear(); searchQuery = ""; firstMessageId = ""
        }
        chatHeader?.closeMenu()
        root.removeAllViews(); currentPage = page; history = null; listScroll = null; composer = null; searchInput = null
        root.setBackgroundColor(color(if (page.startsWith("chat:")) R.color.chat_paper else R.color.surface))
        chatHeader = null; chatBody = null; chatStatus = null; chatComposerRow = null; chatSend = null; composerAllowed = false
        root.requestApplyInsets()
    }
    private fun rule(parent: LinearLayout) = parent.addView(View(activity).apply { setBackgroundColor(color(R.color.chat_rule)) }, -1, dp(1))

    private fun chatAlert(message: String, retry: String, action: () -> Unit): View = row().apply {
        gravity = Gravity.CENTER_VERTICAL
        background = rounded(color(R.color.error_surface), 12)
        setPadding(dp(12), dp(2), dp(4), dp(2))
        addView(text(message, 14).apply {
            setTextColor(color(R.color.error_text)); accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE
        }, weight())
        addView(Button(activity, null, android.R.attr.borderlessButtonStyle).apply {
            text = retry; textSize = 14f; isAllCaps = false; minHeight = dp(48)
            setTextColor(color(R.color.error_text)); setOnClickListener { action() }
        })
        layoutParams = full().apply { bottomMargin = dp(8) }
    }

    private fun dayDivider(time: ZonedDateTime): View = row().apply {
        gravity = Gravity.CENTER_VERTICAL
        setPadding(0, dp(16), 0, dp(2))
        addView(text(time.format(dayFormat), 13, secondary = true).apply {
            setTextColor(color(R.color.chat_muted)); setPadding(0, 0, dp(12), 0)
        })
        addView(View(activity).apply { setBackgroundColor(color(R.color.chat_rule)) }, LinearLayout.LayoutParams(0, dp(1), 1f))
    }

    private fun barIcon(icon: Int, label: String, action: () -> Unit) = ImageButton(activity).apply {
        contentDescription = label; setImageResource(icon); scaleType = ImageView.ScaleType.CENTER
        val attr = android.util.TypedValue()
        activity.theme.resolveAttribute(android.R.attr.selectableItemBackgroundBorderless, attr, true)
        setBackgroundResource(attr.resourceId); setOnClickListener { action() }
        layoutParams = LinearLayout.LayoutParams(dp(48), dp(48))
    }

    private data class Commands(val busy: Boolean, val verify: Boolean, val enable: Boolean, val pending: Boolean)

    private inner class ConversationBar : LinearLayout(activity) {
        private val name = text("", 21, bold = true, heading = true).apply {
            maxLines = 2; ellipsize = android.text.TextUtils.TruncateAt.END; setTextColor(color(R.color.chat_ink))
        }
        private val status = text("", 13, secondary = true).apply {
            setTextColor(color(R.color.chat_muted)); compoundDrawablePadding = dp(4)
        }
        private val identity = column().apply {
            gravity = Gravity.CENTER_VERTICAL; minimumHeight = dp(48); setPadding(dp(4), 0, dp(8), 0)
            addView(name); addView(status)
        }
        private val more = barIcon(R.drawable.ic_chat_more, "更多操作") { showMenu() }
        private val feedback = column()
        private var commands = Commands(false, false, false, false)
        private var menu: PopupMenu? = null

        init {
            orientation = VERTICAL
            val bar = row().apply {
                gravity = Gravity.CENTER_VERTICAL; minimumHeight = dp(68); setPadding(dp(4), dp(4), dp(8), dp(4))
                addView(barIcon(R.drawable.ic_chat_back, "返回") { actions.back() })
                addView(identity, weight()); addView(more)
            }
            addView(bar); rule(this); addView(feedback)
            clickable(identity) { showMenu() }
        }

        fun bind(title: String, kind: String, encrypted: Boolean, busy: Boolean, error: String?, verify: Boolean, enable: Boolean, pending: Boolean) {
            val next = Commands(busy, verify, enable, pending)
            if (commands != next) closeMenu()
            commands = next
            name.text = title
            val security = if (encrypted) "端到端加密" else "未开启端到端加密"
            status.text = "$kind · $security"
            status.setCompoundDrawablesWithIntrinsicBounds(if (encrypted) R.drawable.ic_chat_lock else R.drawable.ic_chat_unlocked, 0, 0, 0)
            identity.isClickable = verify || enable; identity.isFocusable = identity.isClickable
            identity.contentDescription = if (identity.isClickable) "$title，$kind，$security，查看会话选项" else null
            feedback.removeAllViews()
            if (error != null) feedback.addView(notice(error, true, "重试") { actions.refresh() })
        }

        private fun showMenu() {
            val popup = PopupMenu(activity, more, Gravity.END)
            popup.menu.add(if (commands.busy) "刷新中…" else "刷新").apply {
                isEnabled = !commands.busy; setOnMenuItemClickListener { actions.refresh(); true }
            }
            if (commands.verify) popup.menu.add("核对设备").setOnMenuItemClickListener { actions.crypto(); true }
            if (commands.enable) popup.menu.add("开启加密").apply {
                isEnabled = !commands.busy && !commands.pending
                setOnMenuItemClickListener { actions.enableEncryption(); true }
            }
            menu = popup; popup.setOnDismissListener { menu = null }; popup.show()
        }

        fun closeMenu() { menu?.dismiss() }
    }

    private fun header(title: String, back: String?, action: String?, onAction: () -> Unit, busy: Boolean = false) {
        val header = row().apply { gravity = Gravity.CENTER_VERTICAL; setPadding(dp(12), dp(8), dp(8), dp(4)); minimumHeight = dp(64) }
        if (back != null) header.addView(button("‹") { actions.back() }.apply { contentDescription = back; textSize = 32f; minWidth = dp(48); minimumWidth = dp(48); setPadding(0, 0, 0, 0) })
        header.addView(text(title, if (back == null) 29 else 21, bold = true, heading = true).apply { maxLines = 2; setPadding(if (back == null) dp(8) else 0, 0, dp(6), 0) }, weight())
        if (action != null) header.addView(button(if (busy) "刷新中…" else action) { onAction() }.apply { isEnabled = !busy })
        root.addView(header)
    }
    private fun navigation(contacts: Boolean): View = row().apply {
        setPadding(dp(12), dp(8), dp(12), dp(10)); setBackgroundColor(color(R.color.navigation_surface))
        listOf(false, true).forEach { tab ->
            addView(button(activity.getString(if (tab) R.string.contacts else R.string.messages)) { actions.selectTab(tab) }.apply {
                isSelected = tab == contacts; minimumHeight = dp(60); textSize = 14f
                backgroundTintList = ColorStateList.valueOf(color(if (isSelected) R.color.selected_surface else R.color.navigation_surface))
                setCompoundDrawablesWithIntrinsicBounds(0, if (tab) R.drawable.ic_contacts else R.drawable.ic_message, 0, 0)
                compoundDrawableTintList = ColorStateList.valueOf(color(if (isSelected) R.color.accent else R.color.text_secondary)); compoundDrawablePadding = dp(2)
                contentDescription = (if (tab) "通讯录" else "消息") + if (isSelected) "，已选中" else ""
            }, weight())
        }
    }
    private fun empty(title: String, hint: String, icon: Int): View = column(20).apply {
        gravity = Gravity.CENTER; setPadding(dp(20), dp(52), dp(20), dp(52))
        addView(ImageView(activity).apply { setImageResource(icon); imageTintList = ColorStateList.valueOf(color(R.color.accent)); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO }, LinearLayout.LayoutParams(dp(40), dp(40)))
        addView(text(title, 21, bold = true).apply { gravity = Gravity.CENTER; setPadding(0, dp(20), 0, dp(8)) })
        addView(text(hint, 15, secondary = true).apply { gravity = Gravity.CENTER })
    }
    private fun notice(message: String, error: Boolean = false, action: String? = null, onAction: (() -> Unit)? = null): View = column(12).apply {
        background = rounded(color(if (error) R.color.error_surface else R.color.selected_surface), 12)
        addView(text(message, 14).apply { if (error) setTextColor(color(R.color.error_text)); accessibilityLiveRegion = View.ACCESSIBILITY_LIVE_REGION_POLITE })
        if (action != null) addView(button(action) { onAction?.invoke() })
        layoutParams = full().apply { setMargins(dp(12), dp(6), dp(12), dp(6)) }
    }
    private fun scrollBody(): ScrollView = ScrollView(activity).apply {
        isFillViewport = true; clipToPadding = false; isSaveEnabled = false
        root.addView(this, LinearLayout.LayoutParams(-1, 0, 1f))
    }
    private fun column(padding: Int = 0) = LinearLayout(activity).apply { orientation = LinearLayout.VERTICAL; if (padding > 0) setPadding(dp(padding), dp(padding), dp(padding), dp(padding)) }
    private fun row() = LinearLayout(activity).apply { orientation = LinearLayout.HORIZONTAL }
    private fun text(value: String, size: Int, bold: Boolean = false, secondary: Boolean = false, heading: Boolean = false) = TextView(activity).apply {
        text = value; textSize = size.toFloat(); setTextColor(color(if (secondary) R.color.text_secondary else R.color.text_primary)); setLineSpacing(dp(2).toFloat(), 1f)
        if (bold) setTypeface(typeface, Typeface.BOLD)
        if (Build.VERSION.SDK_INT >= 28 && heading) isAccessibilityHeading = true
    }
    private fun label(value: String) = text(value, 14, bold = true).apply { setPadding(0, 0, 0, dp(8)) }
    private fun field(hint: String, type: Int) = EditText(activity).apply {
        this.hint = hint; inputType = type; setSingleLine(true); textSize = 16f; minimumHeight = dp(52)
        setTextColor(color(R.color.text_primary)); setHintTextColor(color(R.color.text_secondary)); isSaveEnabled = false
        importantForAutofill = View.IMPORTANT_FOR_AUTOFILL_NO
        setPadding(dp(12), dp(10), dp(12), dp(10))
        background = rounded(color(R.color.navigation_surface), 12).apply { setStroke(dp(1), color(R.color.divider)) }
        contentDescription = hint
    }
    private fun button(label: String, primary: Boolean = false, action: () -> Unit) = Button(activity).apply {
        text = label; textSize = 14f; isAllCaps = false; minHeight = dp(48); minimumHeight = dp(48); minWidth = dp(64); minimumWidth = dp(64)
        setPadding(dp(12), dp(4), dp(12), dp(4)); setTextColor(color(if (primary) R.color.on_accent else R.color.accent))
        backgroundTintList = ColorStateList(arrayOf(intArrayOf(-android.R.attr.state_enabled), intArrayOf()), intArrayOf(color(R.color.divider), color(if (primary) R.color.button_accent else R.color.navigation_surface)))
        setOnClickListener { action() }
    }
    private fun avatar(name: String, size: Int): View = text(name.trim().take(1).ifBlank { "·" }, if (size > 50) 26 else 21, bold = true).apply {
        gravity = Gravity.CENTER; setTextColor(color(R.color.accent)); background = rounded(color(R.color.selected_surface), size / 2)
        layoutParams = LinearLayout.LayoutParams(dp(size), dp(size)); importantForAccessibility = View.IMPORTANT_FOR_ACCESSIBILITY_NO
    }
    private fun clickable(view: View, action: () -> Unit) {
        view.isFocusable = true; view.isClickable = true
        val attr = android.util.TypedValue(); activity.theme.resolveAttribute(android.R.attr.selectableItemBackground, attr, true)
        view.foreground = activity.getDrawable(attr.resourceId)
        view.setOnClickListener { action() }
    }
    private fun rounded(fill: Int, radius: Int) = GradientDrawable().apply { setColor(fill); cornerRadius = dp(radius).toFloat() }
    private fun full() = LinearLayout.LayoutParams(-1, -2)
    private fun weight() = LinearLayout.LayoutParams(0, -2, 1f)
    private fun color(id: Int) = activity.getColor(id)
    private fun dp(value: Int) = (value * activity.resources.displayMetrics.density + .5f).toInt()
    private val clockFormat = DateTimeFormatter.ofPattern("HH:mm")
    private val dayFormat = DateTimeFormatter.ofPattern("yyyy年M月d日  EEEE", Locale.CHINESE)
    private fun localTime(value: String): ZonedDateTime? = try { OffsetDateTime.parse(value).atZoneSameInstant(ZoneId.systemDefault()) } catch (_: java.time.format.DateTimeParseException) { null }
    private fun formatTime(value: String): String = localTime(value)?.format(clockFormat).orEmpty()
    private fun fileSize(bytes: Long): String = if (bytes < 1024) "$bytes B" else if (bytes < 1024 * 1024) "${bytes / 1024} KB" else String.format(Locale.ROOT, "%.1f MB", bytes / (1024.0 * 1024.0))
}
