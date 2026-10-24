# 聊天与 AI 资料的加密约定

[加密设计](encryption.md) · [执行计划](../project/secure-local-plan.md)

本文记录私聊、群聊及 AI 资料使用的共同协议。完成状态以进度与验证记录为准。

## 选型

采用 Matrix 的 Olm/Megolm 协议和官方加密 SDK：浏览器使用 `@matrix-org/matrix-sdk-crypto-wasm 18.9.0`，Agent 使用 `matrix-sdk-crypto`、`matrix-sdk-sqlite 0.19.1`。使用稳定的 v1 算法。Rust 为管理员历史授权打开自定义 to-device API 功能，不启用实验加密算法。SDK 管理设备签名、Olm 会话、Megolm 发送链及入站历史密钥；应用负责现有账号权限、设备确认、可靠保存、投递和资料冲突。

选择它主要因为本项目需要多设备、长久历史与恢复。SimpleX 的连接和安全码设计值得参考，但其消息中继和设备存储模式与现有云端历史不同；MLS/OpenMLS 更适合以成员变更驱动的群组，但历史恢复与账号设备管理仍需另外建设。本次不移植 Matrix 社交服务器，也不自行设计棘轮协议。

依据：[Matrix 加密概念](https://matrix.org/docs/matrix-concepts/end-to-end-encryption/)、[Rust SDK](https://docs.rs/matrix-sdk-crypto/0.19.1/matrix_sdk_crypto/)、[官方 WASM SDK](https://matrix-org.github.io/matrix-sdk-crypto-wasm/)、[OpenMLS](https://github.com/openmls/openmls)。库的审计不等于本项目接入已获审计；[2026 年协议问题分析](https://matrix.org/blog/2026/02/analysis-of-reported-issues-in-vodozemac/)也说明身份核验和密钥分发不能省略。

## 身份与密文

- 内部 Matrix 用户标识为 `@<账号 UUID>:llmn`，聊天房间为 `!<窗口 UUID>:llmn`。这些是加密 SDK 的名字，不改变原账号或唯一私聊。
- 每个浏览器安装或 Agent 产生独立设备 UUID，不能复制活动 SDK 数据库来建立第二设备。节点设备同时受当前 AI 绑定版本约束。
- 公开设备钥和交叉签名钥采用 SDK 原始 JSON。首次创建身份前查询云端；已有身份时必须解锁原设备、由可信设备授权或恢复，不重新生成身份替换旧身份。
- 发送只向已验证设备分享会话；首次联系人或 AI 身份须核对指纹，变化须重新确认。登录和节点配对不自动完成加密信任。
- `sealed` 的结构为 `{version:1, roomId, sender, deviceId, eventId, content, deviceKeys, signatures}`；`content` 是 SDK 输出的 `m.room.encrypted` 内容，`deviceKeys` 为设备公开的 `{ed25519,curve25519}`，不含秘密。正文 `text` 在传输与云端存储中为空。
- 对完整封装去除 `signatures` 后使用 Matrix canonical JSON，再由 SDK `sign` 生成账号主密钥和设备密钥两份签名。接收端先取得已独立核验的账号主公钥，验证主签名，再用其中签认的设备 Ed25519 公钥验证设备签名；同时核对 Curve25519 公钥及设备 ID 与 SDK 密文一致。Web 验签使用 `@noble/curves`，Rust 使用 SDK 的 vodozemac；不自行实现签名算法。
- 这两份签名对新消息和恢复历史都必需。Matrix 标准历史备份导入后不保留原 Olm 投递的发送设备认证，因此 SDK 的 `CrossSigned` 解密设置会拒绝这种恢复记录。应用先完成上述强制双签核验，再由 SDK 检查密文并解密；不修改 SDK 的 imported 状态、不假造原投递证明，也没有缺签名时的降级路径。账户主密钥尚未核验或恢复秘密未获授权时不能发送或读回内容。
- SDK 加密事件类型固定为 `com.llmn.content.v1`。内部正文为 `{version:1, server, kind, conversationId, messageId, senderId, revision, text, replyTo, mentions, runId, sequence}`。`server` 是标准化 HTTPS origin；`kind` 为 `message`、`delta`、`resource` 或 `contactIntent`；第三批增加的附件、来源和窗口版本绑定见下文。未适用字段用 null，mentions 用数组。解密后与收到的外部标识核对，不能把别的窗口、修订或流片段搬过来。创建消息必须由发送方先生成 messageId；事件 ID 使用 `$<messageId>.<revision>.<sequence或final>`。发送前读到的窗口 revision 可作为签名内层的受众版本；云端 position 和服务器生成时间不伪装为发送端已签认的值。
- 所有发送先由 SDK 保存会话进展，再将完整请求和密文持久保存，最后联网。崩溃可能浪费一个未发送的会话序号，但不得发出未保存的密文；重试不重新加密。最终结果与累计流式片段均受保护。

## 密钥服务

所有路径均位于现有 `/api` 下，真人用原会话，节点用原 Bearer；节点请求只能操作当前绑定 AI。服务器保存公钥、密文与投递元数据，不实现内容解密。

| 端点 | 约定 |
| --- | --- |
| `POST /crypto/devices` | `{deviceId,name}`，注册当前主体设备；同 ID 不允许换主体/绑定 |
| `GET /crypto/devices?accountId=...` | 返回设备 ID、名字、当前绑定及 revoked 状态；公钥由 keysQuery 返回 |
| `POST /crypto/devices/{id}/revoke` | 撤销自己的设备；AI 由管理员或当前节点管理，接收端重新查询并更新发送会话；不能收回旧明文 |
| `POST /crypto/requests` | `{deviceId,requestId,kind,body}`；kind 为 keysUpload、keysQuery、keysClaim、toDevice、signatures、signingKeys、keysBackup；返回 Matrix 标准 JSON 响应 |
| `GET /crypto/sync?deviceId=...&after=...` | 标准 Matrix 形状：next_batch、to_device.events、device_lists.changed/left、device_one_time_keys_count、device_unused_fallback_key_types；客户端成功处理后持久推进游标 |
| `GET /node/crypto/conversations` | 当前绑定节点读取其 AI 当前加入的加密窗口元信息，返回 `{conversations:[NodeConversation]}`，不含原文；先发现窗口并应用资料，再读取受资料就绪检查保护的任务材料 |
| `GET/PUT /crypto/backup` | 账号的 `{version,algorithm,auth_data}`；PUT 另带 deviceId、expectedVersion（首次 null），按版本比较保存；SDK 签名、公钥与本机恢复秘密核对，不能盲信服务端公钥 |
| `GET /crypto/backup/keys` | 当前版本的标准 `{rooms:{roomId:{sessions:...}}}`；keysBackup 上传同一形状，携带 version |
| `GET/PUT /crypto/recovery` | `{version, ciphertext}`；PUT 另带 deviceId、expectedVersion，原版本与相同密文重试幂等，旧恢复版本不覆盖新版本。ciphertext 为 base64 的 Matrix 附件加密字节；恢复秘密绝不在这个对象内 |

keysUpload、keysQuery、keysClaim、signatures、signingKeys 的 body 为标准 Matrix 请求。toDevice 统一为 `{event_type,txn_id,messages}`，Web 适配器从 SDK 请求属性补齐前两项。管理员历史授权另带 `llmn_purpose:"aiAdminHistory"`；它不进入 Matrix 事件，服务器在提交和同 requestId 重放时都检查当前 AI 节点及当前管理员身份，不能只凭双方仍有私聊关系放行。已经合法投递的历史材料无法保证追回。其他投递只接受当前有效设备与获授权的相关账号，密文去重依赖原 requestId；签名上传不能删除既有签名。未知请求类型明确失败。SDK 不需要的 Matrix API 不提供空成功。

恢复采用 SDK 的标准历史密钥备份和 SecretsBundle。建立 backup 私钥后，SecretsBundle 含交叉签名私钥及 backup 私钥；用 SDK 的 Matrix 附件加密封装一次，将密文字节存 recovery，将完整解密信息保存为用户下载的恢复文件。恢复文件还记录服务器、账号、恢复版本；文件含秘密，不能当普通聊天导出。每次历史更新上传 SDK 加密的历史钥，不改恢复文件。轮换恢复材料时生成新版本并要求保存新文件。新设备恢复后产生新设备身份、导入 SecretsBundle 和入站历史钥，不复制旧 Olm 活动会话。

可信设备授权也可通过经指纹确认的 Olm to-device 密文传送 SecretsBundle/历史钥；禁止明文的秘密请求。AI 首次节点身份及管理员设备相互核对后才能分享恢复资料。恢复设备查询历史发送者公钥后，由恢复的 user-signing 签名链重新认可原有联系人；查询不等于信任，陌生或替换的主密钥仍须重新核验。恢复或解锁失败必须显示原因，不尝试登录密码或明文回退。Web 使用独立本机解锁口令保护 SDK IndexedDB；节点私有目录保管 SDK 数据库和本机解锁材料。

## 聊天启用

`POST /chat/conversations/{id}/encryption` 携带 `{deviceId,requestId,expectedConversationRevision}`，不可逆启用现有窗口。私聊由成员开启，群聊由群主或群管理员开启。响应和窗口 DTO 包含 `encryption:{version:1,activationPosition}`。发送端先确保当前每个成员至少有一台已核验的设备，并准备恢复材料；缺少时指出需要完成核验的成员，不静默跳过。服务端检查当前成员、窗口版本、有效设备与 AI 节点能力。已有原文明确标为尚未迁移，不悄悄改写。启用后的消息、修改、AI delta/result、约定、同机补传与资料管理拒绝明文。

新密文绑定发送时的 `conversationRevision`，服务器保存对应的 `audienceRevision`。接收端核对两者；读取旧消息时不拿现在的群版本替换发送时的版本。加密发送和编辑携带 `expectedConversationRevision`，成员名单改变后，按旧名单准备的请求不能作为新消息提交。已完成命令仍返回当前权威记录，不重复创建消息。

每条新真人消息和每次 AI 新运行建立新的 Megolm 发送会话，防止管理员拿到的旧历史链继续解出新的发言。同一条回复的流片段及未知结果重试保持原会话与冻结密文。

同机发送的 `keyMessages` 和返回 DirectView 中的同名字段，携带官方 Olm to-device 请求 `{event_type,txn_id,messages}`。本机 Agent 从已授权的真人身份派生 sender，客户端从已绑定 AI 身份派生 sender，两端只读取自己的收件设备，并先由 SDK 处理密钥包，再解开正文。原请求持久保存，联网后沿用原 txn/request ID；本机密钥投递记录不依赖云端回执存活。断网前必须已核验设备并建立 Olm 会话，缺少这一准备时明确提示联网，不改走明文。

## 群历史、附件与接续

群成员仍共享完整群历史。已持有历史密钥的授权端，经 Olm 向当前群成员的可信设备分享该群的历史钥。内部事件为 `com.llmn.group-history.v1`，内容是 `{conversationId,revision,keys}`；外层始终是 `m.room.encrypted`。投递请求另带 `llmn_purpose:"conversationHistory"`、`llmn_conversation_id`、`llmn_expected_revision`，服务器在首次投递和重放时均核对双方当前成员身份与群版本。接收端只接受经过 SDK 验证的 Olm 事件，并在导入前检查当前权限、版本及每份密钥所属的房间。不能把 AI 的恢复秘密、其他窗口或角色资料密钥混入群历史。

加入群不意味着立即拿到历史钥。尚无可用分享端时显示历史待恢复；已持有记录的端可以分享，节点按当前设备与历史变化去重分享。移除成员或撤销设备后，未来发言使用新会话。已经合法取得的历史钥和明文无法追回。节点按 AI 和窗口分别保存加密状态，同节点多个 AI 在同一群中也使用各自身份。

图片和文件先在端点使用 SDK Attachment 加密。请求的 `encryptedAttachments:[{index,data}]` 只含密文字节；云端附件元数据只有索引、密文长度、`kind:"encrypted"` 和 `application/octet-stream`。原文件名、类型、长度和 SDK `mediaEncryptionInfo` 放在消息的签名内层 `attachments` 中。下载端先验证消息身份，再验证附件哈希、解密并检查原有格式与大小限制；普通文件不自动作为模型输入。修改文字保留原附件；旧明文附件的整体转换留到第四批。

群话题接续继续使用原来的消息 ID 和修订，签名内层 `reference` 与外层来源选择对应。来源预览、模型材料和导出均在授权端解密；私聊没有启用加密时，也必须先解开引用的加密群内容才能交给模型。来源修改、撤回或失去共同群权限仍使引用和派生材料失效，不因持有旧钥而绕过当前权限。

同机聊天缓存已准备的来源及附件。私有 IPC 的 `PrepareReference` 在联网时核对来源并准备缓存，`Attachment` 读取指定消息和修订的本机字节；界面列表只返回附件元数据。离线发送沿用同一份密文、附件和来源选择。补传重新检查绑定、双方当前共同群权限、来源版本及窗口状态，只保存原记录，不排新的模型任务。

## 约定、搜索与导出

约定联系使用客户端生成的 `intentId`，外层保留执行所需的目标、时间、状态和版本，正文为空。签名内层 `kind:"contactIntent"` 绑定窗口、创建者、目标 AI、约定 ID、时间和正文；内容版本固定为 `1`，不随排队、执行等状态修订改变。时间按 UTC 瞬间核对。节点解密后沿用既有免打扰、额度和开始确认，不伪造真人消息。旧终态约定可以查看并标为尚未迁移的明文记录，不能作为新的加密任务执行。

加密聊天的正文搜索在客户端分页读取有权访问的历史并解密后执行，关键词不发给服务端。导出同样读取完整历史，按选择解密图片与文件。解密失败或仍有本机未同步记录时，界面说明原因，不把缺失内容的结果称作完整导出。聊天导出不包含恢复秘密、SDK 密钥库或附件解密密钥。

## AI 资料

资料范围 `{aiId,conversationId,kind}`：profile 的 conversationId 为 null；memory、summary、summarySettings 归具体窗口。profile 为 AI 与其可信管理员；memory 为 AI 与当前窗口的有权成员；summary 和 summarySettings 为 AI 与同时具有窗口访问权的管理员。不同范围使用不同 SDK room，不能把整个 AI 的恢复秘密交给普通群成员。

- 资源 ID 由客户端产生 UUID。profile 与 summarySettings 在同一范围内只允许一份，先列举已有条目再保存；并发新建返回冲突。scope 编码为 `aiId.kind.conversationId或global`，room 为 `!<scope>:llmn`；scope 内不使用冒号，以免被解释为 Matrix 服务器端口。
- `GET /crypto/resources?aiId=...&conversationId=...&kind=...` 返回 `{items:[...]}` 当前可读条目。
- `GET /crypto/resource-participants?aiId=...&conversationId=...&kind=...` 返回 `{accounts:[账号ID],revision}`，按同一资料范围计算有权接收者；公开名单不等于设备已受信任，端点仍需核验密钥。资料要求 AI 与本次作者有可信设备，其他管理员只向其已核验设备分享；尚未设置设备的管理员不阻塞保存。私聊要求双方均有可信设备。
- `PUT /crypto/resources/{id}`：`{requestId,deviceId,aiId,conversationId,kind,expectedRevision,expectedAudienceRevision,sealed,sources,tombstone}`。expectedRevision 首次为 `"0"`，expectedAudienceRevision 来自参与名单；sources 沿用消息 ID/版本。条目内部也绑定 scope/id/expectedRevision/expectedAudienceRevision。服务端在保存事务中核对资料版本和权限名单版本，拒绝按旧名单准备的迟到写入。`tombstone` 为 null、forgotten 或 stale；遗忘或失效清除 sealed，保留墓碑，不能由旧版本恢复。
- 接收账号或可信设备集合变化时，通过 SDK 轮换发送会话；仅停止投递新钥无法阻止已持有旧 Megolm 链的设备读取后续内容。端点断网期间仍只能使用最后获知的权限状态。
- 返回 `{id,aiId,conversationId,kind,revision,sealed,sources,tombstone,applied}`；applied 为 null 或 `{bindingVersion,revision,status}`，status 为 applied 或 rejected，具体错误正文也须加密。
- `POST /crypto/resources/{id}/applied`：当前绑定节点提交 `{deviceId,revision,status}`，旧绑定/旧版本不能确认当前版本。
- 窗口相关资源更新提升材料版本并使关联在飞工作失效。配置新版本已知但无法应用时暂停新运行。节点恢复时先处理来源失效、遗忘及当前资料，再确认可运行。
- AI 退出群后，该 AI 在群内的记忆、摘要与摘要设置清除密文并留下失效版本；其他 AI 的资料不受影响。重新入群不会恢复这些旧资料。重新启用摘要须由有权管理员按当前版本明确保存设置；旧记忆、旧摘要不能用原 ID 恢复。
- 客户端持久保存已点击保存的原命令与密文，断线自动重交同一意图；冲突保留编辑。聊天未知发送仍按原规则手动重试。云端保存与节点已应用分别显示。

以上端点的大小、权限、来源和 CAS 在服务端事务边界校验；端点密文结构和内容格式在授权端校验。内部直接沿用已成立的类型和不变量，不堆叠备用路径或静默兜底。

## 存量转换与正式切换

第四批使用专门的历史转换入口，不能调用普通消息编辑接口。转换保留原消息 ID、修订、顺序、时间、作者、回复和来源关系，以及各人的已读状态。已有密文不重新加密；撤回记录保持撤回。只读归档仍只读，不借迁移重新启用模型或约定。

旧消息的加密签名属于实际迁移者。消息和约定另带 `historyMigration:{id,actorId,sourceDigest}`；签名内层用 `kind:"historyMessage"` 或 `"historyContactIntent"`，`senderId` 仍为历史作者，`historyMigration` 与外层一致。封装的 `sender` 为迁移者，消息事件 ID 为 `$<原ID>.<原修订>.migration.<迁移ID>`。约定正文的内层修订和事件 ID 修订固定为 `1`，提交时仍核对原约定的状态修订。客户端与节点验证迁移者双签并显示历史迁移标记，不能声称原作者曾对旧消息做过端到端签认。

云端入口为 `/crypto/history-migrations`：`POST /preview` 读取范围与摘要，`POST /` 在备份、恢复材料确认和预览匹配后冻结窗口；`GET /{id}` 查看状态，`GET /{id}/items` 分页读取待转换原文，`PUT /{id}/items/{kind}/{itemId}` 提交已持久保存的原命令与密文。清单保留原 audienceRevision 供验读重建旧记录。每项校验来源摘要及修订，云端只替换存储形式，不使原引用和记忆失效。未知结果保持原 requestId 和密文重试。旧设备撤销后，可显式读取 `GET /crypto/history-migration-receipts/{requestId}`，以当前真人会话和当前窗口权限核对自己的指纹回执；与已保存 operation/body 的 SHA-256 一致才解除待确认，无回执仍是未知，不重加密或声称未提交。

迁移要求当前真人私聊成员或当前群主/管理员，归档仅授予转换权限。开始前列明并处理未完成任务、约定和本机待同步记录；迁移时冻结内容写入与调度，权限撤销仍即时生效。成员或来源版本改变时暂停并重新核对，不扩大旧授权。

附件先写新的密文存储代次，再在事务中切换读取位置；旧明文文件作为受限备份保留，旧附件元数据留在 cloud_history_attachment_backups，记录位置与清理计划，实际删除另行确认。服务端数据库与附件、节点工作库也须有一致备份；普通聊天导出不能替代这些备份。

`POST /{id}/finish` 只在全部项目转换、当前权限仍有效且完成独立恢复验读后关闭转换入口，并将 `encryption.historyComplete` 设为 true、激活位置设为 0。独立验读使用全新临时 SDK 实例和恢复材料，不借用当前活动设备已经持有的密钥；验读清单覆盖消息、约定及附件，并重建转换前记录与附件摘要，比对 sourceDigest；有效签名但内容不同也不能完成。恢复秘密始终留在授权端，云端只接受验读确认摘要。

节点声明 `historyMigrationV1` 能力后才能处理迁移历史。旧节点资料提供单独的预览、原意图持久化和 CAS 发布入口；保留来源与遗忘墓碑，不调用模型补造旧摘要。角色、记忆、摘要和设置均同步完成后，才算资料可交接。未确认的本机发送或执行必须先按原规则解决，不能用迁移重新加密未知请求或重做模型调用。

服务端切换为强制加密后，新消息、约定、同机补传、管理资料及旧格式导入均不能走明文路径；新窗口需先完成设备授权与加密准备。切换前先核对存量、节点能力和恢复结果，切换不自动恢复暂停、旧队列或过期约定。

## 正常交接与强制更换

管理员通过 `POST /ai/{id}/node/handover`（requestId、expectedBindingVersion）让旧节点停接新工作；取消使用 `/handover/cancel`。已开始的调用可以收尾，已有记录、资料和历史钥继续同步，不能重做未知调用。管理界面和节点 sync 分别返回 `handoverState` 与 `llmn_handover_state`。

节点确认没有待提交工作、当前资料已应用且历史钥备份完成后，调用 `POST /node/handover-ready`（deviceId、expectedBindingVersion、backupVersion）。云端再次检查其已知任务、资料和备份；后续又有资料或备份更新时，退回同步中。加密 AI 只有准备完成才能正常更换节点；旧节点损坏时，管理员可以明确选择 force，接受未同步内容缺失。新节点建立独立设备，恢复账号秘密与已同步历史，再应用当前资料和失效记录；旧设备与凭据不能继续提交。
