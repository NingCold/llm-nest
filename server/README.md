# 服务端

[项目首页](../README.md) · [开发指南](../docs/development.md)

[cloud](cloud/README.md) 是默认客户端连接的独立云端服务，提供邀请注册、登录、联系人、真人与 AI 的私聊和群聊，以及账号管理、接话设置和一次性约定。聊天保存在 PostgreSQL，附件保存在独立目录。服务负责把任务交给获得授权的 Agent，不直接调用模型。

云端服务可使用 [Dockerfile](cloud/Dockerfile) 和 [Compose](cloud/deploy/compose.yaml) 部署。当前测试部署仅通过 Tailscale 的 HTTPS 8443 端口访问，使用测试 CA 和私有测试收件箱，不等同于公开邮件服务上线。

角色、模型参数、记忆和摘要保存在 Agent。管理员可在线维护这些资料，云端只转发请求；模型 Key 留在运行设备。节点的运行方法见[节点说明](../agent/node/README.md)。

[http](http/README.md) 与 [social](social/README.md) 保留原来的本地 HTTP/SocialFeature、SQLite 和模型协调实现，供回归及旧库迁移核对。`web-server` 不是默认云端客户端的后端；旧记录通过[迁移工具](../docs/migration.md)进入云端，内部意识资料交给节点。
