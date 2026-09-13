# 干净 Windows 安装验收

此文是待执行步骤，不代表干净系统已通过验收。先在一台已具备 Windows Sandbox 的主机，或可重置的 Windows 虚拟机中执行。当前开发机没有可启动的 Sandbox；准备脚本不会启用 Windows 功能、申请管理员权限或启动沙盒。

## 准备

在仓库根目录执行 `./scripts/prepare-windows-sandbox.ps1`。它将最新 NSIS 安装包、SHA-256 和本清单复制到独立的 `target/sandbox-acceptance-<id>/package`，输出 `.wsb` 路径。也可用 `-InstallerPath` 指定 MSI。双击 `.wsb` 后，文件夹会只读映射到客户系统的 `C:\LLM-Nest-Package` 并在资源管理器中打开。网络保持 Sandbox 默认行为。

不要将开发目录、`.env`、会话数据或真实 API Key 复制进客户系统。UI 安装与卸载在客户系统中进行；关闭 Sandbox 会丢弃其中的数据，结束前另存验收记录。

## 记录环境

- Windows 版本、架构、显示缩放比例；是否有管理员权限。
- 安装前是否已有 WebView2 Runtime，以及版本。干净用户配置不等于缺少 WebView2；若系统预装了它，将这一分支标为未测。
- 安装包名称、SHA-256、网络是否可访问 Microsoft 下载端点。

## 操作与通过条件

1. 运行安装包，记录安装目录、安装/下载提示和退出结果。NSIS 默认当前用户安装；MSI 默认系统级安装，两种包各用一次全新的客户系统验收。
2. 从开始菜单的 `LLM-Nest` 启动，进程文件名为 `LLM-Nest.exe`。无需 Node、Rust、项目目录或开发服务器；出现白底应用图标、左上角鸟巢标识和“桌面模式”。
3. 首次没有供应商时显示配置引导、禁用发送。模型和思考下拉均显示无模型说明，点击“添加模型”能进入模型设置。新建供应商时 Key 未填不能创建；编辑时留空保留现有凭据。
4. 切换浅色和深色主题，检查黑/白标识、设置页和文字对比度。缩到 800 × 600，使用长对话标题检查顶栏操作可见。快速调整宽高、垂直贴边、最大化与还原，记录是否出现黑影或黑框；当前窗口关闭系统阴影并使用直角边缘。
5. 新建对话并修改标题、生成设置。退出重开，确认配置与历史恢复。无需真实模型即可验证这些本地流程；真实模型/工具链另见已有验收报告。
6. 退出后覆盖安装，确认已有配置与历史保留；卸载后确认程序、快捷方式及卸载项移除，再重装检查数据保留。
7. 若安装前没有 WebView2，确认安装器能下载并安装它，随后应用成功启动；记录下载失败时的提示与重试结果。缺少 WebView2 且断网时，不应把安装失败误报为通过。

当前包使用 Tauri 默认的 `downloadBootstrapper`，没有 WebView2 时需要联网；不属于离线完整安装包。详见 [Tauri Windows 安装文档](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)。映射配置遵循 [Microsoft Windows Sandbox 文档](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file)。
