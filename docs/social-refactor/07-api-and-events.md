# 07 API 与事件

[总纲](overview.md) · [存储与迁移](06-storage-and-migration.md) · [界面与桌面](08-frontend-and-desktop.md)

本文集中维护当前传输契约，后续能力单独列出，不预先固定尚未实现的路由。源码入口为 [HTTP](../../crates/web-server/src/social_api.rs)、[IPC](../../frontends/tauri/src-tauri/src/social_commands.rs)、[共享 DTO](../../features/social/src/wire.rs) 与 [持久事件](../../crates/events/src/social_event.rs)。

## 1. 共享业务入口

HTTP 与 Tauri 调用同一 SocialFeature 业务方法，传输层不实现身份、权限、记忆或调度规则。Feature trait 只负责生命周期与 downcast。显式记忆管理位于 SocialFeature 内部 memory 模块，ConsciousnessService 读取并构造上下文；窗口摘要由 summary 模块沿此边界生成与维护，原消息仍由聊天用例维护。

外部业务字段为 camelCase，枚举值按各自 serde 契约；ID 为 UUID 字符串，revision、sequence、cursor 为十进制字符串。请求使用 requestId；expectedRevision 针对被修改对象，不能拿空间 revision 替代消息或成员 revision。

个人屏蔽命令使用 `requestId`、`blocked`、`expectedRevision`（无条目为 null），返回带 `listed`、`blocked`、`revision` 的 ContactEntry；操作者取宿主当前账号。Conversation 的 `directBlocked` 表达任一方屏蔽后的私聊状态，群聊始终为 false。变更沿用 `contact_changed`，相关运行、计划和约定同时发出各自变更事件。详见[屏蔽规则](contact-blocking.md)。

## 2. 当前接口

HTTP 前缀为 `/api/social`。Web 与 Tauri 调用同一 SocialFeature。init 只返回公开账号、个人通讯录、窗口与模型选项；内部设定与绑定只通过管理员接口读取。实际操作者由后端固定，不接受客户端 senderId/operatorId 替换身份。当前完整接口列于下表。历史导入 DTO 沿用存储边界的 snake_case 字段，其余业务 DTO 为 camelCase。

