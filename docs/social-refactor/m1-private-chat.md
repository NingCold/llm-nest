# M1 首批：真实长期私聊

[总纲](overview.md) · [实施 TODO](TODO.md) · [交付与验证](10-delivery-and-validation.md)

日期：2026-10-09。完成 M1 的第一条业务链路：创建 AI 联系人与模型绑定 → 打开唯一私聊 → 保存真人消息 → 意识系统准备近期上下文 → 模型流式执行 → 原子收尾 → Web 展示与重连读取。本批没有调用付费 provider；执行验收通过本机模拟服务完成。

本文件是第一批完成时的历史记录。后续 [群聊](m1-group-chat.md) 改用共享持久队列，[消息/控制](m1-message-controls.md) 改为持久停止与修订事务；下文的进程内队列和取消行为仅描述首批。当前能力见 [已完成](completed.md)，可调用接口见 [API 契约](07-api-and-events.md)，下一步见 [正在推进](in-progress.md)。

## 实际结构

```text
真实 Web → HTTP ─┐
基础 IPC 命令 ───┴→ SocialFeature
                    ├→ 联系人/唯一私聊/消息用例 → SocialStore
                    ├→ ConsciousnessService → PreparedContext + 来源快照
                    └→ 独立 AiRun → AiClient 快照 → 协议适配
Runtime → SocialResources(store/clock)、AiClient、生命周期
SocialStore → SQLite 单写线程 → 实体 + 持久事件引用 + 命令回执
```

| 落点 | 本批责任 |
| --- | --- |
| `features/social/src/feature.rs` | 初始化本地真人、联系人创建/修改、唯一私聊、文本发送、读取、取消与后台任务生命周期 |
| `features/social/src/consciousness.rs` | 本窗口可见近期原文、角色设定、预算、版本化来源与权限依赖 |
| `features/social/src/execution.rs` | 配置快照、每空间互斥执行、正文/思考、500ms 检查点、失败/取消/终态 |
| `features/social/src/wire.rs` | HTTP/IPC 共用的 camelCase DTO 和业务错误；计数器使用十进制字符串 |
| `crates/events/src/social_generation.rs` | 临时生成片段与执行提示；不写入持久事件日志 |
| `crates/storage/src/social/reads.rs` | 授权读取、同一事务的初始化概览与上下文材料、运行详情 |
| `crates/storage/src/social/schema-v2.sql` | 独立 run_details 表，保存实际模型、思考、用量、耗时及错误 |
| `crates/web-server/src/social_api.rs` | 命令、分页历史、持久事件回放与 SSE 订阅 |
| `frontends/tauri/src-tauri/src/social_commands.rs` | 同一 DTO 与 Feature 方法的基础 IPC 命令；本批未接实时订阅 |
| `frontends/web/src/social/App.tsx` | 真实消息/通讯录、联系人编辑、私聊、详情与既有设置/Markdown 复用 |
| `frontends/web/src/social/client.ts` / `live-store.ts` | HTTP/SSE、回执、独立草稿、游标、生成、历史合并与重连 |

Runtime 不依赖具体 SocialFeature。迁移期旧 ChatFeature 仍服务桌面、CLI/TUI 与旧 HTTP；新私聊不调用其“保存 user 并生成”的方法，不把 Session 当作 Conversation。新代码只实现当前用例所需能力，输入/权限/存储/协议边界校验，内部依赖明确不变量，不添加备用模型或模拟降级。

## 身份、发送和执行

联系人档案与 ExecutionBinding 分开保存，绑定严格解析 provider、模型和思考强度。未绑定可以创建联系人；用户指定不存在的模型返回错误且不创建联系人，不替换为其他模型。修改档案使用 expectedRevision；成功后保持 ActorId、关系和原私聊 ID。

真人消息独立提交，后端事务分配 sequence；发送回执表示消息已保存。M0 的 AppendMessage 调整为可选 expectedRevision，普通追加不因同时生成导致的空间 revision 变化而拒绝。需要乐观锁的调用仍可提供明确版本，旧回执格式保持可读。

