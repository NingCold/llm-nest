# LLM Nest 文档

[返回项目首页](../README.md)

使用方法、设计说明和项目进度分别放在下面几个入口。第一次使用，从使用指南开始即可。

## 按你想做的事查找

| 想做什么 | 从哪里开始 |
| --- | --- |
| 注册登录、私聊、发送附件和保存聊天 | [使用指南](usage.md) |
| 部署一个仅内网可用的测试服务 | [部署说明](../server/cloud/deploy/README.md) |
| 创建 AI、配置本机或远程 Agent | [使用指南](usage.md#创建-ai-并连接-agent) · [节点说明](../agent/node/README.md)；底层与旧宿主配置见[配置参考](configuration.md) |
| 从源码运行、修改界面或检查改动 | [开发指南](development.md) |
| 带着旧聊天进入云端，或为 AI 更换运行设备 | [迁移指南](migration.md) |
| 找到某项功能对应的代码 | [客户端](../client/README.md) · [服务端](../server/README.md) · [Agent](../agent/README.md) · [连接器](../connector/README.md) · [共用模块](../crates/README.md) |
| 制作安装包、准备一个版本 | [构建与发布](releases.md) |
| 理解账号、聊天和意识系统的设计 | [设计说明](design/README.md) |
| 了解同机文字私聊、断网续聊和加密安排 | [同机聊天与加密执行计划](project/secure-local-plan.md)；设备、恢复和资料同步规则见[加密方案](design/encryption.md)，实际验证与部署状态见[当前安排](project/current.md) |
| 了解已做完什么、现在做什么、以后可能做什么 | [已完成](project/completed.md) · [当前安排](project/current.md) · [待办与候选](project/backlog.md) |
| 查看云端社交、独立 AI 账号与 Agent 接入的分步安排 | [云端社交与 Agent 接入计划](project/cloud-plan.md) |
| 查看下一轮客户端、服务端与 Agent 的三仓安排 | [拆仓计划](project/repository-split-plan.md) |
| 检查功能，了解 Windows 和 Linux 的验证情况 | [验证指南](validation/README.md) |

参与代码修改时，也请阅读 [AGENTS.md](../AGENTS.md) 中的项目约定。

## 文档怎样维护

使用指南写当前可用的操作，设计说明区分已确认的目标与现有实现，项目进度写安排，验证文档写已检查的环境和仍缺少的检查。一个话题只维护一份主要说明，其他地方用链接引用。

完成一批工作后，更新对应说明和进度；有新的验证结果时，更新相应平台记录。无需为每个小改动新建一篇报告。已经过时的过程记录从目录中移除，需要追溯时查 Git 历史。
