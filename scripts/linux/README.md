# Linux 构建与检查

[脚本说明](../README.md) · [发布指南](../../docs/releases.md#本地-linux-打包)

这些脚本用于构建 Linux 桌面应用，并检查安装包能否启动、聊天和保存记录。

准备好系统依赖后，在仓库根目录运行：

```bash
bash scripts/linux/build.sh
```

脚本会安装前端依赖、运行测试，再生成 deb 和 AppImage。需要独立构建环境时，可以使用本目录的 [Dockerfile](Dockerfile)，具体命令见[发布指南](../../docs/releases.md#本地-linux-打包)。

[smoke.py](smoke.py) 检查包内程序和启动情况；[social_gui_acceptance.py](social_gui_acceptance.py) 在桌面中检查聊天操作，也提供 `--manual` 模式供人工检查中文输入、复制粘贴和窗口行为。`webdriver.py` 是它使用的浏览器驱动辅助代码。

脚本参数可通过 `python3 scripts/linux/smoke.py --help` 和 `python3 scripts/linux/social_gui_acceptance.py --help` 查看。
