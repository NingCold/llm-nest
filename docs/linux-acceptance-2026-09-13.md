# Linux 首次构建与安装包验证（2026-09-13）

后续桌面自动验收、修复包与人工待验清单见 [2026-09-16 接续记录](linux-acceptance-2026-09-16.md)。本报告保留原包与当时验证范围。

## 结论与范围

Ubuntu 24.04 容器内的源码测试、离线完整流程、`.deb` 和 AppImage 打包通过。`.deb` 在另一个干净 Ubuntu 24.04 镜像中通过 apt 安装，并以非 root 用户通过包内 worker 和 Xvfb 启动检查。AppImage 在同一运行环境中通过解包入口启动检查。

**尚未完成真实 GNOME/Wayland 桌面验收。** 此结论不包含双击启动、输入法、剪贴板、窗口管理、不同显示缩放和真实模型聊天。Windows 本轮只做平台配置回归与重建，不替换用户已接受的安装版本。

## 环境与隔离

| 项目 | 实际环境 |
| --- | --- |
| 远程宿主 | Ubuntu 26.04.1 LTS，x86_64，24 个逻辑 CPU，约 60 GiB 内存 |
| 图形环境 | 已安装 GNOME Shell 50.1；检查时只有 GDM 登录界面，无已登录的用户桌面 |
| 构建基线 | Docker 中的 Ubuntu 24.04；没有直接使用宿主 26.04 编译发布包 |
| 工具链 | Rust/Cargo 1.97.0、Node.js 24.18.0、pnpm 11.22.0 |
| 依赖 | 锁定 Cargo.lock 与两份 pnpm-lock.yaml；GTK 3 / WebKitGTK 4.1 |
| 并行度 | Cargo 使用 8 个构建 job |
| 运行验证 | 独立 Ubuntu 24.04 运行镜像；无 Rust、Node 或源码，非 root 用户；D-Bus + Xvfb |

基础 Ubuntu 镜像 digest 为 `sha256:224a1869083a311ef3f13648a154ba79832fbef6364d31493642ca03082da254`。构建镜像 ID 为 `sha256:8c6fc2f84df2d4269facd5d0acb75416374a716ceb076bdda88441b279572b7f`。记录 digest 用于识别本轮环境，不代表 apt 软件源或构建下载工具被永久冻结。

远程使用独立源码目录；没有传输 `config/llmn.toml`、`.env`、个人 API Key 或会话目录，没有调用厂商 API。Docker 通过已有的非交互 sudo 权限运行，未修改宿主用户组、登录桌面或系统安装的软件。具体远程路径仅保存在本机忽略的 `target/linux-build-location.json`，不写入公共文档。

## 实现调整

- 公共 Tauri 配置使用系统装饰；Windows 完整窗口数组移入 `tauri.windows.conf.json`，逐项比较与用户已接受的 `91f9a9b` 一致。
- 前端区分 `IS_DESKTOP` 与 `CUSTOM_WINDOW_CHROME`。Linux 继续使用 Tauri IPC，但不挂载 Windows 的自绘按钮、拖动区域和描边监听。
- 加入 Ubuntu 24.04 构建容器、Linux 构建与包验证脚本；补齐 Linux 工具进程超时后的退出/回收测试。
- 增加 Windows/Linux CI 和安装包草稿发布流程；目前只完成本地脚本与工作流静态验证，未执行 GitHub 托管运行。

## 已通过检查

