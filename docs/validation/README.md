# 如何验证

[开发指南](../development.md) · [Windows](windows.md) · [Linux](linux.md) · [设计](../design/README.md)

当前默认入口支持云端真人与 AI 的私聊、群聊、节点意识系统、接话约定，以及 AI 管理和桌面本机运行。云端各批验证见下文；保留的本地模块仍按原回归脚本验证，历史平台记录只适用于当时的版本。

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

## 云端管理与本机运行验证

2026-10-23，第 4 步使用独立 PostgreSQL、授权内网 HTTPS/WSS 服务、本机模拟模型和独立 Agent 验证。本轮不调用真实模型，不迁移旧数据，也不替换日常安装版或原有 Agent 目录。

- 云端 31 项测试在真实 PostgreSQL 上全部通过，无跳过。新增覆盖公开资料版本与回执、初始管理员权限、普通管理员撤权、在线能力声明与返回前复核，以及只读聊天/附件权限和读态不变。
- Agent 31 项单测、连接器 13 项测试通过；独立外部示例的模拟 WSS 测试通过，覆盖任务、取消、撤权和不支持的管理请求。角色保存原子更新与指纹回执、当前档案重放、在飞配置冻结和托管进程恢复均有检查。
- 前端 25 组测试、类型检查和生产构建通过。最终审查修复旧保存响应删除新待确认命令、能力错误码不一致，补测通过后重新构建。关闭退到后台时解除输入锁已有回归测试。桌面实际保存还发现表单字段 `nodeName` 覆盖浏览器自身属性，已改名并在真实浏览器和 WebView 中确认保存成功。
- `scripts/cloud_management_acceptance.py` 在部署服务上使用三位测试真人、一个 AI 和实际 Agent 进程完成验收，只有 3 次模拟调用：保存时在飞回复保持原配置、后续采用新配置；普通管理员权限和撤权、只读他人与 AI 的聊天及附件、原身份/读态不变、停止/重启及离线不排队修改均通过。
- Edge 实际页面验证了角色保存确认和只读聊天；补验上一批免打扰时间的键盘编辑、保存后显示及清空，测试窗口已恢复原设置。
- 桌面库 27 项测试通过，2 项专用环境/子进程入口按原设定跳过。实际启动发现状态查询与节点争抢目录锁，已改为先接收节点准备完成确认，再查询状态；新增真实二进制回归覆盖 20 次隐藏启动、立即查询与停止，另在已配对的独立目录重复 20 次启动、60 次查询、20 次停止，全部通过。

Windows 独立桌面实际完成登录、本机模型配置、密钥本地保存、配对授权、启动和在线状态检查。首次关闭说明显示三个选项；取消关闭后输入恢复，选择后台运行后主窗口隐藏，节点保持在线并完成 1 次模拟回复。用户实测通过托盘“打开 LLM Nest”、恢复输入和“停止本机 Agent”，随后云端查询确认节点离线。

严格 no-deps Clippy、工作区格式和四项依赖边界检查通过。最终服务与 Web 已更新到授权内网，部署前保留 PostgreSQL 备份。验收后关闭测试窗口与模拟进程、撤销本轮测试绑定，日常 Agent 保持运行。Windows 干净安装、其他设备及 GNOME 验收仍按[待验收清单](../project/backlog.md)保留；开发构建不代替安装验收。

```bash
# 使用独立测试数据库，逐例执行。
cargo test -p cloud-server --locked -- --test-threads=1
cargo test -p agent-node -p connector --locked
cargo test -p connector --example external_node --locked
cargo test -p tauri-frontend --lib --locked
pnpm --dir client/web typecheck
pnpm --dir client/web test
pnpm --dir client/web build
python scripts/cloud_management_acceptance.py --binary "$LLMN_AGENT_BINARY" --accounts "$LLMN_TEST_ACCOUNTS" --ca "$LLMN_TEST_CA" --workdir "$LLMN_TEST_WORKDIR"
```

