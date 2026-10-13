# 04 参与与调度

[总纲](README.md) · [聊天与消息](02-conversations-and-messages.md) · [AI 执行](05-ai-execution.md)

M3 第一批已实现参与节奏；证据见 [实现记录](m3-participation-rhythm.md)。第 8 节的约定联系属于下一批，不能视为当前能力。

## 1. 职责

SocialFeature 负责唤醒、等待、全局执行限额与关闭；SocialStore 在事务内选择目标、保存 ParticipationPlan、复核授权、预留额度及更新 Bout。当前实现位于 feature.rs/execution.rs 和 storage/social/rhythm.rs，不为 ParticipationService/Scheduler 名称单独建立空框架。

意识系统为已经选定的 AI 与空间准备上下文，不决定谁发言。摘要是独立内部任务，不计入社交发言轮数、不触发接话或主动联系。当前群成员共享完整群历史；暂停和未被点名只影响发言资格，不缩小可见范围。

## 2. 触发与决策

真人消息、结构化 @ / 回复、成功提交的 AI 消息可创建计划；规则、成员或控制变化唤醒队列重新检查。token delta、思考、失败片段、工具轨迹和摘要完成均不触发自动接话。没有合格计划就不调用模型。

决策以实际计划和窗口 ParticipationState 表达。state.reason 为 waiting、input_pending、turn_limit、daily_limit、token_limit、no_candidate、expired、idle、user_ended、policy_changed；失败细节仍在计划 failure 中。没有隐藏推理日志或自然语言意图识别。

## 3. 确定性规则与优先级

开始前检查活动账号、窗口和成员、暂停、来源与版本、FIFO 队首、not_before/expires_at、模型配置和并发。自动计划另外复核 limitedAuto 与 policy_revision、轮数和每日预留额度。

新真人消息撤销尚未开始的自动计划，明确 @ / 回复按选择顺序排队。已开始运行保持原触发对象与快照，新输入进入后续计划；不会静默删除正在显示的草稿。暂停优先于 @，文本中的 @ 不替代结构化 mentions。

同空间最多一个 AI 在生成，SocialFeature 全局最多 4 个社交 AI 运行；摘要另有一个独立槽位。取得全局槽位后重新读取计划，防止等待期间已被合并、取消或过期的旧计划开始。

## 4. 首版参与规则

| 规则 | 当前行为 |
| --- | --- |
| direct_reply | 唯一私聊自动响应；默认等待 800ms，可设置 0–2000ms |
| mentioned_only | 新群默认仅响应明确 @ / 回复；普通输入只保存 |
| limited_auto | 群详情显式开启，普通输入可以触发一个候选，完整回复后再评估下一个 |
| 独立暂停 | 窗口/成员 paused 停止相应计划与运行，成员历史可见范围保持不变 |

自动候选按稳定账号 ID 顺序轮转，跳过当前发言者、暂停、停用或已退出成员。这是确定性的轮流接话，不宣称具备语义相关性判断。模型文本不创建结构化 @，不能自行提高发言额度。开启规则本身不重放旧消息，也不授权未来主动联系。

## 5. 连续输入与有限接话

私聊仅合并短等待期内、尚未开始、没有引用/回复/mentions 的普通输入计划，每个计划最多 20 条。保留全部原消息和发送回执；input_ids 保存所有输入，trigger_id 指向最后一条。新输入延后 not_before；已开始的计划和各自携带来源的引用/回复计划不合并。任一合并来源修改或撤回都会取消该排队计划。

意识系统必须完整容纳全部合并输入；不能以摘要或历史裁剪丢掉其中一条。容量不足会明确失败，原消息仍保留。

默认自动接话上限为无人插话连续 3 次 AI 发言、每个 Bout 12 次自动调用、每群每 UTC 日 48 次自动调用及 262144 tokens 预留额度。可配置范围依次为 1–6、1–48、1–200、1024–4000000。自动计划间等待 800ms。连续计数包括已成功的显式回复；显式 @ / 回复不受自动额度阻断，也不消耗自动额度。

