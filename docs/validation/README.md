# 如何验证

[开发指南](../development.md) · [Windows](windows.md) · [Linux](linux.md) · [设计](../design/README.md)

当前默认入口为云端真人聊天。云端验证见下文；保留的本地模块仍按原回归脚本验证，历史平台记录只适用于当时的版本。

先验证改动所属模块及相关调用链，再按范围扩大。使用独立临时配置、数据和模拟 provider；不要把个人配置、会话或密钥放进 fixture、CI 或发布资源。真实模型验收会产生费用，必须已有明确授权。

在仓库根目录执行：

```bash
python scripts/check-boundaries.py
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked
pnpm --dir client/web typecheck
pnpm --dir client/web test
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
```

全量 Rust 检查用 `cargo test --workspace --locked`。Linux 先安装开发依赖，Windows 发布使用 MSVC；桌面须经 Tauri CLI 构建以嵌入生产前端，详见[发布指南](../releases.md)。上述是可执行入口，不表示每次文档更新都重新运行。

`acceptance.py` 检查空配置、旧接口退役、本地访问、显式导入/重启与 worker；`social_acceptance.py` 以模拟 provider 驱动真实业务。编译不证明安装通过，模拟模型不证明真实模型能力，自动输入中文不证明真实输入法候选正常。

## 云端真人聊天验证

2026-10-22，本机 Windows 与用户授权的 Debian 测试服务器、Ubuntu 客户端完成以下检查：

- PostgreSQL 集成测试覆盖邀请保留和消费、验证与重发、密码重置及会话撤销、私聊权限、重复命令、编辑撤回、附件、列表和搜索、资料事件及旧数据样本映射。
- 前端类型检查、完整测试、Web 构建通过；桌面原生请求、会话隔离与流分帧测试、MSVC 检查及嵌入式调试构建通过。
- Windows 桌面同一原生网络库使用显式测试 CA 访问 HTTPS 健康接口返回 200，保持证书与主机名校验。服务端 Clippy 严格检查、工作区格式及三项依赖边界检查通过。
- 独立服务容器部署在 Tailscale 内网，HTTPS 使用明确指定的测试 CA；数据库不开放主机端口，测试邮件只写私有收件箱。
- Windows 与 Ubuntu 通过 HTTPS 登录同一服务，互发文字和附件，验证下载与 ZIP 内容、重新登录和服务重启后继续查看。第三个账号无法读取私聊、附件、导出或搜索结果，跨站修改请求被拒绝。
- Windows 独立桌面窗口实际登录、发送文字并接收 Ubuntu 的实时回复；置顶、已读更新、中文正文搜索和图片预览正常。通过原生保存窗口下载文件，落盘字节与服务端原件一致；正常关闭、更新调试构建并重新登录后，历史、置顶和未发送文字草稿均恢复。
- 桌面连接反馈已修正并实测：修改地址立即禁用登录，不可达地址显示错误；恢复正确地址后实际读取服务返回，显示已连接再允许登录。使用原有 TLS 验证，没有调整 VPN 或系统证书设置。
- 测试只使用新建账号与数据，没有调用模型、迁移日常数据或更新已安装程序。

本轮内网及 Windows 桌面聊天验收已完成。Ubuntu 此轮为 SSH 下的 HTTPS 客户端测试，不包含 GNOME 图形界面。真实输入法候选、平台安装与其他缩放不在本轮复验范围；正式证书、真实 SMTP 送达和新版安装包未验收。

数据库测试需设置独立的 `CLOUD_TEST_DATABASE_URL`；未设置时会跳过依赖数据库的检查，不能视为通过。CI 单独启动 PostgreSQL 执行这组测试。

```bash
cargo test -p cloud-server --locked
cargo test -p tauri-frontend --lib cloud::tests --locked
python scripts/check-boundaries.py
```

部署后的跨机器检查使用 `scripts/cloud_acceptance.py`，参数和准备方法见[云端部署说明](../../server/cloud/deploy/README.md)。脚本会创建专用测试账号并重启测试应用容器，输出文件包含测试登录资料，应保留在忽略目录。旧库样本检查只是迁移设计核对，不是正式数据导入工具。

