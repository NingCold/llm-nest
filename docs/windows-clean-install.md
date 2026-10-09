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
2. 从开始菜单的 `LLM Nest` 启动，进程文件名仍为 `LLM-Nest.exe`。无需 Node、Rust、项目目录或开发服务器；出现鸟巢标识及默认的消息 / 通讯录社交界面。内部安装标识继续沿用 `LLM-Nest`。
3. 首次没有供应商时，在「我的账号 → AI 角色管理」创建未绑定模型的 AI，打开其唯一私聊。真人消息应能保存，并显示 AI 尚不能执行的提示；不产生模拟回答。设置中能进入模型服务配置；新建供应商时 Key 未填不能创建，编辑时留空保留现有凭据。管理员在角色管理中维护每个 AI 的模型与参数，公开资料不包含内部设定。旧 Harness 的无模型禁止发送规则仅在显式旧入口验收。
4. 切换浅色和深色主题，检查黑/白标识、设置页和文字对比度。缩到 800 × 600，使用长对话标题检查顶栏操作可见。快速调整宽高、垂直贴边、最大化与还原，记录是否出现残留黑影或异常粗框。Windows 11 普通窗口应显示系统圆角和清晰细边框；上下同时贴住工作区、但未最大化时显示矩形细描边，移开后恢复系统圆角。最大化/全屏不增加描边；Windows 10 使用直角与细边框回退。检查窗口失去焦点后的边界仍可区分。另检查顶部/侧边任务栏、不同缩放的显示器之间移动，避免把仅触碰一条边的普通窗口误判为上下贴边。
5. 新建 AI 账号、唯一私聊和群，修改公开资料与管理员维护的内部设定；退出重开，确认身份、关系、配置与历史恢复。移除联系人不改变账号状态，重新添加仍打开原私聊。用独立 loopback 模拟 provider 检查群内三个 AI 顺序回复、保存失败不丢草稿、生成中退出与重启保留片段，不自动重放。真实 API 仅在明确授权后执行。
6. 退出后覆盖安装，确认已有配置与历史保留；卸载后确认程序、快捷方式及卸载项移除，再重装检查数据保留。
7. 若安装前没有 WebView2，确认安装器能下载并安装它，随后应用成功启动；记录下载失败时的提示与重试结果。缺少 WebView2 且断网时，不应把安装失败误报为通过。

当前包使用 Tauri 默认的 `downloadBootstrapper`，没有 WebView2 时需要联网；不属于离线完整安装包。详见 [Tauri Windows 安装文档](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)。映射配置遵循 [Microsoft Windows Sandbox 文档](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file)。
