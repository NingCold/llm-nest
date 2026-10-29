# LLM Nest 图标

[应用资源](../README.md)

[app-icon.svg](app-icon.svg) 是应用图标的源文件。修改它后，在仓库根目录运行：

```bash
pnpm --dir desktop icons
```

脚本会生成桌面使用的 ICO、ICNS 和 PNG，以及网页图标和界面标识。首次运行前，需要按[开发指南](../../docs/development.md)安装前端依赖。

生成的桌面图标位于 `desktop/src-tauri/icons`，网页资源位于 `web/public`。界面内的标识会随浅色、深色主题调整颜色。
