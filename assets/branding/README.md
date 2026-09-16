# LLM Nest 图标

`app-icon.svg` 是图标的唯一编辑源，由项目根目录中用户设计的 `LLMNest_icon.png` 提取主体轮廓后，用 VTracer 转换为 8 条 Bézier 路径。SVG 没有嵌入位图；原图外围的黑色角块已去掉，改为透明圆角外缘、白色底板和黑色主体。

在 `frontends/tauri` 安装依赖后运行 `pnpm icons`，同步生成：

- `src-tauri/icons`：Windows ICO、macOS ICNS 和 Tauri 所需 PNG 尺寸。
- `../web/public/icon.svg` 与 `icon.png`：白底网站图标。
- `../web/public/brand/mark.svg`：透明底、收紧留白的界面标识。`BrandMark` 组件在浅色主题显示黑色、深色主题显示白色；系统应用图标不随主题反色。

生成脚本使用项目锁定的 Tauri CLI；不依赖 Python、VTracer 或额外图像软件。VTracer 仅用于最初的矢量转换，后续直接编辑 SVG。额外的平台尺寸只保存在忽略的 `target/icon-assets` 下。
