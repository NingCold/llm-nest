# 自带 Agent

[项目首页](../README.md) · [接入计划](../docs/project/cloud-plan.md)

这里放 AI 如何准备上下文、连接模型和生成回复的代码。

- [core](core/README.md)：选取聊天、记忆和摘要，组织一次回复所需的材料和预算。
- [ai-client](ai-client/README.md)：连接模型服务，处理文字、图片和流式回复。
- [tools](tools/README.md)：已有的工具执行基础，当前聊天尚未开放工具。
- [node](node/README.md)：独立节点的配对、运行与每个 AI 的角色、模型配置。

`node` 提供独立的 `llmn-agent` 程序，连接云端账号并调用上述库。它不启动旧本地 Runtime；角色、模型参数、节点凭据、每日预算和执行检查点保存在独立 SQLite 中。一个节点目录可以配对多个 AI，模型调用全局串行。

```powershell
cargo run -p agent-node -- init C:\private\llmn-agent
# 编辑该目录的 node.toml：设置 server，私有 CA 可填 ca_file。
cargo run -p agent-node -- pair C:\private\llmn-agent
# 在“我的账号 → 我管理的 AI”中授权屏幕显示的配对码。
cargo run -p agent-node -- run C:\private\llmn-agent
```

配置默认引用 `API_KEY_CHATECNU` 环境变量，不在文件中填写明文 Key。目录包含节点凭据，应保留初始化设置的私有权限，不提交、分享或打包。服务器必须使用 HTTPS/WSS；只有本机开发允许 HTTP。私有 CA 同时用于 HTTPS 与 WSS 验证。

每次调用先持久预留当天次数和完整上下文 token 容量，再取得云端开始确认，最长运行 120 秒。断开或取消会停止生成；重启只提交保存的结果或报告不确定状态，不重新调用模型。已预留预算不退还。普通附件只提供名称、类型和大小；图片通过有权限的节点接口下载并作为图片输入。

在仓库根目录检查核心：

```bash
cargo test -p builtin-agent -p ai-client --locked
```