| 用例 | HTTP（省略前缀） | Tauri command |
| --- | --- | --- |
| 启动快照 | GET /init | social_init |
| 公开账号/修改资料 | GET/PATCH /accounts/{id} | social_account / social_update_account |
| 个人通讯录添加/移除 | POST /contacts/{id} | social_set_contact |
| 个人屏蔽/解除 | POST /contacts/{id}/block | social_set_contact_block |
| 获授权 AI 列表/创建 AI | GET/POST /ai-accounts | social_managed_accounts / social_create_ai |
| 管理详情/修改设定与绑定 | GET/PATCH /ai-accounts/{id} | social_managed_account / social_update_ai |
| 初始管理员授予/撤销 | POST /ai-accounts/{id}/administrators | social_set_administrator |
| 管理员停用/恢复账号 | POST /ai-accounts/{id}/control | social_control_account |
| AI 只读账号快照 | GET /ai-accounts/{id}/view | social_account_view |
| AI 视角分页历史 | GET /ai-accounts/{id}/view/conversations/{conversation}/messages | social_account_view_messages |
| 检查/导入历史 JSON | GET/POST /history-import | social_preview_legacy / social_import_legacy |
| 打开唯一私聊 | POST /accounts/{id}/direct | social_open_direct |
| 创建独立群 | POST /groups | social_create_group |
| 空间详情 | GET /conversations/{id} | social_conversation |
| 成员加入/移出 | POST /conversations/{id}/members | social_change_membership |
| 联系授权与免打扰 | POST /conversations/{id}/contact-settings | social_update_contact_settings |
| 创建一次性约定 | POST /conversations/{id}/contact-intents | social_create_contact_intent |
| 取消一次性约定 | POST /conversations/{id}/contact-intents/{intent}/cancel | social_cancel_contact_intent |
| 接话计划 | GET /conversations/{id}/plans | social_plans |
| 接话规则 | POST /conversations/{id}/participation | social_update_participation |
| 分页历史/发送文字与附件 | GET/POST /conversations/{id}/messages | social_messages / social_send |
| 单窗口导出（可携附件） | GET /conversations/{id}/export?format=markdown\|json&includeImages=true\|false&includeFiles=true\|false | social_export_conversation |
| 个人已读/未读/置顶 | POST /conversations/{id}/list-state | social_update_conversation_list |
| 正文搜索 | GET /messages/search | social_search_messages |
| 原消息与前后记录 | GET /messages/{id}/context | social_message_context |
| 单条消息 | GET /messages/{id} | social_message |
| 消息图片 | GET /messages/{id}/images/{index}?revision={revision} | social_message_image |
| AI 视角消息图片 | GET /ai-accounts/{account}/view/messages/{id}/images/{index}?revision={revision} | social_account_view_image |
| 保存原图 | GET /messages/{id}/images/{index}/download?revision={revision} | social_save_image（account 为 null） |
| AI 视角保存原图 | GET /ai-accounts/{account}/view/messages/{id}/images/{index}/download?revision={revision} | social_save_image（account 为目标账号） |
| 保存普通文件 | GET /messages/{id}/files/{index}/download?revision={revision} | social_save_file（account 为 null） |
| AI 视角保存普通文件 | GET /ai-accounts/{account}/view/messages/{id}/files/{index}/download?revision={revision} | social_save_file（account 为目标账号） |
| 按窗口/AI 读取及创建记忆 | GET/POST /conversations/{id}/memories/{actor} | social_memories / social_create_memory |
| 编辑记忆 | PATCH /memories/{id} | social_update_memory |
| 遗忘记忆 | POST /memories/{id}/forget | social_forget_memory |
| 群话题来源预览 | POST /references/preview | social_reference_preview |
| 窗口摘要分页 | GET /conversations/{id}/summaries?before={sequence} | social_summaries |
| 开始下一段/重试摘要 | POST /conversations/{id}/summaries | social_request_summary |
| 摘要模型与启用设置 | POST /conversations/{id}/summary-settings | social_configure_summaries |
| 编辑/撤回真人消息 | POST /messages/{id}/revise；POST /messages/{id}/withdraw | social_revise_message / social_withdraw_message |
| 窗口控制 | POST /conversations/{id}/control | social_control_conversation |
| 成员暂停/恢复 | POST /conversations/{id}/members/participation | social_pause_member |
| 运行详情 | GET /runs/{id} | social_run_details |
| 停止一次运行 | POST /runs/{id}/cancel | social_cancel_run |
| 持久变更回放 | GET /events?after={cursor} | social_events |
| 长期订阅 | GET /subscribe?after={cursor} | social_subscribe |
| 确认一次桌面推送 | 由 SSE 读取提供传输背压 | social_ack_subscription |
| 释放桌面订阅 | 关闭 SSE 连接 | social_unsubscribe |

设置独立使用 `GET /api/settings` / `settings_snapshot`，仅返回 providers 与 version。模型服务及目录接口保留；旧 `/api/init`、`/api/config`、`/api/sessions`、`/api/cancel` 与旧聊天 IPC 已删除。

历史导入正文为预览中的所选 `sessions`，保留原导入 DTO 的 snake_case 字段（source_id/session_id/checksum 等），不接受路径或 operatorId。全目录检查重复 ID，再逐个复核所选预览；幂等性由来源 checksum 和稳定映射保证，不启动 AI。

Message.images 返回 `{index,mime,byteLength}` 数组，Message.files 返回 `{index,name,mime,byteLength}` 数组；index 是原消息 body 的 part 索引，撤回后数组为空。图片读取返回 `{mime,data}`，HTTP 设置 `Cache-Control: no-store`。普通接口复核当前操作者的成员身份，AI 视角接口在同一只读事务复核管理员和目标账号成员身份；revision 不符或已撤回拒绝读取。仅消息发送 JSON 路由上限为 40 MiB，以容纳最多 8 MiB 图片与 20 MiB 文件的 base64，业务层分别检查限制；图片还校验真实格式和尺寸。接口不接受本地路径或外部 URL。

