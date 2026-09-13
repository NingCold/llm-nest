# Tauri Windows 安装包验收（2026-09-13）

本次验收使用 Windows 本机安装的 NSIS release 包、真实 WebView2 桌面窗口和 Tauri IPC。模型测试使用 ChatECNU `ecnu-max`，仅发送合成的算术/测试请求，凭据来自环境变量；测试数据与原有会话隔离。

## 打包

本机工具链：Rust 1.97.0 MSVC、Visual Studio Community 2026、WebView2 152.0.4191.66、Tauri 2.11.5、Tauri CLI 2.11.4。

在 `frontends/tauri` 下运行：

```powershell
pnpm install --frozen-lockfile
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
pnpm exec tauri build --target x86_64-pc-windows-msvc --bundles nsis --ci
```

前端独立构建到 `frontends/tauri/dist`（忽略生成文件），Tauri 从 `../dist` 嵌入资源。原配置 `../web/dist` 指向不存在的目录，现已修正。构建时 `--emptyOutDir` 清除这个专用目录的旧产物，避免旧版本 JS 重复进入安装包。发布构建不依赖 Vite 或 Web Server 运行。

首次安装的 release 默认配置为 `<storage::default_data_dir()>/llmn.toml`（Windows 通常为 `%APPDATA%/llmn/llmn.toml`）。目录与初始 `[providers]` 文件按需创建，已有文件不会被覆盖。设置页可添加第一个服务商；删除最后一个服务商后仍能重启和浏览历史，聊天需先重新配置模型。

`LLMN_CONFIG` 可显式指定配置文件；指定路径不存在时显示错误，不静默回退。开发构建仍可读取项目的 `config/llmn.toml`。`LLMN_DATA_DIR` 可指定隔离验收目录。

本机全局 Cargo 配置指向一个不可连接的本地代理。验收构建仅在子进程环境中设置 `CARGO_HTTP_PROXY` 为空以恢复下载，未更改用户全局配置。

首次准备 NSIS 时，用户缓存目录中的目录移动返回 Windows 错误 17。启用 `bundle.useLocalToolsDir=true` 后，下载、校验和打包均成功；工具缓存留在项目 `target/.tauri`。工具缓存选择和解包逻辑可参见 [Tauri NSIS 实现](https://github.com/tauri-apps/tauri/blob/dev/crates/tauri-bundler/src/bundle/windows/nsis/mod.rs)。

## 验收记录

| 项目 | 实机结果 |
| --- | --- |
| NSIS 安装 | 退出码 0；安装到 `%LOCALAPPDATA%/Programs/LLM Nest`，生成开始菜单快捷方式和卸载注册表项 |
| 无配置首次启动 | 从安装目录运行，显示“桌面模式”，自动创建初始配置并提示添加供应商；聊天按钮禁用 |
| GUI 添加供应商 | 选择 ChatECNU 模板、填写测试凭据并创建成功；服务商与两个模型写入临时配置 |
| 生成设置保存 | 在 GUI 将最大输出设为 1536，显示“已保存”；重启、卸载重装后仍为 1536，温度 0.7 保留 |
| API 错误与重试 | 使用明确无效的测试凭据，显示“无效的令牌”及重新生成按钮；随后仅将测试配置改为 `API_KEY_CHATECNU` 环境变量引用，重试成功 |
| 真实聊天与隔离工具 | `31+47` 返回 `78`；模型实际调用安装目录内宿主启动的 `add` 工具，执行 45 ms；两次模型请求合计 866 tokens |
| 最强思考模式 | `29×53` 返回 `1537`；保存 72 字符思考内容、515 tokens、首 token 7261 ms、总耗时 7383 ms |
| 消息反馈和会话标题 | GUI 点赞写入真实 UUID 对应消息；会话重命名为“Tauri 安装包实机验收”并保留 |
| 取消与继续 | 点击停止生成后状态落盘为 cancelled；同会话下一条请求返回“恢复成功”，562 tokens、4873 ms |
| 配置错误启动与恢复 | 故意损坏临时 TOML，窗口显示解析错误；修复文件后点击“重新连接”恢复，不缓存失败启动状态 |
| 重启恢复 | 会话 JSON 与重启前 SHA-256 完全一致；思考记录、反馈、取消状态和会话级 max 模式恢复 |
| 自动滚动修复 | 修复虚拟行测量产生的程序性滚动误停跟随；从历史位置发送新消息后立即滚到新消息，回复结束仍显示“自动跟随正常” |
| 覆盖安装 | 退出码 0；会话文件哈希不变，应用重新启动正常 |
| 卸载 | 退出码 0；应用 exe、开始菜单快捷方式、卸载注册表项均移除；隔离测试数据保留 |
| 最终包重装 | 退出码 0；配置与会话文件哈希完全不变；启动后恢复最新回复、模型与设置 |

UI 验收使用真实 Windows 窗口、鼠标和键盘；没有通过 Web HTTP 服务替代 Tauri IPC。真实凭据只由 Rust 后端从环境读取；测试时没有把实际 Key 输入 GUI 或写入安装包。所有模型输入都是本次合成验收文本。

代码检查：Runtime **52** 项测试通过，Tauri **9** 项测试通过；其中包括首次创建配置、不覆盖已有文件、显式缺失配置报错、空目录添加/移除供应商并重启等回归。Tauri 旧消息 ID 测试已改为验证持久化 UUID。Web `pnpm typecheck` 和 `pnpm test` 通过，release 安装包构建通过。

`cargo fmt --all -- --check`、`git diff --check` 和 `cargo clippy -p runtime -p tauri-frontend --offline --locked` 通过；Clippy 仍有现有的 8 条风格/参数数量建议，没有构建错误。

## 最终产物

- 安装包：`target/x86_64-pc-windows-msvc/release/bundle/nsis/LLM Nest_0.1.0_x64-setup.exe`
- 大小：5,783,704 字节（约 5.52 MiB）。
- SHA-256：`5c4ca03a6b70af61ed380aab612d75886b7affbab0c982c6249c3ab5bbaae938`
- 安装后 exe SHA-256：`654e2272713f7bc88a03c193ca7abd64e8434d55c993216621b6f551e6299503`
- 核对已安装 exe 与当前 release 二进制，除了 Tauri 打包将 `__TAURI_BUNDLE_TYPE_VAR_UNK` 标记为 `NSS` 的三个字节外完全一致，确认验收的是当前构建。
- 本机隔离记录：`%TEMP%/llmn-tauri-fbub_1nm`，含配置、会话、阶段日志、哈希快照、脱敏 report.json 和 package.json。测试目录不是应用默认数据目录。

## 边界

本轮验证的是当前 Windows 本机的 x64 NSIS 包。没有验证 MSI、其他 Windows 版本、未安装 WebView2 的干净系统、跨版本数据库迁移、代码签名及 SmartScreen 分发体验。当前包没有配置发布签名或自动更新。

仍可继续改进长标题下顶栏控件的挤压与换行、首次创建供应商时 API Key 为空的说明，以及前端主入口体积。本轮验收通过不代表这些发布体验已经完善。
