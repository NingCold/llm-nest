# LLM-Nest 文档

[项目首页](../README.md) · [开发约定与架构](../AGENTS.md)

## 使用与开发

| 文档 | 解决什么问题 |
| --- | --- |
| [配置与模型路由](configuration.md) | 配置在哪里、如何放置密钥、接入模型和调整思考强度 |
| [开发指南](development.md) | 准备环境、运行四个入口、执行验证 |
| [构建与 GitHub 发布](releases.md) | 生成 Windows/Linux 安装包，运行 CI，创建版本草稿 |

## 当前验收证据

每份报告只证明其中列出的环境与场景。历史报告中的产物哈希可能对应旧版本，不应与最新安装包混用。

| 文档 | 范围 |
| --- | --- |
| [Linux 桌面接续验收](linux-acceptance-2026-09-16.md) | 已有包、XWayland/Wayland 自动 GUI、修复重建与待人工项 |
| [Linux 首次构建与包验证](linux-acceptance-2026-09-13.md) | Ubuntu 24.04 构建基线、安装依赖、worker、无界面启动与待测 GUI |
| [Windows 上下贴边细边框](docked-window-frame-2026-09-13.md) | DWM/自绘细边框、窗口状态切换、用户确认的基线 |
| [Tauri Windows 安装验收](tauri-acceptance-2026-09-13.md) | Windows 安装包与首次启动，及仍未覆盖的安装场景 |
| [离线完整流程](acceptance-2026-09-13.md) | 模拟 provider 下的真实 HTTP、会话、配置与错误链路 |
| [ecnu-max 真实接口](live-acceptance-2026-09-13.md) | 真实模型的回复、思考、工具、取消和历史；需要密钥与调用额度 |

## 方案与历史

- [Linux 构建环境选择](linux-build-plan.md)：解释远程主机、容器与 WSL 的分工；当前结果以上面的验收报告为准。
- 目录中的其他日期报告是对应轮次的记录。代码当前约束请读 [AGENTS.md](../AGENTS.md)，无需逐篇拼接历史规则。
