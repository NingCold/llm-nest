# API 与事件

[总纲](overview.md) · [存储与迁移](storage.md) · [界面与桌面](frontend.md)

## 已实现的云端接口

聊天的 `sealedV1`、Matrix 公钥与设备信箱、恢复密文和 AI 资料 CAS 接口集中见[加密契约](crypto-contract.md)。启用后新消息必须携带密文封装，正文为空；流式、结果与同机补传不允许明文回退。旧未启用窗口沿用普通接口。群、附件、跨窗引用和约定复用同一加密格式；完整搜索和导出在授权客户端解密完成。旧历史转换留待第四批。

默认 CloudApp 使用 `cloud-server` 的 `/api`，与保留的本地 `/api/social` 是两套不同契约。所有聊天操作者取登录会话，不接受客户端 senderId/operatorId。浏览器使用同源 HttpOnly Cookie；桌面经原生网络层持有内存 Cookie。修改请求带 `X-LLMN-Client: 1`，服务器检查来源与跨站请求。JSON 业务错误为 `{code,message}`，常用状态为 400、401、403、404、409、500；入口解析错误沿用 HTTP 状态。

| 用途 | 方法与路径（省略 `/api`） |
| --- | --- |
| 邀请注册 / 修正邮箱 / 重发验证 | POST `/auth/register`；PATCH `/auth/registration`；POST `/auth/resend` |
| 验证邮箱 / 登录 / 退出 | POST `/auth/verify`；POST `/auth/login`；POST `/auth/logout` |
| 密码找回 / 重置 | POST `/auth/forgot`；POST `/auth/reset` |
| 当前账号 / 修改公开资料 | GET / PATCH `/me` |
| 登录会话 / 撤销会话 | GET `/auth/sessions`；DELETE `/auth/sessions/{id}` |
| 账号、通讯录、私聊/群聊一致快照及 cursor | GET `/chat/init` |
| 按名字或完整 UUID 查账号 | GET `/chat/accounts?query=…` |
| 添加 / 移除联系人 | POST `/chat/contacts`，`{accountId,listed}` |
| 查看联系人状态 / 屏蔽私聊 | GET `/chat/contacts/{accountId}`；PUT `/chat/contacts/{accountId}/block` |
| 打开双方唯一私聊 | POST `/chat/direct`，`{accountId}` |
| 历史 / 发送 | GET / POST `/chat/conversations/{id}/messages` |
| 编辑 / 撤回自己的消息 | PATCH `/chat/messages/{id}`；POST `/chat/messages/{id}/withdraw` |
| 已读、手动未读、置顶 | PUT `/chat/conversations/{id}/list` |
| 正文字面搜索 | GET `/chat/search?query=…&conversationId=…&beforePosition=…` |
| 原消息前后记录 | GET `/chat/messages/{id}/context` |
| 附件读取 / 图片预览 | GET `/chat/messages/{id}/attachments/{index}?revision=…`；图片可加 `inline=true` |
| 单窗口导出 | GET `/chat/conversations/{id}/export?format=markdown\|json&includeImages=true\|false&includeFiles=true\|false` |
| 群话题来源预览 | POST `/chat/conversations/{id}/reference-preview` |
| 在线节点记忆与摘要 | POST `/chat/conversations/{id}/ai/{aiId}/consciousness` |
| 持久变更提示 SSE | GET `/chat/events?after=…` |

ID 是 UUID，revision、position、listRevision、readPosition、cursor 是十进制字符串。公开 Account 为 `{id,name,bio,kind,status}`，status 为 active/disabled。Conversation 为 `{id,kind,name,peer,members,revision,directBlocked,lastMessage,latestPosition,unreadCount,manualUnread,pinned,readPosition,listRevision}`；kind 为 direct/group，私聊 name 为当前对方名字，peer 为对方 Account、成员角色均为 member，群聊 peer 为 null。members 为 `[{account,role:owner|admin|member}]`，不使用虚拟账号代表群。消息返回稳定 id/conversationId/senderId、senderName、replyTo、mentions、aiResponses、text、createdAt/updatedAt、revision/position、committed、generating、interrupted、failed 或 withdrawn 状态，以及 `attachments[{index,kind,name,mime,byteLength}]`。index 是该云端消息的附件索引，不是旧本地 MessageBody 的 part index。

消息发送使用 `{requestId,text,replyTo?,mentions?,reference?,images:[{mime,data}],files:[{name,mime,data}]}`，data 为原字节 base64，不接受路径或 URL。图片支持 PNG/JPEG/WebP，每条最多 4 张、单张 4 MiB、合计 8 MiB、边长 4096；文件最多 4 个、单个 10 MiB、合计 20 MiB。发送 HTTP 正文上限 40 MiB，服务端检查真实图片格式及解码。编辑使用 `{requestId,expectedRevision,text}`，保留附件；撤回使用相同 ID/版本字段，清空正文和附件。下载复核当前成员和当前 revision，普通文件仅 attachment/octet-stream，响应 no-store/nosniff；图片下载采用稳定消息 ID、索引和识别扩展名。

replyTo 为同窗口未撤回的 committed 消息 ID 或 null，mentions 为有序当前成员 ID 数组，可提及真人；只认结构化字段，正文中的名字或 `@名字` 不触发模型。senderName 保留消息署名，不依赖发送者仍在群内。群聊按 mentions 顺序选出有效 AI，再追加被回复消息的 AI 发送者并去重；默认没有明确目标不调用模型；显式开启 limitedAuto 后，普通输入和成功的 AI 回复可以按规则继续接话。私聊普通连续输入可合并为一次任务，原消息分别保存。

