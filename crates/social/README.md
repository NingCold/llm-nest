# social · 社交数据

[共用模块](../README.md)

账号、联系人、聊天窗口、消息和记忆都需要清楚的含义。这个模块把这些概念写成共用的数据类型，让保存记录、处理聊天和展示界面的代码说同一种语言。

- [identity.rs](src/identity.rs)：账号资料、联系人和 AI 设定。
- [conversation.rs](src/conversation.rs) 与 [message.rs](src/message.rs)：聊天窗口、成员和消息。
- [consciousness.rs](src/consciousness.rs)：记忆、摘要和它们引用的内容。
- [contact_intent.rs](src/contact_intent.rs)：约定联系的数据类型。

聊天和权限操作由 [cloud-server](../../server/cloud/README.md) 完成。设计背后的想法见[设计说明](../../docs/design/README.md)。

在仓库根目录运行测试：

```bash
cargo test -p social --locked
```
