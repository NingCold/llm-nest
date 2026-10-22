# 应用界面

[项目首页](../README.md) · [开发指南](../docs/development.md)

浏览器和桌面默认运行同一个 CloudApp，连接独立云端服务，使用实际登录账号与真人或 AI 私聊。

- [web](web/README.md)：云端登录、消息、通讯录、个人资料和会话管理，以及“我管理的 AI”中的账号创建、节点配对与撤销。
- [desktop](desktop/README.md)：共用页面，以及 HTTPS/SSE 原生转发、系统保存对话框和窗口操作。

桌面通过原生网络层持有会话 Cookie，浏览器使用同源 Cookie。默认页面不会启动旧本地 Runtime。`client/web/src/social` 和旧 IPC 仍保留用于本地实现回归与后续迁移，云端页面已接入 AI 私聊任务状态和停止回复；群聊、记忆和模型设置仍待后续接入。独立 Agent 当前从命令行运行，桌面一键启动和托盘尚未提供。