管理联调脚本使用 `cloud_acceptance.py` 的三位专用账号与未存在的忽略目录；`--serve-ui` 保留模拟节点供页面检查，创建该目录下的 `stop` 文件后清理。具体路径、模型模拟地址、账号凭据及证书不写入仓库。

## 云端接话与约定验证

2026-10-23，第 3 步第三批在独立 PostgreSQL、内网 HTTPS/WSS 服务和两个本机 Agent 进程上验证，模型使用模拟服务。本机日常 Agent 配套升级到协议 v4，保留账号、绑定、档案和 SQLite，已确认重新在线；没有发送真实模型请求。

- 最终源码的云端 27 项测试在真实 PostgreSQL 上全部通过，无失败或跳过。包括权限、重复命令、连续输入、群轮转与额度、UTC 日切换/回拨、结束与暂停、免打扰延后及过期、执行中恢复；既有认证、附件、意识与节点回归同轮通过。
- Agent 28 项、连接器 12 项测试通过，覆盖独立 contact 触发、合并输入与图片的必要材料、自动 token 账本、开始前撤销预留、时钟回拨和恢复不重调模型。
- 前端全部 24 组测试、类型检查和生产构建通过；最终文案修改后重跑聊天、群操作、接话三组测试及类型检查、构建。包含实际组件渲染、请求身份隔离、未确认请求手动重试、编辑中版本变化、时间换算与权限。
- 修改模块的严格 no-deps Clippy、工作区格式及四项依赖边界检查通过。
- `scripts/cloud_participation_acceptance.py` 使用两位真人、两个 AI、两个实际节点连接远端服务，检查默认显式接话、私聊连续输入、群内轮转及额度、暂停/恢复/结束、独立约定、免打扰延后/跳过、屏蔽范围、离线过期与执行中重启。每轮 9 次模拟调用，不读取真实模型 Key。
- Edge 实际操作已确认规则保存、暂停与恢复、创建未来约定及取消。当时检查时间输入期间浏览器连接中断，免打扰截止时间未完成人工检查；已在上面的第 4 步验证中补验通过。免打扰行为也由自动和双节点测试覆盖。

最终源码与 Web 构建已部署，部署后双节点验收再次通过。部署前保留 PostgreSQL 备份，仍只在授权内网开放，HTTPS/WSS 保持测试 CA 和主机名验证。未迁移日常旧库、更新已安装桌面客户端或复验 GNOME、安装包。482 个文档文件链接检查通过。

```bash
# 先设置独立 CLOUD_TEST_DATABASE_URL，未设置时的跳过不算通过。
# 数据库测试共用调度锁，按顺序运行以免测试之间争锁影响短超时场景。
cargo test -p cloud-server --locked -- --test-threads=1
cargo test -p agent-node -p connector --locked
cargo clippy -p cloud-server -p agent-node -p connector --all-targets --no-deps --locked -- -D warnings
python scripts/cloud_participation_acceptance.py --binary "$LLMN_AGENT_BINARY" --accounts "$LLMN_TEST_ACCOUNTS" --ca "$LLMN_TEST_CA" --workdir "$LLMN_TEST_WORKDIR"
```

联调脚本使用 `cloud_acceptance.py` 的专用账号文件，工作目录必须尚不存在且受 Git 忽略；需要保留节点供网页检查时加 `--serve-ui`，检查后创建该目录下的 `stop` 文件。脚本结束时停止模拟进程并撤销本轮节点，保留测试聊天供核对。

## 云端真人聊天验证

2026-10-22，本机 Windows 与用户授权的 Debian 测试服务器、Ubuntu 客户端完成以下检查：