普通文件下载统一返回 application/octet-stream、Content-Disposition: attachment、UTF-8 文件名、no-store 与 nosniff；不内联页面内容。IPC `social_save_file` 参数与 social_save_image 相同，复用原生对话框后的权限复核与原子文件写入，返回 cancelled 或 saved/fileName；Web 返回 downloaded/fileName。文件输入为 name/mime/data，最多 4 个、单个 10 MiB、合计 20 MiB，允许零字节；名称和 MIME 规则见[文件记录](file-messages.md)。

单窗口导出接受窗口 ID、`format`（markdown/json）、独立的 `includeImages` / `includeFiles`（HTTP 省略时均为 false，IPC 由客户端明确传入），不接受操作者、文件正文或目标路径。两个附件选项均关闭时 HTTP 返回 `{fileName,mediaType,messageCount,content}` JSON 文件内容包；任一开启时返回 ZIP 二进制，`Content-Disposition` 使用 UTF-8 filename*，`X-LLMN-Message-Count` 给出完整消息数。两者均设置 `Cache-Control: no-store`；ZIP 另设 `X-Content-Type-Options: nosniff`。Web 下载对应 Blob。IPC 的[保存适配](../../frontends/tauri/src-tauri/src/conversation_export.rs)打开原生“另存为”，取消返回 `{outcome:"cancelled"}`；成功写入返回 `{outcome:"saved",fileName,messageCount}`，文件名取实际保存目标。两个适配共用 SocialFeature 的读取和格式化，不产生持久事件。记录 JSON 的版本为无附件 v1、仅携图 v2、包含文件选项 v3，filePath 为 ZIP 中的实际相对路径；结构见[文件导出记录](file-message-export.md)。

保存原图走独立 download 路由，返回原始二进制、实际图片 MIME、UTF-8 文件名、no-store 和 nosniff；重新复核消息版本和对应读取权限。IPC `social_save_image` 接受 `{id,index,revision,account}`，account 仅指定需复核管理员权限的只读账号视角，不替换实际操作者。保存位置只能来自原生对话框，关闭对话框后重新读取并校验；取消/成功结果分别为 `{outcome:"cancelled"}` / `{outcome:"saved",fileName}`。Web 使用 `{outcome:"downloaded",fileName}`，不声称已写入磁盘。

主要命令输入：

| 命令 | 字段与版本 |
| --- | --- |
| AI 创建/修改 | requestId、name、bio、persona、binding；修改追加账号 expectedRevision。binding 为 providerId/modelId/reasoningEffort/temperature/maxTokens，可为空。创建自动赋予实际创建者初始管理权 |
| 公开资料修改 | requestId、name、bio、账号 expectedRevision；自己可修改，修改 AI 需管理权 |
| 个人通讯录 | requestId、listed、条目 expectedRevision；尚无条目时 expectedRevision 为 null，之后使用自己的条目版本，不使用 Actor.revision |
| 管理员授予/撤销 | requestId、accountId、granted、被管理 AI 的 expectedRevision；仅初始管理员可以操作，不能撤销初始管理员 |
| 账号停用/恢复 | requestId、账号 expectedRevision、action(archive/restore)；需管理权，与联系人移除、窗口归档分别处理 |
| 打开私聊 | requestId；即使联系人未列出仍沿用原窗口；停用 AI 的已有私聊返回原 ID，不自动恢复，也不能为停用 AI 创建新私聊 |
| 创建群 | requestId、title、memberIds |
| 成员加入/移出 | requestId、actorId、action(add/remove)、空间 expectedRevision；没有历史范围选项 |
| 发送文字与附件 | requestId、text、replyTo；images 可省略，元素为 mime/data，files 可省略，元素为 name/mime/data（data 为无 data URL 前缀的 base64）；mentions 可省略，按顺序选择接话 AI；reference 可省略/null，或 ReferenceSelection |
| 个人列表状态 | requestId、列表 expectedRevision、action：{kind:read,through} / {kind:unread} / {kind:pin,pinned}；初始 revision="1"，操作者由后端固定 |
| 编辑/撤回 | requestId、消息 expectedRevision；编辑另带 text |
| 窗口控制 | requestId、空间 expectedRevision、action(pause/resume/end/archive/restore) |
| 成员暂停/恢复 | requestId、actorId、成员 expectedRevision、paused |
| 联系设置 | requestId/settings；settings 含 revision（预期设置版本）、enabled、quietUntil（UTC/带偏移 RFC3339 或 null）、quietAction(defer/skip)，默认 revision="1"、关闭 |
| 创建约定 | requestId/actorId/text/dueAt/expiresAt；时间为明确带偏移 RFC3339，操作者与目标窗口由服务端推导；创建不写真人消息 |
| 取消约定 | requestId/expectedRevision（意图版本），窗口与 intent 均由路径/IPC 参数限定；仅创建者可取消 |
| 创建/编辑记忆 | requestId、kind(preference/fact/shared_experience/open_topic)、text(1–2000 字符)、source(null 或 messageId/revision)；编辑追加记忆 expectedRevision。窗口与 AI 由创建路径确定，编辑不能修改归属 |
| 遗忘记忆 | requestId、记忆 expectedRevision；返回已清空正文的 forgotten 条目，旧成功回执也只返回当前状态 |

