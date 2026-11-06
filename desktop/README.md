# 桌面应用

[应用界面](../README.md) · [开发指南](../docs/development.md)

桌面应用使用与 Web 相同的 CloudApp。启动后填写独立云端服务的 HTTPS 地址，注册或登录后与真人和 AI 私聊、群聊。需要让 AI 在这台电脑上运行时，再配置本机 Agent。

[原生云端网络层](src-tauri/src/cloud.rs) 负责 JSON、SSE 和附件传输，不把会话令牌交给网页脚本。登录时默认不保留会话；Windows 可勾选“在此设备保持登录 30 天”，由当前系统用户的 DPAPI 保护令牌，到期或退出后重新登录，不保存账号密码。其他原生平台暂不支持保持登录。服务器地址会在本机保存。下载和聊天导出通过系统“另存为”保存，取消、失败和写入成功分别反馈。

本机加密资料使用独立口令。已有可信设备解锁并确认保存恢复文件后，可在“可信设备与恢复”另行开启默认关闭的 30 天自动解锁；Windows 用系统保护本机口令，其他原生平台暂不可用。手动锁定会关闭聊天内容并暂停自动解锁，下次输入口令后恢复；本机 Agent 继续运行。退出会清除自动材料，原 SDK 密钥库保留。新设备仍须可信设备授权或恢复文件。功能是否已随你使用的安装包交付，以[当前安排](../docs/project/current.md)中的版本为准；公开包仍为 alpha.13。

文字与附件草稿按服务器、实际账号和窗口保存在本机 IndexedDB。关闭窗口前先等待草稿保存；失败时保留窗口并提示重试。关闭窗口不停止远端云端服务。可在“我管理的 AI”中配置本机模型、授权节点并启动独立 Agent。同一服务器与真人账号共用一个本机节点，可配对多个 AI。首次关闭时说明后台行为并提供停止选项；继续后台运行后可从托盘打开窗口或停止本机 Agent。托盘“停止本机 Agent 并退出”会停止由桌面管理的本机节点和未完成配对；远程节点不受影响。

## 同机私聊

当前源码支持与这台电脑运行的 AI 直接私聊。首次在线准备后，文字和密文附件走受限本机通道；断云期间可继续保存，AI 回复仍要求模型服务可访问。重启仍需联网验证登录，未同步记录不能当作云端已收到。详细准备、同步与切回云端步骤见[使用指南](../docs/usage.md#同机私聊)，交付版本见[当前安排](../docs/project/current.md)。

## 开发

准备好 Rust MSVC（Windows）、Node.js、pnpm 和平台依赖，并先按[开发指南](../docs/development.md#准备桌面-agent-产物)指定已核验的 Agent 程序及清单，在仓库根目录运行：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir desktop install --frozen-lockfile
pnpm --dir desktop exec tauri dev
```

Tauri 开发和打包入口调用 `prepare-agent.mjs`，按明确版本、目标三元组和 SHA-256 清单准备 Agent，不编译 Agent 源码。开发可用显式指定的本地已构建程序；正式下载使用 HTTPS 固定产物。缺少清单、版本或匹配文件时直接报错。资源目录的程序及 `agent-artifact.json` 一起进入安装包；不会查找相邻源码或 Cargo 输出。

本机配置与私有凭据按 `LLMN_DATA_DIR`（未设置则 `~/.llmn`）下的 `desktop-agents/<服务器哈希>/<真人 ID>/` 隔离。模型 Key 只保存在该受限本机目录，作为环境变量注入子进程，不经过云端、命令行参数或读取配置 IPC。停机后修改配置或增加配对，再明确启动；不会自动重启已停止的节点。测试使用临时 `LLMN_DATA_DIR`，避免接触正常节点。

Windows/Linux 环境准备见[开发指南](../docs/development.md)。测试站点使用私有 CA 时，可在启动进程前设置 `CLOUD_CA_CERT_FILE` 指向测试 CA 的 PEM 文件；该选项只用于授权测试，不关闭 TLS 校验，也不打包私有证书。

桌面不再链接旧本地 Runtime、存储或模型执行库；模型目录和最终校验由独立 Agent 提供。窗口和打包代码见 [src-tauri](src-tauri/README.md)，发布与图标见[发布指南](../docs/releases.md)和[图标说明](../assets/branding/README.md)。
