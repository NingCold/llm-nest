# Windows 上下贴边细边框（2026-09-13）

## 本次行为

窗口上下同时贴住当前显示器工作区、但没有最大化或全屏时，显示矩形细边框。移开一边后恢复 Windows 11 系统圆角和原生描边；最大化/全屏隐藏额外描边。浅色活动边框为 `#9a9a9a`，深色为 `#686868`，失焦相应减淡。

- 通过 `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` 和 `GetMonitorInfoW.rcWork` 比较物理边界，排除任务栏、不可见的原生缩放边缘，允许 2 个物理像素的误差。仅贴住一边不会进入此状态。
- 贴边时请求 DWM 直角并停用 DWM 描边，前端补一条按系统缩放折算的 1 物理像素内描边，避免两层边框叠加。覆盖层不占布局、不接收鼠标事件。
- 保留 `shadow=false`、`transparent=true`、`Window.setBackgroundColor` 和不透明 HTML 背景，不恢复曾引起黑影的 tao 边缘留量。
- 原生监听位置、大小、DPI 和焦点变化，缓存状态；普通拖动中不反复写入 DWM 属性或向前端发相同状态。前端先监听再获取快照，用 revision 过滤迟到的事件/返回结果。

## 自动检查与实机观察

环境为本机 Windows 11，原生 UI 操作使用 Computer Use；模型配置和会话使用独立临时目录，没有调用厂商 API。

- 前端 `pnpm typecheck`、`pnpm test` 通过；Rust 窗口几何单测通过，覆盖顶部/底部任务栏、负坐标显示器、高 DPI 的物理坐标、一边接触、误差边界和无效几何。
- `cargo fmt --all -- --check`、`git diff --check` 通过。MSVC release Clippy 完成，本次窗口模块无新增警告；业务依赖保留现存提示。
- 使用临时 Tauri 配置覆盖，将独立测试窗口初始化为 1000×912、顶部 y=0，恰好占满本机工作区高度。确认浅色/深色矩形细描边、最大化后隐藏描边、还原至原高度后恢复细描边。
- 将该窗口底边向上缩到高度 800 后恢复圆角，随后缩到 800×600，完成画面未见残留黑影。
- 测试配置只用于独立 fixture EXE。正式安装包用仓库原始配置重新构建，没有 `--config` 覆盖，安装后启动尺寸仍为 1200×800，原生圆角正常；最大化完成后没有额外描边。

## 安装与产物

NSIS 已覆盖更新当前用户安装 `%LOCALAPPDATA%/Programs/LLM-Nest`，安装退出码 0。更新前确认旧窗口处于空配置、无会话/发送任务状态并正常关闭。默认数据目录中的 JSON/TOML 哈希前后一致；当时只有 1 个配置文件，不能据此声称验证了真实历史数据升级。

| 产物 | 大小 | SHA-256 |
| --- | ---: | --- |
| `target/x86_64-pc-windows-msvc/release/bundle/nsis/LLM-Nest_0.1.0_x64-setup.exe` | 5,803,340 字节 | `25eb84d05757dbc2ba4d48a7266980f54c1c29fa15f3529df32468473d549717` |
| `target/x86_64-pc-windows-msvc/release/bundle/msi/LLM-Nest_0.1.0_x64_en-US.msi` | 7,675,904 字节 | `4b8a2faccecfb9323ef98491ca5c134e48a182736989cb7cc344f5538074f044` |

release EXE、NSIS 安装器、已安装 EXE、卸载器和 MSI 解包 EXE 均通过图标资源检查。release、NSIS 安装和 MSI 解包的 EXE 在归一化 Tauri 包类型标记后内容一致，SHA-256 为 `6f285f82d908c0bb0343723bb4ab0aa0be8576f16fb75ad20ff503c8fe1de8de`。

本轮隔离目录由 `target/docked-frame-acceptance-location.txt` 指向，保存 fixture 配置/EXE、构建及 Clippy 日志、哈希核对和上一版 NSIS 备份。fixture 与正式应用启动的 stderr 均无窗口错误。

## 未覆盖范围

**后续用户复核（2026-09-13）：** 用户确认“Windows 的边框验收通过了”。据此将 `91f9a9b` 作为已接受的窗口基线。以下自动检查的覆盖限制仍保留；本次确认不扩展为多显示器、Windows 10 或 MSI 系统安装通过。

原生 GUI 验收的上下贴边状态由初始几何触发，未可靠复现用户的真实鼠标拖动吸附过程，也没有高速缩放逐帧录像。用户仍需复核原来的上下拉伸动作，确认细线粗细及切换手感。多显示器/不同 DPI、Win10 回退和全屏分支没有新增实机验收。

MSI 仅完成构建、行政解包与文件核对，未执行系统级安装。干净系统、缺少 WebView2、Linux 实机仍不在本轮通过范围内。Linux 环境选择另见 [方案](linux-build-plan.md)。
