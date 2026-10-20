# 干净 Windows 安装检查

[Windows 已知结果](../validation/windows.md) · [发布](../releases.md) · [显示名与安装标识](windows-branding.md)

用户自 2026-10-10 起暂缓干净安装、缺少 WebView2、MSI 与卸载重装任务。本文是恢复时的操作清单，不表示已通过。使用可重置 Windows 虚拟机或已有 Windows Sandbox；重新构建当时版本并记录包哈希，不能用旧包证明当前界面。

## 准备

在仓库根运行 `./scripts/prepare-windows-sandbox.ps1`，可用 `-InstallerPath` 指定 MSI。脚本复制包、哈希和清单到独立 `target/sandbox-acceptance-<id>/package`，生成 `.wsb`；启动后只读映射到客户系统 `C:\LLM-Nest-Package`。脚本不启用 Windows 功能或代为启动 Sandbox，网络沿用其默认行为。

只映射验收包，不携带开发目录、个人 `.env`、会话或 API Key。关闭 Sandbox 会丢弃数据，结束前保存结果。

记录 Windows 版本/架构/显示缩放/权限、WebView2 是否预装及版本、安装包 SHA-256 和网络条件。干净用户配置不等于缺少 WebView2；系统预装时将缺失分支标为未测。

## 操作与通过条件

1. NSIS 和 MSI 分别在全新环境安装。记录目录、安装提示及退出结果；NSIS 默认当前用户，MSI 默认系统级安装。
2. 从开始菜单 `LLM Nest` 启动，进程为 `LLM-Nest.exe`，不依赖项目目录、Node、Rust 或开发服务器，进入消息/通讯录界面。
3. 空模型配置下创建未绑定 AI，真人消息可保存并显示执行不可用提示，不生成模拟回复。配置模型和密钥后，以独立模拟 provider 检查私聊、群聊、账号管理及重启历史；真实 API 须另有授权。
4. 检查深浅主题、800×600、长标题、拖动/调整大小/最大化还原、焦点切换及不同显示缩放。Windows 11 普通窗口圆角细边框，上下贴满工作区时矩形细描边，最大化/全屏不叠加；Windows 10 回退细边框。覆盖侧边任务栏、跨显示器及仅碰一条边的情况。
5. 检查图片/文件选择和拖放、草稿重开、原生保存取消/覆盖及导出；生成中退出后保留中断片段，不自动重发。
6. 覆盖安装后配置与历史保留；卸载后程序/快捷方式/卸载登记移除；原路径重装后检查数据保留。分别验证旧 NSIS→新 NSIS、旧 MSI→新 MSI，不把交叉安装当作升级通过。
7. 真正缺少 WebView2 时检查下载安装、启动，以及断网失败提示和重试。当前 `downloadBootstrapper` 需要联网，不是完整离线安装包。

参考 [Tauri WebView2 安装选项](https://v2.tauri.app/distribute/windows-installer/#webview2-installation-options)及 [Windows Sandbox 映射配置](https://learn.microsoft.com/en-us/windows/security/application-security/application-isolation/windows-sandbox/windows-sandbox-configure-using-wsb-file)。
