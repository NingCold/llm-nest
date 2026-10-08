# 07 API 与事件

[总纲](README.md) · [存储与迁移](06-storage-and-migration.md) · [界面与桌面](08-frontend-and-desktop.md)

状态：目标契约，尚未实现。下面的 HTTP 路由与 IPC 名称均为新增规划。

## 1. 共享业务入口

SocialFeature 暴露具体方法，Feature trait 继续只管理 id、initialize、shutdown 和 downcast。HTTP 与 Tauri 从同一业务方法取得结果，不能分别实现记忆、权限或调度逻辑。

记忆管理与摘要查询统一委托内部意识系统（ConsciousnessService），发送和原消息读取仍走聊天用例。Consciousness 是系统名，具体 API 保留 memories、summaries、contextReferences 等明确用途，不新增通用意识控制端点或第三个导航入口。

增加共享 DTO 模块承载业务响应与事件 wire 类型，外部字段统一 camelCase。内部模型可用 snake_case。ActorId、MessageId、RunId、revision、sequence 和 cursor 对外均为字符串。

## 2. 目标接口清单

HTTP 前缀为 `/api/social/v1`，表中 route 省略前缀。写操作使用后端当前操作者，clientRequestId 与 expectedRevision 按用途提供。

| 用例 | HTTP | Tauri command |
| --- | --- | --- |
| 启动快照 | GET /init | social_init |
| 修改本地用户资料 | PATCH /self | social_update_self |
| 联系人列表/创建 | GET/POST /ai | social_list_ai / social_create_ai |
| AI 档案详情 | GET /ai/{id} | social_get_ai |
| 修改/归档 AI | PATCH /ai/{id} | social_update_ai |
| 修改 AI 执行配置 | PUT /ai/{id}/execution | social_update_ai_execution |
| 空间列表/创建 | GET/POST /conversations | social_list_conversations / social_create_conversation |
| 空间详情 | GET /conversations/{id} | social_get_conversation |
| 修改标题/归档/恢复空间 | PATCH /conversations/{id} | social_update_conversation |
| 查看窗口摘要及来源 | GET /conversations/{id}/summaries | social_list_conversation_summaries |
| 成员增加/修改/退出 | PUT /conversations/{id}/members/{actorId} | social_update_membership |
| 消息分页/发送 | GET/POST /conversations/{id}/messages | social_list_messages / social_send_message |
| 单条消息查询 | GET /messages/{id} | social_get_message |
| 消息编辑/撤回 | PATCH/DELETE /messages/{id} | social_edit_message / social_redact_message |
| 更新已读位置 | PUT /conversations/{id}/read-position | social_mark_read |
| 暂停/恢复参与 | POST /conversations/{id}/pause 或 /resume | social_pause_conversation / social_resume_conversation |
| 结束当前交谈 | POST /conversations/{id}/end-bout | social_end_bout |
| 运行详情 | GET /runs/{id} | social_get_run |
| 取消一次运行 | POST /runs/{id}/cancel | social_cancel_run |
| 查询/手动创建记忆 | GET/POST /memories | social_list_memories / social_create_memory |
| 记忆详情 | GET /memories/{id} | social_get_memory |
| 修改/确认/遗忘记忆 | PATCH /memories/{id}，POST /memories/{id}/confirm，DELETE /memories/{id} | social_update_memory / social_confirm_memory / social_forget_memory |
| 查询/创建/取消联系意图 | GET/POST /intents，POST /intents/{id}/cancel | social_list_intents / social_create_intent / social_cancel_intent |
| 联系意图详情 | GET /intents/{id} | social_get_intent |
| 补齐持久变更 | GET /changes?after={cursor} | social_get_changes |
| 长期订阅 | GET /events?after={cursor} | social_subscribe + social-event |

provider、目录同步和配置等已有接口继续保留；旧 `/api/sessions` 与 chat-event 在过渡期明确标 legacy，不与新契约混用。初期可以按里程碑逐项实现，不提供返回假成功的占位路由。

创建 direct Conversation 按 Relationship 查找唯一现有空间；重复请求返回已有空间，归档后的恢复沿用同一 ID。创建 group 才建立独立群窗口。记忆查询/编辑接口由聊天详情或联系人详情使用，不对应独立记忆主入口；候选确认接口在后续提取功能加入时实现。

取得归档私聊只返回原空间，不自动改为 active；用户明确恢复时通过更新空间命令保存状态，恢复不重放已取消计划。

“与某 AI 私聊继续”先获取该关系的唯一私聊，再通过发送消息的可选 contextReferences 指定来源窗口、选中消息/版本或话题范围；打开窗口本身不发送消息、不启动生成。引用字段只传标识，不由前端提供一段声称来自群的正文。后端与意识系统复核来源、目标和访问权，校验后与私聊消息同事务保存。用户显式分享选中的正文时，通过普通消息发送形成新的目标空间消息；它与 ContextReference 分别处理，不授予源空间读取权。

## 3. 发送与幂等

```json
{
  "clientRequestId": "request-uuid",
  "body": [{ "type": "text", "text": "一起讨论一下这个想法" }],
  "replyTo": null,
  "mentions": ["ai-actor-uuid"]
}
```

以上是占位 ID 的结构示意，实际 ID 必须校验为合法 UUID。空间由 route 或命令参数指定，senderId 从后端上下文确定。

contextReferences 默认为空，M2 与跨窗口接续一起实现；引用失效或无权限返回明确错误，不静默扩大可见范围或丢弃用户选中的引用。窗口摘要查询只返回当前操作者可见的分段、有效状态与来源；生成使用的 AI 视图仍由 ContextBuilder 按目标 AI 单独校验。

