# 05 AI 执行

[总纲](README.md) · [意识系统](03-consciousness.md) · [参与与调度](04-participation-and-scheduling.md)

本文维护所属模块的规则与扩展目标。 实施状态见 [已完成](completed.md) / [尚未完成](pending.md)，当前调用契约见 [API 与事件](07-api-and-events.md)。

## 1. 保留的运行能力

保留 `crates/ai-client` 的 ModelSelection 校验、按 provider + effective protocol 路由、共享 SSE 分帧、工具配对规则和配置快照；保留工具注册表及固定白名单子进程。新执行模块放在 `features/social/src/execution.rs`。执行通过意识系统取得背景，不另写一套摘要、召回或记忆逻辑。

现有 `features/chat` 在迁移期间服务尚未切换的旧入口，随后由相同社交用例替代或随入口明确退役，遵守 [新架构优先与兼容边界](README.md#新架构优先与兼容边界)。新执行调用不得复用“先写入一个 user 消息”的旧 chat 方法，否则 AI 主动联系或群聊接话会伪造真人输入。

## 2. AiRun 对象与状态

当前 AiRun 为 id、message_id、bout_id、status、revision、snapshot。snapshot 记录目标 AI/空间、档案/绑定/关系版本、参与 epoch、预算与来源依赖；ParticipationPlan 关联 run_id。实际模型、思考、用量、计时和错误保存于独立 RunDetails。自动接话额度由 participation_daily_budget 在开始运行时保守扣减；意图和工具审计随后续用例扩展，不复制为聊天正文。

RunId 是独立 UUID，不能继续借 MessageId 表示运行。一轮工具循环属于同一 AiRun，默认只产生一个可见社交消息，内部协议消息和工具日志保存在执行记录中。

状态为 running / succeeded / cancelled / failed / interrupted。排队状态属于 ParticipationPlan，开始事务才创建 running AiRun；当前没有单独持久化的 dispatch 标记，重启不重放任何已开始运行。running 不等于已经付费或供应商已接收，后续审计需单独表达。只有 running 可以转终态，终态不可被迟到 delta 或重复完成覆盖。

## 3. 启动流程

1. 等待 ParticipationPlan 的时点，取得全局执行槽位后重读队首及计划版本，确定目标 AI、空间和触发消息范围。
2. 取得同一 Runtime 配置快照，解析该 AI 的 ExecutionBinding，校验温度、输出上限与思考模式，固定 GenerationOptions 并分配输出预算；不读取全局 Harness 生成设置。
3. 按实际输出预留调用 ConsciousnessService.prepare_context，取得 PreparedContext，固定档案、实际材料来源、成员权限与版本。
4. 再检查档案/绑定版本、暂停、epoch、预算和并发，事务保存 AiRun、generating 消息与开始事件；自动计划同时扣减每日次数和整个输入/输出容量额度，拒绝时整个开始事务回滚。
5. 调用快照 AiClient。网络调用不能发生在数据库事务或全局写锁内；后续扩展外部 dispatch 审计时明确记录边界。

无法解析模型时返回候选与可恢复错误，终结计划，不更换身份或静默替换模型。授权和版本在启动过程中变化时，放弃旧计划并重新评估。

## 4. 社交消息转模型上下文

目标 AI 自己的历史完整消息可以映射为 assistant；其他参与者的发言按协议允许的方式保留为带发送者标签的内容。群里其他 AI 的消息不能全部映射成目标 AI 自己的 assistant 历史。

意识系统内部 ContextBuilder 输出中立的 `common::Message` 列表及 ContextSnapshot，ConsciousnessService 以 PreparedContext 返回；执行层与当前工具循环组合后交给 ai-client 做具体协议转换。发送者标签、回复目标和时间线属于本次内容结构，GUI 不构造协议 JSON。当前已包含近期原文、档案、摘要、显式记忆和显式跨窗口引用；M3 合并计划的全部输入为必要原文。

上下文按 (ActorId, ConversationId) 构造，每次 AiRun 固定自己的快照。同一角色默认复用执行绑定，私聊和各群的消息范围与摘要分别选择；可选 provider 会话 ID、续接链或压缩缓存不得跨该范围复用。软件不依赖一个跨全部聊天窗口的常驻模型上下文。

从群聊接续到私聊时，ContextBuilder 按 ContextReference 读取相关且允许的源消息/摘要，并将来源空间、发送者与时间作为内容标注。快照包含跨窗口来源及权限版本；最终提交复核这些依赖，不能只检查目标私聊版本。本群摘要由当前成员共享；复用时检查来源版本与有效状态，退出源群后不能继续通过旧引用读取。

工具调用与结果是运行内部轨迹，仅按对应协议成对发送。不能把另一个 AI 的工具调用当成自己的历史调用；中断调用补齐失败结果并按既有规则过滤。

历史思考、反馈、用量、计时、ID、中断元数据均不直接发送 provider。OpenAI 兼容 reasoning_content/reasoning 的已有处理规则继续保留。

## 5. 流式生成与检查点

模型 delta 更新生成缓冲并发布临时 GenerationEvent。约每 500ms 有变化时保存草稿检查点；工具循环接入后，调用/结果仍需写穿透；普通 delta 不逐 token 写入持久事件日志。

首版可以保持流式正文显示，联系人名称和回复对象在 run.started 时固定。思考内容及工具日志默认进入运行详情，不能自动成为群里所有 AI 的记忆来源。

每条临时事件包含 runId、messageId、actorId、conversationId 和单 run 内递增 deltaSeq。重连不重放 token，而是读取持久草稿或最终消息；客户端收到终态后忽略迟到片段。

## 6. 成功、取消与错误

角色设定与绑定修改只影响新运行，已经开始的请求保留原配置；旧准备若在启动前发生配置变化则拒绝启动。

温度与输出上限在保存及新运行启动时按同一解析规则校验；明确不支持的组合报错，不丢弃参数或替换模型。运行快照持久化绑定版本与实际输入/输出预算，尚未保存完整历史参数或完整 provider 请求审计。默认值与协议约束见 [生成参数记录](m1-generation-parameters.md)。

成功：再次校验 Actor/成员状态、Bout 状态、参与 epoch 和受撤销的意识系统来源；同事务保存最终消息、usage/timings、run.succeeded、计划与意图终态和持久事件。事务成功才广播完成。

显式取消：先原子保存最近已落盘片段与 cancelled 终态，再通知执行任务停止；迟到收尾不能覆盖终态。Feature 关闭和异常收尾保存已接收的片段。工具循环接入后还须补齐未回答工具调用并停止 worker。暂停、结束 Bout、移除成员、遗忘所用记忆等撤销操作优先于成功提交；使用事务版本检查保证竞争结果确定。

失败：网络错误、EOF、协议错误、保存失败分别记录原因。先完成可持久化的中断收尾再通知终止；磁盘不可写时不伪报已保存，启动恢复处理仍为 running 的检查点。

消费端断开：只取消订阅，运行继续由后端托管。显式 cancelRun、Feature 关闭或 Runtime 退出才进入中断收尾。慢订阅者不会通过堵塞 token 通道无限阻塞执行。

## 7. 工具与预算

社交 AI 默认没有工具权限。后续工具授权用例通过 ExecutionBinding 引用固定 ToolPolicy；授权的工具集合在启动时固定，每次执行前检查可撤销权限。角色文本、群消息和模型返回不能自行注册工具。

默认 echo/add 继续使用宿主 worker 入口、64 KiB 输入/输出、Registry 超时与 panic 捕获；Windows Job Object 和 Linux 现有边界继续保持。工具循环次数与社交自动发言轮数分开计数。

M3 自动运行启动前预留 token/次数预算，结束后根据实际 usage 结算；usage 缺失采用保守上限记录，不估造精确费用。prompt_tokens 已含缓存输入，缓存命中率仍为 cached / prompt。货币预算需要有效价格来源，首版主要使用次数与 token 限额。

## 8. 验收

1. 同群不同 AI 分别构造正确身份与允许的上下文。
2. 配置热更新期间，一次 run 的模型解析和协议路由来自同一快照。
3. 工具循环、reasoning、EOF、取消和 provider 错误均保留旧有可靠性。
4. 订阅断开仍完成保存，用户取消才终止运行。
5. 在成功提交前暂停或收回权限，最终结果不能覆盖停止状态。
6. 重启将 running 标 interrupted，不重放外部调用或工具。
7. 存储失败不显示成功，临时 delta 无法使终态消息重新生成。
8. 结束 Bout 与成功提交竞争时，结束后该 Bout 不再发布成功回复；未来独立约定不被误删。
