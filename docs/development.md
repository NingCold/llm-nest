# 开发指南

[项目首页](../README.md) · [模型配置](https://github.com/NingCold/llm-nest-agent/blob/main/docs/configuration.md) · [设计文档](design/README.md) · [发布指南](releases.md)

默认入口是云端真人与 AI 私聊、群聊：CloudApp 连接独立 `cloud-server`，社交数据保存在 PostgreSQL。桌面仅管理独立 Agent，旧本地宿主已移除；旧 SQLite 仍可通过独立迁移工具导入。

## 准备环境

CI 与云端 Dockerfile 使用 Rust 1.99.1，前端使用 Node.js 24.18.0、pnpm 11.22.0；脚本需要 Python 3.11+。Windows 使用 MSVC 工具链。按对应构建入口准备版本，避免因工具差异改动锁文件。还需可用的 `corepack` 命令：许可生成脚本通过 `corepack pnpm@11.22.0` 读取本机依赖，单独安装全局 pnpm 不替代这一要求。

先安装共用前端和桌面壳的依赖：

```bash
pnpm --dir web install --frozen-lockfile
pnpm --dir desktop install --frozen-lockfile
```

两份 package.json 和锁文件独立维护。两种入口的 React 源码都在 `web`。

### Windows

安装 Rust MSVC 工具链、Visual Studio C++ 构建工具和 WebView2。发布包使用 MSVC；仓库 `.cargo/config.toml` 中的 MinGW linker 只用于显式选择 Windows GNU target 的构建。

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
```

准备 Agent 产物后再启动桌面，见下文。

### Linux

安装 GTK 3、WebKitGTK 4.1、OpenSSL 开发包和编译工具。Ubuntu 24.04 的完整依赖和固定工具版本见 [Dockerfile](../scripts/linux/Dockerfile)；也可按[发布指南](releases.md#本地-linux-打包)在容器内构建。

Windows/Linux 使用共用内容内窗口控制。Linux 当前窗口布局仍需 GNOME 实机复验，已验收范围和待查项目见 [Linux 验证](validation/linux.md)。

## 从源码运行

所有命令从仓库根目录执行。先按[服务端说明](https://github.com/NingCold/llm-nest-server)准备独立服务，客户端仓不编译服务端。新服务端与 Agent 远端本轮尚未发布，链接为计划地址。本地第二批验证使用明确提交的测试镜像和固定产物。

开发 Web 页面时另开终端：

```bash
pnpm --dir web dev
```

打开 `http://localhost:1420`，将后端 `CLOUD_PUBLIC_URL` 配为这个页面来源。Vite 的 `/api` 默认代理到 `http://127.0.0.1:8787`，可用 `LLMN_API` 指定其他开发后端。正式部署使用同源 HTTPS，不通过 Vite 提供公开服务。

桌面运行：

```bash
pnpm --dir desktop exec tauri dev
```

在页面填写云端 HTTPS 服务器地址。原生网络层默认仅保管内存 Cookie；Windows 显式选择保持登录后用当前用户 DPAPI 保存令牌，重启仍联网核对会话，非 Windows 原生端暂不支持。登录与 SDK 解锁独立，规则见[账号设计](https://github.com/NingCold/llm-nest-server/blob/main/docs/design/accounts.md#登录会话与本机解锁)。客户端不读取模型配置或节点 SQLite。授权测试使用私有 CA 时，启动前设置 `CLOUD_CA_CERT_FILE` 为测试 PEM 文件；不关闭 TLS 校验，不将测试证书与个人配置打包。

Tauri 开发和构建入口会先准备独立 `llmn-agent` 可执行文件，再构建共用前端；生成文件不进 Git。桌面运行时从资源目录启动 Agent，界面本身不调用模型。验收本机配置、配对与托盘时，设置临时 `LLMN_DATA_DIR`，按服务器和真人账号隔离节点，避免使用日常资料。构建细节见[桌面说明](../desktop/README.md)。

### 准备桌面 Agent 产物

桌面构建不会编译 Agent 源码。在 Agent 仓单独构建并打包后，显式提供实际产物：

```powershell
$env:LLMN_AGENT_VERSION = "<产物清单中的固定版本>"
$env:LLMN_AGENT_TARGET = "x86_64-pc-windows-msvc"
$env:LLMN_AGENT_MANIFEST = "<已核验的本地清单绝对路径或固定 HTTPS URL>"
# 本地离线验证另设以下两项；必须对应清单中的程序与 notices
$env:LLMN_AGENT_FILE = "<程序绝对路径>"
$env:LLMN_AGENT_NOTICES_FILE = "<notices.file 对应文本的绝对路径>"
pnpm --dir desktop exec tauri dev
```

Linux 使用对应目标三元组与无 `.exe` 的程序。版本必须匹配程序 `--version`。已有官方 HTTPS 产物时，将 `LLMN_AGENT_MANIFEST` 指向固定版本清单，省略 `LLMN_AGENT_FILE` 和 `LLMN_AGENT_NOTICES_FILE`，从清单的 HTTPS URL 获取程序和第三方声明，分别核对 SHA-256。缺少或不匹配时构建失败，不查找兄弟源码或自动改用其他版本。客户端与 Agent 可分别发版。

### 生成随包声明

桌面安装包构建需要官方 [cargo-about 0.9.3](https://github.com/EmbarkStudios/cargo-about/releases/tag/0.9.3) 预编译工具，下载后核对官方 SHA-256；各平台固定摘要见[发布工作流](../.github/workflows/release.yml)。将工具加入 PATH，或设置 `CARGO_ABOUT` 为可执行文件绝对路径。Web 开发和普通类型检查不需要该工具；桌面构建按本仓 `about.toml`、`about.hbs` 和锁文件生成当前目标的 Rust 声明。它会运行 Cargo metadata，并可能联网取得依赖或缺失的许可正文，不编译 Rust。

Web 构建从已安装的生产依赖读取完整许可正文，再合并 `packaging/licenses` 中按精确版本保存的补充材料；不会下载 Matrix SDK 源码。升级依赖时，重新检查缺失正文、附带字体与预编译 WASM 的内部依赖，更新固定文本、`sources.json` 的来源及 SHA-256。Matrix WASM 的源码提交、锁文件、分发 WASM 和声明摘要保存在对应 provenance 文件；升级该 SDK 时须用新版本的固定源码和锁文件重新生成，不能沿用旧版本声明。补充材料缺失或摘要不符会中止构建。Web 和桌面构建自动附带第三方声明及本项目 LICENSE、NOTICE，不提交派生 dist 或资源文件。

### 独立 Agent 与外部接入

`agent-node` 包提供 `llmn-agent`，从本机 CLI 初始化、配对和运行，具体配置见[节点说明](https://github.com/NingCold/llm-nest-agent/blob/main/node/README.md)。初始化私有目录后，设置服务器及可选 CA，在“我的账号 → 我管理的 AI”创建 AI 并填写配对码。模型 Key 只通过节点环境变量引用，不加入云端配置；节点 SQLite 保存 AI 档案、专用凭据和检查点，与旧本地库分开。

外部程序可只依赖[连接器](https://github.com/NingCold/llm-nest-server/blob/main/connector/README.md)。固定回答示例覆盖任务接收、回复、取消、撤权及不支持的管理请求，有独立的模拟 WSS 测试；不调用模型，也不代替持久恢复的自带 Agent。在线角色与模型管理由节点声明 `profileManagement` 能力，旧节点和示例不会被当成支持该能力。

## 检查改动

先检查受影响模块和调用链，再按范围扩展。常用命令：

```bash
python scripts/check-boundaries.py
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked
pnpm --dir web typecheck
pnpm --dir web test
```

跨模块改动再运行 `cargo test --workspace --locked` 和 `cargo clippy --workspace --all-targets --locked`。产物、迁移与发布检查不需要真实模型：

```bash
python scripts/test_build_artifacts.py
node --test desktop/scripts/prepare-agent.test.mjs
python scripts/test_release.py
```

云端及 Agent 联调使用隔离服务和模拟模型，见[验证指南](validation/README.md)。真实模型调用需要另有明确授权。

每批编译结束后，由代理主动整理产物。等所有并行编译结束，先预览再清理：

```bash
python scripts/clean-build-artifacts.py
python scripts/clean-build-artifacts.py --apply
```

脚本只删除 `target` 下默认及常见 Windows、Linux、macOS 目标的 `incremental` 增量缓存，未知或自定义目标目录保留。先只读列出完整的文件和目录清单，再逐文件删除；目录由深到浅确认空后逐个删除，不使用递归删除命令或 API。依赖、构建缓存、可执行文件、安装包和工作资料都保留，避免每次全量重编；下次受影响的 Rust 编译会重新生成增量缓存。有 Cargo/rustc 正在运行，或候选包含链接、junction 时，脚本停止。自身验证使用独立临时目录：`python scripts/test_clean_build_artifacts.py`。

清理是每批工作的收尾步骤，不挂在并行构建钩子上。使用 PowerShell 逐文件删除时默认不加 `-Force`；遇到文件属性或占用报错，先检查具体原因，不自动强制删除。工具策略拒绝时保留预览，交由用户执行，不能写成已经清理。安装包保留当前和上一可回退版，不在多个工作目录重复复制；账号、聊天、密钥和恢复备份按内容核对，不按日期自动删除。

云端页面与桌面实际网络转发需要单独验收，见 [Windows](validation/windows.md) / [Linux](validation/linux.md) 和[验证指南](validation/README.md)。

## 云端数据库测试

为测试单独准备 PostgreSQL，并设置 `CLOUD_TEST_DATABASE_URL` 后执行：

```bash
node web/tests/cloud-api.cjs
```

服务端 PostgreSQL 与跨仓组合验收由服务端仓维护；本仓只运行客户端检查。

远端测试服务当前只通过 Tailscale HTTPS 8443 开放，使用测试 CA 和私有 outbox；不在文档记录真实地址、证书私钥、路径或凭据。浏览器与桌面端到端和部署验收范围见[验证指南](validation/README.md)。

## 提交前

修改边界和必须保持的行为见 [AGENTS.md](../AGENTS.md)，模块职责见[设计文档](design/README.md)。共享 DTO 或事件变化需要同步 HTTP 和 Tauri 两条链路。

构建会生成 `web/dist`、`desktop/dist` 以及桌面 `gen/schemas` 下的权限 schema/manifest，Git 已忽略这些文件。桌面权限修改落在 `capabilities/*.json`。

图标唯一编辑源为 `assets/branding/app-icon.svg`，更新后生成派生尺寸：

```bash
pnpm --dir desktop icons
```

客户端发版同步 Web/桌面 package.json、Tauri 配置和桌面 Cargo.toml 的显式 `package.version`，不要求与服务端/Agent 同版；用 `python scripts/release.py validate` 校对，详见[发布指南](releases.md)。

提交说明写清问题、结果和验证范围。检查没有混入个人配置、聊天数据、密钥或临时产物。项目许可证尚待确定。
