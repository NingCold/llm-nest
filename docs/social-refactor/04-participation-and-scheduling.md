# 04 参与与调度

[总纲](overview.md) · [聊天与消息](02-conversations-and-messages.md) · [AI 执行](05-ai-execution.md)

M3 两批首版范围已实现；证据见 [参与节奏](m3-participation-rhythm.md) 与 [约定联系](m3-contact-intents.md)。本文描述当前规则，扩展范围独立列出。

## 1. 职责

SocialFeature 负责唤醒、等待、全局执行限额与关闭；SocialStore 在事务内选择目标、保存 ParticipationPlan、复核授权、预留额度及更新 Bout。当前实现位于 feature.rs/execution.rs 和 storage/social/rhythm.rs，不为 ParticipationService/Scheduler 名称单独建立空框架。

意识系统为已经选定的 AI 与空间准备上下文，不决定谁发言。摘要是独立内部任务，不计入社交发言轮数、不触发接话或主动联系。当前群成员共享完整群历史；暂停和未被点名只影响发言资格，不缩小可见范围。

## 2. 触发与决策

真人消息、结构化 @ / 回复、成功提交的回应消息和到期的一次性约定可创建计划；规则、成员或控制变化唤醒队列重新检查。token delta、思考、失败片段、工具轨迹和摘要完成均不触发自动接话。没有合格计划就不调用模型。

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

默认自动接话上限为无人插话连续 3 次 AI 发言、每个 Bout 12 次自动调用、每窗口每 UTC 日 48 次主动调用及 262144 tokens 预留额度。可配置范围依次为 1–6、1–48、1–200、1024–4000000。自动计划间等待 800ms。一次性约定共用这些轮数/次数/容量上限，但不需要开启 limitedAuto；每个意图只生成一次联系，成功后不继续自动接话。连续计数包括已成功的显式回复；显式 @ / 回复不受自动额度阻断，也不消耗自动额度。

每日额度在运行开始事务中按整个允许的输入容量 + 输出预留扣减，不依赖供应商 usage；它是保守的调用上限，不是账单估算。开始后失败、取消、崩溃也不返还。达到次数或容量上限就等待新授权，不缩减模型输出、不自动切换模型。真人输入重置连续计数，新 Bout 重置该 Bout 计数；结束、暂停、重启与模型更换不清零每日额度。UTC 日跨过后使用新账本；时钟回拨不能重新打开已花过的旧日期额度。

## 6. 等待、结束与暂停

- Wait：不生成假消息，保留 Bout 与话题。限额或无候选时显示原因。
- EndBout：显式结束取消当前窗口队列与运行，保留长期窗口、消息和额度记录。
- PauseConversation / PauseMember：沿用现有 epoch/成员版本及取消边界；恢复仅响应新消息。
- CancelRun：停止本次生成；不会因失败/取消自动续接下一位，也不永久暂停账号。
- 修改接话规则：校验 policy.revision，同事务取消排队及在飞自动计划；显式计划保留原来的时点与授权。修改本身不发言。

默认空闲 300 秒结束，可设置 30–3600 秒。只在没有活动运行或排队计划时关闭 Bout；到点不调用模型生成告别语。活动时间使用已保存的真人输入或运行终态，时钟回拨不倒退。自然语言“先到这里”暂不解析，用户可使用确定性的“结束当前交谈”按钮。

独立授权的 pending 未来约定与 Bout 分开，显式结束保留它们；已经领取、排队或执行的本次联系随当前计划停止。关闭主动联系会撤销全部未完成意图。

## 7. 持久参与计划

ParticipationPlan 保存 id、conversation_id、actor_id、trigger_id、trigger_sequence、ordinal、membership_revision、status、run_id、failure、revision、created_at，并增加 automatic、input_ids、not_before、expires_at、policy_revision。同一消息/目标唯一，明确目标顺序按 ordinal 保存；合并更新计划原 ID 和版本，旧请求重试不另建计划。

状态沿用 queued / running / succeeded / failed / cancelled / interrupted。真人消息与计划原子保存；成功回复、计划终态与后继自动计划同事务保存。消息触发的新计划有效期 5 分钟；合并后从最新输入计算。约定计划使用用户明确选择的 expiresAt。过期计划取消并显示原因，不发布积压回复。旧库计划缺少新增可选时间时保持其原有一次性授权，后续新计划使用新规则。

SQLite v10 新增 participation_policies、participation_state、participation_daily_budget；v11 扩展计划触发：trigger_id 与 contact_intent_id 恰有一个，旧消息计划不变；意图不填虚构 MessageId。状态/预算变化发布 participation_changed 引用事件，Web/IPC 重新读取窗口快照。

