# Web 界面

[应用界面](../README.md) · [开发指南](../docs/development.md)

默认入口是 [CloudApp](src/cloud/App.tsx)，提供邀请注册与邮箱验证、登录及密码找回、联系人、真人与 AI 的私聊和群聊、附件、消息搜索与导出。聊天详情提供记忆、摘要、接话设置、暂停与结束、一次性约定和免打扰。浏览器直接访问同源 `/api`，桌面也使用同一套页面，通过原生网络层转发相同 API。

云端数据沿用原有消息/通讯录布局和视觉样式，账号位于左下角，设置提供外观与关于。“我的账号 → 我管理的 AI”提供公开资料、管理员、角色与模型、意识资料及只读聊天查看。未启用加密时角色和模型需在线节点确认保存；加密资源另显示云端保存与节点应用状态；模型 Key 只在运行设备填写。桌面共用此页面，另外提供本机 Agent 配置与启动入口。

登录时可勾选“在此设备保持登录 30 天”，默认不勾选，也不保存账号密码。本机加密资料仍由独立口令保护；在自己的设备上，可于“可信设备与恢复”另行开启 30 天自动解锁。手动锁定后须重新输入口令，退出会清除自动解锁材料，原密钥库保留。新设备仍需要可信设备授权或恢复文件；浏览器的自动解锁材料留在当前浏览器资料中，只适合自己的设备。

点击左下角的锁即可暂时锁定聊天，解锁后回到原窗口。锁屏上也可以进入恢复文件页面。

消息发送前会把文字、附件和不可变重试命令保存到 IndexedDB，按服务器、实际账号和窗口隔离。断线或结果未确认时保留原命令供手动重试，不自动发送。草稿不跨设备同步。

## 开发

聊天加密需显式开启，并先完成可信设备与恢复设置。客户端使用官方 Matrix SDK，强制验证设备与账号 master 双签；失败时不降级发送明文。私聊、群聊、附件、跨窗引用及约定共用加密协议，搜索和完整导出在客户端解密。角色、模型、记忆和各 AI 摘要使用密文资源 CAS，可离线编辑后待同步。默认聊天草稿仍是本机明文，旧明文历史不会自动转换。详见[加密契约](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/crypto-contract.md)与[当前安排](../docs/project/current.md)。

在仓库根目录执行：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir web dev
```

开发页为 `http://localhost:1420`，`/api` 默认代理到 `http://127.0.0.1:8787`；启动 `cloud-server` 时将 `CLOUD_PUBLIC_URL` 设为页面地址。可通过 `LLMN_API` 改变代理目标。后端配置见[云端服务](https://github.com/NingCold/llm-nest-server/blob/main/server/README.md)。

```bash
pnpm --dir web typecheck
pnpm --dir web test
pnpm --dir web build
```

构建输出 `dist`，由 `cloud-server` 提供。云端状态、网络和样式位于 [src/cloud](src/cloud/App.tsx)；公共入口见 [src/App.tsx](src/App.tsx)。

`src/chat` 保存共用聊天组件、视觉样式和草稿存储。云端状态和权限仍由 `src/cloud` 管理。

服务端部署使用固定版本的 Web 包，构建后执行：

```bash
python scripts/package-web.py --dist web/dist --version 0.1.1-alpha.11 --output /新的产物目录
```

以上命令从仓库根目录运行，版本须与实际客户端构建一致。Web 包和 `web-manifest.json` 一起交给服务端，使用方式见[部署说明](https://github.com/NingCold/llm-nest-server/blob/main/server/deploy/README.md)。
