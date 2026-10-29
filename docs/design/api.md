# API 与事件

[总纲](overview.md) · [存储与迁移](storage.md) · [界面与桌面](frontend.md)

## 已实现的云端接口

聊天的 `sealedV1`、Matrix 公钥与设备信箱、恢复密文和 AI 资料 CAS 接口集中见[加密契约](crypto-contract.md)。启用后新消息必须携带密文封装，正文为空；流式、结果与同机补传不允许明文回退。旧未启用窗口沿用普通接口。群、附件、跨窗引用和约定复用同一加密格式；完整搜索和导出在授权客户端解密完成。旧历史由专用迁移入口分项转换，协议与冻结、恢复验读规则见加密契约；正式切换使用 `CLOUD_REQUIRE_ENCRYPTION` 拒绝明文写入。

默认 CloudApp 使用 `cloud-server` 的 `/api`，与保留的本地 `/api/social` 是两套不同契约。所有聊天操作者取登录会话，不接受客户端 senderId/operatorId。浏览器使用同源 HttpOnly Cookie；桌面经原生网络层保管 Cookie，默认只在内存，显式保持登录后可在 Windows 使用当前用户 DPAPI 持久保存，不交给网页脚本。修改请求带 `X-LLMN-Client: 1`，服务器检查来源与跨站请求。JSON 业务错误为 `{code,message}`，常用状态为 400、401、403、404、409、500；入口解析错误沿用 HTTP 状态。

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

`POST /auth/login` 和 `POST /auth/verify` 的 JSON 均接受可选布尔 `rememberLogin`，缺省为 false。false 时服务端会话绝对有效 8 小时，Web 不设置持久 Cookie 的 Max-Age，桌面仅保管进程内会话；true 时服务端会话及持久 Cookie 绝对有效 30 天，普通请求和 `/me` 不滑动续期。Windows 原生仅在本次请求明确选择 true 且服务端返回有效持久会话时，才用当前用户 DPAPI 保存令牌；不能从旧服务的持久 Cookie 反推用户同意。非 Windows 原生端暂不支持保持登录。重启先联网验证保存的会话，未登录与过期不进入旧账号聊天。

原生自动解锁另提供以下 IPC，不属于云端 HTTP 接口，且不由保持登录自动开启。`cloud_unlock_support` 返回当前平台是否支持；其他三个命令均复核当前已在线确认的真人身份：

| 命令 | 页面参数与结果 |
| --- | --- |
| `cloud_unlock_save` | `{deviceId,secret,expectedServer,expectedAccountId}` → `{expiresAt}` |
| `cloud_unlock_load` | `{deviceId,expectedServer,expectedAccountId}` → `{secret,expiresAt}` 或 null |
| `cloud_unlock_remove` | `{deviceId,expectedServer,expectedAccountId}` → 成功无正文 |

`deviceId` 必须是有效非空 UUID；`expectedServer` 和 `expectedAccountId` 只核对调用发出时的当前身份，不选择服务器、账号或存储目录，也不授予访问另一个账号的权限。不匹配时明确失败，不能把迟到请求保存到新会话下。口令仅通过专用解锁 IPC 交给本机客户端打开 SDK，不加入普通配置、云端 DTO 或日志。自动材料的可信设备、恢复确认及到期规则见[加密契约](crypto-contract.md#本机解锁与自动材料)。

原生清理失效会话时发送 `cloud-auth-lost` 事件，载荷为 `{server,accountId,message}`，其中 accountId 与 message 可为 null，分别标识失效范围及清理错误；不包含 Cookie、token、口令或其他秘密。页面先核对当前服务器及账号，才关闭对应 SDK、结束聊天会话并显示清理错误，旧服务器或旧账号事件不能结束新会话。

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

授权准备及新批次同步完成当前权限核对后，若窗口仅有 queued/running 云端任务，返回 409 `conversation_busy`，表示暂时等待，不记为永久 `direct_conflict`。节点保留原请求及本机记录，继续正常轮询；云端任务结束后重新核对并刷新历史或同步原批次，不重调模型。暂停或 uncertain 仍为冲突，权限、绑定、来源、材料版本及历史变化继续按原规则拒绝；普通 409、未知响应和网络失败不能当作明确暂忙。已有同步回执的幂等重放沿用原权威边界，不借暂忙处理跳过新历史或清除真实冲突。

同步回执的 `head` 是该回执最后一条消息的当前权威 `{messageId,revision,position}`，表示本批已接纳的边界。重放旧请求不能把其他设备后来发送、节点尚未读过的消息算成已同步；只有 prepare 的 `base` 取窗口真实末条。若回执丢失后云端已有新消息，原请求仍可幂等取得本批回执，但后续本机批次使用旧边界同步时应返回冲突，不能跳过新历史。

桌面原生命令包括 `local_chat_prepare`、`local_chat_view`、`local_chat_send`、`local_chat_stop` 和 `local_chat_sync`，返回 `DirectView`。发送携带下文的固定消息标识、密文、附件及引用；原生层从当前登录会话选择真人及受限节点目录，不接受网页指定路径或操作者。`DirectView` 区分每条消息的 status/local/synced，并提供 running、syncState（pending/synced/conflict）、error 和 hasMore；本机顺序不冒充云端 position。网页每 500ms 刷新当前同机窗口。

Agent 的 `run-managed` 在目录锁内创建受限 Unix socket 或拒绝远程客户端的 Windows 命名管道，`direct-endpoint.json` 中的认证 token 仅由原生层读取。IPC 提供 Status、Prepare、View、Send、PrepareReference、Attachment、Stop、Sync，所有窗口读写绑定授权真人；普通 `run` 不创建此通道。加密 Send 带稳定消息 ID、sealed、keyMessages、encryptedAttachments、replyTo、reference 和 expectedConversationRevision，沿用原请求重试。原生退出、401 或切服清除内存身份，在飞响应复核会话版本；Windows 保持登录后重启仍须联网核对保存的会话，不提供离线重新登录。

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

## 发送与幂等

变更命令携带 requestId；结果未知时沿用同一请求和内容手动重试，不生成第二次发送。服务端先复核当前身份与权限，再核对命令指纹和回执，返回当前权威对象；修订、撤回或撤权不能被旧回执复活。消息、个人读态、回执和持久事件按所属用例在同一 PostgreSQL 事务保存。回执不保存正文或附件原字节。

SSE 按实际登录账号从持久事件补齐，客户端重新读取并成功应用权威状态后才推进游标。页面重载和订阅断开只释放显示订阅，不取消云端任务或独立节点运行。

## 桌面草稿关闭协调

草稿位于客户端 IndexedDB，关闭确认不传草稿正文或文件路径。仅主 WebView 可调用以下命令：

| 命令/事件 | 契约 |
| --- | --- |
| desktop_register_draft_guard | 输入 page UUID，替代旧页面并释放旧等待 |
| desktop_unregister_draft_guard | 输入 page UUID，只释放匹配页面 |
| drafts-prepare-close | 原生层发出本次关闭 request UUID；页面停止编辑，完成附件读取与草稿事务 |
| desktop_finish_draft_close | 输入 page、request UUID 与 error；只接受匹配当前页面和关闭请求的回报 |
| desktop-close-retained | 取消关闭或转入后台后解除页面关闭锁 |
| backend-shutdown-failed | 保存、状态检查或停止失败；保留窗口并允许重试 |

原生层最多等待草稿确认 10 秒，失败保持窗口可用。确认成功后检查独立本机 Agent，按后台、取消或停止选择处理；显式退出停止本机工作后退出。桌面不再初始化或关闭旧宿主 Runtime。
