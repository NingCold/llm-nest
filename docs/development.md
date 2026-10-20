# 开发指南

[项目首页](../README.md) · [模型配置](configuration.md) · [设计文档](design/README.md) · [发布指南](releases.md)

## 准备环境

仓库 CI 和 Linux 构建容器固定使用 Rust 1.97.0、Node.js 24.18.0、pnpm 11.22.0；脚本需要 Python 3.11+。建议本机使用相同版本，避免因工具差异改动锁文件。

先安装共用前端和桌面壳的依赖：

```bash
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
```

两份 package.json 和锁文件独立维护。两种入口的 React 源码都在 `frontends/web`。

### Windows

安装 Rust MSVC 工具链、Visual Studio C++ 构建工具和 WebView2。发布包使用 MSVC；仓库 `.cargo/config.toml` 中的 MinGW linker 只用于显式选择 Windows GNU target 的构建。

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
pnpm --dir frontends/tauri exec tauri dev
```

### Linux

安装 GTK 3、WebKitGTK 4.1、OpenSSL 开发包和编译工具。Ubuntu 24.04 的完整依赖和固定工具版本见 [Dockerfile](../scripts/linux/Dockerfile)；也可按[发布指南](releases.md#本地-linux-打包)在容器内构建。

Windows/Linux 使用共用内容内窗口控制。Linux 当前窗口布局仍需 GNOME 实机复验，已验收范围和待查项目见 [Linux 验证](validation/linux.md)。

## 从源码运行

所有命令从仓库根目录执行。桌面版会初始化本地 Runtime，无需另开 Web 服务；首次配置规则见[模型配置](configuration.md#配置和数据的位置)。

```bash
pnpm --dir frontends/tauri exec tauri dev
```

Web 首次运行先新建被 Git 忽略的 `config/llmn.toml`：

```toml
[providers]
```

然后在两个终端分别运行：

```bash
# 终端一：Runtime 和 HTTP API，默认 127.0.0.1:8787
cargo run -p web-server --locked
```

```bash
# 终端二：React 开发服务器，/api 代理到 8787
pnpm --dir frontends/web dev
```

打开 `http://localhost:1420`。生产静态页面使用 `pnpm --dir frontends/web build`，由 Web 服务的默认 `frontends/web/dist` 目录提供；也可以用 `LLMN_WEB_DIST` 指定目录。Web 入口面向本机，开发代理不提供公开部署认证。

桌面和 Web 使用相同的默认数据目录。需要同时运行多个独立 Runtime 时，分别设置 `LLMN_DATA_DIR`；遇到目录锁错误，关闭占用进程或换目录，保留锁文件。

## 检查改动

先检查受影响模块和调用链，再按范围扩展。常用命令：

```bash
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
```

跨模块改动再运行 `cargo test --workspace --locked` 和 `cargo clippy --workspace --all-targets --locked`。完整离线验证入口：

```bash
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
python scripts/social_acceptance.py --only-image-capabilities
python scripts/social_acceptance.py --only-files
```

这些脚本使用独立临时数据和本机模拟 provider，具体覆盖范围见[验证指南](validation/README.md)。真实模型调用需要另有明确授权。

想在模拟环境中人工检查共用 Web 界面，可以运行：

```bash
pnpm --dir frontends/web build:social-runtime
python scripts/social_acceptance.py --dist frontends/web/.social-runtime-dist --serve
```

打开脚本打印的地址，结束时在打印的 Fixture 目录创建 `STOP` 文件或中断脚本。显式指定 Rust target 时，通过 `--binary` 选择对应的 `web-server` 产物。桌面实际 IPC、安装包和视觉验证见 [Windows](validation/windows.md) / [Linux](validation/linux.md)。

## 提交前

修改边界和必须保持的行为见 [AGENTS.md](../AGENTS.md)，模块职责见[设计文档](design/README.md)。共享 DTO 或事件变化需要同步 HTTP 和 Tauri 两条链路。

构建会生成 `frontends/web/dist`、`frontends/web/.social-runtime-dist`、`frontends/tauri/dist` 以及桌面 `gen/schemas` 下的权限 schema/manifest，Git 已忽略这些文件。桌面权限修改落在 `capabilities/*.json`。

图标唯一编辑源为 `assets/branding/app-icon.svg`，更新后生成派生尺寸：

```bash
pnpm --dir frontends/tauri icons
```

提交说明写清问题、结果和验证范围。检查没有混入个人配置、聊天数据、密钥或临时产物。项目许可证尚待确定。
