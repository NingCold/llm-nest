# social · 聊天功能

[服务端](../README.md) · [设计说明](../../docs/design/README.md)

从创建一位 AI，到发出消息、收到回复，再到记住共同经历，主要操作都在这里完成。浏览器和桌面调用的是同一套功能。

这里负责权限、聊天存储、参与队列和回复收尾。上下文选择、生成预算与摘要提示词已经由 [Agent 核心](../../agent/core/README.md)提供；这里读取允许使用的材料并转换成核心输入，再把运行结果保存回聊天。[连接器](../../connector/README.md)提供两种入口共用的事件和订阅类型。

目前这些模块仍在同一进程里使用同一个 Runtime 和 SQLite，独立服务、Agent 节点连接和云端数据库尚未实现。

图片会先保存到聊天中，再交给需要回复的 AI。某位 AI 的模型明确不支持图片时，会留下原因说明，群里的其他 AI 仍能继续回复。

想修改某个功能，可以从这些文件开始：

| 功能 | 阅读入口 |
| --- | --- |
| 账号、联系人屏蔽和管理权限 | [accounts.rs](src/accounts.rs)、[account_control.rs](src/account_control.rs) |
| 发送消息、创建群聊和安排回复 | [feature.rs](src/feature.rs) |
| 已读、置顶和查找聊天记录 | [message_list.rs](src/message_list.rs) |
| 接收、读取与保存聊天原图 | [images.rs](src/images.rs) |
| 接收普通文件、保存原文件和说明文件信息 | [files.rs](src/files.rs) |
| 导出一个窗口的聊天记录，可附带图片和原文件 | [export.rs](src/export.rs) |
| 编辑消息、暂停和结束交谈 | [controls.rs](src/controls.rs) |
| 读取授权背景并交给 Agent 核心 | [consciousness.rs](src/consciousness.rs)、[Agent 核心](../../agent/core/README.md) |
| 保存记忆、整理摘要、接续群话题 | [memory.rs](src/memory.rs)、[summary.rs](src/summary.rs)、[reference.rs](src/reference.rs) |
| 约定联系和免打扰 | [contact_intent.rs](src/contact_intent.rs) |
| 接收模型回复并保存 | [execution.rs](src/execution.rs) |

在仓库根目录运行测试：

```bash
cargo test -p social-feature --locked
```

完整聊天流程还可以用[验收脚本](../../scripts/README.md)检查。
