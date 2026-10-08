# 04 参与与调度

[总纲](README.md) · [聊天与消息](02-conversations-and-messages.md) · [AI 执行](05-ai-execution.md)

状态：目标设计，尚未实现。

## 1. 职责

ParticipationService 决定谁应该发言、等待或结束；Scheduler 在授权时间或条件到达时重新进行参与决策。二者位于 `features/social`，由 Runtime 的 Feature 生命周期托管。模型可以提供建议，确定性规则拥有最终决定权。

意识系统（Consciousness）为已经选定的 AI 与空间准备背景，不决定是否发言。参与服务可以使用允许的触发信息评估计划；进入执行后再通过 ConsciousnessService.prepare_context 准备请求。摘要与未来后台巩固是内部任务，不占用社交 Bout 发言轮数，也不触发主动联系；其模型调用仍受独立预算和共享资源限额控制。

参与机制不使用后台无限循环聊天来模拟在线。没有符合条件的事件和计划，就不调用模型。

## 2. 触发与决策

触发包括：真人消息、明确 @、回复某 AI、用户邀请发言、计划到期、成员/权限变化、停止命令、运行结束。临时 token delta 不触发其他 AI 的参与。

决策结果建议为 Speak、Wait、Ignore、EndBout；附带目标 ActorId、触发引用、based_on_seq、not_before、expires_at、reason_code、policy_revision 和 dedupe_key。

reason_code 是可理解的有限枚举，例如 mentioned、direct_reply、scheduled_contact、paused、not_authorized、budget_exhausted。它不保存或展示模型隐藏推理。

## 3. 确定性规则与优先级

决策依次检查：

1. Actor、空间、成员是否活动，执行配置是否有效。
2. 参与是否暂停、主动联系是否授权、当前是否免打扰。
3. 触发内容与所需历史是否可见。
4. 触发或计划是否已经处理、是否过期。
5. 空间与全局并发、自动轮数和预算是否允许。
6. 明确 @、回复和邀请优先于普通相关性判断。
7. 没有明确目标时，按成员模式选择候选；可输出 Wait 或 Ignore。

暂停和停止优先于 @。手动邀请可以创建一次性发言授权，但界面应明确这是恢复或单次许可，后端不能仅凭提示词绕过暂停。

## 4. 首版参与模式

| 模式 | 初版行为 |
| --- | --- |
| direct_reply | 私聊真人消息触发该 AI 回复，允许合并连续输入 |
| mentioned_only | 群里仅 @、明确回复或邀请触发；新群默认模式 |
| limited_auto | 允许有限自动接话；M3 加入 |
| paused | 不新建运行，取消队列中的参与计划 |

主动联系是另外的 opt-in 设置，不随 limited_auto 自动开启。群里 @ 多个 AI 时按明确队列逐个回应，每个 AI 都有独立运行记录。

首版一个空间最多一个 AI 在生成，全局并发另有上限。等待 AI 不阻止真人发送，也不阻止其他空间显示消息。

## 5. 连续输入与有限接话

M1 可以逐条排队回应，先保证连续消息不丢失；M3 加入私聊可配置的短等待窗口，合并尚未开始的回应计划。实现先以约 800ms 为实验起点，具体值经过体验验证后确认。更新队列的目标消息范围，不合并或改写原始消息。

正在执行的运行保持输入快照和回复对象；新消息形成后续计划。用户明确停止、改变权限或取消该 run 时中断。生成尚未开始的旧计划可以替换，正在显示的草稿不能静默删除。

自动接话只在消息 finalized 后评估，工具日志和部分输出不触发。首版实验参数可采用无人输入连续 AI 发言上限 3 次、单 Bout 自动发言上限 12 次；最终默认值在 M3 验证后确定。

真人消息可以重置“连续无人输入”计数，但不能重置每日自动预算或绕过空间暂停。多个 AI 互相 @ 同样受限；达到限额转 waiting 或 ended，并解释原因。

## 6. 等待、结束与暂停

- Wait：暂不发言，保留当前 Bout 与未完成话题，不显示假消息。
- EndBout：将当前交谈标 ended，取消该 Bout 的未执行发言计划和活动运行；最终提交复核 Bout 状态，空间与历史仍存在。
- PauseConversation：提高 participation_epoch，暂停全部自动参与并取消活动运行；真人可以继续记录消息。
- PauseMember：只暂停指定 AI。
- CancelRun：仅取消本次生成，不永久暂停联系人。

用户说“今天先到这里”可以作为结束候选；始终提供确定性的界面结束按钮。模型判断结束不能删除约定、改变权限或自主创建新联系计划。

显式结束清除本次接话队列并中断本 Bout 活动运行；用户另行设置的未来约定保留并在详情中可见。若用户同时关闭主动联系，则撤销相应 ContactIntent。空闲规则在没有活动运行和可执行发言计划时结束，不额外付费生成告别语。

## 7. 持久参与计划

ParticipationPlan 保存 id、conversation_id、bout_id、actor_id、trigger_ref、based_on_seq、not_before、expires_at、dedupe_key、policy_revision、status。

状态为 queued / claimed / running / completed / cancelled / expired。计划入库与处理进度更新原子完成。基于持久事件消费时，同事务更新 consumer offset 和计划，避免崩溃后漏掉或重复安排。

领取时复核所有权限、限额和 epoch；生成提交前再次复核可撤销条件。过期排队计划终结并发事件，不悄悄积累到下次启动集中发送。

## 8. 主动联系意图

ContactIntent 保存 owner_actor_id、ai_actor_id、conversation_id、trigger、payload、due_at、expires_at、timezone、authorization_ref、idempotency_key、status。

首版支持一次性时间约定。条件触发限定到本地、已授权的业务事件；“随时关注所有活动”不属于默认权限。计划显示具体时区和时间，内部使用 UTC；重复日程与夏令时规则后续再设计。

状态建议为 pending / claimed / executing / succeeded / skipped / cancelled / failed / uncertain。到期先领取，再复核成员、暂停、免打扰、有效期和预算，通过后创建参与计划或运行；意图本身不能直接调用模型。

正常执行结束时，消息、AiRun 与 ContactIntent 终态一起提交。相同 idempotency_key 不重复创建意图；完成过的意图不再领取。

## 9. 时间、重启与失败

实时等待使用可注入单调时钟；持久 due_at 使用 UTC，测试使用 fake clock。调度采用可取消的异步等待，关闭时停止领取新计划。

应用重启后：

- queued/pending：未过期且仍授权的计划重新评估；免打扰时按有效期延后或 skipped。
- claimed：确认尚未外部 dispatch 的记录可归还队列。
- running/executing：恢复已保存草稿并标 interrupted；无法确认外部请求或副作用结果的意图标 uncertain，不自动重放。
- 已过期：终结，默认不补发积压问候。

本地幂等不等于外部调用 exactly-once。失败重试只能在明确安全、未 dispatch 或用户要求时执行，带退避和次数限制。

## 10. 验收

1. 未被叫到的 AI 在 mentioned_only 模式下不调用 provider。
2. @ 多个 AI 形成有限队列，真人仍可连续发送。
3. 暂停与运行提交并发时，停止事务生效后不会再发布成功回答。
4. 自动 AI 互聊到达轮数或预算上限后停下。
5. 到期意图在免打扰、权限撤回、联系人归档时不能发言。
6. fake clock 下验证延迟、空闲结束、过期、时钟回退与重启补发边界。
7. 崩溃恢复不重复发布消息或自动重放工具。
