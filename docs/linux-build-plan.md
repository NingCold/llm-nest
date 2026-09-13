# Linux 构建环境方案（2026-09-13，初始方案存档）

**执行进展：** 本方案随后已开始实施，Ubuntu 24.04 容器中的测试、`.deb` 和 AppImage 构建已通过。当前状态见 [Linux 验收记录](linux-acceptance-2026-09-13.md)，复现步骤见 [发布指南](releases.md)。下文保留选择环境时的判断，不代表所有检查仍未进行。

## 环境选择

建议以远程 Linux 机器作为主构建和 GNOME 实机验收环境，WSL 2 Ubuntu 用于 Windows 上的日常开发与快速检查。相同发行版、架构和依赖下，WSL 2 也能生成有效的 Linux 安装包；选择远程机器是因为其资源和真实桌面更适合本项目的持续验收，并非 WSL 的产物天然不可靠。

经用户授权进行 SSH 只读检查，远程机器报告 Ubuntu 26.04.1 LTS、x86_64、24 个可用逻辑 CPU、约 60 GiB 内存、约 517 GiB 可用磁盘，安装 GNOME Shell 50.1、Docker 29.1.3 和 Rust/Cargo 1.97.0。检查时用户没有已登录的 GNOME 图形会话，仅有 GDM 登录界面。尚未检查 Docker 操作权限、Node/pnpm 完整路径或 WebKitGTK 开发依赖，也未安装软件、同步项目或运行构建。

## 构建与验收分开

- 首期可声明支持 Ubuntu 24.04+ x86_64：远程机器使用固定的 Ubuntu 24.04 容器构建 `.deb` 和 AppImage，并固定 Rust/Node/pnpm 版本及使用仓库锁文件。
- 如需支持 Ubuntu 22.04，应把构建基线降至 22.04 并单独验收。不要直接用 26.04 宿主环境打包后宣称兼容所有旧版 Linux。Tauri 要求在提供 WebKitGTK 4.1 的最旧目标系统上构建，较新的 glibc 会提高运行门槛，见 [Tauri AppImage 文档](https://v2.tauri.app/distribute/appimage/)。
- 构建容器用于编译、测试和打包；图标、拖动、缩放、剪贴板、主题、输入法和桌面集成需要在真实图形会话中检查。先用远程机器的 GNOME/Wayland，再补最旧支持系统；其他桌面或显示协议只有验收后才承诺支持。
- WSLg 可运行 X11/Wayland 图形应用，但其桌面集成发生在 Windows 上，不能代替 GNOME 的窗口管理行为验收，见 [Microsoft WSL GUI 文档](https://learn.microsoft.com/en-us/windows/wsl/tutorials/gui-apps/)。

## 项目适配顺序

1. 保留共用 React 前端与 Rust 业务层，将 Windows 窗口配置移到平台专用配置；Linux 初版使用系统装饰并相应隐藏自绘窗口控制区，避免两套标题栏。
2. 检查 Linux 编译、配置/存储目录、文件锁、工具 worker 的退出清理与资源限制。Windows Job Object 的保障不能自动算作 Linux 已有保障。
3. 构建两种包并检查包内工具 worker、依赖和独立启动；在没有源码或开发环境的用户会话中做聊天、取消、历史恢复及设置验收。

目前可以由本任务通过 SSH 负责构建相关终端操作，不要求远程 Codex 接管。若后续分配远程任务，应使用单独 checkout/分支，并指定同一个待验收提交，避免两边同时改动同一工作目录。图形验收仍需可访问的桌面会话与实际证据。
