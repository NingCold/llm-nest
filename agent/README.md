# 自带 Agent

加密聊天由官方 Matrix SDK 处理 Olm/Megolm，消息强制核验设备与账号 master 双签。节点解密后复用上下文与模型调用；角色、模型、记忆和每 AI 摘要以分范围密文资源同步。设备授权与恢复见[节点说明](node/README.md)，协议见[加密契约](../docs/design/crypto-contract.md)。

[项目首页](../README.md) · [接入计划](../docs/project/cloud-plan.md)

这里放 AI 如何准备上下文、连接模型和生成回复的代码。

- [core](core/README.md)：选取聊天、记忆和摘要，组织一次回复所需的材料和预算。
- [ai-client](ai-client/README.md)：连接模型服务，处理文字、图片和流式回复。
- [crypto](crypto/README.md)：官方 Matrix SDK 与节点密钥库。
- [node](node/README.md)：独立节点的配对、运行与每个 AI 的角色、模型配置。

`node` 提供独立的 `llmn-agent` 程序，连接云端账号并调用上述库。角色、模型参数、记忆和摘要保存在运行设备上，模型 Key 不交给社交服务器。一个节点目录可以配对多个 AI，同时最多处理四个聊天回复，摘要单独串行处理。

桌面应用可在“我管理的 AI → 在这台电脑上运行”配置并启动它，也可以从命令行独立运行：

```powershell
cargo run -p agent-node -- init C:\private\llmn-agent
# 编辑该目录的 node.toml：设置 server，私有 CA 可填 ca_file。
cargo run -p agent-node -- pair C:\private\llmn-agent
# 在“我的账号 → 我管理的 AI”中授权屏幕显示的配对码。
cargo run -p agent-node -- run C:\private\llmn-agent
```

配置默认引用 `API_KEY_CHATECNU` 环境变量，不在文件中填写明文 Key。目录包含节点凭据，应保留初始化设置的私有权限，不提交、分享或打包。服务器必须使用 HTTPS/WSS；只有本机开发允许 HTTP。私有 CA 同时用于 HTTPS 与 WSS 验证。

每次调用先持久预留当天次数和完整上下文 token 容量，再取得云端开始确认，最长运行 120 秒。断开或取消会停止生成；重启只提交保存的结果或报告不确定状态，不重新调用模型。已开始的失败、取消和恢复不退还预算；仅云端明确以 409 `conversation_busy` / `task_not_ready` 确认尚未开始时，节点撤销本次意图与预留后等待新的可运行任务。普通附件只提供名称、类型和大小；图片通过有权限的节点接口下载并作为图片输入。

节点按 AI 和窗口保存显式记忆与各自的历史摘要，结合云端原文组织回复。记忆可从聊天详情维护，管理员可以为 AI 的窗口摘要选择独立模型；群话题接续使用共同可见的原文。节点重连先核对来源变化，失效资料不继续用于回复。有限自动接话和一次性约定已接入；角色与模型可由管理员在线修改，新回复采用保存后的配置。具体运行、预算和恢复行为见[节点说明](node/README.md)。

在仓库根目录检查核心：

```bash
cargo test -p builtin-agent -p ai-client -p agent-node --locked
```

桌面附带单独构建并核验的 Agent 产物。用 `python agent/scripts/package-agent.py --binary <已构建程序> --output <新产物目录> --version <Agent版本> --target <目标三元组>` 生成程序与 SHA-256 清单；程序的 `--version` 必须匹配。打包不包含节点目录或密钥，也不会调用 Cargo。`llmn-agent catalog` 提供带 Agent 版本的模型目录 JSON，客户端无需包含模型适配器。