## 原有界面接入云端的 Web 验证

2026-10-22，沿用原有侧栏、聊天和联系人样式后，在 Edge 中使用独立测试账号与 PostgreSQL 验证。网页和服务运行于本机回环地址，数据库使用经 SSH 连接的独立测试库；未修改现有测试部署或日常数据。

- 真实账号登录、错误密码提示、刷新后保持登录正常；可以查看登录会话、撤销当前会话并重新登录，历史和草稿保留。
- 中文与 emoji 消息发送成功，另一个账号的文字和图片通过实时订阅出现。
- 图片可在弹窗预览；Esc 关闭后键盘焦点回到原图按钮。
- Shift+Enter 换行，未发送的多行文字草稿刷新后完整恢复；搜索可定位原文并返回聊天。
- 通讯录、联系人资料、我的账号与外观设置沿用原布局；浅色和深色主题、窄屏聊天及联系人返回列表已检查。
- 前端类型检查、全部测试及 Web 构建通过。新增测试实际渲染认证和聊天组件，检查表单字段、原界面结构和待确认发送状态。

此结果适用于本轮 Web 界面，不代替桌面窗口、真实中文输入法和安装包验收。没有更新本机已安装程序，也没有接入云端 AI 或群聊。

## 本地模块回归场景

| 改动范围 | 应核对的结果 |
| --- | --- |
| 账号与聊天 | AI 身份及唯一私聊稳定，当前成员共享完整群历史；移除联系人不等于停用；屏蔽、管理员与只读账号视角权限正确 |
| 消息与执行 | 生成中连续发送、修订/撤回、暂停/取消、断线重连与重启不丢终态、不重复发布、不重放执行 |
| 意识与来源 | 记忆/摘要/群引用按角色与窗口隔离；源版本或成员变化使派生材料失效，迟到结果不能覆盖 |
| 配置与额度 | 在飞请求使用同一配置快照；非法模型和参数明确报错；自动轮数、额度、约定到期和停止规则正确 |
| 存储 | 事务失败无伪成功，目录冲突有排他锁，迁移与重复导入保留历史、原文件和幂等关系 |
| 附件和界面 | 图片/文件选择、取消、处理、剪贴板/拖放、草稿重开、原生保存/覆盖/失败与导出字节正确；生成及附件处理中关闭不丢材料 |
| 平台与包 | 真实目标设备的窗口/缩放/输入、实际路径、快捷方式、安装标识及 worker；两条 HTTP/IPC 适配保持同一业务语义 |

测试检查用户可见结果和边界，不只镜像实现。工具执行、云端 AI 节点和云端群聊尚未接入，不计为已通过。

## 最近一次代码拆分验证

2026-10-22，在本机 Windows 上验证四部分目录与核心库拆分：

- Rust MSVC 全工作区测试、格式检查与 Clippy 完成；Clippy 仍有原代码的风格提示。
- 共用前端类型检查、全部 18 组测试及 Web/桌面前端构建通过。
- Agent 核心与连接器依赖边界检查通过；另一个独立 Cargo 工程成功引用连接器并解析订阅事件。
- 本地宿主验收通过。Release 构建通过完整模拟聊天验收和普通文件专项，覆盖上下文、摘要、来源失效、5.6 MiB 图片组合、12 MiB 文件组合及携带附件的导出。
- 发布版本与脚本检查、文档链接检查通过。未进行安装包制作、安装更新或新的桌面人工验收。

本轮 Debug 构建在两组大附件请求中触发脚本的 15 秒超时，图片单独复测也超时；Release 构建在原等待上限内通过。没有修改业务行为或放宽超时，也未把 Debug 的完整验收记为通过。复现发布构建检查：

```powershell
cargo +stable-x86_64-pc-windows-msvc build -p web-server --release --target x86_64-pc-windows-msvc --locked
python scripts/social_acceptance.py --binary target/x86_64-pc-windows-msvc/release/web-server.exe
python scripts/social_acceptance.py --binary target/x86_64-pc-windows-msvc/release/web-server.exe --only-files
```
