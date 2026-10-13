# 共用模块

[项目首页](../README.md) · [开发指南](../docs/development.md)

这里放的是应用反复用到的基础部分，例如连接模型、保存聊天、读取设置。浏览器和桌面应用共用这些代码。

| 模块 | 做什么 |
| --- | --- |
| [common](common/README.md) | 消息内容、用量和时间等通用数据 |
| [social](social/README.md) | 账号、联系人、聊天、记忆和约定的数据定义 |
| [ai-client](ai-client/README.md) | 连接不同模型服务，接收回复 |
| [events](events/README.md) | 告知界面新消息、资料变化和回复进展 |
| [storage](storage/README.md) | 保存和读取聊天数据，导入历史记录 |
| [runtime](runtime/README.md) | 启动服务、管理配置、完成退出保存 |
| [web-server](web-server/README.md) | 让浏览器访问本地应用 |
| [tools](tools/README.md) | 执行工具的基础代码，供后续工具功能使用 |

账号和聊天的具体操作在 [features/social](../features/social/README.md)。各模块 README 提供阅读入口，详细设计可从[设计文档](../docs/social-refactor/README.md)查找。
