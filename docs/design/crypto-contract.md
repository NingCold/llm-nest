# 私聊加密的接入约定

[加密设计](encryption.md) · [执行计划](../project/secure-local-plan.md)

本文是第二批的实现契约。完成状态以进度与验证记录为准。

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
- SDK 加密事件类型固定为 `com.llmn.content.v1`。内部正文为 `{version:1, server, kind, conversationId, messageId, senderId, revision, text, replyTo, mentions, runId, sequence}`。`server` 是标准化 HTTPS origin；`kind` 为 `message`、`delta` 或 `resource`。未适用字段用 null，mentions 用数组。解密后与收到的外部标识核对，不能把别的窗口、修订或流片段搬过来。创建消息必须由发送方先生成 messageId；事件 ID 使用 `$<messageId>.<revision>.<sequence或final>`。窗口 revision、云端 position、时间等服务器分配值不伪装为发送端已签认的值。
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

## 私聊启用

`POST /chat/conversations/{id}/encryption` 携带 `{deviceId,requestId}`，不可逆启用现有私聊。响应和窗口 DTO 包含 `encryption:{version:1,activationPosition}`。先确保双方可信设备及恢复材料可用；服务端检查当前成员、设备与 AI 节点能力。已有原文明确标为尚未迁移，不悄悄改写。启用后的消息、修改、AI delta/result、同机补传与资料管理拒绝明文；本批不支持的附件、引用、约定明确不可发送。群和旧库切换属于后续批次。

每条新真人消息和每次 AI 新运行建立新的 Megolm 发送会话，防止管理员拿到的旧历史链继续解出新的发言。同一条回复的流片段及未知结果重试保持原会话与冻结密文。

同机发送的 `keyMessages` 和返回 DirectView 中的同名字段，携带官方 Olm to-device 请求 `{event_type,txn_id,messages}`。本机 Agent 从已授权的真人身份派生 sender，客户端从已绑定 AI 身份派生 sender，两端只读取自己的收件设备，并先由 SDK 处理密钥包，再解开正文。原请求持久保存，联网后沿用原 txn/request ID；本机密钥投递记录不依赖云端回执存活。断网前必须已核验设备并建立 Olm 会话，缺少这一准备时明确提示联网，不改走明文。

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
- 客户端持久保存已点击保存的原命令与密文，断线自动重交同一意图；冲突保留编辑。聊天未知发送仍按原规则手动重试。云端保存与节点已应用分别显示。

以上端点的大小、权限、来源和 CAS 在服务端事务边界校验；端点密文结构和内容格式在授权端校验。内部直接沿用已成立的类型和不变量，不堆叠备用路径或静默兜底。

## 正常交接与强制更换

管理员通过 `POST /ai/{id}/node/handover`（requestId、expectedBindingVersion）让旧节点停接新工作；取消使用 `/handover/cancel`。已开始的调用可以收尾，已有记录、资料和历史钥继续同步，不能重做未知调用。管理界面和节点 sync 分别返回 `handoverState` 与 `llmn_handover_state`。

节点确认没有待提交工作、当前资料已应用且历史钥备份完成后，调用 `POST /node/handover-ready`（deviceId、expectedBindingVersion、backupVersion）。云端再次检查其已知任务、资料和备份；后续又有资料或备份更新时，退回同步中。加密 AI 只有准备完成才能正常更换节点；旧节点损坏时，管理员可以明确选择 force，接受未同步内容缺失。新节点建立独立设备，恢复账号秘密与已同步历史，再应用当前资料和失效记录；旧设备与凭据不能继续提交。
