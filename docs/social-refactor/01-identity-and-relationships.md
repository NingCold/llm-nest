# 01 身份与关系

[总纲](README.md) · [聊天与消息](02-conversations-and-messages.md) · [运行时与配置](09-runtime-and-configuration.md)

状态：完整目标设计；M1 已实现联系人创建/修改、独立执行绑定、严格模型校验与群成员管理与共享群历史。归档/删除、参与设置等仍待后续，当前范围见 [私聊](m1-private-chat.md) 与 [群聊](m1-group-chat.md)。

## 1. 职责与落点

`crates/social/src/identity.rs` 定义身份、档案与关系；`features/social/src/identity.rs` 负责创建、修改、归档和查询用例。模型解析由 Runtime/AiClient 负责，档案不拼接供应商协议 JSON。意识系统读取档案与关系偏好，为目标 AI 准备上下文；档案编辑仍由身份用例负责。

首版有一个持久化本地 human Actor；所有 AI 都拥有独立 Actor。接口从后端当前用户上下文获取操作者，不接受前端通过任意 senderId 冒充另一个参与者。

## 2. 核心对象

| 对象 | 关键字段 | 约束 |
| --- | --- | --- |
| Actor | id、kind、display_name、avatar_ref、status、revision、created_at | id 为 UUID；kind 首版创建后不可修改 |
| AiProfile | actor_id、persona、background、interests、speaking_style、disclosure、profile_version | 一份档案属于一个 ai Actor；修改有版本 |
| ExecutionBinding | id、actor_id、provider_id、model_id、reasoning_effort、temperature、max_tokens、tool_policy_id、revision | 只存配置引用，不存 API Key 或 provider endpoint |
| Relationship | id、participant_a、participant_b、preferences、status、revision | 无向参与者对唯一；偏好显式记录归属与方向 |

Actor.status 初版为 active / archived。归档保留历史发送者，禁止新增运行，取消待执行意图；重新启用须经过显式操作。

AiProfile.persona 是对外角色设定，不能授予工具、绕过暂停或扩大记忆权限。disclosure 提供 AI 标记及运营说明；首版可说明角色在本地配置、调用用户选择的模型。“意识系统（Consciousness）”指软件管理的背景与记忆机制，按 [系统定义](03-consciousness.md#0-名称与定义) 说明，不声称角色具有主观体验或真人身份。

## 3. 身份连续性

- 修改名字和头像不改变 ActorId；旧消息保留发送时的展示快照，详情可访问当前档案。
- 更换模型更新 ExecutionBinding，关系、记忆、群成员与历史不迁移到新 Actor。
- 意识系统按 ActorId 与来源范围关联记忆和上下文，更换模型不新建一套意识数据；请求快照与 provider 续接链仍按各窗口构造。
- 复制角色创建新 ActorId，默认复制公开档案，不复制关系记忆、私聊记录或主动联系意图。
- 导入角色模板只导入明确的公开字段；记忆和工具权限必须另外设置。
- 档案或模型更新后的新运行使用新版本，在飞运行继续使用启动时快照。用户可显式取消旧运行。
- 身份稳定是数据保证，行为风格一致性需用不同模型实测；不能承诺模型更换完全无感。

## 4. 创建与修改流程

创建 AI：校验档案大小和文本字段 → 校验执行配置 → 原子保存 Actor、AiProfile、ExecutionBinding → 提交 actor.created 事件 → 返回保存后的档案。

空 providers 是有效启动状态。允许创建未配置模型的 AI 草稿，executionReady=false，界面提供配置入口；该 AI 不能启动生成。用户选择了错误模型时仍严格报错，不把错误降级成草稿或替换模型。

修改档案使用 expectedRevision。冲突返回最新版本与可重试错误；失败不更新前端的“已保存”状态。身份档案字段与模型配置可以分别保存，禁止为了修改名字重写整份 TOML。

变更执行配置先用完整配置快照校验 provider、model、reasoning effort 与参数；校验和保存失败保留原绑定。删除 provider 后，把受影响 AI 标为需要重新配置，并取消不能执行的待办意图，不静默切换模型。

## 5. 关系行为

首次创建真人与 AI 私聊时，在事务中取得或创建对应 Relationship 及唯一 direct Conversation。一个真人与同一个 AI 只有一个长期私聊窗口；再次打开、结束当前交谈或恢复归档均沿用原 ConversationId，不创建第二个窗口。该 AI 参加的每个群另有独立空间。

历史导入记录保留独立来源，不强行合并发送者身份；用户指定归属新联系人后，在其已有唯一私聊中接续，通过授权历史引用使用旧内容。历史来源不形成该联系人额外的长期私聊窗口。

关系保存用户明确表达的相处偏好，例如回复长度、称呼、是否允许主动联系。偏好有来源与修改时间，记忆记录通过 relationship_id 关联。首版不设定自动增长的“好感度”或“亲密值”。

Relationship.preferences 承载显式相处设置，MemoryRecord 承载带来源的开放偏好与共同事实。意识系统按当前设置解释材料；冲突时显式设置优先，记忆文本不能改写主动联系、参与或工具开关，也不复制一份关系设置作为权威数据。

真人与 AI 的私聊偏好可以影响该私聊中的表达；进入群聊后的行为由群成员设置和群约定共同决定。私聊中的偏好原文不会直接公开给群成员。

移除联系人默认执行归档，界面分别说明保留历史、删除记录和遗忘记忆的效果。删除历史不自动删除 AI 在其他群中的身份。关系删除与内容清除必须调用对应业务用例，不依赖级联删除误删群消息。

## 6. 未来多人扩展

ActorId 与未来账号 ID 分开；账号证明谁可控制一个 human Actor，成员关系决定在哪个空间可操作。首版的本地用户身份不构成远程认证方案。

“替某人发言的 AI 代理”留作后续类型扩展。独立 AI 与代理的授权、署名和责任不同，首版所有 AI 均以自身 ActorId 发言。

## 7. 验收

1. 创建三个使用同一模型的 AI，仍有三个独立身份与私聊。
2. 更换模型不改变历史发送者、关系和记忆归属。
3. 档案修改与模型配置修改均能拒绝过期 revision，保存失败无内存污染。
4. 未配置模型的 AI 可以展示，但不能启动运行；无效模型返回可用候选。
5. 归档取消后续参与和待联系意图，历史头像/名字仍能解释。
6. 复制 AI 不带入原有私聊记忆或权限。
7. 同一联系人重复开始私聊、归档后恢复和更换模型均返回同一个私聊空间；加入不同群不改变该唯一性。