ReferenceSelection 为 {conversationId, sources}；sources 只接受 1–8 个 message（kind/conversation_id/message_id/revision），可附带一个覆盖所选消息的 summary（kind/summary_id/revision）。预览参数为 {actorId, selection}，返回 {conversationId,title,messages,summary}；检查双方当前群成员和全部版本，不写消息、不调用模型、不接受客户端正文。发送时目标 AI/窗口和发起者由唯一私聊与实际操作者推导。

Message 增加 contextReference（无引用为 null），字段为 target/initiatedBy/attachedTo/selection/stale；target 沿用领域 ContextScope 的 actor_id/conversation_id，selection 为 camelCase，SourceRef 内部字段为 snake_case。修改/撤回来源与退群使关联消息的引用 stale，并通过已有 message_changed/conversation_changed/run_changed 等事件通知；回执重读返回当前状态，不回放旧原文。快照 SourceRef::Reference 固定关联消息版本，源原文/摘要另行记录，跨窗口材料必须有显式引用及双方权限依赖。

Memory 返回 id/actorId/conversationId/kind/text/status/revision/source/createdBy/updatedBy/createdAt/updatedAt。列表包含 confirmed 和 stale，排除 forgotten；来源查看复用单条消息接口。当前窗口成员可以管理本空间记忆，创建/编辑要求目标 AI 仍是成员，原消息必须为同窗口 committed 的确定版本。归档窗口或 AI 已离群时仍可遗忘已有记忆。管理员权限不会扩大范围。来源修订/撤回和记忆编辑/遗忘提升上下文版本、中断该窗口在飞回复；新增记忆只作用于新运行。

Conversation.memoryOwnerIds 列出本窗口仍有 confirmed/stale 条目的 AI，包括已离群 AI，供详情页继续查看与遗忘；不使它重新成为成员。模型请求只读取当前运行 AI 的 confirmed 条目。

共同 Account 字段为 id/kind/name/avatarRef/bio/status/revision，真人与 AI 相同。ManagedAi 包含 account、persona、binding、administrators(accountId/initial)、isInitialAdministrator，仅供有权限的管理接口。联系人操作返回 ContactEntry(ownerId/accountId/listed/revision)。

binding.temperature 与 maxTokens 可省略或为 null；前者使用模型服务默认值，后者使用意识系统自动输出预算，均不继承全局 GUI 参数。显式值按协议/思考模式与模型容量校验，不静默裁减。init 的 ModelOption 增加 generationLimits，以 default/off/low/medium/high/max 为键，各值为 temperatureMax（null 表示该模式不可调）、minOutputTokens、maxOutputTokens（null 表示未声明）；当前社交返回的上限还受上下文容量约束。该数据描述模型选项，不包含任何 AI 的内部参数。字段的共享 HTTP/IPC 验证见 [参数记录](m1-generation-parameters.md)。

AccountView 返回 operatorId 与 snapshot；snapshot.selfId/selfAccount 为被查看 AI，操作者仍为真实管理员。每次读取快照与分页历史复核管理授权，窗口按该 AI 的当前成员身份筛选；查看停用 AI 仍可行，退出群后不能读取该群。它是可手动刷新的只读快照，不使用实际操作者的全局事件订阅，不提供代发消息或远程登录令牌。

