# Linux 构建与检查

[脚本说明](../README.md) · [发布指南](../../docs/releases.md#本地-linux-打包)

准备好系统依赖后，在仓库根目录运行：

```bash
bash scripts/linux/build.sh
```

脚本只构建客户端。先显式设置 LLMN_AGENT_MANIFEST、LLMN_AGENT_VERSION、LLMN_AGENT_TARGET；本地固定产物另设 LLMN_AGENT_FILE。清单按 SHA-256 验证，缺少时失败，不编译 Agent。

需要独立构建环境时，可使用本目录的 [Dockerfile](Dockerfile)，具体命令见[发布指南](../../docs/releases.md#本地-linux-打包)。拆仓后客户端改为接收独立 Agent 构建提供的固定产物；脚本不读取其他仓源码。

[smoke.py](smoke.py) 必须核对包内 Agent 的 `agent-artifact.json`、实际程序摘要和 `--version`，并检查桌面动态库依赖；`--gui` 仅在隔离 Xvfb 中检查程序持续启动，不代表前端 IPC、聊天、GNOME/Wayland 或安装升级验收。使用已安装的程序时，显式提供其 Agent 资源目录：

```bash
python3 scripts/linux/smoke.py --deb package.deb --gui
python3 scripts/linux/smoke.py --appimage package.AppImage --gui
python3 scripts/linux/smoke.py --binary /path/to/LLM-Nest --agent-directory /path/to/agent --gui
```

检查产生的临时目录和日志会保留并打印路径，核验后按项目清理约定处理。参数见 `python3 scripts/linux/smoke.py --help`。

原 `social_gui_acceptance.py` 针对已退役的本地产品，不适用于当前 Cloud 界面，已随旧宿主移除；`webdriver.py` 保留为启动检查及后续当前客户端测试的辅助代码。真实桌面聊天、输入、复制粘贴、窗口及恢复验收仍需按[验证指南](../../docs/validation/README.md)单独执行。
