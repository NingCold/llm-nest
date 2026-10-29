# Linux 已知结果

[验证入口](README.md) · [开发指南](../development.md) · [待验收](../project/backlog.md)

**2026-10-20 用户明确暂缓 Linux。当前无标题栏布局和新增附件流程尚未在 GNOME 实机重新构建验收。**

## 已有结果及范围

| 日期与环境 | 结果 | 对应范围 |
| --- | --- | --- |
| 2026-10-09，Ubuntu 24.04.5 构建及运行环境 | deb 实际安装，deb/AppImage 的社交 GUI/IPC、worker 和恢复检查通过 | 容器/Xvfb 自动检查；Ubuntu 24.04 真实桌面待验 |
| 2026-10-09，Ubuntu 26.04.1、GNOME 50.1 已登录 Wayland 桌面 | deb 原生 Wayland/XWayland、AppImage FUSE/XWayland 自动流程通过，覆盖聊天、设置、权限、失败重试及中断恢复；XWayland GTK scale 2 实际 DPR 2 | 当时的系统标题栏版；Wayland 请求 scale 2 实际 DPR 1，单列记录 |
| 同次 GNOME 用户确认 | 用户回复“已列项目均通过”：真实中文输入法候选与回车、跨应用中文/emoji/多行剪贴板、窗口拖动及控制、菜单/Dock/任务切换名称和图标 | 清单含可选 150% 缩放，但未另采显示参数；确认限该宿主和当时的包 |

这些结果保留旧版真实桌面与自动检查的分别范围；其中本地社交宿主和 worker 现已退役，不证明当前 Cloud 客户端通过。当前无标题栏布局和新增附件流程仍按本页顶部状态待复验。

2026-10-20 只读 SSH 探测时宿主可达，但只有 GDM 登录界面，没有已登录用户的 Wayland 桌面。未上传、构建或启动新 GUI。历史包哈希不作为当前版本的可用产物推荐；恢复时重新记录源码和新包哈希。

## 恢复检查时

使用 `scripts/linux/build.sh`、`scripts/linux/smoke.py` 及对应容器，说明见 [Linux 脚本](../../scripts/linux/README.md)。这些用于构建与包启动，不等于桌面人工检查。

`scripts/linux/Dockerfile.smoke` 建立不含开发工具链的 deb 运行检查环境；构建上下文放一份 deb 和 `smoke.py`，镜像安装该包并以非 root 用户运行。AppImage 使用另行准备的运行环境，不直接套用此 Dockerfile。单独运行入口为：

```bash
python3 scripts/linux/smoke.py --deb <包路径> --gui
python3 scripts/linux/smoke.py --appimage <包路径> --gui
```

也可用 `--binary <程序> --agent-directory <Agent资源目录>` 检查已安装 ELF。脚本强制核对随包 Agent 的固定版本记录、SHA-256 和 --version，并检查桌面动态库依赖；`--gui` 只证明隔离 Xvfb 中持续启动，不证明前端 IPC、聊天或真实桌面交互。临时资料与日志打印路径后保留，按清理约定处理。

原本地社交 GUI 验收脚本针对已退役宿主，已移除，不可用于当前 Cloud 界面；`scripts/linux/webdriver.py` 保留供当前启动与后续客户端测试复用。旧脚本的历史通过结果不扩展为新版通过。

恢复验收时，在已登录 GNOME 中检查当前包的云端登录与聊天、独立 Agent 配对/启停/后台、同机通信及恢复，再分别检查窗口拖动/控制/缩放、中文输入法、剪贴板和文件拖放、图片处理、草稿重开、原图/普通文件/ZIP 保存，以及生成中关闭。使用隔离资料和模拟模型，不增加未授权付费调用。分别记录 X11/Wayland、缩放、源码及包哈希和实际结果；AppImage FUSE 与 deb 安装启动各有独立范围。工具链、依赖及打包方式见[开发](../development.md)和[发布](../releases.md)。

当前仍按用户要求暂停；本轮脚本和结构调整不代表已经重新构建、安装或验收 Linux。
