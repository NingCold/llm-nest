# 开发指南

[项目首页](../README.md) · [模型配置](configuration.md) · [设计文档](design/README.md) · [发布指南](releases.md)

默认入口是云端真人聊天：CloudApp 连接独立 `cloud-server`，社交数据保存在 PostgreSQL。旧本地 Runtime/SQLite 保留作回归与 Agent 迁移参考，不由默认桌面页面启动。

## 准备环境

现有 CI/Linux 构建脚本使用 Rust 1.97.0、Node.js 24.18.0、pnpm 11.22.0，云端 Dockerfile 使用 Rust 1.99.1；脚本需要 Python 3.11+。Windows 云端和桌面检查使用 stable MSVC 工具链。按对应构建入口准备版本，避免因工具差异改动锁文件。

先安装共用前端和桌面壳的依赖：

```bash
pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/desktop install --frozen-lockfile
```

两份 package.json 和锁文件独立维护。两种入口的 React 源码都在 `client/web`。

### Windows

安装 Rust MSVC 工具链、Visual Studio C++ 构建工具和 WebView2。发布包使用 MSVC；仓库 `.cargo/config.toml` 中的 MinGW linker 只用于显式选择 Windows GNU target 的构建。

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
pnpm --dir client/desktop exec tauri dev
```

### Linux

安装 GTK 3、WebKitGTK 4.1、OpenSSL 开发包和编译工具。Ubuntu 24.04 的完整依赖和固定工具版本见 [Dockerfile](../scripts/linux/Dockerfile)；也可按[发布指南](releases.md#本地-linux-打包)在容器内构建。

Windows/Linux 使用共用内容内窗口控制。Linux 当前窗口布局仍需 GNOME 实机复验，已验收范围和待查项目见 [Linux 验证](validation/linux.md)。

## 从源码运行

所有命令从仓库根目录执行。云端服务配置见 [server/cloud](../server/cloud/README.md)，容器定义见 [Compose](../server/cloud/deploy/compose.yaml)。先准备独立 PostgreSQL、附件目录和邮件发送环境，构建网页：

```bash
pnpm --dir client/web build
cargo run -p cloud-server --locked
```

必须配置 `CLOUD_DATABASE_URL`、`CLOUD_PUBLIC_URL`、`CLOUD_ATTACHMENTS_DIR`，以及 SMTP 或私有测试收件箱 `CLOUD_MAIL_OUTBOX`。静态目录默认 `client/web/dist`，监听默认 `127.0.0.1:8787`。首次通过 `cargo run -p cloud-server --locked -- invite 7` 生成邀请，再在页面注册并验证邮箱。不要将连接串、邀请码、邮件链接或会话提交到仓库。

开发 Web 页面时另开终端：

```bash
pnpm --dir client/web dev
```

打开 `http://localhost:1420`，将后端 `CLOUD_PUBLIC_URL` 配为这个页面来源。Vite 的 `/api` 默认代理到 `http://127.0.0.1:8787`，可用 `LLMN_API` 指定其他开发后端。正式部署使用同源 HTTPS，不通过 Vite 提供公开服务。

桌面运行：

```bash
pnpm --dir client/desktop exec tauri dev
```

在页面填写云端 HTTPS 服务器地址。原生网络层持有内存 Cookie，重启需重新登录；默认不读取 `config/llmn.toml` 或打开旧 SQLite。授权测试使用私有 CA 时，启动前设置 `CLOUD_CA_CERT_FILE` 为测试 PEM 文件；不关闭 TLS 校验，不将测试证书与个人配置打包。

旧本地服务仍可用 `cargo run -p web-server --locked` 维护 Runtime/SocialFeature 与 SQLite，需要独立的 `LLMN_DATA_DIR` 和模型配置，见[模型配置](configuration.md)。默认 CloudApp 不消费其 `/api/social` 接口；不要把两个服务当成同一个后端，也不要为测试启动真实模型调用。

## 检查改动

先检查受影响模块和调用链，再按范围扩展。常用命令：

```bash
python scripts/check-boundaries.py
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked
pnpm --dir client/web typecheck
pnpm --dir client/web test
```

跨模块改动再运行 `cargo test --workspace --locked` 和 `cargo clippy --workspace --all-targets --locked`。旧本地实现的离线回归入口（不代表云端客户端验收）：

```bash
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
python scripts/social_acceptance.py --only-image-capabilities
python scripts/social_acceptance.py --only-files
```

这些脚本使用独立临时数据和本机模拟 provider，具体覆盖范围见[验证指南](validation/README.md)。真实模型调用需要另有明确授权。

旧模拟 provider 验收脚本属于本地 SocialFeature 回归，不会把默认页面切回旧界面。云端页面与桌面实际网络转发需要单独验收，见 [Windows](validation/windows.md) / [Linux](validation/linux.md) 和[验证指南](validation/README.md)。

## 云端数据库测试

为测试单独准备 PostgreSQL，并设置 `CLOUD_TEST_DATABASE_URL` 后执行：

```bash
cargo test -p cloud-server --locked -- --nocapture
node client/web/tests/cloud-api.cjs
```

数据库测试逐例使用隔离 schema，覆盖认证、并发唯一私聊、幂等回执、权限、读态、附件修订/撤回、导出、持久事件与本地样本映射。实际 PostgreSQL 的 10 项测试已通过；没有设置连接串时数据库用例会跳过，不能以此报告完整通过。桌面原生转发可用 `cargo test -p tauri-frontend --lib cloud::tests` 检查。

远端测试服务当前只通过 Tailscale HTTPS 8443 开放，使用测试 CA 和私有 outbox；不在文档记录真实地址、证书私钥、路径或凭据。浏览器与桌面端到端和部署验收范围见[验证指南](validation/README.md)。

## 提交前

修改边界和必须保持的行为见 [AGENTS.md](../AGENTS.md)，模块职责见[设计文档](design/README.md)。共享 DTO 或事件变化需要同步 HTTP 和 Tauri 两条链路。

构建会生成 `client/web/dist`、`client/web/.social-runtime-dist`、`client/desktop/dist` 以及桌面 `gen/schemas` 下的权限 schema/manifest，Git 已忽略这些文件。桌面权限修改落在 `capabilities/*.json`。

图标唯一编辑源为 `assets/branding/app-icon.svg`，更新后生成派生尺寸：

```bash
pnpm --dir client/desktop icons
```

提交说明写清问题、结果和验证范围。检查没有混入个人配置、聊天数据、密钥或临时产物。项目许可证尚待确定。