列表修改使用 `{requestId,expectedRevision,pinned?,readPosition?,manualUnread?}`，其中 expectedRevision 指 listRevision。已读位置必须属于当前窗口且单调，自发和撤回消息不计未读。分页 `before`/`limit` 返回升序 messages、hasMore、同一快照的 latestPosition；搜索按 position 倒序、每页 50，中文/符号按字面、ASCII 英文不区分大小写。

消息和列表命令以实际账号/requestId 去重；同 ID 不同内容冲突。回执只存规范化命令的 SHA256 与实体 ID，重试复核身份后返回当前权威实体，不能借旧回执恢复撤回内容。导出读取完整窗口一致快照，包含稳定发送者 ID、当前正文/状态；replyTo 仅指同窗口，跨窗口引用只导出来源标识，不展开原文或内部记忆。附件选项任一开启返回同时含 chat.md、chat.json 与所选原字节的 ZIP，JSON filePath 对应实际路径；它不是整库备份。

SSE 的事件名为 `change`，SSE id 与数据 cursor 一致，数据为 `{cursor,conversationId?}`，只表示需要重读，不携带正文。事件按登录账号过滤，服务端持续复核会话；客户端仅在快照应用成功后推进 cursor，断线后用最新 cursor 重连，也支持标准 Last-Event-ID。快照与 cursor 一致读取，事件分配前取得事务 advisory lock，避免提交顺序倒置导致漏事件。AI 的生成片段也通过持久检查点和 change 提示刷新，节点侧协议见下节。

### 群管理、屏蔽与权限

以下群命令均带 requestId；除创建外带字符串 expectedRevision，对应 Conversation.revision，成功返回当前 Conversation（退出返回 `{ok:true}`）。建群者自动为 owner，2–100 位成员，允许全真人群；群名 1–100 字。

| 方法与路径（省略 `/api`） | 请求中的业务字段与权限 |
| --- | --- |
| POST `/chat/groups` | `{name,memberIds}`，至少另一位成员 |
| PUT `/chat/groups/{id}` | `{name}`，owner/admin |
| POST `/chat/groups/{id}/members` | `{accountId}`，owner/admin，可邀请不由自己管理的 AI |
| PUT `/chat/groups/{id}/members/{accountId}` | `{role:admin|member}`，仅 owner 任免管理员 |
| DELETE `/chat/groups/{id}/members/{accountId}` | JSON 命令正文；owner 可移出其他非 owner，admin 仅可移出普通成员 |
| POST `/chat/groups/{id}/owner` | `{accountId}`，owner 转让给其他当前真人成员，原 owner 变 member |
| POST `/chat/groups/{id}/leave` | 普通成员/admin 退出；owner 须先转让 |

群角色不来自 AI 管理员身份。群管理修改提升窗口 revision，取消该群未完成任务并清除失效派生片段，向变更前后成员发刷新事件。当前成员可读完整历史；退出/被移出后，历史、搜索、附件、导出均重新拒绝访问，重新加入不恢复旧任务。已经提交的原文不会因成员离开而删除。

GET `/chat/contacts/{accountId}` 返回 `{account,listed,blocked,revision,directBlocked}`，无条目时 revision 为 `"0"`。PUT `/chat/contacts/{accountId}/block` 接收 `{requestId,expectedRevision,blocked}`，返回同一结构；listed 与 blocked 独立，添加/移除联系人不清除屏蔽。任一方向屏蔽只限制双方私聊新消息，取消该私聊未完成任务，保留有效累计片段为 interrupted；共同群历史与正常提及/回复不受影响。解除屏蔽不重放旧任务。

客户端草稿的有序 mentions、replyTo 与文字/附件、待确认发送快照同事务保存。刷新不自动发送，手动重试未知结果使用原 ID 与原内容；仅首次发送得到明确拒绝时才释放 pending 供修改，已有 pending 的重试即使收到 403/404 也不能据此认定旧消息未保存。

### 接话控制与一次性约定

以下接口只接受当前真人会话。私聊真人、群主和群管理员可以修改窗口规则与授权；普通群成员可在授权开启后创建自己的约定，并取消自己尚未完成的约定。管理 AI 不扩大群内权限。所有写操作带 `requestId`，同 ID 只能用于同一内容，回执只保存指纹与目标标识，重试读取当前结果。

| 方法与路径（省略 `/api`） | 请求与返回 |
| --- | --- |
| GET `/chat/conversations/{id}/participation` | `{revision,canManage,policy,state,settings,intents}` |
| PUT 同一路径 | `{requestId,expectedRevision,policy}`，完整替换规则，取消旧自动任务 |
| POST `/chat/conversations/{id}/participation/control` | `{requestId,expectedRevision,action:pause\|resume\|end,aiId?}`；AI 标识仅用于暂停/恢复 |
| PUT `/chat/conversations/{id}/contact-settings` | `{requestId,expectedRevision,enabled,quietUntil,quietAction:defer\|skip}`，版本来自 settings |
| POST `/chat/conversations/{id}/contact-intents` | `{requestId,aiId,content,dueAt,expiresAt,utcOffsetMinutes}` → 当前意图 |
| POST `/chat/conversations/{id}/contact-intents/{intentId}/cancel` | `{requestId,expectedRevision}` → 当前意图 |

policy 包含 `mode:mentionedOnly|limitedAuto`、`directMergeMs`、`maxConsecutiveAi`、`maxBoutCalls`、`maxDailyCalls`、`idleSeconds`。state 提供 `paused`、`pausedAiIds`、`boutId`、`consecutiveAi`、`boutCalls`、`dailyCalls` 与 `reason`。settings 包含自身 revision、enabled、quietUntil 和 quietAction；时间使用 RFC3339 UTC，偏移以分钟记录创建时的本机时区。

