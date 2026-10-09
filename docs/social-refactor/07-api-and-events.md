# 07 API 与事件

[总纲](README.md) · [存储与迁移](06-storage-and-migration.md) · [界面与桌面](08-frontend-and-desktop.md)

本文集中维护当前传输契约，后续能力单独列出，不预先固定尚未实现的路由。源码入口为 [HTTP](../../crates/web-server/src/social_api.rs)、[IPC](../../frontends/tauri/src-tauri/src/social_commands.rs)、[共享 DTO](../../features/social/src/wire.rs) 与 [持久事件](../../crates/events/src/social_event.rs)。

## 1. 共享业务入口

HTTP 与 Tauri 调用同一 SocialFeature 业务方法，传输层不实现身份、权限、记忆或调度规则。Feature trait 只负责生命周期与 downcast。记忆/摘要用例后续由内部意识服务承担，原消息仍由聊天用例维护。

外部业务字段为 camelCase，枚举值按各自 serde 契约；ID 为 UUID 字符串，revision、sequence、cursor 为十进制字符串。请求使用 requestId；expectedRevision 针对被修改对象，不能拿空间 revision 替代消息或成员 revision。

## 2. 当前接口

HTTP 前缀为 `/api/social`。下面的 IPC 命令已注册，但桌面界面与实时订阅尚未接入。联系人和空间列表由 init 返回，暂没有独立列表接口。

| 用例 | HTTP（省略前缀） | Tauri command |
| --- | --- | --- |
| 启动快照 | GET /init | social_init |
| 创建/修改联系人及绑定 | POST /contacts；PATCH /contacts/{id} | social_create_contact / social_update_contact |
| 打开唯一私聊 | POST /contacts/{id}/direct | social_open_direct |
| 创建独立群 | POST /groups | social_create_group |
| 空间详情 | GET /conversations/{id} | social_conversation |
| 成员加入/移出 | POST /conversations/{id}/members | social_change_membership |
| 接话计划 | GET /conversations/{id}/plans | social_plans |
| 分页历史/发送文本 | GET/POST /conversations/{id}/messages | social_messages / social_send |
| 单条消息 | GET /messages/{id} | social_message |
| 编辑/撤回真人消息 | POST /messages/{id}/revise；POST /messages/{id}/withdraw | social_revise_message / social_withdraw_message |
| 窗口控制 | POST /conversations/{id}/control | social_control_conversation |
| 成员暂停/恢复 | POST /conversations/{id}/members/participation | social_pause_member |
| 运行详情 | GET /runs/{id} | social_run_details |
| 停止一次运行 | POST /runs/{id}/cancel | social_cancel_run |
| 持久变更回放 | GET /events?after={cursor} | social_events |
| 长期订阅 | GET /subscribe?after={cursor} | 尚未实现 |

主要命令输入：

| 命令 | 字段与版本 |
| --- | --- |
| 联系人创建/修改 | requestId、name、persona、binding；修改追加联系人 expectedRevision。binding 为 providerId/modelId/reasoningEffort，可为空 |
| 打开私聊 | requestId；归档私聊返回原 ID，不自动恢复 |
| 创建群 | requestId、title、memberIds |
| 成员加入/移出 | requestId、actorId、action(add/remove)、空间 expectedRevision；没有历史范围选项 |
| 发送文本 | requestId、text、replyTo；mentions 可省略，按顺序选择接话 AI |
| 编辑/撤回 | requestId、消息 expectedRevision；编辑另带 text |
| 窗口控制 | requestId、空间 expectedRevision、action(pause/resume/end/archive/restore) |
| 成员暂停/恢复 | requestId、actorId、成员 expectedRevision、paused |

消息分页使用 before=sequence，当前每页最多 200 条，返回 messages/hasMore。空间 DTO 返回当前成员及暂停设置、pendingPlans、activeRun 和 currentBout；运行详情返回独立 RunDetails，旧运行可为 null。

原模型、配置与目录同步接口保留；旧 `/api/sessions` 和 chat-event 服务迁移期 Harness，不与社交契约混用。

## 3. 发送与幂等

```json
{
  "requestId": "00000000-0000-4000-8000-000000000001",
  "text": "一起讨论一下这个想法",
  "replyTo": null,
  "mentions": ["00000000-0000-4000-8000-000000000002"]
}
```

空间由路由或命令参数指定，senderId 从后端当前操作者取得。replyTo 只引用同空间消息；正文中的 @名字不替代 mentions。返回 SendReceipt 的 message 和可选 executionNotice，只表示真人消息已提交，不保证 AI 会回答，也不返回虚构的 committedCursor。

命令回执作用域为操作者 + 命令种类 + requestId。同内容重试返回原实体的当前可见状态，改内容重用 ID 返回冲突；幂等检查先于已成功命令的 revision 检查。回执只保存引用和摘要，撤回后不回放旧正文。取消单个 run 没有 requestId，终态重复取消仍成功；HTTP 返回 204，IPC 返回空成功结果。

