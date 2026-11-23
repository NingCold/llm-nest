# 桌面应用

[项目首页](../README.md) · [使用指南](../docs/usage.md) · [开发指南](../docs/development.md)

桌面应用与 Web 共用聊天界面。填写云端 HTTPS 地址并登录后，可以与真人和 AI 私聊、群聊；下载与聊天导出使用系统保存对话框。Windows 支持可选的 30 天保持登录和本机自动解锁，两者独立且默认关闭，详见[登录与解锁](../docs/usage.md#注册和登录)。

## 让 AI 在这台电脑运行

在“我的账号 → 我管理的 AI”配置本机模型、授权节点并启动 Agent。同一服务器与真人账号共用一个本机节点，可连接多个 AI。首次关闭窗口时可选择继续后台运行、停止并退出或取消；托盘可重新打开窗口或停止节点。电脑休眠、关机会使本机 AI 离线，远程节点独立运行。

准备好同机私聊后，桌面可与本机 Agent 直接通信，断云期间先保存记录，重连再同步；AI 回复仍要求模型服务可访问。Agent 停止不影响读取云端已同步、当前设备有权解密的历史。步骤见[同机私聊](../docs/usage.md#同机私聊)。

公开包与本机试用版的功能和平台验证不同，安装前查看[当前安排](../docs/project/current.md)。

## 从源码运行

准备 Rust、Node.js、pnpm 和[平台依赖](../docs/development.md#准备-web-和桌面环境)，再按[Agent 产物准备](../docs/development.md#准备桌面-agent-产物)指定固定版本、目标平台、程序与第三方声明清单。在仓库根运行：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir desktop install --frozen-lockfile
pnpm --dir desktop exec tauri dev
```

Tauri 入口调用 `prepare-agent.mjs`，核对独立 Agent 产物的版本和 SHA-256，将程序、声明和 `agent-artifact.json` 放入资源目录；不编译 Agent 或读取相邻仓库。目录和配置校验由 Agent 提供。

本机节点按 `LLMN_DATA_DIR`（默认 `~/.llmn`）下的 `desktop-agents/<服务器哈希>/<真人 ID>/` 隔离。模型 Key 保存在受限目录，以环境变量注入子进程，不经过云端、命令行参数或读取配置 IPC。停止 Agent 后修改配置或新增配对，再明确启动。测试使用临时 `LLMN_DATA_DIR`。

授权测试站点若使用私有 CA，启动前设置 `CLOUD_CA_CERT_FILE` 指向可信 PEM；保留 TLS 校验，不将私有证书打包。

## 原生代码

[原生层](src-tauri/README.md)负责 HTTPS/SSE、系统保存、窗口、草稿关闭协调及 Agent 生命周期。会话令牌不交给网页脚本；关闭前等待草稿保存，失败则保留窗口。桌面不链接旧本地 Runtime，也不读取节点 SQLite。

[构建与发布](../docs/releases.md) · [图标资源](../assets/branding/README.md)
