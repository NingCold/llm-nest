# Linux 构建与检查

[脚本说明](../README.md) · [发布指南](../../docs/releases.md#本地-linux-打包)

准备好系统依赖后，在仓库根目录运行：

```bash
bash scripts/linux/build.sh
```

脚本安装前端依赖，单独构建官方 Agent，通过 `agent/scripts/package-agent.py` 生成带固定版本、目标平台和 SHA-256 的产物清单，再准备桌面资源、运行测试并使用 Tauri CLI 生成 deb 和 AppImage。默认使用 Rust 主机目标；`LLMN_AGENT_TARGET` 可指定已准备工具链的 Linux 目标。Agent 产物保留在打印出的临时目录，供核验后按清单清理。脚本不会从旧宿主启动或运行已退役的本地服务验收。

需要独立构建环境时，可使用本目录的 [Dockerfile](Dockerfile)，具体命令见[发布指南](../../docs/releases.md#本地-linux-打包)。拆仓后客户端改为接收独立 Agent 构建提供的固定产物；当前脚本中的 Agent 编译是显式步骤。

[smoke.py](smoke.py) 必须核对包内 Agent 的 `agent-artifact.json`、实际程序摘要和 `--version`，并检查桌面动态库依赖；`--gui` 仅在隔离 Xvfb 中检查程序持续启动，不代表前端 IPC、聊天、GNOME/Wayland 或安装升级验收。使用已安装的程序时，显式提供其 Agent 资源目录：

```bash
python3 scripts/linux/smoke.py --deb package.deb --gui
python3 scripts/linux/smoke.py --appimage package.AppImage --gui
python3 scripts/linux/smoke.py --binary /path/to/LLM-Nest --agent-directory /path/to/agent --gui
```

检查产生的临时目录和日志会保留并打印路径，核验后按项目清理约定处理。参数见 `python3 scripts/linux/smoke.py --help`。

原 `social_gui_acceptance.py` 针对已退役的本地产品，不适用于当前 Cloud 界面，已随旧宿主移除；`webdriver.py` 保留为启动检查及后续当前客户端测试的辅助代码。真实桌面聊天、输入、复制粘贴、窗口及恢复验收仍需按[验证指南](../../docs/validation/README.md)单独执行。
