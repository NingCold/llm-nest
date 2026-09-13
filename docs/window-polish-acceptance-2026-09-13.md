# 窗口与模型入口修整验收（2026-09-13）

## 用户反馈与修改

用户已人工确认上一版的系统图标、窗口拖动、最大化还原和系统缩放正常。本轮处理快速拉伸时的黑影、垂直贴边后的黑框、应用名称和模型菜单空状态。

- Windows 无边框窗口关闭系统阴影，WebView 使用透明合成表面，页面本身仍为不透明背景。`WindowControls` 随明暗主题同步原生窗口底色，减少调整尺寸时暴露黑色底层的情况。调用 `Window.setBackgroundColor`，不能换成同时修改 WebView 背景的 `WebviewWindow` API。关闭阴影也会移除 Windows 11 的系统圆角；当前窗口为直角。
- 应用、窗口、快捷方式、安装器统一命名为 `LLM-Nest`；Rust 二进制名显式设置为 `LLM-Nest`，产物为 `LLM-Nest.exe`。内部 Cargo 包名仍为 `tauri-frontend`，不影响用户看到的名称。应用 identifier `com.llmnest.tauri`、配置及存储目录保持不变。
- 模型下拉没有可用模型时显示说明与“添加模型”；思考菜单没有模型时同样提供入口，有模型但不支持思考时显示原因与“管理模型”。入口直接进入模型设置，支持键盘操作。
- 联网搜索与邻近控件统一为 14px 字号和相同字体族、字重；联网搜索功能仍未接入。
- 修复输入框底部在空模型状态显示 `/`、在不支持思考的模型上显示思考级别的问题，底部状态按当前模型实际支持的级别显示。

窗口调整参考本机 Tauri 2.11.5 / tao 0.35.3 / wry 0.55.1 源码与 [Tauri 同类调整尺寸报告](https://github.com/tauri-apps/tauri/issues/13270)。这是对相似现象的针对性处理，不能据此认定用户的所有显卡、贴边场景均已修复。阴影选项的系统效果见 [Tauri WindowConfig](https://v2.tauri.app/reference/config/#windowconfig)。

## 已执行验证

- `pnpm typecheck`、`pnpm test`、`cargo fmt --all -- --check`、`git diff --check` 通过。最终前端修改后重新完成类型检查、回归测试和 MSVC release 的 NSIS/MSI 构建。
- 浏览器连接真实本地 Web 后端，使用独立临时配置和会话目录，没有发送真实厂商请求。空模型菜单与思考菜单均显示说明，两处添加入口均进入 `#/settings/models`；键盘 Enter 可触发入口。深色弹层文字换行和布局正常。
- 同一后端换成本地虚构模型目录后，不支持思考的模型显示“不支持”和管理入口，支持思考的模型仅列出配置的“关闭、低、高”。切换高至低后顶栏和底部同步，普通模型底部不再显示无效思考级别。
- 原生 release 窗口从 1200×800 快速缩窄至 851×800，再缩至最小 800×600，捕获的完成画面没有残留黑边，模型、思考、主题与窗口控制按钮可见。浅色、深色背景切换未出现窗口背景 IPC 错误。
- 最终已安装的 `LLM-Nest.exe` 使用隔离空配置启动，名称与空状态正确，底部没有多余 `/` 或思考级别。最大化后的顶栏布局及联网搜索字号经过目视检查，随后还原并关闭验收窗口。

## 本机安装迁移

本次更改 NSIS productName 会改变卸载注册表键，旧 `LLM Nest` 不会自然成为新名称的覆盖升级。对本机旧的当前用户安装进行一次保留数据的卸载，再安装 `LLM-Nest`，没有执行旧 E 盘目录的删除或覆盖。

- 新安装目录：`%LOCALAPPDATA%/Programs/LLM-Nest`。
- 桌面与开始菜单的 `LLM-Nest.lnk` 均指向新 `LLM-Nest.exe`，新卸载项显示 `LLM-Nest`，旧当前用户卸载项已移除。
- 卸载前记录实际默认存储目录中的 JSON/TOML 哈希。本机该目录当时有 1 个配置文件、没有会话 JSON；安装后该文件哈希一致。不据此宣称有真实聊天历史的升级已覆盖。
- identifier 与 `llmn` 数据目录未改变；本轮测试使用临时会话，不修改真实模型凭据。
- 老名称版本升级到本次版本需完成这次名称迁移；没有加入通用旧安装自动迁移钩子。后续相同产品名的常规覆盖升级仍按已有安装器流程处理。

## 最终产物

位于 `target/x86_64-pc-windows-msvc/release/bundle`：

| 产物 | 大小 | SHA-256 |
| --- | ---: | --- |
| `nsis/LLM-Nest_0.1.0_x64-setup.exe` | 5,804,014 字节 | `fdcc922ef4e266108852f76221ecdd04c694d5fe469166426cf55db1d5e7d69a` |
| `msi/LLM-Nest_0.1.0_x64_en-US.msi` | 7,602,176 字节 | `58903022e03192d2b16ff56a8c7b6fc939f35014437acddd3a31d5f22e152750` |

- release EXE、NSIS 安装器、已安装 EXE、卸载器、MSI 解包 EXE 的图标资源均通过 `scripts/verify-windows-icons.ps1`。
- 归一化 Tauri 包类型标记后，release、NSIS 安装、MSI 解包的 EXE 内容一致，SHA-256 为 `a0a4ff6b06e727213bfd1e78d06b7be1027d325957ef5b6aef3be5bbbe5e2b5f`。EXE 的 FileDescription 和 ProductName 均为 `LLM-Nest`。
- 最终构建日志：`%TEMP%/llmn-window-polish-final-build.log`。隔离验收目录路径记录在 `target/window-polish-acceptance-location.txt`，其 `final-package.json` 保存包哈希。

## 尚需复核的边界

- 自动化完成了尺寸改变和完成画面检查，未捕获整个高速拉伸过程的逐帧画面；原生拖动接口不能越过窗口边界到屏幕顶部，因此**垂直拉伸贴边后的黑框仍需用户按原操作复核**。
- 本轮桌面验收限于本机 Windows。不同显卡、显示器、DPI 与其他操作系统没有新增覆盖。
- MSI 仅完成构建与行政解包核对，没有执行系统级安装。干净 Windows、缺少 WebView2 的首次启动仍按 [干净系统验收清单](windows-clean-install.md) 待验收。