ContactIntent 提供 `{id,creatorId,aiId,content,dueAt,expiresAt,utcOffsetMinutes,status,revision,reason,taskId}`。状态为 pending/claimed/executing/delivered/cancelled/skipped/expired/failed/uncertain；返回全部活动约定及最近 50 条终态记录。授权内容不可原地修改。结束交谈保留未来 pending 约定，关闭授权撤销全部未完成约定。免打扰只限制约定，普通回复照常排队。控制、到期与状态变化使用现有持久 `change` 事件提示刷新，详细额度和恢复规则见[参与与调度](participation.md)。

## 云端 AI 与节点接口

以下路由均省略 `/api`，共享类型见 `connector::node`，服务端入口为 `server/cloud/src/nodes.rs`、`management.rs` 与 `consciousness.rs`。当前协议版本为 4，JSON 字段与 tagged frame 的 `type` 均为 camelCase；UUID 和版本为字符串，时间为 RFC3339。实现与联调状态见[验证指南](../validation/README.md#云端-ai-节点验证)。

### AI 账号与配对

| 身份 | 方法与路径 | 请求与结果 |
| --- | --- | --- |
| 真人会话 | GET / POST `/ai` | 列出自己管理的 AI；创建 `{requestId,name,bio}`，创建者成为初始管理员 |
| AI 管理员会话 | PUT `/ai/{id}/status` | `{requestId,expectedStatusRevision,status:active|disabled}` → ManagedAi，任一管理员可操作 |
| AI 管理员会话 | POST `/ai/{id}/node` | `{requestId,pairingCode,expectedBindingVersion}`，绑定或替换节点 |
| AI 管理员会话 | POST `/ai/{id}/node/revoke` | `{requestId,expectedBindingVersion}`，撤销节点资格 |
| 配对秘密 Bearer | POST `/node/pairings` | `{nodeId,nodeName}` → `{pairingId,userCode,expiresAt}`，有效期十分钟 |
| 同一配对秘密 | GET `/node/pairings/{id}` | `{state:pending\|authorized,aiId?,bindingVersion?}`；已授权且仍有效绑定可安全重查 |

`ManagedAi` 返回 `{account:{id,name,bio,kind,status},role:initial|admin,bindingVersion,statusRevision,profileRevision,administratorsRevision,node}`，未绑定的版本为 `"0"`，node 为 null 或 `{id,name,online,lastSeenAt,capabilities}`。初始管理员可维护真人管理员名单，任一当前管理员可管理公开资料、节点、状态与在线档案，并打开只读账号视角。

停用 AI 保留账号、历史和节点绑定，取消该 AI 所有未完成任务并保留有效累计片段为 interrupted，拒绝旧任务继续执行/提交；其他 AI 的任务独立。停用期间仍保存发给该 AI 的真人私聊消息，但不为它创建新任务。恢复只响应新消息，不重放旧任务。

节点先生成 UUID nodeId 与 32 随机字节的 64 位小写十六进制 secret，作为配对请求的 `Authorization: Bearer …`，授权后沿用该秘密。服务端仅保存哈希。一个 AI 只有一个有效绑定，一个 nodeId 可承载多个 AI 各自的凭据；替换/撤销提升绑定版本，旧资格及未完成任务同事务失效。节点不得保存或使用真人登录凭据。

### 云端账号管理与只读查看

| 方法与路径 | 请求与结果 |
| --- | --- |
| PUT `/ai/{id}/profile` | `{requestId,expectedProfileRevision,name,bio}` → 当前 ManagedAi |
| GET `/ai/{id}/administrators` | `{revision,administrators:[{account,role:initial|admin}]}` |
| PUT `/ai/{id}/administrators/{target}` | 仅初始管理员，`{requestId,expectedRevision,enabled}` → 当前名单；target 必须为真人，不能修改初始管理员 |
| GET `/ai/{id}/view` | `{account,conversations}`，目标 AI 当前窗口的一致快照 |
| GET `/ai/{id}/view/conversations/{cid}/messages?before=…&limit=…` | 当前窗口消息分页，使用与普通历史相同的 Page 格式 |
| GET `/ai/{id}/view/conversations/{cid}/messages/{mid}/attachments/{index}?revision=…&inline=…` | 当前消息版本的附件，inline 仅用于支持的图片预览 |
| POST `/ai/{id}/management` | `{requestId,operation}` → AgentManagementView，仅当前支持该能力的在线节点 |

所有请求保留真实真人会话。公开资料与管理员修改在事务内检查预期版本并写入指纹回执；同 ID/同内容重试返回当前结果，当前授权失效后不能重用旧回执。账号视角每次快照、分页、附件读取都复核管理员权限与目标 AI 当前成员身份；附件另复核消息窗口、revision 与撤回状态。读取不改变个人已读或事件，不提供代发、独立订阅、通讯录快照或账号视角导出。

`operation` 为 `{type:"view"}` 或 `{type:"saveProfile",expectedRevision,persona,providerId,modelId,reasoningEffort,temperature,maxTokens,autoDailyTokens}`。可空参数以 null 表示默认值。AgentManagementView 为 `{profile:{revision,persona,providerId,modelId,reasoningEffort,temperature,maxTokens,autoDailyTokens},models:[{providerId,modelId,name,reasoningEfforts,imageInput}]}`，imageInput 为 supported/unsupported/unknown。远程 DTO 不含供应商地址、Key 或环境变量名。

云端复核管理员、绑定版本和当前连接后，在内存转发，等待至多 30 秒；返回前再次复核会话、管理权、绑定与连接。节点离线不排队，不支持时返回明确能力错误；超时或连接变化表示结果未确认，保留原 requestId 和原操作重试。节点保存完整档案与指纹回执同事务，重放返回当前视图，普通配置只影响新任务；模型/参数无效返回具体原因或可用候选，版本冲突要求刷新。模型 Key 仍只在节点本机配置。

### 节点任务与材料

所有节点请求带专用 Bearer 与 `X-LLMN-Client: 1`，服务端据此推导当前 AI 和绑定，不接收任意 aiId 切换权限。HTTPS/WSS 验证证书，自定义 CA 必须明确传入；仅回环开发允许 HTTP/WS。

| 方法与路径 | 语义 |
| --- | --- |
| GET `/node/tasks` | 当前绑定的 running 或未过期且轮到本窗口队首的 queued 任务，按持久顺序返回 |
| GET `/node/tasks/{id}` | 查询当前绑定任务，包括终态，供恢复与回执核对 |
| GET `/node/tasks/{id}/materials` | `{task,ai,conversation,messages,sources,reference}`；消息任务读取触发位置以内原文及本窗更早队列且其触发位置不晚于当前的已完成 AI 输出，取最近最多 100 条，再补全部必要合并输入/回复来源；contact 读取当前窗口最近历史，不带隐式群引用；开始后只返回冻结来源 |
| GET `/node/tasks/{id}/messages/{messageId}/attachments/{index}?revision=…` | 当前授权与来源版本复核后的图片原字节；普通文件只提供元信息 |
| POST `/node/tasks/{id}/start` | `{requestId,taskRevision,conversationRevision,sources:[{messageId,revision}],derivedSources,materialScopes,configurationRevision}` → 持久开始确认 |
| POST `/node/tasks/{id}/result` | `{requestId,runId,messageId,confirmationId,status,text,errorCode?,usage?}` → 当前任务 |
| POST `/node/tasks/{id}/reject` | `{requestId,taskRevision,code,message}`，用于模型调用前的配置、材料、预算或能力错误 |
| POST `/chat/ai-tasks/{id}/cancel` | 当前真人成员会话，`{requestId}`，停止该任务 |

`NodeTask` 为 `{id,aiId,conversationId,bindingVersion,revision,trigger,automatic,status,expiresAt}`，status 为 queued/running/completed/failed/interrupted/cancelled/expired/uncertain。trigger 为 `{type:message,messageId,inputIds}` 或 `{type:contact,intentId,revision,creatorId,content,dueAt}`；约定没有触发消息，也不附带群话题引用。automatic 标记有限自动接话和一次性约定，供节点预留自己的自动 token 预算。

消息触发任务有效期五分钟，合并后从最新输入计算；约定使用明确的 expiresAt。同一窗口按 cloud_node_tasks.queue_position 串行开始，未到执行时间的延后约定不阻挡普通消息，跨 AI/节点最多一个 running。非队首返回 409 `task_not_ready`，已有运行返回 409 `conversation_busy`，两者均明确未开始、不返回开始确认；普通 409、网络错误或确认丢失不能视为这个保证。开始回执为 `{taskId,runId,messageId,confirmationId,bindingVersion}`；相同 requestId 和内容可安全重查，改变原命令报冲突。调用顺序与不确定恢复见[执行设计](execution.md#当前云端私聊与群聊执行)。

`NodeConversation` 为 `{id,kind,name,revision,members:[NodeAccount]}`，members 为当前成员，不含客户端群角色包装。`NodeMessage` 为 `{id,senderId,senderName,replyTo,mentions,text,revision,position,createdAt,attachments}`，附件 `{index,kind,mime,byteLength,name?}`，sources 为完整读取材料的 ID/修订列表。消息任务的材料限定于触发位置以内原文，另允许本窗 queue_position 更早、已 completed 且其触发位置不晚于本次触发的任务输出，不纳入后续无关真人输入；contact 没有触发消息位置，读取当前窗口历史。开始前对读取列表与窗口 revision 整体复核并冻结，开始后按冻结 ID/revision 逐条检查，不用新一轮最近历史替换旧依赖。普通文件内容不发送给模型；图片经任务授权接口读取。

result 的 status 仅允许 completed/failed/interrupted/uncertain，正文最多 256 KiB UTF-8，completed 不能为空白；usage 可含 `{inputTokens,outputTokens,cachedTokens}`。首次提交在事务中复核当前绑定、开始确认、来源、取消及有效期，原子保存消息、任务终态及事件。原结果 ID/正文可重交，不重写已有正文或复活已撤回消息。取消、源修订和撤权使旧运行失效，节点不得换 ID 绕过。

真人触发消息附 `aiResponses:[{aiId,taskId,status,error?}]`，按任务持久顺序排列，每位 AI 独立显示和停止，替代旧单一 aiResponse，表示排队、执行和失败，不把错误伪装成 AI 回答。AI 输出 status 为 generating/committed/interrupted/failed；generating 不计未读，终态取得新的 delivery position。客户端以服务器快照为准。

### 桌面与同机 Agent 私聊

首批同机通道用于已登录真人与其管理的本机 AI 的唯一私聊。首次准备必须联网；云端核对双方成员、真人管理权、AI 启用状态、有效节点绑定，以及屏蔽、暂停和未完成云端任务。准备成功后，同一桌面会话可在断云时使用缓存的授权和文字历史；离线节点不能即时获知云端撤权。第一批采用明文；第二批为启用窗口增加相同 sealedV1 封装与 Olm 密钥投递，已通过真实 Windows IPC 验证。第三批增加密文附件及已准备的群来源缓存；局域网连接尚未提供。

| 方法与路径（省略 `/api`） | 请求与返回 |
| --- | --- |
| POST `/chat/conversations/{id}/direct` | 真人会话请求 `{nodeId,bindingVersion}`，返回 `{token}`；token 仅由桌面原生层交给 Agent，不返回网页脚本 |
| POST `/node/direct/prepare` | 节点凭据加 `{grant}`；返回 `DirectBootstrap`：双方账号、真实私聊、bindingVersion、云端末条 `base:{messageId,revision,position}`、最近最多 64 条消息、相关群来源与 hasMore；附件正文另取 |
| POST `/node/direct/sync` | 节点凭据加 `DirectSyncRequest`；接纳已在本机完成或中断的一轮真人/AI 消息，返回该批当前权威消息和 head，不创建云端发言任务 |
| POST `/node/direct/{aiId}/reference` | `{grant,selection}`；复核真人与 AI 的当前共同群及来源修订，返回 `NodeReference` 供本机缓存 |
| POST `/node/direct/{aiId}/attachment` | `{grant,conversationId,messageId,index,revision}`；复核当前权限与修订，返回原附件字节，密文不在服务器解密 |

同步请求携带固定 requestId、grant、conversationRevision、base、实际使用的 sources/derivedSources/materialScopes、configurationRevision，以及本轮稳定 UUID 的消息、终态、用量与错误。云端复核当前授权、绑定、来源、材料版本及末条历史，在同一事务保存消息、回执和事件。相同请求按指纹幂等；确认丢失时重交原请求，不能重开模型，也不能用旧回执恢复已撤回正文。权限、来源或历史变化产生冲突，节点保留本机记录供用户确认。

同步回执的 `head` 是该回执最后一条消息的当前权威 `{messageId,revision,position}`，表示本批已接纳的边界。重放旧请求不能把其他设备后来发送、节点尚未读过的消息算成已同步；只有 prepare 的 `base` 取窗口真实末条。若回执丢失后云端已有新消息，原请求仍可幂等取得本批回执，但后续本机批次使用旧边界同步时应返回冲突，不能跳过新历史。

桌面原生命令包括 `local_chat_prepare`、`local_chat_view`、`local_chat_send`、`local_chat_stop` 和 `local_chat_sync`，返回 `DirectView`。发送携带下文的固定消息标识、密文、附件及引用；原生层从当前登录会话选择真人及受限节点目录，不接受网页指定路径或操作者。`DirectView` 区分每条消息的 status/local/synced，并提供 running、syncState（pending/synced/conflict）、error 和 hasMore；本机顺序不冒充云端 position。网页每 500ms 刷新当前同机窗口。

Agent 的 `run-managed` 在目录锁内创建受限 Unix socket 或拒绝远程客户端的 Windows 命名管道，`direct-endpoint.json` 中的认证 token 仅由原生层读取。IPC 提供 Status、Prepare、View、Send、PrepareReference、Attachment、Stop、Sync，所有窗口读写绑定授权真人；普通 `run` 不创建此通道。加密 Send 带稳定消息 ID、sealed、keyMessages、encryptedAttachments、replyTo、reference 和 expectedConversationRevision，沿用原请求重试。原生退出、401 或切服清除内存身份，在飞响应复核会话版本；桌面重启仍须重新在线登录。

`local_chat_prepare_reference` 和 `local_chat_attachment` 转发对应本机操作。`cloud_save_blob` 只把客户端解密后的字节保存到用户通过原生对话框选定的位置，取消和写入失败会返回界面；不把这些字节传回云端，也不启动旧 Runtime。

### WSS 与重连

`/node/ws` 使用同一 Bearer，单帧上限 1 MiB。客户端先发送 `{type:"hello",protocolVersion:4,nodeId,aiId,bindingVersion,capabilities?}`，服务端复核后返回 `{type:"ready",protocolVersion:4}`；旧版本明确拒绝。

| 方向 | type 与内容 |
| --- | --- |
| 服务端 → 节点 | `tasksChanged`；`cancel:{taskId}`；`deltaAccepted:{taskId,sequence}`；`error:{code,message}` |
| 节点 → 服务端 | `delta:{taskId,runId,messageId,confirmationId,sequence,text}` |
| 服务端 → 节点 | `consciousnessRequest:{relayId,requestId,operatorId,conversation,operation}` |
| 节点 → 服务端 | `consciousnessResponse:{relayId,requestId,view,error}` |
| 服务端 → 节点 | `managementRequest:{relayId,requestId,operatorId,operation}` |
| 节点 → 服务端 | `managementResponse:{relayId,requestId,view,error}` |

capabilities 缺省为空列表；自带节点声明 `["profileManagement"]`，外部 Agent 可以不实现在线档案管理并继续使用 v4 任务协议，这不是 v5 握手。平台按当前连接的实际能力派发请求；relayId 区分每次连接上的派发，requestId 用于节点写入去重。不支持的节点应明确返回 management_unsupported，不能模拟保存成功。节点选中控制帧后完整处理写入，再继续生成循环，不能把已消费命令的异步保存放入可取消分支。

delta 是完整累计正文检查点，最多 256 KiB，每 500ms 至多一次。服务端复核绑定、开始与来源状态，序号单调，同一序号同正文幂等，保存后通知聊天刷新并确认序号。最终 HTTP result 为权威结果；不传思考、内部 persona 或 Key。原生 ping/pong 用于存活检查。

每次连接/重连都重新读取权威任务列表，并按稳定 AI 账号的来源游标补齐意识材料变更；WSS 通知本身不作为完整来源记录。已开始且结果不明不自动重做；节点可重交本地持久保存的原结果。服务端重启把遗留 running 标 uncertain，保留可见片段而不重排。关闭聊天客户端不取消独立节点任务。

### 记忆、摘要与群话题接续

真人通过 POST `/chat/conversations/{cid}/ai/{aiId}/consciousness` 发送 `{requestId,operation}`。操作的 `type` 为 `memories`、`createMemory`、`editMemory`、`forgetMemory`、`summaries`、`configureSummaries` 或 `generateSummary`；具体字段以 `connector::node::ConsciousnessOperation` 为准。记忆写入带类别、正文、可空的 `{messageId,revision}` 来源，编辑和遗忘还带条目 id/revision。摘要设置带版本、enabled 和独立模型绑定；手动整理带可空 retryId。

记忆操作要求实际真人与目标 AI 都是当前窗口成员，摘要操作还要求真人为该 AI 管理员。返回 `ConsciousnessView`，记忆操作只提供 memories，摘要操作提供 summaries/settings/models。节点离线直接报错，超时或断开后显示结果未确认；写操作以原 requestId/operation 重试。每次转发另有新的 relayId，并绑定当前节点连接，迟到响应不能替代新的请求。云端只在内存中转发正文；节点保存完成并宣布材料版本后才能返回成功。

预览与消息发送使用 `reference:{conversationId,sources:[{messageId,revision}]}`，只允许选取双方共同群的 1–8 条 committed 原文。发送成功的消息返回 `contextReference:{conversationId,sources,revision,stale}`，无引用时为 null。云端保存来源标识，不复制正文。任务材料的 `reference` 提供这次获准读取的原文、源窗口和 required；必要引用必须完整带入，普通追问只考虑最近 12 条私聊里的最新有效引用。

| 节点方法与路径 | 内容 |
| --- | --- |
| GET `/node/consciousness/changes?after=…` | 按稳定 AI 账号读取 `{cursor,hasMore,changes}`，变化只含窗口、消息标识和原因 |
| POST `/node/consciousness/validate` | 检查来源 ID/revision 与窗口成员/版本，返回 `{valid}` |
| GET `/node/conversations/{id}/history?after=…&limit=100` | 递增 position 原文及 recentBoundary，供节点分段整理 |
| POST `/node/consciousness/state` | 宣布 `{conversationId,revision}`，revision 为节点上下文 UUID；变化使依赖旧版本的运行失效 |
| POST `/node/summaries/start` | 持久确认独立摘要调用，保存来源、配置版本及节点材料版本，返回 id/confirmationId |
| POST `/node/summaries/{id}/finish` | 提交 ready/failed/interrupted/stale、用量和错误码，不发送摘要正文 |

当前 v4 协议沿用 NodeMessage 的稳定 position、TaskMaterials 的可空 reference，以及 StartRequest 必需的 derivedSources 与 materialScopes。sources 保持近期原文快照；derivedSources 只列实际读到的记忆、摘要和引用的原文依赖，materialScopes 是实际窗口的节点版本。云端只接受本窗口或当前有效引用的来源群，开始、流式提交与最终结果都复核冻结来源和版本。来源游标用于同步，不因其他窗口发生变化而否定当前运行。

角色/模型参数的在线编辑与管理员名单使用上文云端管理接口；模型 Key 只在节点本地配置。详细行为见[意识系统](consciousness.md)。

## 数据迁移接口

`POST /api/migration/preview` 接收 `connector::migration::MigrationSnapshot`（version 1），使用当前真人会话，返回目标真人、将新建的 AI、提醒和 `previewDigest`。`POST /api/migration/import` 接收 `{snapshot, previewDigest}`，返回账号、窗口、消息及其 revision/position、附件索引映射。两者上限 128 MiB，沿用变更请求的来源与客户端标记检查。

唯一来源真人映射到登录账号；每个来源 AI 新建并授予该真人初始管理权，不合并同名账号。预览校验资料结构和来源是否已经导入；附件实际解码及尺寸、数量校验在导入时完成，失败不留下部分账号或聊天。相同 `(owner, sourceId)` 和内容重复提交返回原映射，内容变化返回 409；回执不保存原文或节点资料。

消息的 `importOrigin` 标明导入批次及旧消息版本。归档、旧历史导入窗口返回 `readOnly: true`；当前成员可以读取、搜索、导出和调整自己的列表状态，不能向其中发消息或修改聊天。已退出成员不重新加入。旧消息直接保存，不创建发言任务；所有迁入窗口先暂停，约定与自动摘要需另行确认。

AI 资料包由 `connector::node::transfer::AiTransfer` 定义，仅通过节点本机 CLI 导入，不通过云端上传。节点核对当前授权和来源，并通过 `SummaryItem.importOrigin` 返回摘要来源标记；模型 Key、节点凭据、执行确认及来源游标不在包内。命令与限制见[迁移指南](../migration.md)。

## 当前本地实现的适用范围

以下章节及源码链接描述已实现的单机 HTTP/IPC 契约：固定本地真人、共享 SocialFeature 与 SQLite。源码已划分为 client、server、agent、connector，并提取 Agent 纯核心；本地宿主没有云端认证，也不承载上述节点 WSS。以下 SocialFeature 路由只供本地实现维护和迁移参考，不是默认客户端的云端接口。

本文集中维护当前传输契约，后续能力单独列出，不预先固定尚未实现的路由。源码入口为 [HTTP](../../server/http/src/social_api.rs)、[IPC](../../client/desktop/src-tauri/src/social_commands.rs)、[共享 DTO](../../server/social/src/wire.rs) 与 [持久事件](../../connector/src/social_event.rs)。

### 共享业务入口

HTTP 与 Tauri 调用同一 SocialFeature 业务方法，传输层不实现身份、权限、记忆或调度规则。Feature trait 只负责生命周期与 downcast。显式记忆管理位于 SocialFeature 的 memory 模块；SocialFeature 读取授权材料并转换为 ContextInput，调用 `agent/core` 的 ConsciousnessService 纯函数式组装上下文。窗口摘要的授权、任务、来源校验和存储仍由 SocialFeature 的 summary 模块协调，摘要提示词与生成预算使用 Agent 核心；原消息由聊天用例维护。

外部业务字段为 camelCase，枚举值按各自 serde 契约；ID 为 UUID 字符串，revision、sequence、cursor 为十进制字符串。请求使用 requestId；expectedRevision 针对被修改对象，不能拿空间 revision 替代消息或成员 revision。

个人屏蔽命令使用 `requestId`、`blocked`、`expectedRevision`（无条目为 null），返回带 `listed`、`blocked`、`revision` 的 ContactEntry；操作者取宿主当前账号。Conversation 的 `directBlocked` 表达任一方屏蔽后的私聊状态，群聊始终为 false。变更沿用 `contact_changed`，相关运行、计划和约定同时发出各自变更事件。详见[身份与关系](accounts.md)。

### 当前接口

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

普通文件下载统一返回 application/octet-stream、Content-Disposition: attachment、UTF-8 文件名、no-store 与 nosniff；不内联页面内容。IPC `social_save_file` 参数与 social_save_image 相同，复用原生对话框后的权限复核与原子文件写入，返回 cancelled 或 saved/fileName；Web 返回 downloaded/fileName。文件输入为 name/mime/data，最多 4 个、单个 10 MiB、合计 20 MiB，允许零字节；名称和 MIME 规则见[聊天与消息](conversations.md)。

单窗口导出接受窗口 ID、`format`（markdown/json）、独立的 `includeImages` / `includeFiles`（HTTP 省略时均为 false，IPC 由客户端明确传入），不接受操作者、文件正文或目标路径。两个附件选项均关闭时 HTTP 返回 `{fileName,mediaType,messageCount,content}` JSON 文件内容包；任一开启时返回 ZIP 二进制，`Content-Disposition` 使用 UTF-8 filename*，`X-LLMN-Message-Count` 给出完整消息数。两者均设置 `Cache-Control: no-store`；ZIP 另设 `X-Content-Type-Options: nosniff`。Web 下载对应 Blob。IPC 的[保存适配](../../client/desktop/src-tauri/src/conversation_export.rs)打开原生“另存为”，取消返回 `{outcome:"cancelled"}`；成功写入返回 `{outcome:"saved",fileName,messageCount}`，文件名取实际保存目标。两个适配共用 SocialFeature 的读取和格式化，不产生持久事件。记录 JSON 的版本为无附件 v1、仅携图 v2、包含文件选项 v3，filePath 为 ZIP 中的实际相对路径；结构见[聊天与消息](conversations.md)。

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

共同 Account 字段为 id/kind/name/avatarRef/bio/status/revision，真人与 AI 相同。ManagedAi 包含 account、persona、binding、administrators(accountId/initial)、isInitialAdministrator，仅供有权限的管理接口。联系人操作返回 ContactEntry(ownerId/accountId/listed/blocked/revision)。

binding.temperature 与 maxTokens 可省略或为 null；前者使用模型服务默认值，后者使用意识系统自动输出预算，均不继承全局 GUI 参数。显式值按协议/思考模式与模型容量校验，不静默裁减。init 的 ModelOption 增加 generationLimits，以 default/off/low/medium/high/max 为键，各值为 temperatureMax（null 表示该模式不可调）、minOutputTokens、maxOutputTokens（null 表示未声明）；当前社交返回的上限还受上下文容量约束。该数据描述模型选项，不包含任何 AI 的内部参数。字段的共享 HTTP/IPC 验证见 [AI 执行](execution.md)。

AccountView 返回 operatorId 与 snapshot；snapshot.selfId/selfAccount 为被查看 AI，操作者仍为真实管理员。每次读取快照与分页历史复核管理授权，窗口按该 AI 的当前成员身份筛选；查看停用 AI 仍可行，退出群后不能读取该群。它是可手动刷新的只读快照，不使用实际操作者的全局事件订阅，不提供代发消息或远程登录令牌。

消息分页使用 before=sequence，每页最多 200 条，返回 messages/hasMore/readThrough。最新页的 readThrough 为同一读事务获得的可确认完成位置（十进制字符串）；向前分页和管理员只读视角为 null。空间 DTO 返回当前成员及暂停设置、pendingPlans、activeRun、currentBout 与 participation（policy/state/budgetDay/autoTurnsToday/reservedTokensToday）；运行详情返回独立 RunDetails，旧运行可为 null。

Conversation.listState 返回 readThrough/latestDelivery/unreadCount/markedUnread/pinned/revision；三个位置/版本字段为十进制字符串，unreadCount 为数字，个人状态变更不提升空间 revision。

正文搜索 HTTP query 为 query/conversationId/before，IPC 为同名 input 对象；后两项可省略/null。返回 messages/nextBefore；每页最多 50 条，nextBefore 是排除边界的完成位置，末页为 null。搜索范围按实际操作者当前成员关系决定。消息定位返回 messages/hasMore/hasNewer，包含目标及前后各最多 50 条；hasMore/hasNewer 表示两侧还有记录，无已读确认位置。

接话规则输入为 requestId/policy；policy 包含 revision（预期规则版本）、mergeDelayMs、idleSeconds、limitedAuto、maxConsecutive、maxAutoPerBout、maxAutoPerDay、dailyTokenBudget。默认规则 revision="1"；规则更新提升自己的版本，恢复旧回执返回当前窗口。范围由路径限定，操作者由后端固定。Plan DTO 包含 automatic/inputIds/notBefore/expiresAt，新增 contactIntentId（普通计划为 null）与可空 triggerId，两种触发恰有一个，计时为 UTC 字符串，自动额度失败用 participation_limit 表达，不影响真人消息保存。

Conversation.contactSchedule 返回 settings 与 intents（全部活动及最近 50 条记录）。ContactIntent 的 camelCase 字段为 id/conversationId/actorId/createdBy/authorizationId/text/dueAt/expiresAt/utcOffsetMinutes/status/planId/revision/reason/createdAt/updatedAt。创建/取消返回当前 ContactIntent，设置返回 ContactScheduleView。内容和原始授权不可就地修改，重试旧命令返回当前状态，不复活已取消意图。

模型服务与目录同步接口继续使用；设置快照由 `/api/settings` 和 `settings_snapshot` 提供。旧 `/api/sessions`、旧聊天 IPC 与 chat-event 已删除。联系人关系和 AI 账号管理分别使用上表的专用接口，历史记录通过显式导入进入新版。

模型图片能力覆盖使用设置接口 `POST /api/models/image-input` / IPC `set_model_image_input`，共用 `{provider,model,imageInput}`；imageInput 为 true/false，null 表示跟随目录。成功返回更新后的 ProviderInfo 数组，保存失败保留原配置。ModelInfo 与社交 ModelOption 都提供 `imageInput: {support,source}`，support 为 supported/unsupported/unknown，source 为 config/catalog/builtin/unknown。

普通社交 Init 的 `imageInputs` 仅将 AI 账号 ID 映射到三态能力，不包含模型绑定或内部设定；管理员只读账号视角没有输入区，返回空映射。能力设置不生成聊天事件；界面返回社交页时刷新快照。执行按实际冻结配置检查全部请求，明确不支持图片时保存 RunDetails.error.code 为 `image_input_unsupported`，不调用 provider，真人消息仍保存。详见[运行与配置](runtime.md)。

摘要分页返回 settings/segments/hasMore，每页 50 段、按 throughSequence 降序，before 排除该边界。SummarySettings 返回 id（窗口 ID）、enabled、binding、revision；未配置时 revision="0"、enabled=false。保存参数为 requestId/expectedRevision/enabled/binding，binding 包含 providerId/modelId/reasoningEffort/maxTokens，不包含密钥、温度或 AI 人格。

开始摘要参数为 requestId/retry，retry 为 null 表示下一段，或 {id, expectedRevision} 表示重建同一分段。没有较早原文时返回 null，不调用模型。摘要包含范围、text/status/revision、来源、模型绑定、attempt、错误和时间；sources 沿用 SourceRef 的 snake_case 字段（kind/message_id/conversation_id/revision），其余 DTO 为 camelCase。发起人由后端固定，范围由存储推导；调用方不能指定摘要正文、原文集合或任意操作者。配置/请求回执幂等，冲突返回共享错误。

### 发送与幂等

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

### 持久事件

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

回放至少一次，客户端按 cursor 去重，并在状态应用成功后推进游标。消息变更按 MessageId 读取，已加载的早期消息也能更新。持久事件由 SocialStore 提供，临时生成事件由 SocialFeature 发布；SocialEvent、SocialGenerationEvent、LiveEvent 和 SubscriptionEvent 的共享类型位于 `connector`，HTTP/IPC 消费同一订阅封装。连接器当前不实现节点授权或网络协议，两类事件不依赖旧运行时事件总线。

### 临时生成事件

SocialGenerationEvent 当前有 generation.delta、generation.reasoning_delta 和 execution.notice。正文/思考片段包含 runId、messageId、actorId、conversationId、deltaSeq、text；执行提示包含 conversationId/error，没有 run 的持久游标。

普通 token 不写入持久日志。重连读取已保存草稿/终态，最终记录覆盖临时缓冲，终态后忽略迟到 delta。临时广播溢出发 resync，客户端清除旧历史页与临时缓冲后重新分页，草稿保留；重置前的读取响应不能填回旧正文。

### 启动、订阅与补齐

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

持久日志当前不压缩，尚未实现游标过期、空间过滤 watermark 和统一的所有边界错误。HTTP/IPC 在重放、溢出和订阅释放时使用同一业务语义；宿主退出等待运行中断保存和数据库 writer 收尾，见 [运行时](runtime.md#生命周期与关闭)。

### 错误与后续能力

共享业务错误为 code/message/retryable。目前 invalid_input → 400，not_found → 404，not_authorized → 403，revision_conflict → 409，model_unavailable/budget_exhausted → 422，storage_failure/unavailable → 503。IPC 使用相同业务错误体。社交 HTTP 的 JSON/Path/Query 解析失败也返回该结构，code 为 invalid_input、retryable 为 false；保留传输状态码：非法 JSON 语法、路径和查询为 400，JSON 字段/类型错误为 422，缺少 JSON Content-Type 为 415，超过正文上限为 413。当前不返回 currentRevision。

消息已保存后的参与失败通过 executionNotice 或计划/运行状态表达，不能把保存回执改成失败。账号停用通过 account_disabled 提示，暂停、无效模型与订阅断线分别展示；磁盘不可写时不伪报已保存终态。

重复日程、自然语言约定和工具授权均为未来候选，见[候选清单](../project/backlog.md)。公开接口随具体用例扩展；实际操作者及目标范围由同一业务层核实。

群话题接续的规则见 [意识系统](consciousness.md#从群聊接续到私聊)：打开唯一私聊不发消息，引用只传来源标识并复核双方权限；独立转发仍为候选，未来共享选定内容也不能授予源空间读取权。

本地 HTTP 继续保留 loopback、Host/Origin 和客户端 Header 保护；云端真人认证见本文顶部实际接口，节点接入使用上文独立 v4 协议，不经过这里的本地接口。响应和事件不返回 Key、环境变量值或完整私人配置。目录打开 IPC 仍只接受 config/data 枚举，Web 无服务器目录打开接口。