消息分页使用 before=sequence，每页最多 200 条，返回 messages/hasMore/readThrough。最新页的 readThrough 为同一读事务获得的可确认完成位置（十进制字符串）；向前分页和管理员只读视角为 null。空间 DTO 返回当前成员及暂停设置、pendingPlans、activeRun、currentBout 与 participation（policy/state/budgetDay/autoTurnsToday/reservedTokensToday）；运行详情返回独立 RunDetails，旧运行可为 null。

Conversation.listState 返回 readThrough/latestDelivery/unreadCount/markedUnread/pinned/revision；三个位置/版本字段为十进制字符串，unreadCount 为数字，个人状态变更不提升空间 revision。

正文搜索 HTTP query 为 query/conversationId/before，IPC 为同名 input 对象；后两项可省略/null。返回 messages/nextBefore；每页最多 50 条，nextBefore 是排除边界的完成位置，末页为 null。搜索范围按实际操作者当前成员关系决定。消息定位返回 messages/hasMore/hasNewer，包含目标及前后各最多 50 条；hasMore/hasNewer 表示两侧还有记录，无已读确认位置。

接话规则输入为 requestId/policy；policy 包含 revision（预期规则版本）、mergeDelayMs、idleSeconds、limitedAuto、maxConsecutive、maxAutoPerBout、maxAutoPerDay、dailyTokenBudget。默认规则 revision="1"；规则更新提升自己的版本，恢复旧回执返回当前窗口。范围由路径限定，操作者由后端固定。Plan DTO 包含 automatic/inputIds/notBefore/expiresAt，新增 contactIntentId（普通计划为 null）与可空 triggerId，两种触发恰有一个，计时为 UTC 字符串，自动额度失败用 participation_limit 表达，不影响真人消息保存。

Conversation.contactSchedule 返回 settings 与 intents（全部活动及最近 50 条记录）。ContactIntent 的 camelCase 字段为 id/conversationId/actorId/createdBy/authorizationId/text/dueAt/expiresAt/utcOffsetMinutes/status/planId/revision/reason/createdAt/updatedAt。创建/取消返回当前 ContactIntent，设置返回 ContactScheduleView。内容和原始授权不可就地修改，重试旧命令返回当前状态，不复活已取消意图。

模型服务与目录同步接口继续使用；设置快照由 `/api/settings` 和 `settings_snapshot` 提供。旧 `/api/sessions`、旧聊天 IPC 与 chat-event 已删除。联系人关系和 AI 账号管理分别使用上表的专用接口，历史记录通过显式导入进入新版。

模型图片能力覆盖使用设置接口 `POST /api/models/image-input` / IPC `set_model_image_input`，共用 `{provider,model,imageInput}`；imageInput 为 true/false，null 表示跟随目录。成功返回更新后的 ProviderInfo 数组，保存失败保留原配置。ModelInfo 与社交 ModelOption 都提供 `imageInput: {support,source}`，support 为 supported/unsupported/unknown，source 为 config/catalog/builtin/unknown。

普通社交 Init 的 `imageInputs` 仅将 AI 账号 ID 映射到三态能力，不包含模型绑定或内部设定；管理员只读账号视角没有输入区，返回空映射。能力设置不生成聊天事件；界面返回社交页时刷新快照。执行按实际冻结配置检查全部请求，明确不支持图片时保存 RunDetails.error.code 为 `image_input_unsupported`，不调用 provider，真人消息仍保存。详见[图片能力记录](image-capabilities.md)。

摘要分页返回 settings/segments/hasMore，每页 50 段、按 throughSequence 降序，before 排除该边界。SummarySettings 返回 id（窗口 ID）、enabled、binding、revision；未配置时 revision="0"、enabled=false。保存参数为 requestId/expectedRevision/enabled/binding，binding 包含 providerId/modelId/reasoningEffort/maxTokens，不包含密钥、温度或 AI 人格。

开始摘要参数为 requestId/retry，retry 为 null 表示下一段，或 {id, expectedRevision} 表示重建同一分段。没有较早原文时返回 null，不调用模型。摘要包含范围、text/status/revision、来源、模型绑定、attempt、错误和时间；sources 沿用 SourceRef 的 snake_case 字段（kind/message_id/conversation_id/revision），其余 DTO 为 camelCase。发起人由后端固定，范围由存储推导；调用方不能指定摘要正文、原文集合或任意操作者。配置/请求回执幂等，冲突返回共享错误。

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

