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

打开 `http://localhost:1420`。Web 生产静态构建使用 `pnpm --dir frontends/web build`；开发服务器代理不是公开部署的认证方案。

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

离线验收使用本地模拟 provider，但实际经过 HTTP、ChatFeature、工具子进程和会话存储，涵盖正常回复、思考、取消、错误、断流、编辑、工具循环与恢复。

真实模型验收另用 `scripts/live_acceptance.py`，会发送请求并消耗 API 额度，不属于默认 CI。详见 [真实接口报告](live-acceptance-2026-09-13.md)。

桌面视觉验收应检查实际安装包，并注明 OS、显示缩放、桌面环境和产物哈希。Xvfb 启动、编译成功或截取完成后的窗口图像，都不能替代完整的用户交互验收。

## 架构与贡献

修改边界和数据不变量见 [AGENTS.md](../AGENTS.md)。提交应说明具体问题、最终行为、验证方式和未覆盖范围；当协议、持久化字段或 GUI DTO 改变时，同步修改两个 GUI backend。

`frontends/web/dist` 是已跟踪的 Web 产物；桌面构建单独输出到 `frontends/tauri/dist`。除非任务要求更新 Web 部署产物，不要把桌面构建的输出提交进去。

图标从 `assets/branding/app-icon.svg` 生成：

```bash
pnpm --dir frontends/tauri icons
```

提交前检查没有个人配置、会话记录、API Key、远程主机路径或临时构建文件。本仓库尚未确定许可证；不要自行添加他人的许可证声明。
