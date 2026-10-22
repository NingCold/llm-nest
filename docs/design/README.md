# 设计说明

[文档首页](../README.md) · [项目首页](../../README.md)

这组文档说明账号如何延续、聊天如何保存、AI 如何使用共同经历，以及什么时候开口。四部分代码已拆分，独立云端服务已接入 PostgreSQL 与客户端，AI 节点与私聊已实现并通过本机节点与云端 Web 验证；群聊、显式 @/回复、屏蔽与 AI 启停已实现，集成验收状态见进度文档；意识资料、自动接话与完整管理继续按[云端社交与 Agent 接入计划](../project/cloud-plan.md)实施。各模块开头说明当前云端能力，后续保留的本地规则供相应步骤迁移时核对。

先读[设计总纲](overview.md)，再按问题查阅专项设计。当前能力见[已完成](../project/completed.md)，近期工作见[当前工作](../project/current.md)，待验收与未来候选见[候选清单](../project/backlog.md)。验证环境和证据从[验证入口](../validation/README.md)进入。

| 问题 | 文档 |
| --- | --- |
| 账号、公开资料、联系人与管理员 | [身份与关系](accounts.md) |
| 私聊、群聊、消息、搜索与导出 | [聊天与消息](conversations.md) |
| 上下文、记忆、摘要、引用与遗忘 | [意识系统（Consciousness）](consciousness.md) |
| 接话、等待、额度与约定联系 | [参与与调度](participation.md) |
| 模型请求、流式回复、取消与收尾 | [AI 执行](execution.md) |
| 数据库、事务、迁移、草稿与备份 | [存储与迁移](storage.md) |
| HTTP、IPC、DTO、事件与幂等 | [API 与事件](api.md) |
| 导航、交互、附件与桌面窗口 | [界面与桌面](frontend.md) |
| 启动、配置快照、资源注入与关闭 | [运行与配置](runtime.md) |

搭建和运行见[开发指南](../development.md)，配置格式见[配置指南](../configuration.md)，打包与发布见[发布指南](../releases.md)。修改约束由 [AGENTS](../../AGENTS.md) 维护。
