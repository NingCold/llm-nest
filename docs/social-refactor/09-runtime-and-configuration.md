# 09 运行时与配置

[总纲](overview.md) · [AI 执行](05-ai-execution.md) · [存储与迁移](06-storage-and-migration.md)

本文维护所属模块的规则与扩展目标。 实施状态见 [已完成](completed.md) / [尚未完成](pending.md)，当前调用契约见 [API 与事件](07-api-and-events.md)。

## 1. Runtime 的职责

Runtime 保持共享宿主，负责配置快照、资源注入、Feature 生命周期和关闭。SocialFeature 实现身份、聊天、意识系统（Consciousness）、参与与调度用例；Runtime 不堆积前端专用的业务条件。ConsciousnessService 由 SocialFeature 创建和持有，使用注入的同一 SocialStore、AiClient、Clock 与预算资源，不成为独立 Runtime 或 Feature。

FeatureContext 必需注入 AiClient 与 SocialResources（SocialStore/Clock）。参与队列、并发限制和临时生成广播由 SocialFeature 自己维护，持久事件来自 SocialStore。SessionManager、旧 RuntimeEvent 与工具注册不再属于 Runtime 业务上下文；底层工具库保留，但社交尚未启用。

共享 Runtime 遵守 [新架构优先与兼容边界](overview.md#新架构优先与兼容边界)。生产宿主统一 `Runtime::open(config_path, data_dir)`，直接持有 SQLite 与模型快照；确定性测试通过 `Runtime::with_resources` 注入独立资源，不建立旧会话运行时。

Feature trait 维持生命周期与 Arc downcast；SocialFeature 的 create_ai、send_message、forget_memory 等作为具体方法暴露。Runtime 注册 Feature 时只依赖 trait，不能反向依赖 `social-feature` Cargo 包。

## 2. 启动顺序

1. 所有宿主先处理 `--llmn-tool-worker`，在配置、Key、日志、UI 和存储初始化前退出 worker 路径。
2. 准备实际数据目录，按原规则处理旧默认目录复制，解析并冻结 config/data 路径。
3. 按进程环境 > 配置目录 > 默认数据目录加载缺失环境变量；读取配置与目录缓存，整体校验。
4. 取得统一 DataDirectoryGuard，打开 SocialStore；检查 schema，不加载或改写旧 JSON。
5. 恢复 running 检查点为 interrupted，保留已扣减的自动额度，不重放已开始请求；对应 executing 联系恢复为 uncertain，已过期则 expired，均不重发。
6. 构造 Runtime 资源，初始化 SocialFeature；重新检查 queued 计划、open Bout 与 pending/claimed 约定。每窗口维护一个可唤醒的调度任务，Clock.wait_until 与退出取消并列等待；全局社交并发 4，摘要独立并发 1。一次性约定到期仍复用参与/执行队列，系统时钟等待最多每 30 秒复核 UTC，ManualClock 直接通知；应用退出不留下系统定时任务或后台模型调用。
7. 暴露 init 快照与业务入口。存储和历史恢复未成功时不能发送或启动自动任务。

空文件、仅注释或空 providers 都是有效首次启动状态：可以配置联系人和设置，无法执行 AI。省略 providers 反序列化为空集合，非法语法或类型仍报错，不在解析失败时伪造配置。当前社交私聊已经把真人消息保存与模型可用性分开。

桌面 Runtime 嵌入应用进程，由首次 IPC 初始化请求启动，无需单独启动 Web Server。窗口先展示空白启动层，初始化成功或报错后显示界面；启动失败与订阅重连分别提示，保留具体错误、重试与设置入口。实现与本机验证见 [桌面启动记录](m1-desktop-surface.md)。

## 3. 配置分工

| 内容 | 权威位置 | 修改边界 |
| --- | --- | --- |
| providers、目录、凭据引用 | 现有 TOML/环境配置与目录缓存 | Runtime 校验与原子保存 |
| 公开账号资料、AI 内部档案、通讯录、管理员授权、关系、群参与、联系约定 | SocialStore | 社交用例事务 |
| 意识系统记忆、摘要、来源与快照 | SocialStore；原消息/档案仍是各自权威来源 | 意识系统用例与来源版本校验 |
| 接话节奏与自动额度 | SocialStore 的 ParticipationPolicy/State 和每日额度账本；默认群聊自动接话关闭 | 成员经共享用例显式配置；自动运行同事务预留，重启不返还 |
| 窗口摘要的模型与预算 | SocialStore 的 SummarySettings；默认关闭，成员显式选择独立模型/思考强度/输出上限 | SocialFeature 复用 AiClient 校验，任务使用配置快照；未来提取/巩固配置另行细化 |
| ExecutionBinding | SocialStore，引用 provider/model，保存思考强度、temperature/max_tokens | 获授权管理员经 AI 角色管理用例保存，复用 AiClient 协议约束与模型容量校验 |
| 主题与本地布局 | 现有前端外观设置 | 不影响后端权限 |

GUI 的实际操作者固定为后端的本地真人。账号查看只传被查看的 AI ID；SocialFeature 每次检查管理员授权，存储按该 AI 自身成员关系读取，不替换全局操作者、不连接全局事件流到账号视角。账号查看当前是可手动刷新的只读快照，远程认证与多人联网未实现。

不把人设、记忆或群成员放进 TOML，不把 Key 放进身份数据库。全局生成设置已删除，旧 `[gui]` 只在原配置中保留，不提供运行读写接口；每个 AI 的参数只来自独立 ExecutionBinding。后续若需要新账号默认值，再由明确的创建用例复制，不设隐式全局继承。

思考强度、模型名和 provider 继续严格校验。ExecutionBinding 是中立描述，运行时转换为 ai-client::ModelSelection 与 GenerationOptions；协议差异不进入社交业务层。

## 4. 快照与热更新

一次 run 固定 provider 配置、目录、协议路由和内部设定快照，并记录档案、绑定和上下文版本。档案/绑定版本在启动事务复核；普通设定或模型修改不使已开始运行失效，最终提交仍检查消息来源与参与状态。热更新整体校验成功后发布新版本，失败保留原版本；在飞请求继续原模型配置。

跨窗口引用读取仍使用私聊 AI 的绑定；群摘要只是已显式选中的背景，不触发另一模型调用。快照固定实际使用的群原文、摘要、关联私聊消息版本与双方源群成员依赖，只有这些引用具备跨窗口读取依据。

暂停、结束 Bout、归档、成员退出、权限收回和遗忘是可撤销业务状态，必须在执行前及最终提交时复核，不能因固定快照忽略撤销。

当前每次计划开始时重新解析绑定，失效配置终结对应计划。后续 provider 移除或目录更新的配置用例可主动重新评估依赖 AI 与待办。已有 run 不因普通目录更新静默换路由；用户要求撤销时显式取消。失效绑定返回需配置的执行提示，不能把联系人自动映射到别的模型；当前没有持久化可用性标记。

目录同步仍使用 Pi 公共目录适配、Runtime 校验、原子缓存和用户字段优先级。所有配置重建路径合并缓存；不重写源码/整份配置、不导入远端凭据和端点、不改变默认模型。

模型图片能力为支持/不支持/未知，按精确 provider/model 采用人工 image_input > 同步目录 > 内置声明 > 未知。目录的输入模态缺失时不推断为不支持；同步覆盖放在内部 catalog_image_input，不序列化成人工配置。Runtime::set_model_image_input 持有配置更新锁，解析当前模型、定点修改 TOML、整体校验、原子保存后再发布；null 只移除人工字段。既有 run 的 AiClient 快照不会改变，新运行在分发前检查实际图片。配置与接口见[图片能力记录](image-capabilities.md)。

## 5. 生命周期与关闭

当前 SocialFeature.initialize 恢复运行并继续持久 queued 计划、Bout 与有效约定调度；重复初始化不能产生多个调度器。Clock 与 mock provider 可注入，业务代码不自行创建真实凭据或新的独立 Runtime。

M1 初始化意识系统基本上下文服务；M2 第二批已接入按窗口显式启用的摘要任务，后续提取/巩固不因初始化自动启用。后台任务使用明确模型绑定与独立调用预算，摘要并发为 1，每次唤醒最多 4 段、单次 120 秒；没有配置或输入预算不足时显示可恢复状态，不隐式调用另一个付费模型。

shutdown 顺序：停止接受新执行 → 停止参与调度和意识后台任务领取 → 取消活动运行与后台任务 → 等待中断保存与工具退出 → 刷新状态并释放事件资源 → 关闭数据库 → 释放目录锁。摘要未完成时可以依据来源重新评估，不能在重启时无条件重放已 dispatch 的外部调用；新任务仍受预算与版本约束。

关闭采用有界等待并暴露保存失败。异常退出或超时留下的 running 状态在下次启动恢复为 interrupted；不能通过界面假装关闭前全部完成。宿主进程 worker 的 kill_on_drop/Job Object 行为继续验证。

订阅者数量和页面可见性不控制 Runtime 生存期。窗口、浏览器标签或事件监听消失不会自动取消其他空间的运行。

当前 Tauri 主窗口关闭先向已注册前端请求草稿保存确认，最多等 10 秒；前端冻结编辑、等待正在读取的图片和 IndexedDB 事务完成，失败保留窗口与可用后端。匹配当前页面和关闭请求的成功确认后，才实现上述收尾：停止接收新命令、释放并等待 IPC 订阅任务、等待已经开始的初始化、调用 Runtime::shutdown_features（含 writer 排空），成功才退出。20 秒超时或保存失败通过 backend-shutdown-failed 提示并保留窗口，允许再次关闭重试；不创建后台常驻服务。

## 6. 旧版退役与历史边界

旧 CLI/TUI、ChatFeature、SessionManager、JSON 写存储、旧 HTTP/chat-event 和全局生成设置已移除。Web 与桌面只走 SocialFeature。

历史 JSON DTO 独立在 `storage::legacy`，导入通过 SocialFeature → SocialStore 执行；复用数据目录锁和 writer，不接受任意路径或客户端操作者。设置页提供显式所选导入，重复导入幂等，失败不改原文件，导入结果在新版只读查看。

## 7. 可观测性

日志记录事件、ActorId、ConversationId、RunId、状态、错误分类和用量，默认不记录正文、记忆、完整请求、环境变量值或凭据。调试请求正文需要用户明确开启且限定保存范围。

错误提示引用实际冻结路径与可恢复动作，但不把私人配置或目录内容发送远端。日志和调度统计是运行证据，不能代替持久消息或声称完整任务审计已实现。

## 8. 验收

1. worker 入口仍早于配置与存储初始化，所有宿主均覆盖。
2. 初始化失败能重试，重试不创建重复调度器或漏释放目录锁。
3. 热更新与 run 并发时模型路由一致，撤销状态仍及时生效。
4. 空配置、失效绑定、目录缓存失败与 provider 移除有明确状态。
5. 正常关闭、强制终止、磁盘失败后的恢复行为可复现。
6. 旧 HTTP 返回 404、旧 IPC 未注册，旧查询参数仍进入新版；桌面不回退到 HTTP 或模拟模式。
