# 桌面应用

[应用界面](../README.md) · [开发指南](../../docs/development.md)

这里把 LLM Nest 的页面做成桌面应用。打开窗口时，本地聊天服务也会一起启动；页面本身来自 [web](../web/README.md)。

图片选择、预览、发送前缩放与压缩，以及聊天阅读与 Web 共用界面。处理图片时可以对比原图和结果，确认后再加入草稿。未发送的文字与图片会保存在本机，重开应用后可以继续编辑。关闭时会先等草稿保存完毕；如果保存失败，窗口会留下来提示重试。

保存原图或导出聊天时会打开系统的“另存为”，可以选择文件名和保存位置。导出时可分别勾选“包含图片”和“包含文件”，把记录和所选附件一起保存为 ZIP。

“添加文件”可以发送普通文档、文本和压缩包，文件草稿也会在重开后恢复。聊天里的“保存文件”通过系统对话框另存原文件。目前 AI 只知道文件名、类型和大小，文档内容阅读尚未提供。

聊天时会提示 AI 是否支持看图，设置中的“模型图片能力”可以跟随目录或人工修正。桌面与 Web 使用相同的能力信息和保存方式。

准备好 Rust、Node.js、pnpm 和系统依赖后，在仓库根目录运行：

```bash
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
pnpm --dir frontends/tauri exec tauri dev
```

Windows 和 Linux 的环境准备步骤见[开发指南](../../docs/development.md)。首次启动后，可以在设置里添加模型服务。

窗口、应用启动和本地命令的代码在 [src-tauri](src-tauri/README.md)。制作安装包请看[构建与发布](../../docs/releases.md)，更新图标请看[图标说明](../../assets/branding/README.md)。
