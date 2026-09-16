# 图标与桌面验收补充（2026-09-13）

> 后续纠正：本轮只确认了界面与原生窗口图标，未检查 EXE 的 Windows 图标资源。
> 用户反馈后，实际从当时的发布 EXE 和安装 EXE 提取出了蓝色占位图标；此前的验收不能证明桌面快捷方式或任务栏图标正确。
> 原因是 `tauri-build` 的增量构建未跟踪 ICO 变化。修复、最终包资源核对与新的设置页验收见
> [桌面窗口与设置页验收](desktop-settings-acceptance-2026-09-13.md)。下方安装包哈希为历史批次。

## 改动

- 将用户的 `LLMNest_icon.png` 转为真正的 SVG：8 条 Bézier 路径，保留鸟、巢、星芒，去除外围黑色角块。桌面图标使用白底黑标、透明圆角外缘。
- 左上角、侧栏底部和设置页共用 `BrandMark`，浅色为透明底黑标，深色为透明底白标。同步更新 SVG/PNG favicon、Windows ICO 和 macOS ICNS。
- `assets/branding/app-icon.svg` 为唯一编辑源，`frontends/tauri` 下运行 `pnpm icons` 即可重新生成各端资源。ICO 包含 16、24、32、48、64、256 像素尺寸。
- 顶栏长标题与模型名称按可用宽度截断，操作按钮不压缩；窄窗口隐藏辅助文字。修正 Select 的全体子 span 行截断规则覆盖响应式隐藏的问题。
- 新建供应商时 API Key 为必填且有说明；编辑留空保留现有密钥或环境变量引用，不回显凭据。无认证本地服务在当前配置模型下可使用占位值。
- 修复手写配置只有 `[providers.foo]` 时，删除最后一个供应商导致隐式父表消失、候选配置不能解析的问题。删除后显式保留空 `[providers]`，其他设置保留，可重启。

## 自动检查

- Web `pnpm typecheck`、`pnpm test` 通过。
- Runtime 53 项测试通过，新增“删除隐式父表下最后一个供应商，保留 GUI 参数并重启”回归。
- `cargo fmt --all -- --check`、`git diff --check` 通过。
- `cargo clippy -p runtime --offline --locked` 通过；依赖及 Runtime 仍有 6 条已有风格建议，无错误。
- MSVC release 构建与 NSIS、MSI 打包通过。

## 实机检查

使用 `%TEMP%/llmn-branding-ld9o__fn` 中的合成验收历史副本。程序从 `%LOCALAPPDATA%/Programs/LLM Nest` 启动，使用 Tauri IPC；没有启动开发服务器，也没有发送新的付费模型请求。

- NSIS 覆盖安装退出码 0，原生标题栏和界面均显示新标识。
- 浅色、深色下左上角及底部标识清晰；设置页也使用同一图案。
- 最终安装包分别在约 1200、800 像素宽的原生窗口检查；800 宽时辅助文字隐藏、标题显示省略号，模型、思考级别和主题按钮保持单行可见。极窄可用空间下标题只显示少量字符，悬停可查看全名。
- 选择 DeepSeek 模板后，端点和模型已填充但 Key 留空，“创建”仍禁用；编辑现有 ChatECNU 时显示“留空保持现有密钥”，输入框不回显环境变量中的真实 Key。
- MSI 行政解包（`msiexec /a /qn`）退出码 0。解包程序与 NSIS 安装的程序、当前 release 程序，归一化 Tauri 的三字节包类型标记后内容一致。行政解包不等于系统级安装通过。
- 两次打开设置、切换主题与安装更新前后，隔离会话 JSON 和 `llmn.toml` 的 SHA-256 均保持不变。

## 待完成的发布验收

当前开发机没有可直接启动的 Windows Sandbox 或其他虚拟机。`scripts/prepare-windows-sandbox.ps1` 已执行并验证 XML 和只读映射：只复制安装包、SHA-256 和清单，不映射项目或凭据。脚本不启用系统功能，也不自动启动沙盒。

干净 Windows、安装前缺少 WebView2、MSI 系统级安装/升级/卸载、签名与 SmartScreen 仍未验收。具体步骤见 [干净 Windows 安装验收](windows-clean-install.md)。当前安装器使用 Tauri 默认的联网下载 WebView2 引导程序，不能当作离线完整安装包；参见 [Tauri 官方文档](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)。

前端主入口仍约 1.85 MB（gzip 约 584 KB）；体积优化另行处理。

## 最新安装包

路径均位于 `target/x86_64-pc-windows-msvc/release/bundle`：

| 产物 | 大小 | SHA-256 |
| --- | ---: | --- |
| `nsis/LLM Nest_0.1.0_x64-setup.exe` | 5,792,807 字节 | `179874a85ef6be6bc6d0f48fd1c6c9b34c993a3a24d3fc16f37d5c9d5d619b84` |
| `msi/LLM Nest_0.1.0_x64_en-US.msi` | 7,585,792 字节 | `a144f7cad32cc689b11df51c848142e11556ab6b4e7cd0598b496319b119e0d2` |

最终批次已再次 NSIS 安装与 MSI 解包（退出码均为 0），核对两者程序内容与 release 一致。沙盒入口已用最终 NSIS 重新生成：`target/sandbox-acceptance-993be55b46be475984b994c3be66a25b/LLM-Nest.wsb`。构建日志在 `%TEMP%/llmn-tauri-branding-final-build.log`，脱敏哈希记录在上述隔离验收目录的 `final-package.json`。
