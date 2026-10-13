# 02 聊天与消息

[总纲](README.md) · [身份与关系](01-identity-and-relationships.md) · [API 与事件](07-api-and-events.md)

本文维护空间、消息和交谈的领域约定；当前传输字段见 [API 契约](07-api-and-events.md)，实施进度见状态文件。

## 1. 职责与数据对象

`crates/social` 定义中立类型，SocialFeature 组织用例，SocialStore 原子保存实体、事件引用和回执。当前聊天方法位于 feature.rs，修订与控制位于 controls.rs；按实际规模拆分，不以目标目录名推断模块已存在。

| 对象 | 当前关键字段 |
| --- | --- |
| Conversation | id、kind(direct/group)、relationship_id、title、status、paused、revision、context_epoch、participation_epoch、last_sequence、created_at、legacy |
| Membership | conversation_id、actor_id、status(active/left)、paused、revision |
| SocialMessage | id、conversation_id、sender_id、sender_name、sequence、body、reply_to、mentions、run_id、status、revision、created_at |
| Bout | id、conversation_id、status(open/ended)、revision |

消息修订按 message_id + revision 保存原内容版本；撤回清空其正文。read_positions 已有存储结构，尚无正式已读/未读用例。成员角色/参与模式、Bout 等待状态和详细时间/结束原因随对应用例扩展；不预先要求空字段。

所有 ID 为 UUID；sequence 由数据库在空间内单调分配，排序不依赖时间戳。revision、sequence、epoch 和 cursor 经 wire 转换为十进制字符串。

## 2. 空间与成员

direct 关联本地真人与一个 AI 的唯一 Relationship；包括归档在内，同一参与者对只有一个长期私聊。重复打开、切换话题、结束交谈、更换模型和恢复归档均沿用原 ConversationId。group 各有独立窗口，相同成员可以创建不同群；群创建至少包含一个 AI，后续可以移出全部 AI 而保留窗口。本地真人不能移出。

所有当前群成员共享完整本群历史，包括入群前的记录。是否被 @、是否发言、是否暂停均不改变本群可见范围；私聊和其他群历史不随成员加入共享。移出 AI 后 status=left，撤销源访问权、运行与待执行计划，保留历史发送者。重新加入更新成员版本，不复活旧计划。

逻辑上下文范围为 (ActorId, ConversationId)，每次请求带自己的档案与设置。意识系统按回复对象和容量选材；参与服务决定是否发言。provider 续接 ID 或缓存如被使用，也必须按此范围隔离。

M2 的 [跨窗口接续](03-consciousness.md#23-从群聊接续到私聊) 只引用双方共同可见的相关群来源，复核版本与权限。新私聊消息不发回源群；无源访问权时需要明确转发选中内容。

窗口 status 为 active/archived。paused 只控制接话，真人仍可保存消息；归档只读并取消活动运行和队列，恢复保留原 ID 与暂停设置。legacy 历史空间保持只读，不能恢复为活跃聊天。删除空间与遗忘是独立用例，后续须说明所有来源、记忆和联系意图的影响。

## 3. 消息与运行状态

SocialMessage.status 为 committed / generating / interrupted / withdrawn。真人消息保存成功和 AI 成功回答均为 committed；AI 开始事务才创建 generating 消息。取消、错误和断流留下的可见片段统一为 interrupted，具体 cancelled/failed 原因由 AiRun 和运行详情表达。撤回为 withdrawn 占位。

MessageId 与 RunId 分开。首版一次 AiRun 对应一个可见 AI 消息；思考、工具内部轨迹和协议日志不作为普通群消息，也不进入下一次模型上下文。工具循环属于执行用例，当前社交不开放工具。

窗口完整保存已提交记录及已落盘中断片段，近期选材或摘要不删历史。M1 新发送与 Web DTO 只支持文本；中立 body 和旧记录导入可以保留图片/文件，不代表附件上传与展示链路已完成。

## 4. 发送与连续输入

发送输入使用 requestId、text、replyTo 和有序 mentions。后端确定操作者，校验空间/成员与同空间回复关系，在同一事务保存真人消息、接话计划、事件引用和回执。正文中的 @名字不替代结构化提及。幂等规则集中在 [API 与事件](07-api-and-events.md#3-发送与幂等)。

私聊输入默认触发对方；普通群消息只保存，@ 或回复 AI 才选择接话目标。窗口或目标成员暂停时不安排对应计划；多个目标按持久 FIFO 顺序生成，一个空间最多一个活动运行。其他窗口仍可执行，真人可以持续输入。

已开始运行固定触发对象与上下文，新真人消息形成后续计划；同一触发下的后续 AI 可以读前面已完成的回复。M3 第一批在私聊短等待期内合并未开始的普通输入计划，保留所有原消息；引用/回复不合并。无模型或后续执行失败不撤销已保存真人消息。

## 5. 编辑、撤回与再次回应

仅当前操作者自己的 committed 消息可编辑/撤回，按消息 expectedRevision 检查。编辑保存新版本，保留 ID、顺序、回复关系和后续历史；修改不自动重新生成，用户可发新消息要求再次回应。

编辑/撤回增加来源空间 context_epoch，原子中断依赖该空间的活动运行并保留片段；取消以该消息为触发的未开始计划。撤回还取消明确回复该来源的待执行计划，正文与附件及旧修订内容清空，占位和引用关系保留。其他成员已提交回答继续保存。

M2 启用摘要/记忆后，来源变化还须同步失效其派生材料及跨窗口依赖，读取层在后台清理前就排除旧内容。事件与回执只保留引用，不恢复旧正文。

撤回不清理既有备份，也不能撤销供应商已接收的请求；残留与备份语义见 [存储设计](06-storage-and-migration.md#7-备份遗忘与回退)。

## 6. 空间、交谈与生成的生命周期

Conversation 长期存在，Bout 表达一次交谈，AiRun 表达一次执行。当前 Bout 为 open/ended：第一个 AI 开始运行时创建，没有 AI 接话的真人输入可以没有 Bout。窗口 DTO 返回 currentBout；一次生成完成不会自动结束交谈。

- 停止运行只终结指定 AiRun，其他计划可以继续，不永久暂停 AI。
- 暂停成员只停止该 AI；暂停窗口停止全部 AI，新真人消息仍保存。恢复只允许新输入触发，不补发已取消计划。
- 显式结束关闭当前 Bout 并取消窗口队列/活动运行；没有 Bout 时也清理待执行计划，不删除窗口或历史。
- 新输入的下一次 AI 运行创建新 Bout，可以继续使用允许的历史。
- M3 第一批已加入等待、空闲结束及自动额度；达到额度进入等待，不追加模型调用。约定联系在下一批实现，独立授权的未来意图不因一次交谈结束被误删。

暂停/结束与成功提交由存储事务顺序决定。停止事务已生效后，迟到结果不能改回成功。完整控制范围和竞争验证见 [实现记录](m1-message-controls.md)。

## 7. 验收

1. 三个 AI 以自己的身份/模型发言，私聊唯一，群空间独立。
2. 连续输入均落盘，计划顺序稳定；订阅断开不取消生成。
3. 当前成员共享入群前后历史，暂停与 @ 不改变可见性；移出后不能读取来源。
4. 编辑/撤回保留后续记录；旧回执、引用和迟到回复不能恢复撤回正文。
5. 单次停止、成员暂停、窗口暂停、结束、归档分别作用，恢复不复活取消计划。
6. M2 摘要/跨窗口引用不删原记录、不扩大来源权限；M3 等待/自动结束不删除长期窗口。
