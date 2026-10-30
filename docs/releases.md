# 构建与发布

[开发指南](development.md) · [验证指南](validation/README.md) · [全部文档](README.md)

本页描述仓库的桌面构建与发布流程。当前源码默认连接云端，并携带可选的本机 Agent。云端服务独立部署，见[部署说明](https://github.com/NingCold/llm-nest-server/blob/main/server/deploy/README.md)。

客户端 [0.1.1-alpha.13](https://github.com/NingCold/llm-nest/releases/tag/v0.1.1-alpha.13) 已公开为 Alpha 预发布，桌面固定附带 Agent `alpha.13`。Windows NSIS 已在本机覆盖安装；MSI 已构建和检查，尚未实际安装。Linux deb 与 AppImage 均通过隔离启动及资源校验。日常账号重启后的解锁验读、干净 Windows 和 GNOME 人工验收仍待补齐，见[验证指南](validation/README.md)。

## 安装包范围

| 平台 | 构建环境 | 产物 |
| --- | --- | --- |
| Windows x64 | Windows + MSVC | NSIS `.exe`、`.msi` |
| Linux x64 | Ubuntu 24.04 + WebKitGTK 4.1 | `.deb`、`.AppImage` |

安装包嵌入共用前端与独立 Agent 可执行文件，不携带个人配置、模型密钥或聊天记录。Tauri 构建前运行 `build:agent`，读取明确的 Agent 版本、target 和 SHA-256 清单，将核验后的程序、对应第三方声明及产物记录放入资源目录；开发时可显式提供已构建的本地程序。该步骤不编译 Agent 源码。不要用用户的节点目录作为打包资源。当前没有配置系统代码签名、应用内自动更新，也没有 macOS 或 ARM 的打包流程。

Linux 使用 Ubuntu 24.04 作为构建基线。其他发行版仍需验证运行库兼容性，AppImage 也有宿主依赖。

## 版本和标签

客户端版本在以下四处保持一致：

| 文件 | 字段 |
| --- | --- |
| `desktop/src-tauri/Cargo.toml` | 显式 `package.version`，不继承根工作区 |
| `desktop/src-tauri/tauri.conf.json` | `version` |
| `desktop/package.json` | `version` |
| `web/package.json` | `version` |

每次向测试者交付新包时递增版本，普通提交无需逐次改版本。当前源码为 `0.1.1-alpha.13`；后续预发布可使用 `alpha.N`、`beta.N`、`rc.N`，正式发布时去掉后缀。

MSI 另需纯数字的 `bundle.windows.wix.version`，当前为 `0.1.1.13`。前三段与应用基础版本一致，第四段与 `alpha.N` 中的 N 一致，发布检查会拒绝遗留的旧构建号。Windows Installer 主要比较前三段，预发布包之间的覆盖安装仍需实测。

客户端、服务端和 Agent 分别维护版本，不再要求三个产品同版。修改相应 Cargo 包版本时同步锁文件。客户端版本与发布标签检查：

```bash
python scripts/release.py validate --tag v0.1.1-alpha.13
python scripts/test_release.py
```

示例标签替换为实际版本。确认代码、检查和安装包验收完成后，再在待发布提交上打标签并推送：

```bash
git tag -a v0.1.1-alpha.13 -m "LLM Nest 0.1.1-alpha.13"
git push origin v0.1.1-alpha.13
```

修改版本、构建安装包、推送标签和公开 Release 是分别执行的操作。已公开版本需要修复时，发布新版本，保留旧标签和资产。

## GitHub 工作流

- [ci.yml](../.github/workflows/ci.yml)：检查 PR、main 提交和手动运行，覆盖 Windows/Linux 的 Rust、前端和离线链路。
- [release.yml](../.github/workflows/release.yml)：手动运行生成 Actions artifacts，可显式输入已存在且匹配源码版本的 release_tag 创建草稿；必须输入实际已发布 Agent 的固定 HTTPS 清单和版本；本轮尚无正式 Agent 发布，不伪造默认清单，附带安装包和 SHA-256 清单。

首次使用可在 **Actions → Build installers → Run workflow** 选择分支，下载 `installers-windows-x86_64` 和 `installers-linux-x86_64` 验证。工作流使用临时 `GITHUB_TOKEN`，无需模型 API Key 或个人访问令牌。

标签须与四处客户端版本完整匹配，带后缀的版本标记为 prerelease。重跑可以更新同标签草稿，已公开资产不会被覆盖。维护者验收产物、确认许可证并补充变更说明后，再点击 **Publish release**。

工作流中的模拟模型检查、包启动检查和真人桌面操作覆盖不同范围，查看[验证指南](validation/README.md)了解各自证明了什么。

## 本地 Windows 打包

先按开发指南安装系统和前端依赖，从独立 Agent 构建或正式发布取得清单及对应 Windows 程序。以下路径指向已准备的固定产物，版本须来自实际清单；固定产物须已公开，或由本机明确指定。在仓库根目录的 PowerShell 中运行：

```powershell
$env:RUSTUP_TOOLCHAIN = "stable-x86_64-pc-windows-msvc"
$env:LLMN_AGENT_VERSION = "0.1.1-alpha.13"
$env:LLMN_AGENT_TARGET = "x86_64-pc-windows-msvc"
$env:LLMN_AGENT_MANIFEST = (Resolve-Path target/agent-release-artifact/agent-manifest.json).Path
$env:LLMN_AGENT_FILE = (Resolve-Path target/agent-release-artifact/llmn-agent-0.1.1-alpha.13-x86_64-pc-windows-msvc.exe).Path
$env:LLMN_AGENT_NOTICES_FILE = "<清单 notices.file 对应文本的绝对路径>"
$env:CARGO_ABOUT = "<已校验的 cargo-about 0.9.3 可执行文件绝对路径>"
Push-Location desktop
./node_modules/.bin/tauri.cmd build --target x86_64-pc-windows-msvc --bundles nsis msi --ci --config src-tauri/tauri.windows.conf.json -- --locked
Pop-Location
./scripts/verify-windows-icons.ps1
./scripts/verify-windows-branding.ps1
```

产物位于 `target/x86_64-pc-windows-msvc/release/bundle`。Tauri CLI 会构建并嵌入生产前端，裸 `cargo build --release` 不能代替这一步。

对外显示名为 **LLM Nest**，内部二进制、历史安装目录和注册项继续使用 `LLM-Nest`，用户数据仍在 `~/.llmn`。模板来源和升级约束见[模板说明](../desktop/src-tauri/packaging/windows/README.md)，包检查和安装验收见 [Windows 验证](validation/windows.md)。

## 本地 Linux 打包

在 Linux 的独立 checkout 中构建，分别安装依赖，避免复用 Windows 的 `node_modules` 和 `target`。原生系统依赖见 [Dockerfile](../scripts/linux/Dockerfile)，准备好后运行：

```bash
export LLMN_AGENT_VERSION='<实际清单中的固定版本>'
export LLMN_AGENT_TARGET=x86_64-unknown-linux-gnu
export LLMN_AGENT_MANIFEST='/已准备的Agent产物目录/agent-manifest.json'
export LLMN_AGENT_FILE='/已准备的Agent产物目录/清单对应的Linux程序'
export LLMN_AGENT_NOTICES_FILE='/已准备的Agent产物目录/清单notices.file对应的文本'
export CARGO_ABOUT='/已校验的工具目录/cargo-about'
bash scripts/linux/build.sh
```

脚本安装前端依赖，运行前端检查、Rust 测试，核验指定 Agent 产物后构建 deb 和 AppImage，不编译 Agent。正式发布可将 `LLMN_AGENT_MANIFEST` 设为固定 HTTPS 清单 URL，省略 `LLMN_AGENT_FILE` 和 `LLMN_AGENT_NOTICES_FILE`，从清单下载并分别校验程序和声明。只需要 deb 时设置 `LLMN_LINUX_BUNDLES=deb`。产物位于 `target/$LLMN_AGENT_TARGET/release/bundle`；上述目标为 `target/x86_64-unknown-linux-gnu/release/bundle`。

### 用 Ubuntu 容器构建

以下命令在 Linux shell 中执行，当前目录为不含个人密钥和聊天数据的独立 checkout：

```bash
docker build -t llmn-build:ubuntu24 -f scripts/linux/Dockerfile scripts/linux
docker run --rm --user "$(id -u):$(id -g)" \
  -e HOME=/workspace/target/build-home \
  -e CARGO_HOME=/workspace/target/cargo-cache \
  -e CARGO_BUILD_JOBS=4 \
  -e LLMN_AGENT_VERSION \
  -e LLMN_AGENT_TARGET \
  -e LLMN_AGENT_MANIFEST=/agent-artifacts/agent-manifest.json \
  -e LLMN_AGENT_FILE=/agent-artifacts/清单对应的Linux程序 \
  -e LLMN_AGENT_NOTICES_FILE=/agent-artifacts/清单notices.file对应的文本 \
  --mount type=bind,source=/已准备的Agent产物目录,target=/agent-artifacts,readonly \
  --mount type=bind,source="$PWD",target=/workspace \
  llmn-build:ubuntu24 \
  bash -c 'mkdir -p "$HOME"; bash scripts/linux/build.sh'
```

这里的 HOME 仅用于容器中的临时构建用户。Docker 需要权限时可按宿主配置使用 sudo。工具版本和依赖锁文件固定，Ubuntu 镜像及 apt 仓库仍会更新；流程可重复执行，不保证二进制逐字节相同。首次构建需要联网下载依赖。

包内 Agent、隔离 GUI 启动和 GNOME/Wayland 人工验收命令见 [Linux 验证](validation/linux.md)。

## 固定 Web 产物与服务端构建

先构建客户端 Web，再打包指定版本；服务端只接受版本和 SHA-256 匹配的产物：

```bash
pnpm --dir web build
python scripts/package-web.py --dist web/dist --version 0.1.1-alpha.13 --output /新的web产物目录
```

将 Web 包及 web-manifest.json 交给独立服务端仓，按[服务端部署说明](https://github.com/NingCold/llm-nest-server/blob/main/server/deploy/README.md)创建构建上下文和镜像。客户端仓不包含服务端准备脚本。保留上一组可回退产物；正式 Agent 下载使用长期可获取的固定 HTTPS 地址，不能依赖临时 CI artifact 或 `latest`。

客户端发布工作流只消费指定 Agent 清单，Web 包和清单随 Linux 产物上传。填写 release_tag 时，构建直接 checkout 该标签并核对提交；草稿发布再次核对标签仍指向实际构建提交。未填写时按启动工作流的固定提交生成下载产物。实际远端发布属于拆仓第三批，当前不代表已经公开发布或部署。部署时记录服务端、Web、Agent 与连接器的实际版本、提交和摘要。

## 随包许可材料

发布工作流和 Linux 构建镜像已按固定 SHA-256 安装官方 cargo-about 0.9.3；本地构建须自行准备该版本，可通过 `CARGO_ABOUT` 指定路径。前端许可生成还要求 Corepack 可用，并使用固定 pnpm 11.22.0。Agent 清单必须为目标平台提供 `notices.file`、`notices.sha256`，正式下载另需固定 HTTPS `notices.url`；声明缺失或校验失败时不打包。

Web 包必须包含 `THIRD_PARTY_NOTICES.txt`、`LICENSE.txt`、`NOTICE.txt`。桌面资源包含 Web、当前目标 Rust 和安装器模板的合并声明，以及单独校验的 Agent 声明。依赖升级时先按[开发指南](development.md#生成随包声明)更新精确版本的补充正文与来源证据，再重新构建；不要手工修改生成文件或仅保留许可证名称。公开前核对实际归档和安装包中的这些文件，分别保留各组件的原始许可条件。

## 发布前核对

先按平台验证实际安装包，再核对当前版本的说明和已知限制。Windows 干净安装及系统级 MSI 检查见 [Windows 验证](validation/windows.md)，Linux 运行环境见 [Linux 验证](validation/linux.md)。

下载产物后，Linux 可运行 `sha256sum -c SHA256SUMS.txt`；Windows 可运行 `Get-FileHash <安装包> -Algorithm SHA256` 与清单对照。手动工作流的产物使用 `SHA256SUMS-<platform>.txt`，草稿 Release 还会生成合并后的 `SHA256SUMS.txt`。

项目代码采用 Apache-2.0；发布前生成并随包提供所用第三方组件的许可证及声明。
