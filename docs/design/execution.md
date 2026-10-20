# AI 执行

[总纲](overview.md) · [意识系统](consciousness.md) · [参与与调度](participation.md)

## 执行边界

保留 `crates/ai-client` 的 ModelSelection 校验、按 provider + effective protocol 路由、共享 SSE 分帧、工具配对规则和配置快照；保留工具注册表及固定白名单子进程。新执行模块放在 `features/social/src/execution.rs`。执行通过意识系统取得背景，不另写一套摘要、召回或记忆逻辑。

执行由 SocialFeature 组织，遵守 [新架构优先与兼容边界](overview.md#新架构优先与兼容边界)。AI 主动联系或群聊接话使用独立触发来源，不伪造真人输入。

## AiRun 对象与状态

当前 AiRun 为 id、message_id、bout_id、status、revision、snapshot。snapshot 记录目标 AI/空间、档案/绑定/关系版本、参与 epoch、预算与来源依赖；ParticipationPlan 关联 run_id。实际模型、思考、用量、计时和错误保存于独立 RunDetails。自动接话额度由 participation_daily_budget 在开始运行时保守扣减；完整协议和工具审计尚未实现，不复制为聊天正文。

RunId 是独立 UUID，与 MessageId 分开。当前一次 AiRun 对应一个可见社交消息，社交执行没有工具循环或完整协议日志。未来接入工具时，调用与结果属于运行内部轨迹，按协议成对处理，不冒充其他 AI 的历史。

状态为 running / succeeded / cancelled / failed / interrupted。排队状态属于 ParticipationPlan，开始事务才创建 running AiRun；当前没有单独持久化的 dispatch 标记，重启不重放任何已开始运行。running 不等于已经付费或供应商已接收，后续审计需单独表达。只有 running 可以转终态，终态不可被迟到 delta 或重复完成覆盖。

## 启动流程

1. 等待 ParticipationPlan 的时点，取得全局执行槽位后重读队首及计划版本，确定目标 AI、空间和触发消息范围。
2. 取得同一 Runtime 配置快照，解析该 AI 的 ExecutionBinding，校验温度、输出上限与思考模式，固定 GenerationOptions 并分配输出预算；不读取全局生成设置。
3. 按实际输出预留调用 ConsciousnessService::prepare(material, model, output_tokens)，取得 PreparedContext，固定档案、实际材料来源、成员权限与版本。
4. 再检查档案/绑定版本、暂停、epoch、预算和并发，事务保存 AiRun、generating 消息与开始事件；自动计划同时扣减每日次数和整个输入/输出容量额度，拒绝时整个开始事务回滚。
5. 调用快照 AiClient。网络调用不能发生在数据库事务或全局写锁内；后续扩展外部 dispatch 审计时明确记录边界。

无法解析模型时返回候选与可恢复错误，终结计划，不更换身份或静默替换模型。授权和版本在启动过程中变化时，放弃旧计划并重新评估。

## 社交消息转模型上下文

目标 AI 自己的历史完整消息可以映射为 assistant；其他参与者的发言按协议允许的方式保留为带发送者标签的内容。群里其他 AI 的消息不能全部映射成目标 AI 自己的 assistant 历史。

ConsciousnessService::prepare 构造中立的 `common::Message` 列表及 ContextSnapshot，以 PreparedContext 返回；执行层交给 ai-client 做具体协议转换。发送者标签、回复目标和时间线属于本次内容结构，GUI 不构造协议 JSON。当前已包含近期原文、档案、摘要、显式记忆和显式跨窗口引用；合并计划的全部输入为必要原文；约定联系使用独立触发类型和必要的授权内容，不伪造用户新消息，最终提交复核 ContactIntent。

上下文按 (ActorId, ConversationId) 构造，每次 AiRun 固定自己的快照。同一角色默认复用执行绑定，私聊和各群的消息范围与摘要分别选择；可选 provider 会话 ID、续接链或压缩缓存不得跨该范围复用。软件不依赖一个跨全部聊天窗口的常驻模型上下文。

从群聊接续到私聊时，意识系统按 ContextReference 使用存储复核并读取的相关且允许的源消息/摘要，并将来源空间、发送者与时间作为内容标注。快照包含跨窗口来源及权限版本；最终提交复核这些依赖，不能只检查目标私聊版本。本群摘要由当前成员共享；复用时检查来源版本与有效状态，退出源群后不能继续通过旧引用读取。

底层协议支持工具调用与结果的配对转换；该能力尚未接入社交执行。未来接入时，工具轨迹归属本次运行，不能把其他 AI 的工具调用当成自己的历史，中断调用也需按协议补齐结果。

历史思考、反馈、用量、计时、ID、中断元数据均不直接发送 provider。OpenAI 兼容同时接受 reasoning_content 和 reasoning，优先非空前者，否则取后者，不重复拼接。

图片能力随模型配置快照固定。AiClient 在分发前检查全部请求消息中的图片 part，包括实际选中的历史和跨窗口引用。明确不支持时返回 ImageInputUnsupported，不调用 provider；支持或未知时保留图片照常请求，不探测、不丢图、不更换模型。窗口摘要仍只读取文字与附件占位。详见[运行与配置](runtime.md)。

## 生成参数与容量

temperature 和 max_tokens 属于管理员维护的 ExecutionBinding。留空分别使用模型服务默认温度与意识系统自动输出预算，不继承旧 TOML `[gui]` 参数。保存与新运行使用同一模型/协议/思考模式的限制校验，错误返回实际候选或原因；不支持温度的模式要求明确清空，不能静默忽略。

显式输出上限按原值预留，再校验模型输出能力、思考模式和必要输入容量；不能压回默认值。留空时回答预算取 1024 与上下文容量四分之一的较小值，独立的 Anthropic 思考预算另行预留，最终受模型能力约束。输入预算为容量减实际输出预留。当前问题、直接回复、合并输入与明确引用等必要材料必须完整容纳；较早普通历史按预算选择。运行快照记录实际输入/输出预算，完整历史仍保存在窗口。

## 流式生成与检查点

模型 delta 更新生成缓冲并发布临时 GenerationEvent。约每 500ms 有变化时保存草稿检查点；工具循环接入后，调用/结果仍需写穿透；普通 delta 不逐 token 写入持久事件日志。

流式正文随临时生成事件显示；发送者与回复对象在开始事务创建生成消息时固定，持久变化通过 message_changed/run_changed 通知。思考内容保存在运行详情，不作为后续模型上下文或群共享记忆。社交执行尚未产生工具日志。

每条临时事件包含 runId、messageId、actorId、conversationId 和单 run 内递增 deltaSeq。重连不重放 token，而是读取持久草稿或最终消息；客户端收到终态后忽略迟到片段。

## 成功、取消与错误

角色设定与绑定修改只影响新运行，已经开始的请求保留原配置；旧准备若在启动前发生配置变化则拒绝启动。

温度与输出上限在保存及新运行启动时按同一解析规则校验；明确不支持的组合报错，不丢弃参数或替换模型。运行快照持久化绑定版本与实际输入/输出预算，尚未保存完整历史参数或完整 provider 请求审计。

成功：再次校验 Actor/成员状态、Bout 状态、参与 epoch 和受撤销的意识系统来源；同事务保存最终消息、usage/timings、run.succeeded、计划与意图终态和持久事件。事务成功才广播完成。

显式取消：先原子保存最近已落盘片段与 cancelled 终态，再通知执行任务停止；迟到收尾不能覆盖终态。Feature 关闭和异常收尾保存已接收的片段。工具循环接入后还须补齐未回答工具调用并停止 worker。暂停、结束 Bout、移除成员、遗忘所用记忆等撤销操作优先于成功提交；使用事务版本检查保证竞争结果确定。

失败：网络错误、EOF、协议错误、保存失败分别记录原因。先完成可持久化的中断收尾再通知终止；磁盘不可写时不伪报已保存，启动恢复处理仍为 running 的检查点。

图片能力拒绝使用独立错误码 image_input_unsupported，沿用运行失败收尾；界面说明真人记录已保存、该 AI 未调用模型。其他群成员继续独立执行。开始事务已预留的自动额度遵守失败不返还规则；声明支持后发生的供应商错误不自动改写能力标识。

消费端断开：只取消订阅，运行继续由后端托管。显式 cancelRun、Feature 关闭或 Runtime 退出才进入中断收尾。慢订阅者不会通过堵塞 token 通道无限阻塞执行。

## 工具与预算

社交 AI 尚未开放工具，ExecutionBinding 没有工具授权字段。未来若接入工具，需要单独设计结构化授权、启动时固定的集合与可撤销权限检查；ToolPolicy 不是当前已接通的对象。角色文本、群消息和模型返回均不能注册工具。

默认 echo/add 通过宿主 `--llmn-tool-worker` 的固定编译白名单子进程执行。子进程清空继承环境，输入/输出各限 64 KiB；Registry 默认 30 秒墙钟超时并捕获 unwind panic。Tool::run 必须可丢弃且主动让出执行权，超时不能抢占不 yield 的进程内代码。Windows Job Object 限一个进程、256 MiB committed memory、10 秒用户态 CPU，关闭 Job 终止子进程；配置或附加失败停止执行，不退回进程内。Linux 依赖终止与 kill_on_drop，没有等价完整资源配额或通用沙盒。trusted 注册是显式信任边界，不接收模型指定任意插件；打包后还需验证动态库与退出行为。未来工具循环次数与社交发言轮数分别计数。

自动接话与约定联系共用窗口额度，开始事务按整个输入容量与输出上限预留 token/次数，失败、取消或崩溃不返还；usage 只作实际用量展示，不用于退款或估造精确费用。prompt_tokens 已含缓存输入，缓存命中率仍为 cached / prompt。货币预算需要有效价格来源，首版主要使用次数与 token 限额。