成功返回已保存 MessageDto、conversationRevision 和 committedCursor。此响应只表示消息已保存，不保证 AI 会回答。请求失败保留前端未发送草稿；相同请求 ID 与内容重试返回同一实体，内容不同返回 conflict。

幂等作用域为当前操作者 + 命令种类 + clientRequestId；回执保存实体引用与请求摘要，重复响应读取当前可见状态，不能回放已经撤回的正文。读取型接口复核当前权限，失去访问权返回明确错误或不可见占位。

编辑使用 expectedRevision，针对消息自身版本。新增其他成员消息不应让无关的编辑因整段历史计数变化而失败；修改同一消息则拒绝过期 revision。

## 4. 持久事件

SocialEvent 记录已经提交的业务变更引用。建议事件包括 actor.created/changed、conversation.created/changed、membership.changed、message.created/revised/finalized/redacted、run.started/ended、memory.changed/forgotten、summary.changed/invalidated、intent.changed、participation.changed。memory 与 summary 事件在 M2 加入，仍只携带实体标识、版本和范围，客户端查询当前可见状态。

```json
{
  "schemaVersion": 1,
  "eventId": "event-uuid",
  "eventSeq": "1042",
  "type": "message.finalized",
  "conversationId": "conversation-uuid",
  "actorId": "ai-actor-uuid",
  "messageId": "message-uuid",
  "runId": "run-uuid",
  "entityRevision": "3",
  "occurredAt": "2026-10-08T08:00:00Z"
}
```

事件不重复保存消息、记忆或修订原文；客户端依引用读取当前可见版本。这样撤回或遗忘后，补齐旧事件不会恢复旧正文。事件不是完整事件溯源系统，当前数据库状态是权威数据。

eventSeq 在本地存储全局单调递增，eventId 稳定唯一。持久事件与业务事务同提交，发布采用至少一次语义；客户端按 eventId/sequence 去重。

RuntimeEvent 保持运行时通知职责，新增 SocialEvent 与 GenerationEvent 独立；不通过虚构 RuntimeEvent::Feature 转发业务。

## 5. 临时生成事件

GenerationEvent.type 初版为 generation.delta、generation.reasoning_delta、generation.activity。包含 conversationId、actorId、messageId、runId、deltaSeq 和必要 payload，不拥有持久 eventSeq。

普通 token 不进入持久变更日志。客户端断线后读已保存草稿/终态，不能拼接无法证明完整的旧 token 流。最终消息由持久状态覆盖临时缓冲，run 终态之后的迟到 delta 被忽略。

有限队列负责临时广播，慢消费者丢失片段后接收 resync 提示或重读，不阻塞运行收尾。思考和工具详情的订阅可独立于普通消息列表，并校验访问权。

## 6. 启动、订阅与补齐

启动快照在一致读事务中返回当前用户、联系人、会话列表元数据、活动运行与 snapshotCursor。“会话列表元数据”指标题、成员、末条消息、未读等，不是意识系统生成的 ConversationSummary。较长消息列表和历史摘要分别查询，不把所有历史塞进 init，也不等待后台摘要生成才允许启动。

客户端随后订阅 after=snapshotCursor：后端先准备 live 通道，再捕获持久高水位并回放到该水位，最后切到 live；重复事件去重。通道 overflow 时使用持久日志再次补齐，不能静默跳过。

订阅与回放至少一次，不承诺 exactly-once。客户端只有在事件对应状态应用成功后才推进 cursor。按 conversationId 过滤时，仍需返回扫描 watermark，不能把权限过滤掉的事件当作未处理卡住游标。

游标有效期超出保留窗口返回 cursor_expired/resync_required，客户端重新获取快照。事件日志压缩与备份策略在实际使用量验证后确定；M0 不提前删除尚需消费者处理的事件。

Tauri 先注册监听，再调用 social_subscribe 建立订阅；ack 返回 subscriptionId 和起始 cursor。取消订阅释放监听与后端资源，但不取消 AiRun。HTTP 使用 SSE，`id` 字段对应持久 cursor；临时事件不推进 Last-Event-ID。

## 7. 错误与访问边界

共享错误至少区分 invalid_input、not_found、revision_conflict、not_authorized、model_unavailable、paused、budget_exhausted、storage_failure、cursor_expired。HTTP 使用对应状态码和统一错误体；IPC 返回同样的 code、message、retryable 和可选 currentRevision。

消息已提交后参与失败通过运行/参与状态表达，不把发送请求事后改为保存失败。无法持久化的生成错误不能通过事件伪造已提交终态。

首版本地 HTTP 保留 loopback、Host/Origin 和 DNS rebinding 防护；浏览器 demo 明确与真实后端隔离。未来公网访问需要另外的认证与成员授权设计，不能直接开放当前本地服务。

序列化 DTO 不返回 Key、环境变量值、完整配置或任意路径。路径设置仍通过既有 config/data 枚举的目录打开 IPC；Web 不提供服务器文件夹打开接口。HTTP header 继续使用 Headers.set。

## 8. 验收

1. 同一业务通过 HTTP 和 IPC 产生一致字段、状态、错误与顺序。
2. 初始化与订阅之间发生消息，不漏事件；重复回放不重复消息。
3. 订阅断开后运行完成，重连读到最终消息；迟到 delta 不覆盖终态。
4. 慢订阅者溢出、游标过期和权限过滤均能明确补齐或重置。
5. 撤回/遗忘后补齐旧事件，不能恢复原文。
6. 幂等发送、消息 revision 冲突、无效成员和越权读取均被后端拒绝。
