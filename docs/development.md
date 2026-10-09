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

Linux 首版使用系统标题栏；Windows 专用的圆角、细边框及自绘控制按钮不应用到 Linux。不要通过修改公共窗口参数破坏已验收的 Windows 行为。

## 运行入口

CLI/TUI 从仓库根目录运行，并读取 `config/llmn.toml`：

```bash
cargo run -p cli
cargo run -p tui
```

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

打开 `http://localhost:1420`，普通 Web 默认进入真实消息/通讯录；Tauri 使用相同界面与真实 IPC，`?legacy=1` 保留旧入口。Web 生产静态构建使用 `pnpm --dir frontends/web build`；开发服务器代理不是公开部署的认证方案。隔离桌面模拟模型验收见 [实时 IPC 记录](social-refactor/m1-desktop-ipc.md#复现桌面验收)。

真实社交私聊、群聊与消息控制的隔离离线验收：

```bash
pnpm --dir frontends/web build:social-runtime
cargo build -p web-server --locked
python scripts/social_acceptance.py --dist frontends/web/.social-runtime-dist
```

添加 `--serve` 可检查脚本打印的临时数据页面；结束时在打印的 Fixture 目录创建 `STOP` 文件或中断脚本。Windows MSVC 指定 target 后通过 `--binary target/x86_64-pc-windows-msvc/debug/web-server.exe` 选择产物。脚本使用本机模拟 provider，不读取个人配置；各批验证范围见 [已完成](social-refactor/completed.md)。

社交界面可以先独立开发和验证，无需启动 Runtime：

```bash
pnpm --dir frontends/web dev:social --host 127.0.0.1
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
pnpm --dir frontends/web build:social
```

预览仍使用 1420 端口，不能和普通 Vite 开发服务器同时占用该端口。`build:social` 输出到忽略的 `frontends/web/.social-preview-dist`。这是浏览器模拟界面：无模型请求、无 Rust 社交存储和事件同步；见 [当前预览范围](social-refactor/08-frontend-and-desktop.md#当前-web-交互预览)。普通开发页也可添加 `?social-preview=1`，Tauri 不启用该模式。

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
```

`acceptance.py` 验证保留的 Harness：HTTP、ChatFeature、工具子进程与 JSON 存储。`social_acceptance.py` 验证新社交用例：HTTP、SocialFeature、SQLite、私聊/群聊队列与消息控制。两者都使用临时数据和本地模拟 provider；社交验收的构建命令见上面的运行入口。

真实模型验收另用 `scripts/live_acceptance.py`，会发送请求并消耗 API 额度，不属于默认 CI。详见 [真实接口报告](live-acceptance-2026-09-13.md)。

桌面视觉验收应检查实际安装包，并注明 OS、显示缩放、桌面环境和产物哈希。Xvfb 启动、编译成功或截取完成后的窗口图像，都不能替代完整的用户交互验收。

安装版社交 GUI/IPC 使用 `scripts/linux/social_gui_acceptance.py`，旧 Harness 使用显式旧入口的 `scripts/linux/gui_acceptance.py`。前者支持原生 Wayland、GTK 整数缩放及 `--manual` 隔离人工环境；Windows 包内 EXE 与空环境 worker 使用 `scripts/windows_package_acceptance.py`。参数和包检查步骤见 [发布指南](releases.md#本地-linux-打包)，实际范围见 [M1 阶段记录](social-refactor/m1-stage-validation.md)。

## 架构与贡献

修改边界和数据不变量见 [AGENTS.md](../AGENTS.md)。提交应说明具体问题、最终行为、验证方式和未覆盖范围；当协议、持久化字段或 GUI DTO 改变时，同步修改两个 GUI backend。

`frontends/web/dist` 是已跟踪的 Web 产物；桌面构建单独输出到 `frontends/tauri/dist`。除非任务要求更新 Web 部署产物，不要把桌面构建的输出提交进去。

`frontends/tauri/src-tauri/gen/schemas` 下的 `windows-schema.json`、`linux-schema.json`、`desktop-schema.json` 和权限 manifest 由 Tauri 构建脚本生成，已从 Git 排除。构建时出现或更新这些文件是正常行为，无需提交；需要修改权限时编辑 `capabilities/*.json`。首次克隆后运行桌面构建或 `cargo check -p tauri-frontend` 即可生成当前平台的文件。

图标从 `assets/branding/app-icon.svg` 生成：

```bash
pnpm --dir frontends/tauri icons
```

提交前检查没有个人配置、会话记录、API Key、远程主机路径或临时构建文件。本仓库尚未确定许可证；不要自行添加他人的许可证声明。
