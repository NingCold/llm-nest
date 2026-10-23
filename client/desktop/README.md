# 桌面应用

[应用界面](../README.md) · [开发指南](../../docs/development.md)

桌面应用使用与 Web 相同的 CloudApp。启动后填写独立云端服务的 HTTPS 地址，注册或登录后与真人和 AI 私聊、群聊。默认页面不会初始化旧本地 Runtime；需要让 AI 在这台电脑上运行时，再配置本机 Agent。

[原生云端网络层](src-tauri/src/cloud.rs) 负责 JSON、SSE 和附件传输，会话 Cookie 仅保存在进程内存，不交给网页脚本；重启应用后需要重新登录。服务器地址会在本机保存。下载和聊天导出通过系统“另存为”保存，取消、失败和写入成功分别反馈。

文字与附件草稿按服务器、实际账号和窗口保存在本机 IndexedDB。关闭窗口前先等待草稿保存；失败时保留窗口并提示重试。关闭窗口不停止远端云端服务。可在“我管理的 AI”中配置本机模型、授权节点并启动独立 Agent。同一服务器与真人账号共用一个本机节点，可配对多个 AI。首次关闭时说明后台行为并提供停止选项；继续后台运行后可从托盘打开窗口或停止本机 Agent。托盘“停止本机 Agent 并退出”会停止由桌面管理的本机节点和未完成配对；远程节点不受影响。

## 同机文字私聊

先在“我管理的 AI”配置、配对并启动本机 Agent，再打开自己管理的 AI 私聊，点击“开启同机私聊”。这一步需要联网，云端会核对实际真人、当前私聊与节点绑定；节点准备最近最多 64 条可用文字历史。已有附件或群话题引用的近期历史暂不支持准备同机窗口，界面会返回明确错误。

准备成功后，文字直接交给独立 Agent，先存入节点 SQLite，再由本机模型回复；与云端断开时仍可查看缓存和继续对话，前提是当前桌面会话已登录、Agent 正在运行且模型服务可用。界面分别显示本机生成和云端同步状态。联网后自动补传，也可点击“重试云端同步”；冲突会保留本机记录并提示，不能把未同步当成云端已收到。

同机路线按服务器、真人账号和窗口保存在本机，刷新不会自动切回云端。发送结果未确认时只能手动重试原请求。只有回复结束、记录已同步且没有未确认发送时，才可点击“返回云端发送”，随后使用原有附件、回复和引用功能。同机模式本批仅接受普通文字，暂不提供编辑与撤回。桌面退出后不保存登录令牌；重启须在线重新登录，随后可恢复已保存的同机窗口。

同机通道使用受限命名管道或 Unix socket，不开放网络监听，也不启动旧 Runtime。此功能尚未提供端到端加密或局域网连接。

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
