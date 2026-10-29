# 共用数据类型

[项目首页](../README.md) · [开发指南](../docs/development.md)

- [common](common/README.md)：Agent 模型执行使用的消息内容、用量和时间类型。
- [social](social/README.md)：服务端、连接器和客户端使用的账号、聊天与意识资料定义。

聊天和权限由[云端服务](../server/cloud/README.md)处理，模型执行位于[Agent](../agent/README.md)。旧本地 Runtime 与存储宿主已移除；旧 SQLite 数据继续通过[迁移工具](../docs/migration.md)读取。