- PostgreSQL 集成测试覆盖邀请保留和消费、验证与重发、密码重置及会话撤销、私聊权限、重复命令、编辑撤回、附件、列表和搜索、资料事件及旧数据样本映射。
- 前端类型检查、完整测试、Web 构建通过；桌面原生请求、会话隔离与流分帧测试、MSVC 检查及嵌入式调试构建通过。
- Windows 桌面同一原生网络库使用显式测试 CA 访问 HTTPS 健康接口返回 200，保持证书与主机名校验。服务端 Clippy 严格检查、工作区格式及三项依赖边界检查通过。
- 独立服务容器部署在 Tailscale 内网，HTTPS 使用明确指定的测试 CA；数据库不开放主机端口，测试邮件只写私有收件箱。
- Windows 与 Ubuntu 通过 HTTPS 登录同一服务，互发文字和附件，验证下载与 ZIP 内容、重新登录和服务重启后继续查看。第三个账号无法读取私聊、附件、导出或搜索结果，跨站修改请求被拒绝。
- Windows 独立桌面窗口实际登录、发送文字并接收 Ubuntu 的实时回复；置顶、已读更新、中文正文搜索和图片预览正常。通过原生保存窗口下载文件，落盘字节与服务端原件一致；正常关闭、更新调试构建并重新登录后，历史、置顶和未发送文字草稿均恢复。
- 桌面连接反馈已修正并实测：修改地址立即禁用登录，不可达地址显示错误；恢复正确地址后实际读取服务返回，显示已连接再允许登录。使用原有 TLS 验证，没有调整 VPN 或系统证书设置。
- 本节的真人聊天检查只使用新建账号与数据，没有调用模型、迁移日常数据或更新已安装程序；后续 AI 节点验收单独记录在下节。

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

此结果适用于本轮 Web 界面，不代替桌面窗口、真实中文输入法和安装包验收。该次检查未更新本机已安装程序，也未包含 AI 或群聊；本批后续 AI 验证见下节。

## 云端 AI 节点验证

2026-10-22，第 2 步完成本机独立 Agent 与云端 Web 的文字、图片私聊闭环。最新云端容器已部署到授权内网，部署前保留 PostgreSQL 备份；HTTPS/WSS 都使用显式 CA 并保持证书及主机名验证。

### 模块与模拟检查

- Windows MSVC：连接器 9 项、独立 Agent 7 项测试通过；连接器覆盖序列化、TLS/回环策略、模拟 HTTP 鉴权与错误、WSS 握手/重连、4 MiB 图片流式读取边界。Agent 覆盖材料与图片预算、持久预留和恢复。
- 独立 PostgreSQL：原有 10 项与新增节点 5 项测试分别实际通过；节点用例覆盖配对、绑定、任务开始、结果幂等、来源失效、取消、撤销与 WSS。不是未配置数据库后的跳过结果。
- `cloud-server` 的 no-deps 严格 Clippy、工作区格式检查与四项依赖边界检查通过。前端此前全部 20 组测试通过；最终集成后再次通过 cloud-workspace 测试、类型检查和生产构建。
- `scripts/agent_acceptance.py` 使用最终 `llmn-agent` 二进制连接远端真实 HTTPS/WSS，模型为本机模拟服务。配对及流式正式回复、重启后只处理新工作而不重复模型、取消后拒收迟到结果且下一条可回复、撤权后不再调用均通过，合计 4 次模拟调用。此项证明二进制和远端协议链路，不证明真实模型能力。

模块运行入口：

```bash
cargo test -p connector --locked
cargo test -p agent-node --locked
# 先设置独立 CLOUD_TEST_DATABASE_URL；未设置而跳过不算通过。
cargo test -p cloud-server --locked
cargo clippy -p cloud-server --all-targets --no-deps --locked -- -D warnings
python scripts/check-boundaries.py
```

二进制集成验收需要已运行的隔离测试服务、显式 CA，以及 `cloud_acceptance.py` 生成的私有测试账号文件。通过环境变量指定以下参数，`LLMN_TEST_WORKDIR` 必须指向尚不存在、受忽略规则保护的私有测试目录：

```bash
python scripts/agent_acceptance.py --binary "$LLMN_AGENT_BINARY" --accounts "$LLMN_TEST_ACCOUNTS" --ca "$LLMN_TEST_CA" --workdir "$LLMN_TEST_WORKDIR"
```

