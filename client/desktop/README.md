# 桌面应用

[应用界面](../README.md) · [开发指南](../../docs/development.md)

桌面应用使用与 Web 相同的 CloudApp。启动后填写独立云端服务的 HTTPS 地址，注册或登录后使用真人私聊。默认页面不会初始化旧本地 Runtime，也不要求本机模型配置。

[原生云端网络层](src-tauri/src/cloud.rs) 负责 JSON、SSE 和附件传输，会话 Cookie 仅保存在进程内存，不交给网页脚本；重启应用后需要重新登录。服务器地址会在本机保存。下载和聊天导出通过系统“另存为”保存，取消、失败和写入成功分别反馈。

文字与附件草稿按服务器、实际账号和窗口保存在本机 IndexedDB。关闭窗口前先等待草稿保存；失败时保留窗口并提示重试。关闭窗口不停止远端云端服务。当前没有独立本机 Agent 或托盘后台运行功能。

## 开发

准备好 Rust MSVC（Windows）、Node.js、pnpm 和平台依赖，在仓库根目录运行：

```bash
pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/desktop install --frozen-lockfile
pnpm --dir client/desktop exec tauri dev
```

Windows/Linux 环境准备见[开发指南](../../docs/development.md)。测试站点使用私有 CA 时，可在启动进程前设置 `CLOUD_CA_CERT_FILE` 指向测试 CA 的 PEM 文件；该选项只用于授权测试，不关闭 TLS 校验，也不打包私有证书。

旧本地 IPC 和 Runtime 仍保留为延迟初始化能力，供后续迁移维护，不是默认聊天通路。窗口和打包代码见 [src-tauri](src-tauri/README.md)，发布与图标见[发布指南](../../docs/releases.md)和[图标说明](../../assets/branding/README.md)。