命令回执作用域为操作者 + 命令种类 + requestId。同内容重试返回原实体的当前可见状态，改内容重用 ID 返回冲突；管理权限先于管理命令的回执读取；撤销权限后不能通过旧 requestId 再读内部数据。幂等检查先于已成功命令的 revision 检查。回执只保存引用和摘要，撤回后不回放旧正文。取消单个 run 没有 requestId，终态重复取消仍成功；HTTP 返回 204，IPC 返回空成功结果。

前端保存失败保留输入；未知发送结果继续使用原 requestId/expectedRevision 重试。后续刷新失败不撤销已收到的成功回执。编辑按消息自身版本检查，其他成员追加消息不会使无关编辑冲突。

## 4. 持久事件

SocialEvent 为 cursor + change。change.kind 当前为 actor_changed、contact_changed、conversation_changed、message_changed、memory_changed、summaries_changed、run_changed、bout_changed、plan_changed、participation_changed、contact_schedule_changed、conversation_list_changed。conversation_list_changed 带 conversationId/actorId/revision，客户端重读自己的列表状态；它不触发历史或意识重读。summaries_changed、participation_changed 与 contact_schedule_changed 只带 conversationId，分别重读摘要、窗口接话或约定状态；其余变更带实体 ID、revision 和适用的 conversationId。contact_changed 带 ownerId/accountId；memory_changed 带 conversationId/actorId/memoryId/revision，不含正文。通讯录变更与 Actor 的管理/资料/停用变更分别表达。

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

回放至少一次，客户端按 cursor 去重，并在状态应用成功后推进游标。消息变更按 MessageId 读取，已加载的早期消息也能更新。持久事件由 SocialStore 提供，临时生成事件由 SocialFeature 发布；两者不依赖旧运行时事件总线。

## 5. 临时生成事件

SocialGenerationEvent 当前有 generation.delta、generation.reasoning_delta 和 execution.notice。正文/思考片段包含 runId、messageId、actorId、conversationId、deltaSeq、text；执行提示包含 conversationId/error，没有 run 的持久游标。

普通 token 不写入持久日志。重连读取已保存草稿/终态，最终记录覆盖临时缓冲，终态后忽略迟到 delta。临时广播溢出发 resync，客户端清除旧历史页与临时缓冲后重新分页，草稿保留；重置前的读取响应不能填回旧正文。

## 6. 启动、订阅与补齐

init 在一致读取中返回 selfId/selfName/selfAccount、公开 accounts（含停用账号）、当前操作者的 addressBook、窗口列表元数据、模型选项和 cursor。通讯录按自己的 listed 条目展示，未列出和停用分别处理；accounts 保留用于群成员/历史身份与重新添加。公开快照不包含内部 persona/绑定/管理员，管理页面使用独立的获授权接口。列表 latestMessage 与意识系统的历史摘要分别处理。

HTTP 与 IPC 共用 SocialSubscription：先监听临时广播，再以 after=cursor 读取持久引用，每 250ms 补读已提交变更，单批最多 500 条，临时广播最多保留 256 个事件。SSE 的 change/id 标识持久 cursor，generation 没有 SSE id。订阅断开只释放订阅，生成继续。

桌面命令与推送：

| 项目 | 契约 |
| --- | --- |
| social_subscribe | 输入 id（客户端 UUID）、after（十进制字符串游标）、channel；前端先注册 Channel callback。初始化和首批读取成功后返回空结果，事件可以早于命令返回到达 |
| Channel 事件 | `{kind:"event", sequence:1, event:{kind:"change",event:SocialEvent}}`；内层还支持 generation/event 和 resync。sequence 为本次订阅的 u32 序号，不是持久 cursor |
| social_ack_subscription | 输入 id/sequence，状态应用成功后确认；正文/思考片段和 resync 也按序确认。重复已确认序号成功，提前确认或不匹配序号报 invalid_input |
| social_unsubscribe | 输入 id，幂等释放调用窗口拥有的该订阅；不会取消 AI run，也不能释放其他窗口或替代订阅 |
| 终止推送 | `{kind:"error",error:{code,message,retryable}}` 或 `{kind:"closed"}`；异常关闭触发重连 |

