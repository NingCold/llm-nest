# M1：实时 IPC 与共用桌面界面

[当前契约](07-api-and-events.md) · [界面](08-frontend-and-desktop.md) · [实施顺序](TODO.md)

日期：2026-10-09。本批将真实社交界面接入 Tauri；IPC 指 WebView 与同一桌面进程中 Rust 后端的通信。业务命令通过 invoke 返回，持久引用、正文/思考增量和重新同步通知通过 Channel 持续推送。

## 实现

| 模块 | 本批责任 |
| --- | --- |
| features/social/src/subscription.rs | HTTP 与 IPC 共用的持久事件回放、250ms 补读、256 条临时广播及 resync；丢弃订阅不取消运行 |
| crates/web-server/src/social_api.rs | 将共用订阅转换为既有 SSE 事件，保留客户端 Header 与关闭语义 |
| frontends/tauri/src-tauri/src/social_subscription.rs | 按 WebView 管理订阅、Channel 推送、逐条确认、30 秒确认超时、页面/窗口释放及任务排空 |
| frontends/tauri/src-tauri/src/lib.rs | 主窗口关闭拒绝新命令，等待初始化、订阅任务、Runtime 与 SQLite 收尾；20 秒等待失败保留窗口并报告错误 |
| frontends/web/src/social/client.ts、http.ts、tauri.ts、transport.ts | 共用业务契约与 HTTP/IPC 适配，无失败回退；预注册 Channel，应用成功后确认，断开重连 |
| frontends/web/src/social/live-store.ts | 共用发送/历史/游标状态；每次连接独立生命周期，释放后的读取不推进新连接游标 |
| frontends/web/src/App.tsx、social/App.tsx | Web/Tauri 默认消息与通讯录；Windows 顶栏保留窗口按钮和拖动区域，退出错误独立展示 |

完整命令与事件字段只在 [API 与事件](07-api-and-events.md#6-启动订阅与补齐) 维护。本批没有变更 SQLite schema、身份/群历史权限、参与调度或意识系统内容。

## 自动验证

- Windows MSVC：social-feature 6 项、tauri-frontend 15 项、runtime 55 项、chat 8 项、web-server 4 项测试通过；新增测试覆盖初始间隙回放、断开后继续可用、临时广播溢出、所属窗口/替代订阅、确认序号/重复确认、确认超时和退出打断等待。
- WSL Ubuntu：social-feature 6 项、Tauri 库 16 项及 main 1 项测试通过。这是 Linux 编译与无界面测试，不是 Linux GUI/安装验收。
- 前端 typecheck 与四组回归通过；IPC 回归覆盖共享命令、业务错误、提前到达的事件、应用成功后确认、resync、初始化中断、失败重放、终止帧释放等待和适配器选择。真实状态回归覆盖释放旧连接及重新挂载。
- 新建 Windows MSVC web-server 后，social_acceptance.py 与 acceptance.py 均使用临时数据/loopback 模拟 provider 通过。后者新增 --binary，允许选择实际 MSVC 产物。
- cargo fmt 与受影响模块 clippy 通过；clippy 保留既有 ai-client、chat、旧 Web API 警告。本批不修改这些无关实现。

## 桌面验证范围

通过 Tauri CLI 构建 Windows MSVC 的嵌入前端 debug EXE（--no-bundle），实际启动并通过 computer-use 检查消息/通讯录、私聊、生成期间切页、三 AI 群聊顺序回复、WebView 刷新重连和重启后的历史恢复。

退出验收使用独立的 25 秒模拟回复：观察到正文片段与回复中状态后关闭窗口，进程退出；SQLite 保留 64 个字符的片段、一个 interrupted run 和对应 RunDetails，PRAGMA integrity_check 返回 ok。重启后界面显示“本次回复已中断，片段保留在记录中”，run 数量仍为一，没有自动重放。较短模拟回复在点击关闭前已经完成的场景另算正常完成，不用于证明退出中断。

完整 M1 异常矩阵、安装包覆盖升级、干净系统与 Linux GUI/打包仍在阶段剩余验收中。

没有使用真实供应商 API，没有发布安装器或推送源码。窗口 identifier、Windows DWM/透明合成和安装标识没有调整。

## 复现桌面验收

先通过 Tauri CLI 构建，Windows 在 MSVC 工具链下运行（如默认工具链为 GNU，显式设置 RUSTUP_TOOLCHAIN）：

```powershell
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
pnpm --dir frontends/tauri exec tauri build --debug --no-bundle --target x86_64-pc-windows-msvc --config src-tauri/tauri.windows.conf.json
cargo build -p web-server --target x86_64-pc-windows-msvc --locked
python scripts/social_desktop_fixture.py --server-binary target/x86_64-pc-windows-msvc/debug/web-server.exe --manifest target/desktop-qa/manifest.json --stop-file target/desktop-qa/stop
```

fixture 输出 READY 后，在另一终端读取清单并启动测试应用；清单只引用独立临时目录。WebView 目录也隔离，避免测试草稿与日常桌面数据混用。

```powershell
$desktopFixture = Get-Content target/desktop-qa/manifest.json -Raw | ConvertFrom-Json
$env:LLMN_CONFIG = $desktopFixture.config
$env:LLMN_DATA_DIR = $desktopFixture.data
$env:WEBVIEW2_USER_DATA_FOLDER = $desktopFixture.webview
& ./target/x86_64-pc-windows-msvc/debug/LLM-Nest.exe
```

初始包含三个 AI 联系人、一个长期私聊和一个群。slow 前缀消息提供可观察的生成时间；群内勾选三名接话者后发送，检查顺序与运行详情。切换通讯录/设置或刷新页面后生成继续；生成中关闭主窗口，检查退出和下一次启动的片段/中断状态。测试应用关闭后创建 stop 文件结束模拟 provider，数据与日志保留供检查。
