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
  "max_tokens": null
}
```

```powershell
# 先停止使用该目录的 run；AI id 来自云端账号资料。
cargo run -p agent-node -- profile C:\private\llmn-agent <AI-ID> C:\private\profile.json
```

修改时提升 `revision`；下一次启动与新任务使用保存后的设置。空温度和输出上限使用模型默认温度与意识系统预算。运行期间配置冻结，不在飞行请求中切换供应商。

节点为每个绑定保持 WSS 连接，所有 AI 共享一个模型调用槽；单次调用最长 120 秒。每日次数和完整上下文 token 容量先在 SQLite 预留，已开始后的失败不返还；墙钟回拨不重开旧日预算。模型调用前先保存开始意图、获得云端确认，再保存确认。重启遇到不明确的开始只报告 uncertain，绝不重调模型；已保存 final 可原样重发。节点断线会停止生成，源编辑、取消或撤权由服务端复核并阻止失效结果提交。

节点协议为 v2：材料包含真实窗口种类、名称、版本和当前成员，以及每条消息的实际发送者、回复目标和结构化提及。各次请求按目标 AI 与窗口独立构造，不把所有其他成员都当成一个私聊对方。云端按持久任务顺序串行同群发言，并将前序 AI 已提交回答提供给后续任务；节点只使用本次取得的冻结材料。

正常排队不算模型失败。云端任务列表只提供当前可开始的队首；若开始时明确收到 HTTP 409 `conversation_busy` 或 `task_not_ready`，节点在一个事务中撤销本次尚未开始的意图与预算预留，释放模型槽，等待通知或下一轮任务读取。此例外不适用于未知网络失败或任何已开始调用，原有不重复调用和预算不退还规则继续生效。

普通文件只进入名称、类型和大小说明；图片经授权接口读取后作为原生图片材料，明确不支持图片的模型在调用前拒绝。当前没有工具执行和在线配置转发。

图片下载前先按元信息选材：保留触发消息和被回复原文的必要图片，再从最近历史向前选取，最多 16 张、16 MiB。超过剩余预算的普通历史消息整条略去，保留的消息不单独丢图；必要图片超限会明确失败。实际下载长度还要匹配元信息。准备材料与下载期间继续处理 WSS 心跳和取消。

本地无付费测试：`cargo test -p agent-node --locked`。
