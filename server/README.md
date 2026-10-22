# 服务端

[项目首页](../README.md) · [开发指南](../docs/development.md)

[cloud](cloud/README.md) 是默认客户端连接的独立云端服务，负责邀请注册、邮箱验证、登录会话、联系人、真人与 AI 私聊、节点配对/WSS、任务开始与结果及持久事件同步。社交数据使用 PostgreSQL，附件保存在独立持久目录；服务不加载模型或本地 Runtime。

云端服务可使用 [Dockerfile](cloud/Dockerfile) 和 [Compose](cloud/deploy/compose.yaml) 部署。当前测试部署仅通过 Tailscale 的 HTTPS 8443 端口访问，使用测试 CA 和私有测试收件箱，不等同于公开邮件服务上线。

[http](http/README.md) 与 [social](social/README.md) 保留原来的本地 HTTP/SocialFeature、SQLite 和模型协调实现，供维护、回归与后续 Agent 迁移参考。`web-server` 不是默认云端客户端的后端。独立 Agent 的运行入口见[节点说明](../agent/node/README.md)。群聊和意识资料迁入尚未完成；在线内部配置、管理员授权界面和桌面托盘属于后续步骤。