| 检查 | 结果 |
| --- | --- |
| 共用前端 | typecheck 与现有回归测试通过 |
| Linux Rust 工作区 | `cargo test --workspace --locked`：216 个测试通过；随后新增的 Linux worker 回收测试单独通过 |
| 离线完整流程 | `scripts/acceptance.py` 通过，实际经过 HTTP、ChatFeature、存储和工具进程 |
| Tauri 构建 | deb 和 AppImage 均由 Tauri CLI 构建并嵌入生产前端 |
| deb 控制元数据 | 包名 `llm-nest`、版本 `0.1.0`、架构 `amd64`；声明依赖 `libwebkit2gtk-4.1-0, libgtk-3-0` |
| 干净运行环境安装 | apt 从本地 deb 安装并解析依赖，镜像构建成功 |
| 包内 worker | deb/AppImage 的 ELF 在空环境变量下运行 add，返回 sum=5；拒绝 shell 名称 |
| 无界面 GUI 启动 | 两种包均存活 8 秒，并创建隔离目录的 `.llmn.lock`，证明共用前端已通过 IPC 初始化 Runtime |
| Linux 子进程回收 | 超时丢弃 Future 后，阻塞测试 worker 终止且 `/proc/PID` 消失，未遗留僵尸 |
| Windows 回归 | MSVC NSIS/MSI 重建成功，EXE/NSIS 图标资源匹配；fmt 与 MSVC Tauri Clippy 完成 |
| 发布脚本 | 版本/tag 一致性与产物缺失/重复等失败门禁测试通过；actionlint 1.7.7 检查通过 |

离线流程覆盖设置保存/重启、非法参数、会话 CRUD、正文/思考/用量、编辑与过期写入防护、反馈、工具循环、provider 错误/EOF、重复 run、取消/断开、崩溃恢复及幂等、目录锁和供应商修改。本次未再次调用 ecnu-max；之前的真实 API 证据见 [独立报告](live-acceptance-2026-09-13.md)。

无界面检查中出现 AT-SPI 服务缺失和 EGL DRI3 警告，与最小容器/Xvfb 环境有关。AppImage 还输出 GStreamer `appsink` 缺失提示，但已继续完成 Runtime 初始化；音视频未验收，不能从启动通过推断该类能力正常。桌面实测时应确认是否仍有相关日志。

## 本轮产物

| 文件 | 大小 | SHA-256 |
| --- | ---: | --- |
| `LLM-Nest_0.1.0_amd64.deb` | 8,049,750 字节 | `ff6ddd4361d9b64d521ac9009d3464efaa21594ea06bd0794cef3130dbd7e11f` |
| `LLM-Nest_0.1.0_amd64.AppImage` | 84,732,408 字节 | `811db60f0671e93ee06f07de5d0b5f8b821f7f1377ab682d65a9a062157f4814` |

两种产物已取回本机 `target/`，未上传 GitHub。远程保留独立构建目录和包供后续桌面复核。本机日志包括 `target/linux-build.log`、`target/linux-appimage-build.log`、`target/linux-package-smoke.log`、`target/linux-clean-deb-smoke.log` 和 `target/linux-clean-appimage-smoke.log`；这些包含临时路径的日志不提交仓库。

源码来自 `91f9a9b` 加本轮 Linux 平台配置改动；在后续加入测试/文档/发布脚本前完成二进制构建。新增 worker 测试只有 `cfg(test)` 代码，不改变发布二进制。不要把本轮产物说成由尚未运行的 GitHub Actions 生成。

## 下一轮 GNOME 人工验收

在有用户登录的 GNOME 桌面上，用本轮包测试，记录发行版、Wayland/X11、缩放和包哈希：

1. 安装 deb，确认应用菜单名称/图标及启动；另测 AppImage 添加执行权限后的正常启动与 FUSE 路径。
2. 验证系统标题栏没有叠加自绘按钮，拖动、缩放、最小化、最大化与还原正常；浅色/深色界面可读。
3. 验证中文输入、长文本输入、复制/粘贴和消息滚动；检查 100%/150%/200% 缩放。
4. 首次空配置能进入设置，添加测试供应商后完成回答、取消、错误后重试、重启恢复历史和修改设置。
5. 确认关闭后无残留 worker；使用独立测试目录检查第二实例提示及重启恢复。

运行配置/数据仍应与个人常用环境隔离。Ubuntu 26.04 宿主上的 GNOME 验收通过后，还要在构建基线 Ubuntu 24.04 桌面补测，才能扩大支持承诺。WSLg 只能补充开发验证，不能替代 GNOME 桌面集成。

Linux 资源约束目前仍弱于 Windows Job Object；尚无通用不可信插件沙盒。此轮验证的是固定编译白名单工具的生命周期和包内动态链接，不宣称具备任意命令执行的隔离保障。
