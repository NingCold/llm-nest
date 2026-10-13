# social · 聊天功能

[应用功能](../README.md) · [设计与计划](../../docs/social-refactor/README.md)

从创建一位 AI，到发出消息、收到回复，再到记住共同经历，主要操作都在这里完成。浏览器和桌面调用的是同一套功能。

想修改某个功能，可以从这些文件开始：

| 功能 | 阅读入口 |
| --- | --- |
| 账号、联系人屏蔽和管理权限 | [accounts.rs](src/accounts.rs)、[account_control.rs](src/account_control.rs) |
| 发送消息、创建群聊和安排回复 | [feature.rs](src/feature.rs) |
| 已读、置顶和查找聊天记录 | [message_list.rs](src/message_list.rs) |
| 导出一个窗口的聊天记录 | [export.rs](src/export.rs) |
| 编辑消息、暂停和结束交谈 | [controls.rs](src/controls.rs) |
| 为 AI 准备对话背景 | [consciousness.rs](src/consciousness.rs) |
| 保存记忆、整理摘要、接续群话题 | [memory.rs](src/memory.rs)、[summary.rs](src/summary.rs)、[reference.rs](src/reference.rs) |
| 约定联系和免打扰 | [contact_intent.rs](src/contact_intent.rs) |
| 接收模型回复并保存 | [execution.rs](src/execution.rs) |

在仓库根目录运行测试：

```bash
cargo test -p social-feature --locked
```

完整聊天流程还可以用[验收脚本](../../scripts/README.md)检查。
