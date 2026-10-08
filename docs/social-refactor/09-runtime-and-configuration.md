# 09 运行时与配置

[总纲](README.md) · [AI 执行](05-ai-execution.md) · [存储与迁移](06-storage-and-migration.md)

状态：目标设计，尚未实现。

## 1. Runtime 的职责

Runtime 保持共享宿主，负责配置快照、资源注入、Feature 生命周期和关闭。SocialFeature 实现身份、聊天、记忆、参与与调度用例；Runtime 不堆积前端专用的业务条件。

扩展 FeatureContext 注入 SocialStore、业务变更广播、临时生成广播、Clock 与运行资源限额；保留 AiClient、ToolRegistry 和迁移期 SessionManager。具体类型以实施时最小可用接口为准，不为预留功能增加空框架。

Feature trait 维持生命周期与 Arc downcast；SocialFeature 的 create_ai、send_message、forget_memory 等作为具体方法暴露。Runtime 注册 Feature 时只依赖 trait，不能反向依赖 `social-feature` Cargo 包。

## 2. 启动顺序

1. 所有宿主先处理 `--llmn-tool-worker`，在配置、Key、日志、UI 和存储初始化前退出 worker 路径。
2. 准备实际数据目录，按原规则处理旧默认目录复制，解析并冻结 config/data 路径。
3. 按进程环境 > 配置目录 > 默认数据目录加载缺失环境变量；读取配置与目录缓存，整体校验。
4. 取得统一 DataDirectoryGuard，打开 SocialStore；检查 schema 与旧 JSON 导入状态。
5. 恢复 running 检查点、悬挂预算和意图状态，不重放已 dispatch 请求或工具。
6. 构造 Runtime 资源，初始化 SocialFeature；启动事件消费与调度，读取已提交位置。
7. 暴露 init 快照与业务入口。存储和历史恢复未成功时不能发送或启动自动任务。

无 providers 仍是有效首次启动状态：可以配置联系人草稿和设置，无法执行 AI。目标版本将消息保存与模型可用性分开，切换时同步修改当前 GUI 发送约束。

## 3. 配置分工

| 内容 | 权威位置 | 修改边界 |
| --- | --- | --- |
| providers、目录、凭据引用 | 现有 TOML/环境配置与目录缓存 | Runtime 校验与原子保存 |
| AI 档案、关系、群参与、联系约定 | SocialStore | 社交用例事务 |
| ExecutionBinding | SocialStore，引用 provider/model | Runtime 解析校验后保存 |
| 默认新 AI 执行配置 | GUI 设置，沿用可迁移的全局默认语义 | 不覆盖已存在 AI 的绑定 |
| 主题与本地布局 | 现有前端外观设置 | 不影响后端权限 |

不把人设、记忆或群成员放进 TOML，不把 Key 放进身份数据库。全局默认只影响新建或明确继承默认的配置；切换联系人、加载历史不能覆盖用户保存的默认值。

思考强度、模型名和 provider 继续严格校验。ExecutionBinding 是中立描述，运行时转换为 ai-client::ModelSelection 与 GenerationOptions；协议差异不进入社交业务层。

## 4. 快照与热更新

一次 run 固定 provider 配置、目录与协议路由快照，并记录档案、绑定和上下文版本。热更新整体校验成功后发布新版本，失败保留原版本；在飞请求继续原模型配置。

暂停、成员退出、权限收回和遗忘是可撤销业务状态，必须在执行前及最终提交时复核，不能因固定快照忽略撤销。

provider 移除或目录更新后，重新评估依赖的 AI 配置和待执行计划。已有 run 不因普通目录更新静默换路由；用户要求撤销时显式取消。失效绑定标记需配置，不能把联系人自动映射到别的模型。

目录同步仍使用 Pi 公共目录适配、Runtime 校验、原子缓存和用户字段优先级。所有配置重建路径合并缓存；不重写源码/整份配置、不导入远端凭据和端点、不改变默认模型。

## 5. 生命周期与关闭

SocialFeature.initialize 恢复消费位置并启动受控后台任务；重复初始化不能产生多个调度器。Clock 与 mock provider 可注入，业务代码不自行创建真实凭据或新的独立 Runtime。

shutdown 顺序：停止接受新执行 → 停止调度领取 → 取消活动运行 → 等待中断保存与工具退出 → 刷新状态并释放事件资源 → 关闭数据库 → 释放目录锁。

关闭采用有界等待并暴露保存失败。异常退出或超时留下的 running 状态在下次启动恢复为 interrupted；不能通过界面假装关闭前全部完成。宿主进程 worker 的 kill_on_drop/Job Object 行为继续验证。

订阅者数量和页面可见性不控制 Runtime 生存期。窗口、浏览器标签或事件监听消失不会自动取消其他空间的运行。

## 6. CLI/TUI 与兼容

M0-M1 旧 CLI/TUI 仍可使用旧接口，但它们与新 Runtime 不能同时打开同一目录。兼容读取器与新存储在同一 Runtime 中共享目录锁，不建立第二个 writer。

新 GUI 切换后，旧 chat 输入通过明确的默认 AI/私聊映射调用社交用例；不长期保留两套记忆与发送逻辑。无法表达的群聊或主动联系功能不伪装为旧终端能力。

兼容层只在边界转换 DTO；旧 SessionManager 后续主要服务历史导入和旧记录查询。清除它之前先核对终端、脚本、HTTP 和 IPC 调用链，并决定旧接口退出窗口。

## 7. 可观测性

日志记录事件、ActorId、ConversationId、RunId、状态、错误分类和用量，默认不记录正文、记忆、完整请求、环境变量值或凭据。调试请求正文需要用户明确开启且限定保存范围。

错误提示引用实际冻结路径与可恢复动作，但不把私人配置或目录内容发送远端。日志和调度统计是运行证据，不能代替持久消息或声称完整任务审计已实现。

## 8. 验收

1. worker 入口仍早于配置与存储初始化，所有宿主均覆盖。
2. 初始化失败能重试，重试不创建重复调度器或漏释放目录锁。
3. 热更新与 run 并发时模型路由一致，撤销状态仍及时生效。
4. 空配置、失效绑定、目录缓存失败与 provider 移除有明确状态。
5. 正常关闭、强制终止、磁盘失败后的恢复行为可复现。
6. CLI/TUI 兼容通过相同业务逻辑，平台 IPC 不回退到 HTTP 或 demo。