脚本创建专用测试 AI，实际发起配对、运行二进制并检查任务状态；使用本机模拟模型，不读取真实模型 Key。测试期间会写入云端测试消息并撤销该节点，不能对日常账号运行。账号文件、节点凭据、日志和结果文件留在私有位置，不提交或上传。它不替代单独授权的真实模型验收。

### 真实模型与网页

- Edge 中实际登录现有界面，从“我的账号 → 我管理的 AI”创建“小栖”，在唯一私聊输入文字并实时收到真实回复。
- 本机独立 `llmn-agent` 使用 `ecnu-plus` 完成两次真实调用：文字请求为 230 输入、27 输出 token；96×48 红蓝测试图请求为 455 输入、17 输出 token。回复正确使用“小栖”身份并识别左红右蓝，验证图片从云端授权读取、原生模型输入到正式记录的完整链路。这只证明该次模型与图片样例，不推广为全部模型识图验收。
- 本机独立节点已在后台运行；模型 Key 仅在进程环境，角色档案和专用节点凭据保存在当前用户私有节点目录。文档和仓库不包含该目录、服务器地址、账号登录资料或 Key。

本批没有更新已安装桌面程序、运行桌面新界面的实际验收或在 Ubuntu 启动 Agent。此前真人聊天的 Windows/Ubuntu 记录不代替这些检查；GNOME、安装包和真实中文输入法等平台缺口继续保留。第 3 步为群聊与意识系统，第 4 步为在线配置、管理员授权界面和桌面一键运行/托盘。

## 云端群聊验证

2026-10-22，第 3 步第一批完成。内网测试服务已更新，部署前保留 PostgreSQL 备份；本机现有 Agent 升级至节点协议 v2，原账号、节点档案和私聊历史保留，网页确认节点在线。没有迁入旧本地数据或更新已安装桌面程序。

- 独立 PostgreSQL 实际执行 18 项测试，全部通过、无跳过。覆盖群角色与成员、跨群命令冲突、并发幂等、历史/搜索/附件/导出权限、双节点 FIFO、旧回复必要上下文、准备及运行期间的新消息边界、屏蔽与停用、旧片段保留和恢复不重放；既有认证、附件、来源修订、节点更换与恢复用例继续通过。
- Windows MSVC：Agent 11 项、连接器 10 项测试通过；节点与外部示例使用协议 v2。三个受影响包的 all-targets/no-deps 严格 Clippy、工作区格式及四项依赖边界检查通过。前端类型检查、全部 22 组测试与最终 Web 构建通过；仍有既有的 bundle 大小提示。
- `scripts/cloud_group_acceptance.py` 启动两个真实 Agent 进程连接云端 HTTPS/WSS，使用本机模拟模型。两位真人与两个 AI 的顺序接话、后一 AI 读取前一回复、排除后续无关真人输入、同窗回复、完整入群前历史、成员移除/重新加入、转让/退出、群附件权限、屏蔽保留共同群互动、停用期间保存私聊及恢复不补发均通过。Ubuntu 通过 SSH 下的 HTTPS 客户端读取同一群历史并回复。
- `scripts/agent_acceptance.py` 对最终协议再做私聊二进制回归：配对、流式回复、节点重启不重复调用、取消后继续聊天、撤权后停止均通过，共 4 次模拟调用。
- Edge 实际完成建群、查找并选择其他管理员的 AI、多目标提及与独立状态、同窗口回复、文字/提及/回复草稿刷新恢复、群名修改与管理员任命。联系人屏蔽后输入框禁用，解除后恢复；管理面板停用与恢复测试 AI 正常。

复现群聊联调时，使用与上一节相同的隔离服务、测试账号文件、Agent 二进制和显式 CA，另为本轮选择尚不存在的私有目录：

```bash
python scripts/cloud_group_acceptance.py --binary "$LLMN_AGENT_BINARY" --accounts "$LLMN_TEST_ACCOUNTS" --ca "$LLMN_TEST_CA" --workdir "$LLMN_GROUP_WORKDIR" --client-host "$LLMN_TEST_CLIENT_HOST"
```

