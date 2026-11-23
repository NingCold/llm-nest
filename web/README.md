# Web 界面

[项目首页](../README.md) · [使用指南](../docs/usage.md) · [开发指南](../docs/development.md)

Web 提供邀请注册、邮箱验证、登录与密码找回，以及联系人、真人和 AI 私聊、群聊、附件、搜索和导出。聊天详情可管理记忆、摘要、接话与一次性约定；“我的账号 → 我管理的 AI”管理获授权的 AI。

浏览器访问同源 `/api`，桌面使用同一套 React 页面，经原生网络层访问相同 API。设备解锁、恢复和加密规则见[加密设计](../docs/design/encryption.md)，公开包与当前源码的差异见[当前安排](../docs/project/current.md)。

## 从源码运行

在仓库根目录执行：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web dev
```

打开 `http://localhost:1420`。`/api` 默认代理到 `http://127.0.0.1:8787`，可用 `LLMN_API` 更改目标；服务端的 `CLOUD_PUBLIC_URL` 设为页面地址。后端准备见[服务端说明](https://github.com/NingCold/llm-nest-server/blob/main/server/README.md)。

```bash
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web build
```

输出为 `web/dist`。部署时将固定版本的 Web 包和清单交给服务端，见[Web 打包步骤](../docs/releases.md#固定-web-产物与服务端构建)。

## 代码入口

- [src/App.tsx](src/App.tsx) 加载 [CloudApp](src/cloud/App.tsx)，负责连接、登录、锁屏和恢复。
- [src/cloud](src/cloud/App.tsx) 管理账号、聊天状态、权限、网络与加密。
- [src/chat](src/chat/draft-storage.ts) 提供共用聊天组件、样式和 IndexedDB 草稿。草稿按服务器、账号和窗口隔离，发送前保存冻结命令；未知结果只手动重试原请求。

桌面专属的本机 Agent、系统保存和窗口行为见[桌面应用](../desktop/README.md)。