每日额度在运行开始事务中按整个允许的输入容量 + 输出预留扣减，不依赖供应商 usage；它是保守的调用上限，不是账单估算。开始后失败、取消、崩溃也不返还。达到次数或容量上限就等待新授权，不缩减模型输出、不自动切换模型。真人输入重置连续计数，新 Bout 重置该 Bout 计数；结束、暂停、重启与模型更换不清零每日额度。UTC 日跨过后使用新账本；时钟回拨不能重新打开已花过的旧日期额度。

## 6. 等待、结束与暂停

- Wait：不生成假消息，保留 Bout 与话题。限额或无候选时显示原因。
- EndBout：显式结束取消当前窗口队列与运行，保留长期窗口、消息和额度记录。
- PauseConversation / PauseMember：沿用现有 epoch/成员版本及取消边界；恢复仅响应新消息。
- CancelRun：停止本次生成；不会因失败/取消自动续接下一位，也不永久暂停账号。
- 修改接话规则：校验 policy.revision，同事务取消排队及在飞自动计划；显式计划保留原来的时点与授权。修改本身不发言。

默认空闲 300 秒结束，可设置 30–3600 秒。只在没有活动运行或排队计划时关闭 Bout；到点不调用模型生成告别语。活动时间使用已保存的真人输入或运行终态，时钟回拨不倒退。自然语言“先到这里”暂不解析，用户可使用确定性的“结束当前交谈”按钮。

下一批独立授权的未来约定将与 Bout 分开，显式结束不误删约定；该规则不表示 ContactIntent 已实现。

## 7. 持久参与计划

ParticipationPlan 保存 id、conversation_id、actor_id、trigger_id、trigger_sequence、ordinal、membership_revision、status、run_id、failure、revision、created_at，并增加 automatic、input_ids、not_before、expires_at、policy_revision。同一消息/目标唯一，明确目标顺序按 ordinal 保存；合并更新计划原 ID 和版本，旧请求重试不另建计划。

状态沿用 queued / running / succeeded / failed / cancelled / interrupted。真人消息与计划原子保存；成功回复、计划终态与后继自动计划同事务保存。新计划有效期 5 分钟；合并后从最新输入计算。过期计划取消并显示原因，不发布积压回复。旧库计划缺少新增可选时间时保持其原有一次性授权，后续新计划使用新规则。

SQLite v10 新增 participation_policies、participation_state、participation_daily_budget；不改旧迁移。状态/预算变化发布 participation_changed 引用事件，Web/IPC 重新读取窗口快照。

## 8. 主动联系意图（M3 下一批）

下一批实现显式的一次性 ContactIntent、独立主动联系授权与免打扰。意图需要保存创建者、AI、目标窗口、触发内容、UTC 到期/过期时间、展示时区、授权引用、幂等键和终态。

计划到期先领取并复核成员、暂停、免打扰、有效期和额度，再产生参与计划；意图不能绕过执行用例直接调用模型。消息、AiRun 和意图终态需原子完成。重复日程、条件监控、夏令时复杂规则与自然语言约定识别后续另行设计。

## 9. 时间、重启与失败

Clock 提供 UTC now 与可取消的 wait_until。SystemClock 用 Tokio 单调计时等待，唤醒后仍复核持久 UTC 时点；墙钟回拨可延后执行，不提前发言。ManualClock 通过通知驱动等待，无需在测试里真实等待几分钟。

每窗口只有一个调度任务；新输入/控制唤醒同一任务，不按每条消息积累长期等待任务。启动重新检查 queued 计划及 open Bout：未过期仍授权的计划继续，过期计划取消，已开始运行恢复为 interrupted，禁止重放。关闭先取消定时等待与执行，等待任务和存储收尾，不在退出后开启新调用。

当前未实现 ContactIntent 的 pending/claimed/executing/uncertain 恢复；下一批需区分未派发与外部结果不确定，不把本地幂等声称为外部 exactly-once。

## 10. 验收

以独立临时库和模拟 provider 验证默认不自动发言、合并原文/回执、运行快照稳定、轮数与每日额度、来源修订、暂停/结束/规则撤销、崩溃恢复及过期/空闲。可控时钟覆盖等待、唤醒、回拨和退出；HTTP/IPC 调用同一用例，实际页面检查保存及状态文案。具体结果见 [本批记录](m3-participation-rhythm.md)，约定联系和免打扰留给下一批验收。
