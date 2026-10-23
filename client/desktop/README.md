# 桌面应用

[应用界面](../README.md) · [开发指南](../../docs/development.md)

桌面应用使用与 Web 相同的 CloudApp。启动后填写独立云端服务的 HTTPS 地址，注册或登录后与真人和 AI 私聊、群聊。默认页面不会初始化旧本地 Runtime；需要让 AI 在这台电脑上运行时，再配置本机 Agent。

[原生云端网络层](src-tauri/src/cloud.rs) 负责 JSON、SSE 和附件传输，会话 Cookie 仅保存在进程内存，不交给网页脚本；重启应用后需要重新登录。服务器地址会在本机保存。下载和聊天导出通过系统“另存为”保存，取消、失败和写入成功分别反馈。

文字与附件草稿按服务器、实际账号和窗口保存在本机 IndexedDB。关闭窗口前先等待草稿保存；失败时保留窗口并提示重试。关闭窗口不停止远端云端服务。可在“我管理的 AI”中配置本机模型、授权节点并启动独立 Agent。同一服务器与真人账号共用一个本机节点，可配对多个 AI。首次关闭时说明后台行为并提供停止选项；继续后台运行后可从托盘打开窗口或停止本机 Agent。托盘“停止本机 Agent 并退出”会停止由桌面管理的本机节点和未完成配对；远程节点不受影响。

## 开发

准备好 Rust MSVC（Windows）、Node.js、pnpm 和平台依赖，在仓库根目录运行：

```bash
pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/desktop install --frozen-lockfile
pnpm --dir client/desktop exec tauri dev
```

Tauri 开发入口自动构建 debug Agent；打包入口自动构建 release Agent，再构建共用前端。也可先单独运行 `pnpm --dir client/desktop build:agent:dev` 或 `build:agent`。生成的可执行文件位于忽略的 `src-tauri/resources/`，安装时复制到资源目录的 `agent/`；调试时还可读取相邻 Cargo 输出或进程环境 `LLMN_AGENT_EXECUTABLE` 指定的开发构建。构建脚本遵循 `CARGO_BUILD_TARGET` 与 `CARGO_TARGET_DIR`，Windows 默认使用 MSVC。

本机配置与私有凭据按 `LLMN_DATA_DIR`（未设置则 `~/.llmn`）下的 `desktop-agents/<服务器哈希>/<真人 ID>/` 隔离。模型 Key 只保存在该受限本机目录，作为环境变量注入子进程，不经过云端、命令行参数或读取配置 IPC。停机后修改配置或增加配对，再明确启动；不会自动重启已停止的节点。测试使用临时 `LLMN_DATA_DIR`，避免接触正常节点。

Windows/Linux 环境准备见[开发指南](../../docs/development.md)。测试站点使用私有 CA 时，可在启动进程前设置 `CLOUD_CA_CERT_FILE` 指向测试 CA 的 PEM 文件；该选项只用于授权测试，不关闭 TLS 校验，也不打包私有证书。

旧本地 IPC 和 Runtime 仍保留为延迟初始化能力，供后续迁移维护，不是默认聊天通路。窗口和打包代码见 [src-tauri](src-tauri/README.md)，发布与图标见[发布指南](../../docs/releases.md)和[图标说明](../../assets/branding/README.md)。
