# Web 界面

[应用界面](../README.md) · [开发指南](../../docs/development.md)

默认入口是 [CloudApp](src/cloud/App.tsx)，提供邀请注册与邮箱验证、登录及密码找回、联系人、真人私聊、附件、消息搜索与导出。浏览器直接访问同源 `/api`，桌面也使用同一套页面，通过原生网络层转发相同 API。

云端数据沿用原有消息/通讯录布局和视觉样式，账号位于左下角，设置提供外观与关于。CloudApp 管理认证、云端状态和草稿，`AuthPage` 提供认证页面，`Workspace` 复用原有侧栏、公共组件和样式。此次先验证 Web，尚未更新已安装桌面程序。

消息发送前会把文字、附件和不可变重试命令保存到 IndexedDB，按服务器、实际账号和窗口隔离。断线或结果未确认时保留原命令供手动重试，不自动发送。草稿不跨设备同步。

## 开发

在仓库根目录执行：

```bash
pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/web dev
```

开发页为 `http://localhost:1420`，`/api` 默认代理到 `http://127.0.0.1:8787`；启动 `cloud-server` 时将 `CLOUD_PUBLIC_URL` 设为页面地址。可通过 `LLMN_API` 改变代理目标。后端配置见[云端服务](../../server/cloud/README.md)。

```bash
pnpm --dir client/web typecheck
pnpm --dir client/web test
pnpm --dir client/web build
```

构建输出 `dist`，由 `cloud-server` 提供。云端状态、网络和样式位于 [src/cloud](src/cloud/App.tsx)；公共入口见 [src/App.tsx](src/App.tsx)。

`src/social` 的侧栏、公共组件、视觉样式和草稿存储已由云端页面复用；其本地业务页面和图片处理组件继续保留，供回归及后续迁移使用。其 AI、群、记忆、摘要、约定联系和模型设置不是当前云端页面的已开放功能。`build:social-runtime` 只是构建目录名，不会切换默认产品入口。
