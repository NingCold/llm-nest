# 设计说明

[文档首页](../README.md) · [项目首页](../../README.md)

这组文档说明账号如何延续、聊天如何保存、AI 如何使用共同经历，以及什么时候开口。[云端社交与 Agent 接入计划](../project/cloud-plan.md)第 0–5 步的功能已实现，包括云端聊天、独立 AI 节点、意识系统、账号管理和数据迁移；验收范围与剩余检查见进度文档。各专项文档区分当前云端行为与保留的本地规则。

先读[设计总纲](overview.md)，再按问题查阅专项设计。当前能力见[已完成](../project/completed.md)，近期工作见[当前工作](../project/current.md)，待验收与未来候选见[候选清单](../project/backlog.md)。验证环境和证据从[验证入口](../validation/README.md)进入。

| 问题 | 文档 |
| --- | --- |
| 端到端加密、资料同步与同机离线私聊 | [端到端加密方案](encryption.md) · [同机聊天与加密执行计划](../project/secure-local-plan.md) |
| 账号、公开资料、联系人与管理员 | [身份与关系](accounts.md) |
| 私聊、群聊、消息、搜索与导出 | [聊天与消息](conversations.md) |
| 上下文、记忆、摘要、引用与遗忘 | [意识系统（Consciousness）](consciousness.md) |
| 接话、等待、额度与约定联系 | [参与与调度](participation.md) |
| 模型请求、流式回复、取消与收尾 | [AI 执行](execution.md) |
| 数据库、事务、迁移、草稿与备份 | [存储与迁移](storage.md) |
| HTTP、IPC、DTO、事件与幂等 | [API 与事件](api.md) |
| 导航、交互、附件与桌面窗口 | [界面与桌面](frontend.md) |
| 启动、配置快照、资源注入与关闭 | [运行与配置](runtime.md) |

同机文字私聊与断网接续已获授权实施，第一批已实现并通过自动验证，实际桌面操作仍待验收。它仍采用当前云端明文存储，不包含端到端加密。D1–D9 与 S1–S5 是后续加密批次的设计基线；局域网直连已列入待办，第一批不包含离线重新登录、附件、引用或群聊。

搭建和运行见[开发指南](../development.md)，配置格式见[配置指南](../configuration.md)，打包与发布见[发布指南](../releases.md)。修改约束由 [AGENTS](../../AGENTS.md) 维护。
