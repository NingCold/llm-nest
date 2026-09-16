# Windows 原生圆角与边框（2026-09-13）

## 设计与实现

用户已确认 `f68f39d` 修复了快速缩放与上下贴边的黑影，但直角、弱边缘让浅色窗口难以与背景区分。本轮将圆角和描边交给 Windows DWM，保留上一版的 WebView 合成方式。

- `tauri.conf.json` 的 `shadow=false`、`transparent=true` 和原生背景设置保持不变，HTML 页面依然不透明。`WindowControls` 继续通过 `Window.setBackgroundColor` 同步背景，不能改成会重设 WebView 背景的 `WebviewWindow` 同名 API。
- 本机 tao 0.35.3 的阴影开关还控制 `WM_NCCALCSIZE` 中的窗口边缘留量。直接重新启用该开关会改变已经验证的边缘布局，因此本轮不走这条路径。
- 新增 `window_frame.rs`，仅在 Windows 调用 DWM 的 `DWMWA_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND` 和 `DWMWA_BORDER_COLOR`。不增加透明外边距、不绘制阴影辅助窗口、不修改窗口区域或鼠标命中测试，也不通过 CSS 裁切整个应用。
- 普通窗口请求系统圆角，最大化/贴边交给系统处理，不在每次缩放时反复计算圆角半径。微软说明最大化、贴边等场景按系统策略不使用圆角，且圆角请求不保证在所有窗口配置中生效，见 [Windows 11 圆角指南](https://learn.microsoft.com/en-us/windows/apps/desktop/modernize/ui/apply-rounded-corners)。
- 细描边随应用主题与原生焦点变化：浅色活动 `#9a9a9a`、非活动 `#bcbcbc`；深色活动 `#686868`、非活动 `#4c4c4c`。宽度、圆角抗锯齿由 DWM 管理。[DWM 边框文档](https://learn.microsoft.com/en-us/windows/win32/api/dwmapi/ne-dwmapi-dwmwindowattribute) 要求应用在设置自定义颜色后自行更新激活状态的颜色。
- `set_window_appearance` 仅接收主题布尔值，作用于调用窗口；不暴露任意 HWND/属性，不依赖配置或聊天后端初始化。
- Windows 10 不支持相关属性时，保留直角并显示 CSS 内描边作为兼容回退。回退不占布局空间、不接收鼠标事件，最大化/全屏时隐藏。其他 DWM 错误进入既有窗口错误提示与回退，不阻止聊天后端启动。

## 验证结果

本机 Windows 11 25H2，build `26200.9445`。原生操作使用 Computer Use，应用使用独立临时空配置和数据目录，没有调用真实模型 API。

- `pnpm typecheck`、`pnpm test`、`cargo fmt --all -- --check` 和 `git diff --check` 通过。
- MSVC release 的 NSIS、MSI 打包完成；构建保留已有大资源块提示和链接器输出提示。
- `cargo clippy -p tauri-frontend --release --target x86_64-pc-windows-msvc --offline --locked --features tauri/custom-protocol` 完成。依赖业务 crate 仍有现存 Clippy 提示，本次新增窗口模块没有提示。
- 完整应用在 1200×800 下显示原生圆角和细边框，浅色/深色切换正常，没有窗口外观 IPC 错误提示。
- 浅色窗口快速缩窄到 880×800，再缩到 800×600，完成画面没有残留黑影，圆角、细边框及窗口控制按钮正常。浅色描边可与白色页面区分。
- 最大化到 1707×912 后，动画结束画面为直角，没有多余矩形描边。切回深色并还原至 800×600 后，系统圆角和边框恢复。
- 最终 NSIS 安装后再从 `%LOCALAPPDATA%/Programs/LLM-Nest/LLM-Nest.exe` 启动，圆角与描边正常，保留验收前深色主题。验收实例随后关闭。

## 安装与文件验证

当前用户安装已覆盖更新，产品名、标识、快捷方式目标和用户数据目录保持不变。更新前后默认目录中的 JSON/TOML 哈希一致；本机当时只有 1 个配置文件、没有会话 JSON，不能据此宣称覆盖了真实历史数据升级。

| 产物（`target/x86_64-pc-windows-msvc/release/bundle`） | 大小 | SHA-256 |
| --- | ---: | --- |
| `nsis/LLM-Nest_0.1.0_x64-setup.exe` | 5,801,556 字节 | `41824dc936cd1b4ead7355c6adecd2c6b03183cfc5d2c20bda29a0aa7d93c9a0` |
| `msi/LLM-Nest_0.1.0_x64_en-US.msi` | 7,671,808 字节 | `77d11c35c37e19d2c47fca4dd10fbef562ac91723793f2d808c8429d1269ed39` |

- release EXE、NSIS 安装器、已安装 EXE、卸载器和 MSI 解包 EXE 均通过 Windows 图标资源检查。
- release、NSIS 安装、MSI 解包 EXE 在归一化 Tauri 包类型标记后完全一致，SHA-256 为 `0c35d010aafa6082f524779e349e0e839a502d803fbcdf3afa1e06e92bcc64f4`。
- 构建日志为 `%TEMP%/llmn-native-frame-final-build.log`，Clippy 日志为 `%TEMP%/llmn-native-frame-clippy.log`。本轮隔离目录由 `target/native-frame-acceptance-location.txt` 指向，包含包哈希与 `previous-verified-setup.exe`（上一版已验证的 NSIS 安装包，可用于回退）。

## 验证边界

- 用户确认的上下贴边黑影消失属于上一版。本轮保留了该版本的布局和背景策略，但自动化未取得垂直贴边的可靠触发证据，且没有录制高速拖动的整个逐帧过程，仍需用户复核原操作。
- 未执行不同显示缩放、多显示器、不同显卡、Windows 10 回退或其他操作系统的新增实机验收。非活动边框颜色按焦点事件实现，没有单独完成失焦前后的像素比较。
- MSI 只完成构建和行政解包核对；干净系统、缺 WebView2、MSI 系统级安装仍待原有独立验收，不包含在本轮结论中。
