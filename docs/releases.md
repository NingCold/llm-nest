# 构建与发布

[开发指南](development.md) · [验证指南](validation/README.md) · [全部文档](README.md)

本页描述仓库的桌面构建与发布流程。当前源码默认连接云端，并携带可选的本机 Agent；已发布安装包仍是此前版本，云端版尚未正式发布。云端服务独立部署，见[部署说明](../server/cloud/deploy/README.md)。

## 安装包范围

| 平台 | 构建环境 | 产物 |
| --- | --- | --- |
| Windows x64 | Windows + MSVC | NSIS `.exe`、`.msi` |
| Linux x64 | Ubuntu 24.04 + WebKitGTK 4.1 | `.deb`、`.AppImage` |

安装包嵌入共用前端与独立 Agent 可执行文件，不携带个人配置、模型密钥或聊天记录。Tauri 构建前运行 `build:agent`，将本平台 release Agent 复制到忽略的资源目录，打包到 `agent/`；开发入口使用 debug Agent。不要用用户的节点目录作为打包资源。当前没有配置系统代码签名、应用内自动更新，也没有 macOS 或 ARM 的打包流程。

Linux 使用 Ubuntu 24.04 作为构建基线。其他发行版仍需验证运行库兼容性，AppImage 也有宿主依赖。

## 版本和标签

版本在以下四处保持一致：

| 文件 | 字段 |
| --- | --- |
| `Cargo.toml` | `workspace.package.version` |
| `client/desktop/src-tauri/tauri.conf.json` | `version` |
| `client/desktop/package.json` | `version` |
| `client/web/package.json` | `version` |

每次向测试者交付新包时递增版本，普通提交无需逐次改版本。当前为 `0.1.1-alpha.3`；后续预发布可使用 `alpha.N`、`beta.N`、`rc.N`，正式发布时去掉后缀。

MSI 另需纯数字的 `bundle.windows.wix.version`，当前为 `0.1.1.3`。前三段与应用基础版本一致，第四段是安装器构建号。Windows Installer 主要比较前三段，预发布包之间的覆盖安装仍需实测。

修改 Cargo 版本后运行 `cargo check -p web-server` 更新 `Cargo.lock` 中的工作区包版本，一并提交。然后检查版本和发布脚本：

```bash
python scripts/release.py validate --tag v0.1.1-alpha.3
python scripts/test_release.py
```

示例标签替换为实际版本。确认代码、检查和安装包验收完成后，再在待发布提交上打标签并推送：

```bash
git tag -a v0.1.1-alpha.3 -m "LLM Nest 0.1.1-alpha.3"
git push origin v0.1.1-alpha.3
```

修改版本、构建安装包、推送标签和公开 Release 是分别执行的操作。已公开版本需要修复时，发布新版本，保留旧标签和资产。

## GitHub 工作流

- [ci.yml](../.github/workflows/ci.yml)：检查 PR、main 提交和手动运行，覆盖 Windows/Linux 的 Rust、前端和离线链路。
- [release.yml](../.github/workflows/release.yml)：手动运行时生成 Actions artifacts；推送 `v*` 标签时，两个平台构建并检查成功后创建草稿 Release，附带安装包和 SHA-256 清单。

首次使用可在 **Actions → Build installers → Run workflow** 选择分支，下载 `installers-windows-x86_64` 和 `installers-linux-x86_64` 验证。工作流使用临时 `GITHUB_TOKEN`，无需模型 API Key 或个人访问令牌。

标签须与四处版本完整匹配，带后缀的版本标记为 prerelease。重跑可以更新同标签草稿，已公开资产不会被覆盖。维护者验收产物、确认许可证并补充变更说明后，再点击 **Publish release**。

工作流中的模拟模型检查、包启动检查和真人桌面操作覆盖不同范围，查看[验证指南](validation/README.md)了解各自证明了什么。

## 本地 Windows 打包

先按开发指南安装系统和前端依赖，在仓库根目录的 PowerShell 中运行：

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
Push-Location client/desktop
./node_modules/.bin/tauri.cmd build --target x86_64-pc-windows-msvc --bundles nsis msi --ci --config src-tauri/tauri.windows.conf.json -- --locked
Pop-Location
./scripts/verify-windows-icons.ps1
./scripts/verify-windows-branding.ps1
```

产物位于 `target/x86_64-pc-windows-msvc/release/bundle`。Tauri CLI 会构建并嵌入生产前端，裸 `cargo build --release` 不能代替这一步。

对外显示名为 **LLM Nest**，内部二进制、历史安装目录和注册项继续使用 `LLM-Nest`，用户数据仍在 `~/.llmn`。模板来源和升级约束见[模板说明](../client/desktop/src-tauri/packaging/windows/README.md)，包检查和安装验收见 [Windows 验证](validation/windows.md)。

## 本地 Linux 打包

在 Linux 的独立 checkout 中构建，分别安装依赖，避免复用 Windows 的 `node_modules` 和 `target`。原生系统依赖见 [Dockerfile](../scripts/linux/Dockerfile)，准备好后运行：

```bash
bash scripts/linux/build.sh
```

脚本安装前端依赖，运行前端检查、Rust 测试、离线验收，再构建 deb 和 AppImage。只需要 deb 时设置 `LLMN_LINUX_BUNDLES=deb`。默认产物位于 `target/release/bundle`；指定 Rust target 后，路径多一层 target 名称。

### 用 Ubuntu 容器构建

以下命令在 Linux shell 中执行，当前目录为不含个人密钥和聊天数据的独立 checkout：

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

这里的 HOME 仅用于容器中的临时构建用户。Docker 需要权限时可按宿主配置使用 sudo。工具版本和依赖锁文件固定，Ubuntu 镜像及 apt 仓库仍会更新；流程可重复执行，不保证二进制逐字节相同。首次构建需要联网下载依赖。

包内 worker、隔离 GUI/IPC 和 GNOME/Wayland 人工验收命令见 [Linux 验证](validation/linux.md)。

## 发布前核对

先按平台验证实际安装包，再核对当前版本的说明和已知限制。Windows 干净安装及系统级 MSI 检查见 [Windows 验证](validation/windows.md)，Linux 运行环境见 [Linux 验证](validation/linux.md)。

下载产物后，Linux 可运行 `sha256sum -c SHA256SUMS.txt`；Windows 可运行 `Get-FileHash <安装包> -Algorithm SHA256` 与清单对照。手动工作流的产物使用 `SHA256SUMS-<platform>.txt`，草稿 Release 还会生成合并后的 `SHA256SUMS.txt`。

项目许可证尚待确定，公开发布前需要确认。