省略 `--client-host` 可只检查本机客户端；加 `--serve-ui` 可让模拟节点保持在线以便网页检查，完成后创建 `WORKDIR/stop` 文件，脚本会停止进程并撤销测试绑定。账号文件至少包含 `cloud_acceptance.py` 创建的三个专用测试账号；脚本只对前两个账号建群和测试 AI，第三个检查无权访问。测试资料、节点凭据与截图留在忽略目录。

本批只使用模拟模型，没有新增真实 API 费用，也不证明真实模型的多人交流效果。临时模拟节点已停止并撤销绑定，常用本机节点继续运行。新版桌面界面、Ubuntu GNOME、安装包、正式证书与真实邮件送达仍未在本批验收。

## 云端意识系统验证

2026-10-22，第 3 步第二批接入节点记忆、每 AI 窗口摘要、群话题接续及来源失效。内网服务更新前已备份 PostgreSQL；服务与本机独立 Agent 配套使用节点协议 v3，常用 AI 的账号、绑定和档案沿用，没有新增真实模型调用。

- 服务端完整 PostgreSQL 回归实际执行 22 项，全部通过、无跳过；最终引用导出用例补跑通过，新增管理请求取消清理单测通过，当前共 23 项。覆盖成员/管理员权限、来源变化、材料版本、摘要开始确认、重复命令、转发超时与迟到响应，以及引用导出不展开群正文。
- Windows MSVC：独立 Agent 20 项、连接器 11 项测试通过。覆盖记忆与窗口隔离、实际读取材料、最近 12 条与分段上限、断线来源同步、摘要崩溃不重调、流式回复期间的管理请求、引用必要图片预算和协议序列化。
- 三个受影响 Rust 包的严格 Clippy、工作区格式、四项依赖边界检查通过。前端类型检查、全部 23 组测试、Web 构建通过；仍有既有 bundle 大小提示。组件测试覆盖权限入口、离线反馈、原命令重试、跨页面待确认状态、引用失效及草稿恢复。
- `scripts/cloud_consciousness_acceptance.py` 启动两个实际 Agent 进程连接远端 HTTPS/WSS，使用本机模拟模型。检查记忆保存与重复提交、AI/窗口隔离、私聊记忆不进入群聊、不同管理员的摘要权限、两 AI 各自概括相同原文、群原文进入已有私聊、生成中遗忘、离线编辑后重连失效。主流程共 6 次模拟调用，全部通过。
- Edge 实际保存记忆，确认源变化后的记忆/摘要状态，并按当前来源手动重整摘要；群消息预览后进入已有私聊，文字与引用草稿刷新后恢复，发送后获得模拟回复。修复了打开已有私聊的同步通知误使预览失效并隐藏错误的问题；对应竞态回归通过。网页检查使用同一组独立模拟节点。
- 最终部署后再次通过 `scripts/agent_acceptance.py` 私聊回归：配对、流式回复、重启不重复调用、取消后继续聊天、撤权后停止，共 4 次模拟调用。

复现使用独立测试服务、三个测试账号、受信任测试 CA 和新建的私有工作目录：

```bash
python scripts/cloud_consciousness_acceptance.py --binary "$LLMN_AGENT_BINARY" --accounts "$LLMN_TEST_ACCOUNTS" --ca "$LLMN_TEST_CA" --workdir "$LLMN_CONSCIOUSNESS_WORKDIR"
```

加 `--serve-ui` 可保持模拟节点在线供网页检查；完成后创建 `WORKDIR/stop` 文件，脚本会停止测试进程并撤销测试绑定。脚本不读取真实模型 Key。节点资料、登录信息和截图保留在忽略目录，不加入部署包。

本批验证了云端 Web 和独立节点，不代表真实模型的记忆质量或摘要准确率已验收。没有迁入旧库，没有更新已安装桌面客户端；新版桌面、GNOME、安装包、正式证书与真实邮件送达仍按原清单待验收。

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

测试检查用户可见结果和边界，不只镜像实现。通用工具尚未接入；云端 AI 节点和群聊使用上面的专用验证入口。

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