没有模型、绑定失效或暂停时，真人消息仍保存，并通过 executionNotice 告知没有启动生成。模型错误、预算不足等后续失败不撤销真人消息。页面不预建 AI 回答，实际 begin_run 提交后才读取 AI 消息。生成不持有存储事务或发送锁，所以用户可以继续输入。

同一私聊最多一个生成。重复发送使用原 requestId，返回同一消息；执行检查持久 begin_run 回执，不再次派发已经开始的运行。首批采用真人触发消息 UUID 作为 begin_run 请求 ID，只适用于一个私聊 AI；第二批已用独立持久 ParticipationPlan 替换，按触发消息与目标 AI 区分并固定顺序。

首批等待执行的任务在进程内，当时尚无持久 ParticipationPlan、合并等待窗口或严格多目标 FIFO。重启恢复已开始运行为 Interrupted，不自动重放模型调用，也不自动重派尚未开始的回复。离线验收中的重复请求仅证明本地幂等，不保证外部调用 exactly-once。

## 本批意识系统

每次请求只读本私聊的近期原文，目标为 ActorId + ConversationId。读取范围为真人与目标 AI 都可见的历史，成员身份校验通过后，截至这次触发的真人消息；最多读取最近 200 条 committed 消息，按时间序列排列。中断片段、展示用思考、用量与计时不进入下次模型请求，其他私聊也不进入。

角色名字和人设形成 system 消息；历史保留发送者名称和时间，目标 AI 的发言转换为 assistant，真人发言转换为 user。provider 请求每次重新构造，不共享永久续接窗口。

文本预算暂用 UTF-8 字节数加每条 16 的开销估计，尚非模型 tokenizer。采用模型 context_window；未声明时暂按 8192。输出预留为 min(模型 max_tokens 或 1024、1024、窗口的四分之一)。先计入身份设定，再从最新消息向前选择完整消息，按原顺序发给模型；最新消息或人设放不下时明确提示预算不足，不截断正文。

裁剪只影响本次请求，SQLite 仍完整保存窗口记录，界面可按每页 200 条继续读取历史。当前没有摘要、长期记忆或跨窗口接续，详情明确说明此限制。

来源快照保存入选消息的 ID/revision、档案/绑定/关系版本、成员权限依赖及空间 epoch。开始与成功收尾时由存储复核。人设、绑定或来源在生成中变化，旧回答不能提交为成功，保留为带 context_changed 的中断记录。模型解析和实际派发使用同一 AiClient 配置快照。

## 持久化、恢复和订阅

GUI 宿主显式调用 Runtime::from_config_social_persistent，在原配置与目录缓存路径上注入 SQLite 和时钟。JSON/SQLite 共享一个 DataDirectoryGuard，仍只有一个独立 Runtime 可以写此目录。CLI/TUI 保持原构造路径。

SQLite v1 的 schema 内容与校验值不改写。v1 → v2 迁移先一致备份，再在事务中创建 run_details、更新迁移记录与版本；初始化检查不支持的新版本及迁移校验。旧 JSON 不自动导入；当前 GUI 宿主仍加载旧 SessionManager 供保留的 Harness 接口使用，后续兼容收尾需移除该依赖。这不使旧 JSON 成为社交业务的权威数据。

begin/checkpoint/finish 保存对应运行详情；500ms 周期仅在片段有变化时保存正文与思考。成功回答、Run 终态、详情、持久事件引用同事务。EOF 有明确的 UnexpectedEof 协议错误类型，保存为 Interrupted；provider 错误、空回答或未授权工具调用保存为失败，正文片段显示为中断记录。首批不声明工具，不执行模型意外返回的 ToolCall。

首批用户取消由运行任务完成中断保存；Web 退出时停止 SSE，Feature 取消并等待任务收尾，随后排空 writer。基础取消已经验证，完整暂停/结束及取消与完成事务竞争仍属于第三批。崩溃后只恢复已落盘检查点，不续传 provider token 游标。

持久 SocialEvent 只携带引用，不携带正文；HTTP 每 250ms 读取提交后的变更，单批最多 500 条。订阅先监听临时广播，再读取历史引用，回放通过 SSE change/id 传输；generation 传递正文/思考 delta，广播溢出要求 resync。订阅断开不取消模型任务。

