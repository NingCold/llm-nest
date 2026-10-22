# 共用模块

[项目首页](../README.md) · [开发指南](../docs/development.md)

这里放共用的数据定义，以及现有本地应用仍在使用的存储和运行时。客户端、服务端、Agent 和连接器已有各自目录。

| 模块 | 做什么 |
| --- | --- |
| [common](common/README.md) | 消息内容、用量和时间等通用数据 |
| [social](social/README.md) | 账号、联系人、聊天、记忆和约定的数据定义 |
| [storage](storage/README.md) | 保存和读取聊天数据，导入历史记录 |
| [runtime](runtime/README.md) | 启动服务、管理配置、完成退出保存 |

账号和聊天操作在 [server/social](../server/social/README.md)，模型调用在 [agent](../agent/README.md)，订阅事件在 [connector](../connector/README.md)。云端改造还会继续拆开当前运行时和存储，具体安排见[改造计划](../docs/project/cloud-plan.md)。
