# 设计说明

[设计总纲](overview.md)说明一条消息怎样经过客户端、云端和 Agent，以及身份、权限和数据分别由谁负责。想了解首版能做什么，见[已有能力](../project/completed.md)。

| 问题 | 文档 |
| --- | --- |
| 端到端加密、资料同步与同机离线私聊 | [端到端加密方案](encryption.md) |
| 账号、公开资料、联系人与管理员 | [身份与关系](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/accounts.md) |
| 私聊、群聊、消息、搜索与导出 | [聊天与消息](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/conversations.md) |
| 上下文、记忆、摘要、引用与遗忘 | [意识系统（Consciousness）](https://github.com/NingCold/llm-nest-agent/blob/main/docs/design/consciousness.md) |
| 接话、等待、额度与约定联系 | [参与与调度](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/participation.md) |
| 模型请求、流式回复、取消与收尾 | [AI 执行](https://github.com/NingCold/llm-nest-agent/blob/main/docs/design/execution.md) |
| 数据库、事务、迁移、草稿与备份 | [存储与迁移](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/storage.md) |
| HTTP、IPC、DTO、事件与幂等 | [API 与事件](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/api.md) |
| 导航、交互、附件与桌面窗口 | [界面与桌面](frontend.md) |
| 启动、配置快照、节点管理与关闭 | [运行与配置](https://github.com/NingCold/llm-nest-agent/blob/main/docs/design/runtime.md) |
