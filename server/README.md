# 服务端

[项目首页](../README.md) · [开发指南](../docs/development.md)

[cloud](cloud/README.md) 是默认客户端连接的独立云端服务，提供邀请注册、登录、联系人、真人与 AI 的私聊和群聊，以及账号管理、接话设置和一次性约定。聊天保存在 PostgreSQL，附件保存在独立目录。服务负责把任务交给获得授权的 Agent，不直接调用模型。

云端服务可使用 [Dockerfile](cloud/Dockerfile) 和 [Compose](cloud/deploy/compose.yaml) 部署。当前测试部署仅通过 Tailscale 的 HTTPS 8443 端口访问，使用测试 CA 和私有测试收件箱，不等同于公开邮件服务上线。

加密聊天与 AI 资料由客户端和 Agent 加解密，服务器保存密文、公开密钥和必要的同步信息；节点保留工作副本，模型 Key 留在运行设备。未启用加密的窗口仍沿用原存储，旧历史不会自动转换。节点运行方法见[节点说明](../agent/node/README.md)，实际验证和部署范围见[当前安排](../docs/project/current.md)。

旧本地库通过[迁移工具](../docs/migration.md)进入云端，内部意识资料交给节点。部署使用[固定版本的 Web 产物](cloud/deploy/README.md)，服务端 Docker 构建上下文只包含云端服务、连接器和必要领域类型。