前端保存失败保留输入；未知发送结果继续使用原 requestId/expectedRevision 重试。后续刷新失败不撤销已收到的成功回执。编辑按消息自身版本检查，其他成员追加消息不会使无关编辑冲突。

## 4. 持久事件

SocialEvent 为 cursor + change。change.kind 当前为 actor_changed、conversation_changed、message_changed、run_changed、bout_changed、plan_changed；各自带实体 ID、revision 和适用的 conversationId。

```json
{
  "cursor": "1042",
  "change": {
    "kind": "message_changed",
    "conversationId": "00000000-0000-4000-8000-000000000003",
    "messageId": "00000000-0000-4000-8000-000000000004",
    "revision": "3"
  }
}
```

cursor 在本地库全局单调递增，与业务事务同提交。事件只保存引用，不保存消息、记忆或修订原文；客户端读取当前可见版本，旧事件不能恢复撤回正文。数据库状态是权威数据，不按事件重建历史正文。

回放至少一次，客户端按 cursor 去重，并在状态应用成功后推进游标。消息变更按 MessageId 读取，已加载的早期消息也能更新。RuntimeEvent 仍只负责运行时通知，社交持久事件与生成事件独立。

## 5. 临时生成事件

SocialGenerationEvent 当前有 generation.delta、generation.reasoning_delta 和 execution.notice。正文/思考片段包含 runId、messageId、actorId、conversationId、deltaSeq、text；执行提示包含 conversationId/error，没有 run 的持久游标。

普通 token 不写入持久日志。重连读取已保存草稿/终态，最终记录覆盖临时缓冲，终态后忽略迟到 delta。临时广播溢出发 resync，客户端清除旧历史页与临时缓冲后重新分页，草稿保留；重置前的读取响应不能填回旧正文。

## 6. 启动、订阅与补齐

init 在一致读取中返回本地真人、联系人、窗口列表元数据、模型选项和 cursor，不返回全部历史，也不等待摘要。列表中的 latestMessage 与意识系统的历史摘要分别处理。

HTTP 随后以 after=cursor 订阅：先监听临时广播，再读取持久引用；SSE 的 change/id 推进持久 cursor，generation 没有 SSE id。当前每 250ms 补读已提交变更，单批最多 500 条。订阅断开只释放订阅，生成继续。

持久日志当前不压缩，尚未实现游标过期、空间过滤 watermark 和统一的所有边界错误。后续实时 IPC 应先注册监听，再建立订阅，显式取消释放资源；HTTP/IPC 在重放、溢出和退出时使用同一业务语义。具体订阅命令与字段在接入时确定。

## 7. 错误与后续能力

共享业务错误为 code/message/retryable。目前 invalid_input → 400，not_found → 404，not_authorized → 403，revision_conflict → 409，model_unavailable/budget_exhausted → 422，storage_failure/unavailable → 503。IPC 使用相同业务错误体；Axum 的 JSON/Path/Query 拒绝响应仍由框架产生，统一这部分错误是 M1 剩余工作。当前不返回 currentRevision。

消息已保存后的参与失败通过 executionNotice 或计划/运行状态表达，不能把保存回执改成失败。暂停、无效模型与订阅断线分别展示；磁盘不可写时不伪报已保存终态。

后续能力按所属阶段扩展：联系人生命周期与独立生成参数/工具授权、实时 IPC 和双适配验收归 M1；记忆、摘要与 contextReferences 归 M2；联系意图与自动参与归 M3。已读位置、列表搜索等按独立链路细化，不提供假成功占位接口。

群话题接续的规则见 [意识系统](03-consciousness.md#23-从群聊接续到私聊)：打开唯一私聊不发消息，引用只传来源标识并复核双方权限；显式转发产生新的目标消息，不能授予源空间读取权。

本地 HTTP 继续保留 loopback、Host/Origin 和客户端 Header 保护；未来公网认证另行设计。响应和事件不返回 Key、环境变量值或完整私人配置。目录打开 IPC 仍只接受 config/data 枚举，Web 无服务器目录打开接口。

## 8. 验收

1. HTTP/IPC 字段、业务状态、错误与顺序一致；框架拒绝和实时 IPC 的剩余部分单独验收。
2. 初始化到订阅的间隙不漏事件，重复回放不重复消息，状态应用失败不推进游标。
3. 断开订阅仍完成保存；重连和 resync 后终态覆盖 delta，旧读取不能恢复撤回内容。
4. 幂等命令、revision 冲突、无效成员、越权读取和写入失败均有明确结果。
5. M2 加入遗忘后，旧引用回放仍不能恢复失效内容；日志保留策略变化时补验游标过期与过滤补齐。