客户端状态应用成功后才推进 cursor，失败重连补齐；同一消息按 ID/revision 合并，终态覆盖旧草稿。临时流只从 deltaSeq=1 连续拼接，缺片段就放弃不完整缓冲，并读取已持久化草稿/终态；活跃窗口重连后的运行草稿每 500ms 刷新。持久日志当前不压缩，不宣称已实现游标过期机制。

## 本批接口与适配边界

首批已接入联系人、唯一私聊、分页消息、运行详情/取消与 HTTP 回放/SSE。完整当前路由、命令、字段和错误体只在 [API 与事件](07-api-and-events.md) 维护；不按历史报告调用已改动的接口。

本批 HTTP 使用原 Host/Origin/本地客户端 Header 保护；业务错误为 code/message/retryable，存储错误不向浏览器暴露私人路径。Axum 的格式错误仍使用框架拒绝响应。基础 Tauri 命令通过编译，但本批没有实时订阅、社交桌面 GUI、退出或安装验收。

## Web 与本地复现

普通 Web 默认是真实 SocialApp；`?legacy=1`/`?demo=1` 为迁移期旧入口，`dev:social`/`?social-preview=1` 为独立 U0 模拟预览。真实页面复用基础 UI、Markdown、主题和设置，联系人可配置模型/思考强度，聊天与联系人详情展示当前意识机制，单次回复详情读取实际模型、用量与思考。

真实界面设置目前开放模型服务、外观、配置和关于。旧全局生成参数不会影响联系人请求，因此本批不展示该控制；联系人 temperature/max_tokens 的独立配置后续沿 ExecutionBinding 接入。当前输出预留按上面的意识预算规则计算。

浏览器只用 llm-nest-social-live-drafts-v1 保存草稿与重试 ID，数据库是记录的权威来源。丢失回执时保留 requestId 重试；迟到回执不清空新输入；已保存回执不因后续刷新失败而撤销。

隔离验收的构建、--binary/--dist/--serve 和 STOP 用法统一见 [开发指南](../development.md#运行入口)。脚本使用合成角色、临时库和本机模拟 provider，不复制个人数据，也不调用付费模型。

## 本轮验证

| 验证 | 结果与范围 |
| --- | --- |
| Windows MSVC Rust 受影响包 | ai-client、chat、events、runtime、social-feature、storage、web-server 共 193 项测试通过 |
| Linux/WSL Ubuntu Rust 受影响包 | 对应 193 项测试通过；web-server 单独验证通过 |
| 两平台严格 clippy | social-feature/storage/events 的 all-targets、no-deps、-D warnings 通过 |
| 扩展 clippy | runtime/ai-client/web-server 常规检查通过；既有代码有样式告警，扩展 -D warnings 未通过，未混入无关清理 |
| 前端 | typecheck、旧回归、U0 预览和 live-social.cjs 通过；build:social-runtime 通过，保留原有富文本大块产物提示 |
| 两平台真实 HTTP + 模拟 provider | scripts/social_acceptance.py 通过：唯一私聊、严格绑定、重复回执/单次 dispatch、上下文隔离、思考剥离、连续输入/取消、provider/EOF/工具拒绝、来源变化、保存失败重试、断订阅不取消、杀进程/幂等恢复与不重放 |
| 原 Harness 离线验收 | Linux scripts/acceptance.py 通过：配置、工具 worker、HTTP、JSON 历史和错误恢复 |
| 浏览器实际页面 | Chrome 本地隔离库创建联系人、进入私聊、发送并读取回答、刷新恢复记录/草稿、设置往返、意识详情与模型/用量/思考展示通过 |
| Tauri | Windows MSVC cargo check 通过；未做社交 GUI、打包或安装验收 |

另有文本预算单测与 v1 → v2 数据/备份迁移单测。所有新 fixture 使用合成角色与临时数据。原有个人模型目录改动保留在工作区，不纳入本批提交；未读取个人配置或会话用于验收。

本批后续已经完成群聊与消息控制；历史记录不继续维护未来计划，按 [TODO](TODO.md) 与状态文件接续。
