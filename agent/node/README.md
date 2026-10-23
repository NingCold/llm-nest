# 独立 Agent 节点

在仓库根目录运行：

```powershell
cargo run -p agent-node -- init C:\private\llmn-agent
cargo run -p agent-node -- pair C:\private\llmn-agent
cargo run -p agent-node -- run C:\private\llmn-agent
```

初始化后编辑 `node.toml` 的 `server`；使用私有 CA 时在文件顶层加入 `ca_file = "C:/private/ca.pem"`。HTTPS 与 WSS 都验证证书，仅本机开发允许 HTTP。把模型 Key 留在 `API_KEY_CHATECNU` 环境变量；配置仅接受环境变量引用。初始化目录设置当前用户私有权限，SQLite 中的节点凭据不可分享、提交或打包。

`pair` 显示一次性配对码，在“我的账号 → 我管理的 AI”选择 AI 授权。重复运行 `pair` 可绑定其他 AI，同一目录的所有 AI 由 `run` 运行。中断的待授权配对会继续使用原码，过期自动申请新码。

`node.toml` 的 `providers` 使用 `ai-client` 的供应商与模型配置格式，模型名称和推理强度严格校验。`profile` 是新绑定 AI 的模板，修改模板不会覆盖已经保存的 AI。每个 AI 的角色与模型参数分别保存，更新时提供 JSON：

```json
{
  "revision": "2",
  "persona": "你是一位友善、诚实的 AI 联系人。",
  "selection": { "provider": "chatecnu", "model": "ecnu-max" },
  "temperature": null,
  "max_tokens": null,
  "auto_daily_tokens": 262144
}
```

```powershell
# 先停止使用该目录的 run；AI id 来自云端账号资料。
cargo run -p agent-node -- profile C:\private\llmn-agent <AI-ID> C:\private\profile.json
```

修改时提升 `revision`；下一次启动与新任务使用保存后的设置。空温度和输出上限使用模型默认温度与意识系统预算。运行期间配置冻结，不在飞行请求中切换供应商。

节点为每个绑定保持 WSS 连接，所有 AI 共享 4 个社交模型调用槽，摘要独立使用 1 个槽；同一 AI 仍按顺序执行；单次调用最长 120 秒。每日次数和完整上下文 token 容量先在 SQLite 预留，已开始后的失败不返还；墙钟回拨不重开旧日预算。模型调用前先保存开始意图、获得云端确认，再保存确认。重启遇到不明确的开始只报告 uncertain，绝不重调模型；已保存 final 可原样重发。节点断线会停止生成，源编辑、取消或撤权由服务端复核并阻止失效结果提交。

节点协议为 v4：材料包含真实窗口种类、名称、版本和当前成员，以及每条消息的实际发送者、回复目标和结构化提及。各次请求按目标 AI 与窗口独立构造，不把所有其他成员都当成一个私聊对方。云端按持久任务顺序串行同群发言，并将前序 AI 已提交回答提供给后续任务；节点只使用本次取得的冻结材料。

正常排队不算模型失败。云端任务列表只提供当前可开始的队首；若开始时明确收到 HTTP 409 `conversation_busy` 或 `task_not_ready`，节点在一个事务中撤销本次尚未开始的意图与预算预留，释放模型槽，等待通知或下一轮任务读取。此例外不适用于未知网络失败或任何已开始调用，原有不重复调用和预算不退还规则继续生效。

记忆与窗口摘要按 AI 分开保存在 `node.sqlite`。节点在线时，可从聊天或联系人详情维护显式记忆；AI 管理员还能为它在当前窗口选择专用模型，开启或手动整理摘要。摘要默认关闭，使用同一节点预算，最近 12 条原文保留，每段最多 32 条。群话题带入私聊时，节点读取获准引用的原文，并结合该 AI 自己的资料。完整操作见[使用指南](../../docs/usage.md#记忆摘要和群话题接续)。

编辑、遗忘和来源变化会停止依赖旧资料的回复。节点重连先补齐来源变化，再使用记忆和摘要；未完成摘要在重启后标为中断，不自动重复调用模型。更换运行设备仍需单独迁移这些资料，当前没有迁移包入口。

普通文件只进入名称、类型和大小说明；图片经授权接口读取后作为原生图片材料，明确不支持图片的模型在调用前拒绝。当前没有工具执行；角色和模型参数可以由上述本机档案命令或在线管理入口调整。

图片下载前先按元信息选材：保留所有合并输入、触发消息和被回复原文的必要图片，再从最近历史向前选取，最多 16 张、16 MiB。超过剩余预算的普通历史消息整条略去，保留的消息不单独丢图；必要图片超限会明确失败。实际下载长度还要匹配元信息。准备材料与下载期间继续处理 WSS 心跳和取消。

本地无付费测试：`cargo test -p agent-node --locked`。

自动接话与一次性约定另外按每个 AI、窗口、UTC 日预留完整模型容量，`profile.auto_daily_tokens` 可省略（默认 262144），范围 1024–4000000；该 AI 的 JSON 档案可独立调整。明确点名不扣这份自动额度，所有调用仍受节点顶层 `daily_calls/daily_tokens` 约束。两份额度与开始意图在同一 SQLite 事务预留；仅明确未开始的队列 409 同时释放它们，开始后失败、取消和重启不返还。预算拒绝区分 `token_limit`（窗口自动额度）、`node_call_limit`、`node_token_limit` 和 `budget_clock_rollback`。

消息触发的 `inputIds` 均为必要材料，容量不足时明确失败。约定使用独立触发来源，把授权内容作为一次性约定提供给模型，可在聊天历史为空时执行；不会虚构真人消息，也不沿用共同群引用，当前窗口的有效记忆与摘要照常参与选材。

在线配置管理通过 v4 `Hello.capabilities = ["profileManagement"]` 声明。云端仅转发角色、节点已经配置的模型选项及生成参数，响应不包含模型地址、Key 或环境变量名。管理员的写入由节点校验完整模型选择与参数，按预期版本原子保存；相同 requestId 重试只读取当前档案，回执仅存指纹。保存后的新任务使用新档案，已开始任务保持冻结设置。供应商和 Key 仍只能在节点本机配置。

桌面使用 `pair-json <directory>` 获取 JSON 行：`{type:"pairing",pairingId,userCode,expiresAt}`，授权并持久保存后输出 `{type:"paired",aiId,bindingVersion}`；凭据不输出。`check <directory>` 无网络校验模板及模型参数，输出 `{type:"checked",models:[...]}`，不读取 Key 值。

桌面托管使用 `run-managed <directory>`。取得目录锁、校验并准备完成后，输出 `{type:"ready",instanceId}`；调用方收到这行确认后再查询状态。`status <directory>` 输出 `{type:"status",running,instanceId,aiIds}`，未运行时 instanceId 为 null，aiIds 只读查询已配对账号。`stop <directory>` 请求停止当前托管实例，随后可用 status 确认退出；普通命令行 run 不接受这种停止。关闭桌面不影响独立托管进程，系统休眠或关机会使本机 AI 离线。
