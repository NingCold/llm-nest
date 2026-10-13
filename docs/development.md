# 开发指南

[返回首页](../README.md) · [配置](configuration.md) · [发布](releases.md)

## 环境

本轮固定验证版本为 Rust 1.97.0、Node.js 24.18.0、pnpm 11.22.0、Python 3.11+。标准库文件锁要求 Rust 至少 1.89，但未承诺所有依赖在该最低版本通过。CI 与 Linux 容器使用相同的固定工具版本。

```bash
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
```

两份 package.json 和锁文件独立维护。不要仅安装桌面壳的依赖就运行共用前端。

### Windows

安装 Rust MSVC 工具链、Visual Studio C++ 构建工具和 WebView2。仓库 .cargo/config.toml 只为 Windows GNU target 指定 MinGW linker，未强制所有平台使用 GNU；发布包使用 MSVC。

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
pnpm --dir frontends/tauri exec tauri dev
```

### Linux

Ubuntu 24.04 的构建依赖可参考 [构建 Dockerfile](../scripts/linux/Dockerfile)，主要包括 GTK 3、WebKitGTK 4.1、OpenSSL 开发包和编译工具。各发行版安装命令见 [Tauri 官方前置条件](https://v2.tauri.app/start/prerequisites/#linux)。

当前 Windows/Linux 关闭系统标题栏，使用共用内容内窗口控制；Windows 的 Acrylic、DWM 圆角与贴边细边框仍在平台模块。Linux 新窗口布局尚待构建与 GNOME 实机复验，历史包的系统装饰验收不能替代，见 [本批记录](social-refactor/m1-desktop-surface.md)。修改公共窗口参数时同时检查 Windows 专用配置，维持已验收的边框和退出保存行为。

## 运行入口

Web 和桌面共用聊天功能。配置模型服务后，在 AI 角色管理页选择每位 AI 使用的模型。Web 首次运行需按[配置说明](../config/README.md)准备 `config/llmn.toml`；桌面会自动创建初始配置。

桌面：

```bash
pnpm --dir frontends/tauri exec tauri dev
```

Web 开发需要两个终端：

```bash
# 终端一：Runtime 和 HTTP API
cargo run -p web-server

# 终端二：React 开发服务器，/api 自动代理到 8787
pnpm --dir frontends/web dev
```

打开 `http://localhost:1420`，进入消息和通讯录。Tauri 使用相同界面，通过 IPC 与本地服务通信。Web 生产静态构建使用 `pnpm --dir frontends/web build`；开发服务器代理不是公开部署的认证方案。隔离桌面模拟模型验收见 [实时 IPC 记录](social-refactor/m1-desktop-ipc.md#复现桌面验收)。

真实社交私聊、群聊与消息控制的隔离离线验收：

```bash
pnpm --dir frontends/web build:social-runtime
cargo build -p web-server --locked
python scripts/social_acceptance.py --dist frontends/web/.social-runtime-dist
```

添加 `--serve` 可检查脚本打印的临时数据页面；结束时在打印的 Fixture 目录创建 `STOP` 文件或中断脚本。Windows MSVC 指定 target 后通过 `--binary target/x86_64-pc-windows-msvc/debug/web-server.exe` 选择产物。脚本使用本机模拟 provider，不读取个人配置；各批验证范围见 [已完成](social-refactor/completed.md)。

类型与组件回归使用 `pnpm --dir frontends/web typecheck` 和 `pnpm --dir frontends/web test`。

若本机 pnpm 与 packageManager 锁定版本不同，优先安装固定版本；仅做本地验证时可使用 `pnpm --pm-on-fail=ignore --dir frontends/web <命令>`，不因该提示改动依赖或锁文件。

多个入口若同时运行，请给每个独立 Runtime 设置不同的 `LLMN_DATA_DIR`。目录锁错误应通过关闭占用进程或选择其他目录解决，不要删除锁文件。

## 检查改动

```bash
cargo fmt --all -- --check
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
python scripts/social_acceptance.py --only-image-capabilities
python scripts/social_acceptance.py --only-files
```

`acceptance.py` 验证空配置启动与无模型私聊保存/提示/重启重试、旧接口 404、本地访问限制、模型服务配置原子更新、所选旧 JSON 导入/来源校验/只读历史/重启以及底层 worker。`social_acceptance.py` 验证实际 HTTP、SocialFeature、SQLite、私聊/群聊、意识系统与 M3 调度；`--only-image-capabilities` 独立检查模型图片能力设置与实际调用；`--only-files` 检查普通文件的保存/下载、权限/修订、重试、模型元信息与附件 ZIP 导出。全部使用独立临时数据与本机模拟 provider；CI 和发布检查执行这四条命令。

真实模型验收会产生调用费用，按单独安排执行；既有报告见[历史记录](history.md)。

桌面视觉验收应检查实际安装包，并注明 OS、显示缩放、桌面环境和产物哈希。Xvfb 启动、编译成功或截取完成后的窗口图像，都不能替代完整的用户交互验收。

安装版社交 GUI/IPC 使用 `scripts/linux/social_gui_acceptance.py`，共用驱动位于 `scripts/linux/webdriver.py`，支持原生 Wayland、GTK 整数缩放及 `--manual` 隔离人工环境；Windows 包内 EXE 与空环境 worker 使用 `scripts/windows_package_acceptance.py`。参数和包检查步骤见 [发布指南](releases.md#本地-linux-打包)，实际范围见 [M1 阶段记录](social-refactor/m1-stage-validation.md)。

## 架构与贡献

修改边界和数据不变量见 [AGENTS.md](../AGENTS.md)。提交应说明具体问题、最终行为、验证方式和未覆盖范围；当协议、持久化字段或 GUI DTO 改变时，同步修改两个 GUI backend。

`frontends/web/dist`、`frontends/web/.social-runtime-dist` 与 `frontends/tauri/dist` 均为被忽略的生成产物。两种入口都从 `frontends/web` 的源码构建。

`frontends/tauri/src-tauri/gen/schemas` 下的 `windows-schema.json`、`linux-schema.json`、`desktop-schema.json` 和权限 manifest 由 Tauri 构建脚本生成，已从 Git 排除。构建时出现或更新这些文件是正常行为，无需提交；需要修改权限时编辑 `capabilities/*.json`。首次克隆后运行桌面构建或 `cargo check -p tauri-frontend` 即可生成当前平台的文件。

图标从 `assets/branding/app-icon.svg` 生成：

```bash
pnpm --dir frontends/tauri icons
```

提交前检查没有个人配置、会话记录、API Key、远程主机路径或临时构建文件。本仓库尚未确定许可证；不要自行添加他人的许可证声明。