## 8. 一次性约定联系与免打扰

主动联系默认关闭。真人成员在聊天详情单独开启 ContactSettings.enabled，再明确创建一个 ContactIntent；群有限接话开关、角色文本、记忆与模型输出均不构成主动联系授权。

ContactIntent 保存创建者、稳定 AI 账号、窗口、不可变约定内容、authorizationId（创建命令 ID）、UTC dueAt/expiresAt、创建日期对应的 utcOffsetMinutes、planId、状态、revision、reason 与时间。创建时间在未来一年内，失效时间晚于到期且最多延后七天；每窗口最多 64 个未完成意图。内容修改通过取消并新建完成，不改写旧授权。界面使用本机时区并显示偏移，首版不解释循环时区规则。

状态流为 pending → claimed → executing → delivered；失败、取消、过期、跳过和结果不确定分别为 failed/cancelled/expired/skipped/uncertain。到期在同一事务复核授权、账号、双方当前成员、暂停、有效期和免打扰，再创建唯一参与计划并标 claimed。取得全局执行槽位后再次复核，运行开始事务预留额度、保存 AiRun/生成占位并标 executing。成功消息、运行/计划和意图终态同事务提交；没有伪造的真人消息，AI 消息 replyTo=null。

| 情况 | 行为 |
| --- | --- |
| 免打扰 | 窗口设置 quietUntil 为一次 UTC 截止时间，quietAction 为 defer 或 skip；只影响约定联系，普通回复保持可用 |
| defer | 未开始联系延后到免打扰结束；若先超过 expiresAt 则 expired；延后计划不堵住正常回复 |
| skip | 到期时处于免打扰则 skipped，不补发 |
| 开启免打扰时已经执行 | 停止本次生成并 cancelled，不重试外部请求 |
| 窗口或目标暂停 | 到期时 skipped；恢复先清理暂停期间已到期意图，未来尚未到期约定保留 |
| 账号停用、窗口归档、目标退群 | 取消相关未完成意图；恢复或重新入群不恢复旧授权 |
| 关闭主动联系或单独取消 | 同事务撤销对应意图、排队和在飞运行；保留记录与已扣额度 |
| 运行中到达失效时间 | 可取消时钟等待停止生成并保留片段，意图 expired；最终提交也复核有效期 |
| 模型失效、额度不足、调用失败 | failed，不模拟回答，不自动重试；开始前失败不产生生成占位或额度扣减 |

约定通过 ConsciousnessService 读取当前窗口上下文，完整容纳授权内容；它是独立触发来源，不沿用最近的跨窗口引用。正常模型配置修改只影响新的运行，授权关闭、来源修订等仍能使在飞提交失效。界面展示全部活动约定和最近 50 条记录，全部意图仍持久保存。

## 9. 时间、重启与失败

Clock 提供 UTC now 与可取消的 wait_until。SystemClock 用 Tokio 单调计时等待，最多每 30 秒复核 UTC，唤醒后仍复核持久时点；墙钟回拨可延后执行，不提前发言。ManualClock 通过通知驱动等待，无需在测试里真实等待几分钟。

每窗口只有一个调度任务；新输入/控制唤醒同一任务，不按每条消息积累长期等待任务。启动重新检查 queued 计划、open Bout 与 pending/claimed 意图：未过期仍授权的计划继续，过期计划取消，已开始运行恢复为 interrupted，禁止重放。关闭先取消定时等待与执行，等待任务和存储收尾，不在退出后开启新调用。

未开始的 pending/claimed 联系在重启后按当前授权和有效期重评；executing 恢复为 uncertain（已过期则 expired），不自动再调用模型。意图不是外部 exactly-once 保证。应用关闭时无系统定时器、后台唤醒或离线推送；错过有效期不集中补发。旧 Bout 若已空闲且无队列/运行，先关闭，再为新到期联系建立新 Bout。

## 10. 验收

以独立临时库和模拟 provider 验证默认不自动发言、合并原文/回执、运行快照稳定、轮数与每日额度、来源修订、暂停/结束/规则撤销、崩溃恢复及过期/空闲。可控时钟覆盖等待、唤醒、回拨和退出；HTTP/IPC 调用同一用例，实际页面检查保存及状态文案。具体结果见 [参与节奏记录](m3-participation-rhythm.md) 与 [约定联系记录](m3-contact-intents.md)。重复日程、条件监控、全局/循环免打扰、自然语言时机识别与语义候选选择仍为后续扩展。
