# 检查与发布脚本

[项目首页](../README.md) · [开发指南](../docs/development.md) · [发布指南](../docs/releases.md)

这里的脚本帮助检查聊天功能、制作安装包和准备新版本。以下命令都在仓库根目录执行。

## 检查云端真人聊天

当前默认界面与服务的部署准备见[云端部署说明](../server/cloud/deploy/README.md)。[cloud_acceptance.py](cloud_acceptance.py) 检查已部署的隔离测试服务，创建受邀测试账号，验证两台客户端之间的私聊、附件、修订、权限、搜索和导出；不调用模型。

```bash
python scripts/cloud_acceptance.py --url https://测试服务地址:8443 --ca /本机测试CA/ca.crt --server-host 测试服务器 --server-root /测试服务根目录 --client-host 第二台测试机 --output /私有目录/cloud-test-accounts.json
```

脚本依赖 Python、SSH，以及测试服务器已有的 Compose 部署和私有 outbox。它通过 SSH 在服务器创建邀请、读取测试邮件，并在第二台测试机执行客户端检查；`--server-root` 指向包含 `src/`、`private/` 和 `data/` 的测试目录。输出文件含测试账号与凭据，必须放在忽略的私有目录，不提交或公开。它不是对任意生产服务的只读检查。

[cloud-test-certificate.py](cloud-test-certificate.py) 使用 OpenSSL 为隔离环境生成测试 CA 和 HTTPS 证书：

```bash
python scripts/cloud-test-certificate.py /私有证书目录 测试主机名 测试IP地址
```

生成的 `ca.crt` 用于测试客户端信任，`server.crt` 与 `server.key` 供测试 HTTPS 入口使用。私钥留在服务器私有目录，不放进静态文件目录或仓库；证书不会自动安装到系统信任库，也不代替正式站点证书。

## 检查云端 AI 与意识系统

先构建 `agent-node`，再使用上面生成的独立测试账号。以下脚本都运行真实 Agent 进程，模型由本机模拟服务提供，不读取真实模型密钥：

```bash
python scripts/agent_acceptance.py --binary /Agent程序路径 --accounts /私有目录/cloud-test-accounts.json --ca /测试CA/ca.crt --workdir /新的私有测试目录
python scripts/cloud_group_acceptance.py --binary /Agent程序路径 --accounts /私有目录/cloud-test-accounts.json --ca /测试CA/ca.crt --workdir /另一私有测试目录
python scripts/cloud_consciousness_acceptance.py --binary /Agent程序路径 --accounts /私有目录/cloud-test-accounts.json --ca /测试CA/ca.crt --workdir /意识测试目录
```

它们分别检查私聊恢复、跨节点群聊，以及记忆隔离、各 AI 摘要、群话题接续、遗忘和离线来源失效。群聊和意识脚本可加 `--serve-ui` 保持模拟节点在线，方便接着检查网页；完成后在对应工作目录创建 `stop` 文件。脚本结束会停止模拟节点并撤销测试绑定，云端测试聊天保留。

## 检查云端在线管理

部署包含在线管理接口的当前服务、构建当前 `agent-node` 后，可用三个独立测试账号运行以下脚本（不使用真实模型 Key）：

```bash
python scripts/cloud_management_acceptance.py --binary /Agent程序路径 --accounts /私有目录/cloud-test-accounts.json --ca /测试CA/ca.crt --workdir /新的私有测试目录
```

脚本仅创建并管理新的验收 AI，使用本机两个模拟模型检查角色/模型在线修改、进行中请求快照、当前结果重放、公开资料版本、初始/普通管理员权限、管理员撤销、另一真人私聊的只读查看与附件，以及 `run-managed/status/stop` 和离线不排队。它还检查远程管理及读取到的云端 DTO 不包含模拟端点或凭据；这不代替服务端数据库/日志的独立审计。工作目录在仓库内时必须被 Git 忽略，并且必须尚不存在。

可加 `--serve-ui` 留下在线模拟节点，随后在工作目录创建 `stop` 文件结束。脚本退出会停止本次进程并撤销本次 AI 的节点绑定，测试聊天保留；结果保存在工作目录的 `result.json`。运行记录以实际执行输出为准。

## 保留的本地 API 回归

```bash
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
```

第一个脚本检查应用启动、配置和历史导入；第二个检查私聊、群聊、记忆、摘要与约定联系。它们创建临时数据，模型回复由本机测试服务提供。

只检查图片消息时，运行 `python scripts/social_acceptance.py --only-images`。这会验证发送、重开、修改与撤回、群引用，以及较大图片的保存和读取。

检查模型图片能力时，运行 `python scripts/social_acceptance.py --only-image-capabilities`。它会检查人工设置和恢复跟随目录、未知能力的尝试、历史图片与群引用，以及群内不同 AI 的独立回复。

检查普通文件收发和导出时，运行 `python scripts/social_acceptance.py --only-files`。它会发送文件、重开聊天、下载并核对原始内容，也会检查权限、撤回、大小限制和 AI 实际收到的文件说明；导出检查包括附件选项组合、同名文件、中文链接和 ZIP 中的原文件。

这些脚本调用保留的 `web-server` API，不是当前云端真人聊天的验收，也不会证明 AI 节点已经接通。当前默认前端已经改用云端认证与聊天接口，不能通过给旧宿主设置 `--dist` 或运行 `build:social-runtime` 恢复旧本地产品入口。

## 其他常用脚本

| 要做的事 | 脚本或说明 |
| --- | --- |
| 检查 Agent 核心与连接器的依赖边界 | `python scripts/check-boundaries.py` |
| 准备桌面聊天测试数据 | [social_desktop_fixture.py](social_desktop_fixture.py) |
| 更新应用图标 | [generate-icons.mjs](generate-icons.mjs)，用法见[图标说明](../assets/branding/README.md) |
| 构建和检查 Linux 安装包 | [Linux 脚本](linux/README.md) |
| 检查 Windows 包内程序 | [windows_package_acceptance.py](windows_package_acceptance.py) |
| 检查 Windows 显示名和图标 | [verify-windows-branding.ps1](verify-windows-branding.ps1)、[verify-windows-icons.ps1](verify-windows-icons.ps1) |
| 准备 Windows Sandbox 安装测试 | [prepare-windows-sandbox.ps1](prepare-windows-sandbox.ps1) |
| 校对版本、整理发布文件 | [release.py](release.py)，步骤见[发布指南](../docs/releases.md) |

`social_memory_acceptance.py` 等专项脚本由完整聊天检查调用，用来分别检查记忆、摘要、话题接续和参与节奏。
