# 实现与验收记录

[文档首页](README.md)

这里收录各阶段的实现报告和测试结果，按报告日期阅读。当前使用方式见[项目首页](../README.md)，最新进度见[正在推进](social-refactor/in-progress.md)。

每份报告只证明其中列出的环境与场景。历史报告中的产物哈希可能对应旧版本，不应与最新安装包混用。

| 文档 | 范围 |
| --- | --- |
| [Windows 桌面体验验收（2026-10-20）](social-refactor/windows-desktop-experience-2026-10-20.md) | 当前 Tauri EXE 的附件选择、草稿关闭恢复、图片处理、原生保存与覆盖失败、中断回复恢复；Linux 与安装任务暂缓 |
| [首版本地社交体验收尾](social-refactor/first-release-experience.md) | 完整流程复验、五类实际问题修复、浏览器附件草稿重启与实际 ZIP 下载；平台验收仍独立保留 |
| [通用文件附件第二批](social-refactor/file-message-export.md) | 图片/文件独立导出选项、同一快照原字节、重名与空文件、Markdown 链接和 JSON v3、编译产物清理 |
| [通用文件附件第一批](social-refactor/file-messages.md) | 文件选择/草稿/发送/保存、原字节下载、只读元信息、权限与修订复核 |
| [图片消息第五批](social-refactor/image-capabilities.md) | 目录图片能力、人工覆盖与来源、聊天发送提示、历史图片/引用检查、混合能力群聊与隔离验收 |
| [图片消息第四批](social-refactor/image-processing.md) | 发送前缩放与压缩、原图/结果预览、确认后保存、透明与照片方向、多图总量验证 |
| [图片消息第三批](social-refactor/image-drafts.md) | 本机图片草稿持久保存、跨刷新/重开恢复、完整命令重试、失败提示与桌面关闭等待 |
| [图片消息第二批](social-refactor/image-message-export.md) | 原图另存为、Markdown/JSON 携图 ZIP、版本与权限复核、Web 下载及桌面文件保存 |
| [图片消息第一批](social-refactor/image-messages.md) | 选图/预览/发送、持久历史、群话题图片接续、四种模型协议与隔离验收 |
| [单窗口聊天导出](social-refactor/conversation-export.md) | 完整历史的 Markdown / JSON、附件占位、Web 下载与桌面另存为 |
| [联系人屏蔽](social-refactor/contact-blocking.md) | 私聊停止、共同群正常、双方约定取消、解除不补发，Web/IPC 与 SQLite v13 |
| [消息列表与正文搜索](social-refactor/message-list.md) | 个人已读/未读与置顶、完整正文分页搜索、原消息定位、Web/IPC 与 SQLite v12 |
| [README 与旧版残余整理](social-refactor/readme-cleanup.md) | 面向读者的模块入口、设计与历史分离、闲置前端清理 |
| [M1 私聊实现与验证](social-refactor/m1-private-chat.md) | 首条真实 Web/HTTP 链路、模型模拟、SQLite v2 与恢复 |
| [M1 群聊实现与验证](social-refactor/m1-group-chat.md) | 共享群历史、结构化 @ / 回复、持久 FIFO、多 AI 上下文与 v3/v4 迁移 |
| [M1 消息与控制实现验证](social-refactor/m1-message-controls.md) | 修订/撤回、来源失效、成员/窗口暂停、结束交谈、归档/恢复及 v5 迁移 |
| [M1 实时 IPC 与桌面](social-refactor/m1-desktop-ipc.md) | 共用订阅、Channel 确认与恢复、共用社交 GUI、退出保存及双平台编译/测试 |
| [M1 账号管理与联系人关系](social-refactor/m1-account-management.md) | 共同资料、管理授权与只读视角、关系移除、管理员停用及 v6 迁移 |
| [M2 显式记忆](social-refactor/m2-explicit-memory.md) | 接手核对、按 AI/空间保存与读取、来源失效、遗忘、Web/IPC 及 v7 迁移 |
| [M2 窗口分段摘要](social-refactor/m2-window-summaries.md) | 专用模型与预算、后台任务、来源失效、上下文读取、详情管理及 v8 迁移 |
| [M2 群话题接续](social-refactor/m2-cross-window-continuation.md) | 唯一私聊引用草稿、原文/摘要来源、共同成员权限、运行失效、Web/IPC 及 v9 迁移 |
| [旧架构退役与历史导入](social-refactor/legacy-retirement.md) | 新版唯一运行链路、旧产品移除、所选历史导入及完整回归 |
| [M3 约定联系](social-refactor/m3-contact-intents.md) | 独立授权、一次性联系、免打扰、到期/过期、共享额度、取消与崩溃恢复、Web/IPC 及 v11 迁移 |
| [M3 参与节奏](social-refactor/m3-participation-rhythm.md) | 私聊合并、有限自动群接话、空闲结束、持久额度、可控等待、Web/IPC 及 v10 迁移 |
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
