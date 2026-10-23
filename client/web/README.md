# Web 界面

[应用界面](../README.md) · [开发指南](../../docs/development.md)

默认入口是 [CloudApp](src/cloud/App.tsx)，提供邀请注册与邮箱验证、登录及密码找回、联系人、真人与 AI 的私聊和群聊、附件、消息搜索与导出。聊天详情提供记忆、摘要、接话设置、暂停与结束、一次性约定和免打扰。浏览器直接访问同源 `/api`，桌面也使用同一套页面，通过原生网络层转发相同 API。

云端数据沿用原有消息/通讯录布局和视觉样式，账号位于左下角，设置提供外观与关于。“我的账号 → 我管理的 AI”提供公开资料、管理员、角色与模型、意识资料及只读聊天查看。未启用加密时角色和模型需在线节点确认保存；加密资源另显示云端保存与节点应用状态；模型 Key 只在运行设备填写。桌面共用此页面，另外提供本机 Agent 配置与启动入口。

CloudApp 管理认证、云端状态和草稿，`AuthPage` 提供认证页面，`Workspace` 复用原有侧栏、公共组件和样式。开发构建和已安装版本的验证状态见[当前安排](../../docs/project/current.md)。

消息发送前会把文字、附件和不可变重试命令保存到 IndexedDB，按服务器、实际账号和窗口隔离。断线或结果未确认时保留原命令供手动重试，不自动发送。草稿不跨设备同步。

## 开发

私聊加密需显式开启，并先完成可信设备与恢复设置。客户端使用官方 Matrix SDK，强制验证设备与账号 master 双签；失败时不降级发送明文。角色、模型、记忆和各 AI 摘要使用密文资源 CAS，可离线编辑后待同步。默认聊天草稿仍是本机明文，SDK 密钥库使用独立口令保护。群、附件、跨窗引用、约定及完整搜索导出的加密尚未开放，旧明文历史不会自动转换。详见[加密契约](../../docs/design/crypto-contract.md)与[当前安排](../../docs/project/current.md)。

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

`src/social` 的侧栏、公共组件、视觉样式和草稿存储由云端页面复用。云端聊天详情已提供节点记忆、AI 私有摘要和群话题接续；它们使用云端权限与独立节点，不启动本地聊天服务。本地业务页面和图片处理组件仍供回归及后续迁移使用，`build:social-runtime` 不会切换默认产品入口。
