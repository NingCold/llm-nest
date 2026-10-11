# LLM-Nest 文档

[项目首页](../README.md) · [开发约定与架构](../AGENTS.md)

## 使用与开发

| 文档 | 解决什么问题 |
| --- | --- |
| [配置与模型路由](configuration.md) | 配置在哪里、如何放置密钥、接入模型和调整思考强度 |
| [开发指南](development.md) | 准备环境、运行四个入口、执行验证 |
| [构建与 GitHub 发布](releases.md) | 生成 Windows/Linux 安装包，运行 CI，创建版本草稿 |

## 社交转型设计与实现

首版为一个本地真人与多个 AI 私聊、群聊。先读 [总纲](social-refactor/README.md) 确认范围，再按状态文件与 [TODO](social-refactor/TODO.md) 查进度。专项文档维护所属模块的设计与当前契约，批次记录保存当时的实现和验证证据。

| 文档 | 负责内容 |
| --- | --- |
| [已完成](social-refactor/completed.md) / [正在推进](social-refactor/in-progress.md) / [尚未完成](social-refactor/pending.md) | 当前实现、活跃批次与剩余能力；区分预览、业务接入和平台验收 |
| [重构总纲](social-refactor/README.md) | 产品范围、统一术语、目标目录、依赖方向与里程碑 |
| [身份与关系](social-refactor/01-identity-and-relationships.md) | Actor、AI 档案、模型绑定与关系连续性 |
| [聊天与消息](social-refactor/02-conversations-and-messages.md) | 私聊/群聊、成员、发送者、修订与交谈生命周期 |
| [意识系统（Consciousness）](social-refactor/03-consciousness.md) | 系统边界、分层上下文、摘要、记忆、跨窗口召回与遗忘 |
| [参与与调度](social-refactor/04-participation-and-scheduling.md) | 发言规则、队列、等待/结束、约定联系与恢复 |
| [AI 执行](social-refactor/05-ai-execution.md) | 协议上下文、独立运行、工具、检查点与收尾 |
| [存储与迁移](social-refactor/06-storage-and-migration.md) | SQLite、事务、目录锁、备份与旧 JSON 导入 |
| [API 与事件](social-refactor/07-api-and-events.md) | HTTP/IPC、命令幂等、持久事件、订阅和补齐 |
| [界面与桌面](social-refactor/08-frontend-and-desktop.md) | 导航、状态、交互、共用前端与平台基线 |
| [运行时与配置](social-refactor/09-runtime-and-configuration.md) | 初始化、资源注入、快照、关闭与旧入口兼容 |
| [交付与验证](social-refactor/10-delivery-and-validation.md) | M0–M3、端到端验收、兼容收尾与待细化决策 |

## 实现与验收记录

每份报告只证明其中列出的环境与场景。历史报告中的产物哈希可能对应旧版本，不应与最新安装包混用。

| 文档 | 范围 |
| --- | --- |
| [M1 私聊实现与验证](social-refactor/m1-private-chat.md) | 首条真实 Web/HTTP 链路、模型模拟、SQLite v2 与恢复 |
| [M1 群聊实现与验证](social-refactor/m1-group-chat.md) | 共享群历史、结构化 @ / 回复、持久 FIFO、多 AI 上下文与 v3/v4 迁移 |
| [M1 消息与控制实现验证](social-refactor/m1-message-controls.md) | 修订/撤回、来源失效、成员/窗口暂停、结束交谈、归档/恢复及 v5 迁移 |
| [M1 实时 IPC 与桌面](social-refactor/m1-desktop-ipc.md) | 共用订阅、Channel 确认与恢复、共用社交 GUI、退出保存及双平台编译/测试 |
| [M1 账号管理与联系人关系](social-refactor/m1-account-management.md) | 共同资料、管理授权与只读视角、关系移除、管理员停用及 v6 迁移 |
| [M2 显式记忆](social-refactor/m2-explicit-memory.md) | 接手核对、按 AI/空间保存与读取、来源失效、遗忘、Web/IPC 及 v7 迁移 |
| [M1 独立生成参数](social-refactor/m1-generation-parameters.md) | 管理员按 AI 保存温度与输出上限、协议约束、意识预算与双适配验证 |
| [M1 共用界面现代化](social-refactor/m1-ui-modernization.md) | 中性双主题与蓝色、聊天/账号/设置布局、响应式与键盘检查 |
| [M1 桌面启动与界面](social-refactor/m1-desktop-surface.md) | 空白原生玻璃启动、内容内窗口控制、紧凑侧栏、空配置启动修复与 Windows 实测；安装任务暂缓 |
| [M1 桌面拖动与本机更新](social-refactor/m1-desktop-drag.md) | 扩大侧栏/头部拖动区域、保留交互与正文选字、备份更新现有程序及实际检查范围 |
| [M1 异常、安装包与平台](social-refactor/m1-stage-validation.md) | 双平台异常复验、四种包、Windows 覆盖升级与退出保存、GNOME 自动/人工确认及剩余干净 Windows 检查 |
| [Windows 本机卸载重装](windows-reinstall-2026-10-09.md) | NSIS 实际卸载重装、原目录/登记/快捷方式恢复、个人配置保留与隔离社交历史读取；不替代干净系统或 MSI 安装验收 |
| [M1 联系人生命周期](social-refactor/m1-contact-lifecycle.md) | 归档/恢复、移除默认归档、跨空间取消、历史与群身份保留及双适配验证 |
| [M0 领域与存储基础](social-refactor/m0-implementation.md) | 中立类型、事务/锁、旧 JSON 导入和跨平台验证 |
| [用户目录迁移与设置入口](home-directory-2026-09-16.md) | Windows/Linux 路径与迁移测试、目录入口、尚待点击确认的项目 |
| [Linux 桌面接续验收](linux-acceptance-2026-09-16.md) | 已有包、XWayland/Wayland 自动 GUI、修复重建与待人工项 |
| [Linux 首次构建与包验证](linux-acceptance-2026-09-13.md) | Ubuntu 24.04 构建基线、安装依赖、worker、无界面启动与待测 GUI |
| [Windows 上下贴边细边框](docked-window-frame-2026-09-13.md) | DWM/自绘细边框、窗口状态切换、用户确认的基线 |
| [Tauri Windows 安装验收](tauri-acceptance-2026-09-13.md) | Windows 安装包与首次启动，及仍未覆盖的安装场景 |
| [离线完整流程](acceptance-2026-09-13.md) | 模拟 provider 下的真实 HTTP、会话、配置与错误链路 |
| [ecnu-max 真实接口](live-acceptance-2026-09-13.md) | 真实模型的回复、思考、工具、取消和历史；需要密钥与调用额度 |

## 方案与历史

- [Linux 构建环境选择](linux-build-plan.md)：解释远程主机、容器与 WSL 的分工；当前结果以上面的验收报告为准。
- 目录中的其他日期报告是对应轮次的记录。代码当前约束请读 [AGENTS.md](../AGENTS.md)，无需逐篇拼接历史规则。
