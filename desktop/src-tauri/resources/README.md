# 随包 Agent 资源

`pnpm --dir desktop build:agent` 将显式指定的独立 Agent 产物放到本目录，不编译 Agent，也不查找相邻仓库或 Cargo 输出。先设置固定版本、目标平台及产物清单；本地文件还需提供对应程序和第三方声明。脚本核对版本清单与 SHA-256 后生成程序、声明和 `agent-artifact.json`，Tauri 将它们打包到 `agent/`。

生成文件不进 Git，不放入个人节点目录、模型密钥或聊天资料。配置方法见[开发指南](../../../docs/development.md#准备桌面-agent-产物)。
