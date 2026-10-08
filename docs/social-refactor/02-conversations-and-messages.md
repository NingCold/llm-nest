# 02 聊天与消息

[总纲](README.md) · [身份与关系](01-identity-and-relationships.md) · [API 与事件](07-api-and-events.md)

状态：目标设计，尚未实现。

## 1. 职责

空间、成员和消息是持久化的社交记录。`crates/social` 定义类型与约束，`features/social/conversations.rs` 组织用例。工具调用、模型思考和运行检查点不作为普通群成员消息插入聊天记录。

## 2. 数据对象

| 对象 | 关键字段 |
| --- | --- |
| Conversation | id、kind(direct/group)、title、status、participation_settings、revision、context_epoch、participation_epoch、created_at |
| Membership | conversation_id、actor_id、role、status、history_from_seq、participation_mode、revision |
| SocialMessage | id、conversation_id、sender_id、sender_snapshot、message_seq、kind、body、reply_to、mentions、bout_id、run_id、status、revision、created_at |
| MessageRevision | message_id、revision、operation、author_id、content、changed_at |
| ReadPosition | conversation_id、reader_id、last_read_seq、updated_at |
| Bout | id、conversation_id、status、started_at、ended_at、end_reason、last_human_seq |

所有 ID 对外为字符串 UUID。message_seq 由数据库在每个空间内单调分配；revision 和序列以十进制字符串传给 GUI，避免 JavaScript 大整数精度问题。排序依据 sequence，不依赖时间戳相同或本机时钟回退。

## 3. 空间与成员

direct 默认包含本地真人和一个 AI，关联 Relationship；group 包含本地真人和一个以上 AI。空间不存唯一 ModelSelection，每个 AI 使用自己的绑定。

成员状态为 active / left / removed。首版本地真人担任 owner，AI 是 member；领域保留 human 成员的表示，但多人登录与远程权限尚未实现。

AI 加入群时，用户明确选择历史范围：默认从加入后的消息开始；可以授权当前话题或此前群历史。history_from_seq 定义连续历史窗口，其他共享片段使用显式上下文授权记录。私聊历史不因入群自动共享。

离开或移除成员原子增加 participation_epoch，撤销其运行和待参与计划；context_epoch 同步更新。历史消息保留 sender_id 和展示快照，已离开的成员不能继续读取新消息。

Conversation.status 为 active / archived。pause 属于参与设置，不禁止真人发送；归档默认只读。删除空间是单独清除操作，须列清对历史、摘要、记忆来源和联系意图的影响。

## 4. 消息与运行

SocialMessage.kind 表达 text / attachment / notice。M1 新消息发送先支持 text，notice 由后端生成，不能通过客户端伪造；历史导入保留附件。完整附件输入在上传、导出和权限链路实现并验收后启用，复用可持久化的二进制内容语义。

消息状态为 generating / complete / cancelled / failed / interrupted / redacted。真人消息成功提交即为 complete。后端选中 AI 并启动 AiRun 时才创建 generating 消息；用户发送消息不预建 AI 回答。

AiRun 与 MessageId 分开，首版一个运行对应一个可见 AI 消息；工具循环内部可以有多个 provider 消息。停止和重启恢复保留已保存草稿及中断标记，中断片段默认不进入下一次模型上下文。

reply_to 引用同空间消息，mentions 由结构化 ActorId 表达。消息文字中出现某个名字不会自动授予历史访问权，也不会绕过暂停规则。

## 5. 发送与连续输入

发送命令包含 clientRequestId、内容、附件引用、replyTo 和 mentions。后端确定 sender_id，校验空间与目标成员，原子分配 message_seq、保存消息和变更事件，然后返回真实 MessageId。

clientRequestId 用于幂等重试；同一作用域相同请求返回同一结果，内容不同返回冲突。前端临时 ID 只标识尚未确认的发送记录，不能冒充持久消息 UUID。

空间有 AI 正在生成时，真人仍可发送。新消息进入参与队列；首版运行继续使用已经固定的上下文，并明确回复对象。针对停止、撤销权限或特定 run 的取消立即生效。未开始的回复计划可合并新输入。

## 6. 编辑、撤回与再次回应

编辑操作按 MessageId + expectedRevision 定位。保存 MessageRevision，更新当前内容，保留空间后续消息，并增加 context_epoch。引用该旧版本的候选记忆和摘要标为待更新。

只有当前操作者控制的消息可以编辑；AI 已提交消息的纠正可通过追加回应表达。首版“请重新回答”发送新的请求或明确的回应意图，不删除其他成员的历史。

撤回产生 redacted 占位，保留 MessageId、顺序和引用关系；正文、附件和派生内容从正常读取和检索中排除。旧修订内容不能继续经常规接口读出。事件保存变更引用，不保存撤回后仍能回放的原正文。

删除与撤回不承诺撤销已经发送到外部模型的内容。备份清理与磁盘残留的产品语义见存储设计。

## 7. 空间、交谈与生成的生命周期

Conversation 长期存在；Bout 表达某次交谈；AiRun 表达一次执行。Bout.status 初版为 active / waiting / ended。

- 第一条真人消息可以创建新 Bout。
- 没有待发言计划时，Bout 转 waiting。
- 显式结束、空闲达到规则或预算耗尽可以转 ended；空闲等待本身不等于结束。
- ended 空间中的新真人消息创建新 Bout，先前上下文按权限与相关性继续可用。
- 定时联系只有通过参与规则后才能开始新 Bout；暂停或拒绝主动联系时不能重开。

“本次生成完成”“这个 AI 暂停参与”“当前交谈结束”使用不同命令和界面状态。

## 8. 验收

1. 群里三个 AI 的消息可以按发送者、引用和提及正确区分。
2. AI 生成期间连续发送两条真人消息，两条均落盘且有稳定顺序。
3. 编辑早期消息不会删除其他成员后续记录，旧版本派生记忆失效。
4. 同一发送命令重复提交只产生一条消息；改内容重用请求 ID 被拒绝。
5. 新入群 AI 默认读不到加入前历史和私聊内容。
6. 结束 Bout 不删除空间；下一次聊天可以新建 Bout 并接续允许的历史。
