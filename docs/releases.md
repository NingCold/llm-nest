# 构建与 GitHub 发布

[返回首页](../README.md) · [开发环境](development.md) · [Linux 验收](linux-acceptance-2026-09-13.md)

## 当前交付范围

| 平台 | 构建环境 | 产物 |
| --- | --- | --- |
| Windows x64 | Windows + MSVC | NSIS `.exe`、`.msi` |
| Linux x64 | Ubuntu 24.04 + WebKitGTK 4.1 | `.deb`、`.AppImage` |

安装包嵌入共用 React 前端，不附带模型密钥或用户会话。当前未配置系统代码签名、应用内自动更新，也没有构建 macOS 或 ARM 安装包。

Linux 以 Ubuntu 24.04 为构建基线，不能据此承诺所有 Linux 发行版兼容。较新系统编译的 glibc 等依赖会抬高运行门槛，AppImage 也仍有宿主依赖；选择基线的依据见 [Tauri AppImage 文档](https://v2.tauri.app/distribute/appimage/)。

## GitHub 自动化如何工作

仓库提供两份工作流：

- [ci.yml](../.github/workflows/ci.yml)：PR、main 分支提交和手动运行时，检查 Windows/Linux 的 Rust、前端和离线完整链路。
- [release.yml](../.github/workflows/release.yml)：手动运行时构建可下载的 Actions artifacts；推送 `v*` 标签时，在两个平台构建和检查成功后创建 **草稿 Release**，附带四种安装包及 SHA-256 校验和。

草稿由维护者下载、验收、补充变更说明后点击 **Publish release** 公开。构建失败不会生成草稿；重跑只允许更新同标签的草稿，不覆盖已经公开发布的文件。工作流采用 GitHub 官方支持的 Tauri 构建方式，但用仓库脚本统一版本校验、产物收集和验证步骤，见 [Tauri GitHub Actions 指南](https://v2.tauri.app/distribute/pipelines/github/)。

### 第一次启用

1. 把这些工作流和脚本提交并合入 GitHub 默认分支 `main`。如果默认分支改名，同步修改 CI 的分支过滤条件。
2. 打开 GitHub 仓库 **Actions → Build installers → Run workflow**。先选择待测分支运行一次；完成后，从该次运行的 Artifacts 下载 `installers-windows-x86_64` 和 `installers-linux-x86_64`。
3. 验收下载的包并明确项目许可证后，再创建正式版本标签。首次构建失败时先查看失败步骤的日志，不重复推送或移动已有标签。

不需要添加模型 API Key、个人访问令牌或 SSH 私钥到 GitHub。构建 job 仅有 `contents: read`；生成草稿的 job 使用 GitHub 临时提供的 `GITHUB_TOKEN`，单独声明 `contents: write`。组织策略仍可能限制工作流运行或写权限，参见 [GitHub 的 GITHUB_TOKEN 文档](https://docs.github.com/en/actions/tutorials/authenticate-with-github_token)。

### 发布一个版本

以下四处版本必须一致：

| 文件 | 字段 |
| --- | --- |
| `Cargo.toml` | `workspace.package.version` |
| `frontends/tauri/src-tauri/tauri.conf.json` | `version` |
| `frontends/tauri/package.json` | `version` |
| `frontends/web/package.json` | `version` |

修改 Cargo 版本后，运行 `cargo check -p cli` 更新工作区包在 `Cargo.lock` 中的版本，一并提交锁文件。随后检查：

```bash
python scripts/release.py validate --tag v0.1.0
python scripts/test_release.py
```

将 `v0.1.0` 换成要发布的实际版本。确认检查通过、改动已合入并推送后，在该提交上打标签：

```bash
git tag -a v0.1.0 -m "LLM-Nest 0.1.0"
git push origin v0.1.0
```

标签会触发打包；进入仓库 Releases 查看生成的草稿。`v0.1.0-beta.1` 这类预发布标签也必须与四处版本完整匹配，草稿会标记为 prerelease。已发布版本有问题时应修复后发新版本，避免修改旧标签和旧资产。

校验下载文件：Linux 使用 `sha256sum -c SHA256SUMS.txt`（全部产物在同目录时）；Windows 使用 `Get-FileHash <安装包> -Algorithm SHA256` 与清单对照。

**工作流已在本地做语法和脚本验证，尚未在 GitHub 托管 runner 上实际运行。** 第一次手动运行是剩余的 CI 验收步骤。当前流程不执行真实模型测试，也不会自动公开草稿。

## 本地 Windows 打包

先按开发指南安装两份前端依赖，然后在 PowerShell 中运行：

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
pnpm --dir frontends/tauri exec tauri build --target x86_64-pc-windows-msvc --bundles nsis,msi --ci -- --locked
./scripts/verify-windows-icons.ps1
```

产物在 `target/x86_64-pc-windows-msvc/release/bundle`。应使用 Tauri CLI，不能把裸 `cargo build --release` 当作已经嵌入生产前端的安装包。

## 本地 Linux 打包

在 Linux 的独立 checkout 中构建，避免复用 Windows 的 node_modules/target。原生构建需要安装 [Dockerfile 中列出的依赖](../scripts/linux/Dockerfile)，然后运行：

```bash
bash scripts/linux/build.sh
```

该脚本依次执行前端检查、Rust 测试、离线完整流程和两种安装包构建。只要 deb 时可设置 `LLMN_LINUX_BUNDLES=deb`。默认产物在 `target/release/bundle`；指定 Rust target 的构建会多一层 target 名目录。

### 用固定 Ubuntu 容器构建

以下命令在 Linux shell 中执行，当前目录应为独立、无个人密钥和会话的 checkout。若 Docker 需要 sudo，可在命令前加 `sudo`，不必为构建调整宿主用户组权限。

```bash
docker build -t llmn-build:ubuntu24 -f scripts/linux/Dockerfile scripts/linux
docker run --rm --user "$(id -u):$(id -g)" \
  -e HOME=/workspace/target/build-home \
  -e CARGO_HOME=/workspace/target/cargo-cache \
  -e CARGO_BUILD_JOBS=4 \
  --mount type=bind,source="$PWD",target=/workspace \
  llmn-build:ubuntu24 \
  bash -c 'mkdir -p "$HOME"; bash scripts/linux/build.sh'
```

Rust、Node、pnpm 版本与依赖锁文件固定；Ubuntu 基础镜像和 apt 仓库仍会更新。这是可重复执行的构建流程，不承诺每次得到逐字节相同的二进制。首次构建需要网络下载依赖。

## 检查包，而不仅是源码

有 Xvfb、D-Bus 和 GTK/WebKitGTK 运行库的 Linux 环境中可以运行：

```bash
python3 scripts/linux/smoke.py --deb target/release/bundle/deb/*.deb --gui
python3 scripts/linux/smoke.py --appimage target/release/bundle/appimage/*.AppImage --gui
```

脚本验证包内 worker 在空环境变量下工作、拒绝非白名单工具，并在隔离配置/存储中检查 GUI 启动与前端到 Runtime 的 IPC。AppImage 使用解包入口，**不验证 FUSE 挂载或文件管理器双击启动**。

干净系统依赖检查使用一个仅包含包和脚本的构建目录：

```bash
mkdir -p target/linux-smoke-context
cp target/release/bundle/deb/*.deb target/linux-smoke-context/
cp scripts/linux/smoke.py target/linux-smoke-context/
cp scripts/linux/Dockerfile.smoke target/linux-smoke-context/Dockerfile
docker build -t llmn-smoke:ubuntu24 target/linux-smoke-context
docker run --rm llmn-smoke:ubuntu24
```

真实 GNOME/Wayland 的输入法、剪贴板、窗口缩放、主题和完整聊天仍需要桌面操作验收，清单见 [本轮 Linux 记录](linux-acceptance-2026-09-13.md)。自动打包、系统代码签名和应用内自动更新是三个独立事项，当前只落地了自动打包及草稿发布。
