# Agent 模型与本机配置

[使用指南](usage.md) · [开发指南](development.md) · [节点说明](../agent/node/README.md)

浏览器和桌面连接云端服务，模型由独立 Agent 调用。桌面在“我的账号 → 我管理的 AI”配置本机节点；远程节点在运行设备维护自己的配置。服务器地址、模型 Key 和节点凭据留在运行设备，角色、模型选择、温度和输出上限按 AI 账号管理。

## 桌面管理的节点

桌面数据目录默认是用户主目录下的 `.llmn`，可用 `LLMN_DATA_DIR` 指定测试目录。节点按 `desktop-agents/<服务器哈希>/<真人ID>/` 隔离。该目录包含私有配置、凭据和节点 SQLite，不提交或打包。

桌面以 `desktop-config.json` 为配置来源，配对或启动时重建 `node.toml`，直接修改后者会被覆盖。模型 Key 通过本机受限配置注入子进程环境，不经过云端或网页读取接口。修改配置前先停止节点，保存后明确启动。桌面模型目录来自随包 Agent 的 `catalog` JSON，完整配置由 Agent 校验。

## 独立命令行节点

```powershell
llmn-agent init C:\private\llmn-agent
# 编辑 node.toml：服务器、可选私有 CA、供应商与模型。
llmn-agent check C:\private\llmn-agent
llmn-agent pair C:\private\llmn-agent
llmn-agent run C:\private\llmn-agent
```

`node.toml` 的 `providers` 使用 [ai-client](../agent/ai-client/README.md) 的配置格式。Key 仅接受环境变量引用，初始化模板使用 `API_KEY_CHATECNU`；不要把真实 Key 写进示例或提交到仓库。HTTPS 与 WSS 验证证书，私有 CA 使用 `ca_file`，仅本机开发允许 HTTP。

每个 AI 的模型、思考强度、温度和输出上限属于其档案。温度留空使用模型服务默认值，输出上限留空使用意识系统预算。非法模型和参数明确报错，不自动替换模型。已开始的任务保持冻结设置，新配置用于后续任务。节点总预算及每窗口自动额度见[节点说明](../agent/node/README.md)。

## 聊天资料与恢复

登录会话、客户端 SDK 密钥库、节点数据和默认明文草稿分别保存。保持登录不会授权新设备，也不代替恢复文件。加密资料由客户端和 Agent 处理，角色、模型、记忆和摘要可保存为密文待同步资源；当前节点确认应用后才生效。旧本地 `llmn.toml` 不再是桌面或云端产品的配置入口；旧聊天迁入见[迁移指南](migration.md)。

云端环境变量和部署用法见[云端服务](../server/cloud/README.md)及[部署说明](../server/cloud/deploy/README.md)。