每个 WebView 只有一个社交订阅。替代订阅、页面开始重载、窗口销毁和应用退出释放旧订阅及 callback；后端至多有一条未确认推送，30 秒未确认则关闭并给出错误。模型不等待订阅者，临时广播溢出用 resync 补齐。前端按状态应用成功后的持久 cursor 重连，失败连接中的迟到读取不推进新连接游标。

草稿持久化属于客户端，无新增社交 HTTP 接口、DTO 或持久事件。桌面宿主另外提供以下关闭协调命令（仅主 WebView 可调用）：

| 命令/事件 | 契约 |
| --- | --- |
| desktop_register_draft_guard | 输入 page UUID，注册当前页面；替代旧页面释放旧等待 |
| desktop_unregister_draft_guard | 输入 page UUID，仅释放匹配页面 |
| drafts-prepare-close | 宿主向主页面发出本次关闭 request UUID；页面停止编辑，等待附件读取与草稿事务完成 |
| desktop_finish_draft_close | 输入 page、request UUID 与 error（成功为 null）；只接受当前页面和当前请求的确认 |

页面在注册前装好监听；主窗口重载/销毁释放旧等待。宿主最多等 10 秒，失败通过 backend-shutdown-failed 保留窗口并恢复编辑；成功才进入既有 Runtime 关闭阶段，未注册的初始页面可以直接关闭。该确认不传输草稿正文或文件路径。

持久日志当前不压缩，尚未实现游标过期、空间过滤 watermark 和统一的所有边界错误。HTTP/IPC 在重放、溢出和订阅释放时使用同一业务语义；宿主退出等待运行中断保存和数据库 writer 收尾，见 [运行时](09-runtime-and-configuration.md#5-生命周期与关闭)。

## 7. 错误与后续能力

共享业务错误为 code/message/retryable。目前 invalid_input → 400，not_found → 404，not_authorized → 403，revision_conflict → 409，model_unavailable/budget_exhausted → 422，storage_failure/unavailable → 503。IPC 使用相同业务错误体。社交 HTTP 的 JSON/Path/Query 解析失败也返回该结构，code 为 invalid_input、retryable 为 false；保留传输状态码：非法 JSON 语法、路径和查询为 400，JSON 字段/类型错误为 422，缺少 JSON Content-Type 为 415，超过正文上限为 413。当前不返回 currentRevision。

消息已保存后的参与失败通过 executionNotice 或计划/运行状态表达，不能把保存回执改成失败。账号停用通过 account_disabled 提示，暂停、无效模型与订阅断线分别展示；磁盘不可写时不伪报已保存终态。

后续能力按所属阶段扩展：阶段完整双适配验收归 M1；有限自动参与及一次性联系意图、免打扰已在 M3 两批接通；重复日程与自然语言约定另行设计。工具授权在具体产品需求确定后独立实施，不作为 M1 前置。授权与账号查看需由同一业务层验证真实操作者及目标范围，不接受前端任意替换 senderId。已读位置、列表搜索和联系人屏蔽已接通；其余能力按独立链路细化，不提供假成功占位接口。

群话题接续的规则见 [意识系统](03-consciousness.md#23-从群聊接续到私聊)：打开唯一私聊不发消息，引用只传来源标识并复核双方权限；显式转发产生新的目标消息，不能授予源空间读取权。

本地 HTTP 继续保留 loopback、Host/Origin 和客户端 Header 保护；未来公网认证另行设计。响应和事件不返回 Key、环境变量值或完整私人配置。目录打开 IPC 仍只接受 config/data 枚举，Web 无服务器目录打开接口。

## 8. 验收

1. HTTP/IPC 字段、业务状态、错误与顺序一致；框架拒绝和阶段完整异常场景单独验收。
2. 初始化到订阅的间隙不漏事件，重复回放不重复消息，状态应用失败不推进游标。
3. 断开订阅仍完成保存；重连和 resync 后终态覆盖 delta，旧读取不能恢复撤回内容。
4. 幂等命令、revision 冲突、无效成员、越权读取和写入失败均有明确结果。
5. M2 加入遗忘后，旧引用回放仍不能恢复失效内容；日志保留策略变化时补验游标过期与过滤补齐。
