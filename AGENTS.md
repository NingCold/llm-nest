# AGENTS.md — LLM-Nest

本文件面向修改仓库的开发者和编码代理，记录当前架构、必须维持的行为和验证方式。用户入门见 [README](README.md)，环境搭建见 [开发指南](docs/development.md)，平台验证见 [验证指南](docs/validation/README.md)。

## 项目定位与当前范围

产品目标为云端社交服务、独立真人/AI 账号和用户托管的 Agent。[云端社交与 Agent 接入计划](docs/project/cloud-plan.md)第 0 步设计确认已完成，仓库已按 client/server/agent/connector 拆分。第 1 步已实现独立云端真人服务和默认云端客户端；第 2 步已实现云端 AI、节点配对与 WSS、独立 Agent、私聊流式回复及持久恢复，已完成本机 Agent 与云端 Web 闭环验证。群聊和意识资料迁入属于第 3 步，在线管理与桌面后台入口属于第 4 步。当前验证状态见进度文档。

保留一个仓库，划清客户端、服务端、自带 Agent 和连接器四个模块的边界：前三者独立运行，连接器可独立引用。云端 PostgreSQL 保存社交记录，附件使用独立持久目录；自带 Agent 的 SQLite 保存角色、模型参数、记忆和每个 AI 自己的窗口摘要。模型 Key 与节点凭据只在节点保管，不进迁移包；外部 Agent 自选内部实现。

首轮真人邀请注册，后续再开放；采用邮箱 + 密码登录，含邮箱验证与找回密码。AI 使用管理员授权的专用可撤销节点凭据，无需邮箱密码；该 AI 所有管理员可管理节点，只有初始管理员可增删管理员。群采用群主、群管理员、普通成员三级，AI 管理权不自动授予群管理权。具体目标见[账号设计](docs/design/accounts.md#已确认的云端账号方向)和[群管理设计](docs/design/conversations.md#已确认的云端群管理方向)。

官网与桌面均可创建同一种云端 AI 账号，桌面可选择自带 Agent 在本机运行；管理入口统一为“我的账号 → 我管理的 AI”，创建和只读查看 AI 都保留管理员实际登录身份。首轮一个 AI 同时只有一个有效运行节点，一个节点可运行多个 AI；管理员手动更换节点后旧节点失去该 AI 的执行与发言资格。账号与云端聊天历史沿用，角色设定、记忆与摘要需迁移。

关闭桌面窗口默认让本机 Agent 继续后台运行，首次关闭说明并提供停止选项，托盘可随时停止；远程节点不受影响，本机休眠或关机则本机 AI 离线。官网经平台转发在线管理请求，节点保存后才成功，离线不排队修改内部配置；转发正文不写云端存储或日志，模型 Key 只在节点本地填写。

云端维护发言任务与次数，节点维护调用、并发和 token 预算；模型调用前取得云端持久开始确认，已开始而结果不明的调用不自动重做。首轮单机容器部署，客户端 HTTPS/SSE，节点主动 WSS 连接；迁移先备份预览、保留旧库，原文迁云端、意识资料迁节点。细节见[已确认约定](docs/project/cloud-plan.md#已确认的运行与迁移约定)与模块设计。这些是目标约束，不代表现有程序已完成改造。

当前默认入口是 `client/web/src/cloud`，沿用原有前端的消息/通讯录布局、侧栏和视觉样式，云端账号、聊天和草稿由 CloudApp 管理。登录后复用 `social` 的界面组件不意味着启动本地 Runtime 或开放本地 AI 功能。Web 与 Tauri 都连接 `server/cloud` 的 HTTPS JSON/SSE。服务独立使用 PostgreSQL 和附件目录，不加载本地 Runtime、SQLite 或模型配置。桌面 IPC 转发云端请求并保留原生保存、窗口和草稿关闭交互，默认页面不启动保留的延迟 Runtime。桌面二进制仍为 `LLM-Nest`，内部 Tauri Cargo 包名为 `tauri-frontend`。

首轮邀请由维护者生成，可撤销、默认七天有效且只能注册一次；注册保留三十分钟，验证邮箱后原子创建账号并消耗邀请。密码使用 Argon2，令牌仅存哈希，真人身份来自 HttpOnly 会话 Cookie。网页变更请求检查 `X-LLMN-Client` 与来源，桌面凭据只在原生内存，重启重新登录。测试服务仅在 Tailscale 内网使用，测试 CA 明确信任、私有 outbox 不公开；不关闭 TLS 验证，不提交具体服务器连接资料。

云端联系人为有向关系，双方唯一私聊；每次读取、搜索、下载和导出检查当前认证账号。消息、个人读态、命令摘要回执与持久事件同事务；回执不存正文或附件原字节，重放返回当前权威消息，撤回不能被旧回执复活。附件按生成的消息 UUID 与索引存放，读写时复核 revision；SSE 按账号从持久事件补齐，客户端成功刷新后推进游标。草稿按服务器、账号、窗口隔离，发送结果未确认时只允许手动重试同一请求。详细云端契约见 [API](docs/design/api.md) 与 [存储](docs/design/storage.md)。

下文的 SocialFeature、旧 SocialStore/SQLite、群聊、显式记忆、摘要和接话规则记录保留的本地实现，供回归及后续迁移核对；云端 AI 的当前行为以 connector::node 与 agent/node、server/cloud 的协议为准，不能把旧本地管理和意识功能当作已接入。旧 Harness、CLI/TUI 与 demo 已退役。工具 worker 仍是底层能力，社交执行尚未开放工具；MCP、通用编码代理、Shell/文件工具、插件市场、知识库和工作流编排尚未实现。不要根据目录名、预留类型或旧测试声称云端功能已经可用。

原本地首版支持一个真人与多个 AI 私聊、群聊。主入口为消息/通讯录，记忆管理位于聊天详情与联系人详情；每个角色与用户有唯一长期私聊，各群独立，群成员共享入群前后的完整群历史。软件上下文与记忆机制统一称为 [意识系统（Consciousness）](docs/design/consciousness.md)。本地 SocialFeature 读取并检查材料，调用 `agent/core` 按角色与空间构造请求。

ActorId 同时作为稳定账号 ID；真人/AI 共用公开 Account 资料，内部 persona/执行绑定只经管理员接口返回。ContactEntry 是按 owner 的有向联系人条目，listed 与 blocked 独立，移除不改 Actor.status、私聊、群成员或运行。创建者是初始管理员，可授予/撤销普通管理员；普通管理员不能转授或移除初始管理员。停用/恢复只经账号管理权限执行，恢复不重放旧队列。账号查看是带权限复核的只读快照/分页，实际操作者不变，不接收任意客户端 operatorId，不复用全局订阅或提供人工代发。SQLite v6 迁移保留旧停用状态与历史，不将内部 persona 复制成公开 bio；v7 增加显式记忆，v8 增加窗口分段摘要、配置与来源修订，v9 增加跨窗口引用依赖索引，v10 增加接话规则/状态/每日额度，v11 增加约定联系设置与意图、计划的独立触发来源，v12 增加消息完成顺序和个人已读/未读/置顶状态并替换闲置 read_positions，v13 增加个人屏蔽状态，不改写旧迁移。

当前能力见 [已完成](docs/project/completed.md)，任务顺序见 [当前安排](docs/project/current.md)，剩余范围见 [待办与候选](docs/project/backlog.md)。云端测试使用独立 PostgreSQL 与测试邮件；本地模型回归继续使用临时数据和模拟 provider。本文维护修改约束，不重复维护各批测试数量。

## 工作前先确认

- 读取 `git status --short`，保留用户的未提交修改。不要混入无关生成文件、环境配置或测试数据。
- 通过实际调用链确认类型、签名和行为；过往报告描述的是当时的版本，不应覆盖当前代码。
- 修改只落在负责该行为的模块。复用成熟的协议与基础能力；社交用例以新架构约定为准，不为适配旧接口改变领域或生命周期，不为单一前端复制业务逻辑。
- 避免过分防御性编程：在外部输入、存储事务、权限和模型协议边界校验；内部使用已经成立的类型与不变量。不重复校验、不吞错或静默回退，不为假设性场景提前增加空框架、备用路径和配置开关。
- 正常验证使用独立临时配置/数据和模拟 provider。真实 API 验收会产生费用，仅在已有明确授权时执行。
- `config/llmn.toml`、`.env`、用户会话、API Key 不得提交、打包、上传到 CI 或打印到日志。保留环境变量引用，不把它替换为明文。
- 图标的唯一编辑源是 `assets/branding/app-icon.svg`；不要单独修改某一个派生尺寸。
- 已授权的操作若因工具或权限限制无法执行，可脚本化的部分应提供脚本和运行方法交给用户；说明影响范围，能预览的先验证预览，不把未执行写成完成。

## 模型分工约定

- 用户已授权：需求和边界明确的日常功能、局部修复、测试与文档整理，默认委派给 GPT-6.1-Sol 子代理执行，无需逐次询问。
- 主代理负责明确架构与行为约定、划分互不冲突的修改范围、审核实现和验证结果，并完成文档一致性检查及最终交付。跨模块工作先确定共享契约，再分派实现。
- 计划按完整可体验的结果安排批次，写明范围和结束条件；前端、后端、验证与文档是批次内的工作，不逐个升级为产品阶段。新想法放入候选清单，不自动延长当前批次或成为下一批；依赖明确且互不冲突的工作可并行。
- 委派时提供必要的项目背景、约束、文件范围与验收要求；保留用户未提交修改。子代理负责自己范围内的实现与相关检查，统一提交由主代理完成。
- 主代理和子代理都遵守上面的“避免过分防御性编程”约定；任务交代与代码审查明确检查这一点。边界校验保留，内部直接使用已成立的不变量，不叠加重复检查、静默兜底或假设性扩展。

## Git 工作约定

- 普通功能修改、问题修复和文档更新默认直接在 `main` 上开发并提交，不自动新建 `codex/*` 分支。
- 仅在用户明确要求，或较大的实验性改动确需隔离时新建分支；创建前说明原因。
- PR 不作为日常默认流程。需要创建时，单独说明用途和目标分支，不把推送分支当作已经合入 `main`。
- 多台机器接续工作前，确认工作区状态并同步 `main`；保留未提交修改，不能用强制覆盖解决分歧。
- 汇报时分别说明本地提交、远端推送与合并的实际状态；用户要求提交不等于要求推送、创建 PR 或发布版本。

## 代码地图

| 路径 | 责任 |
| --- | --- |
| `crates/common` | Message、ContentPart、ID、Usage、计时、中断与运行检查点等公共类型 |
| `crates/social` | 中立身份/空间/消息/运行类型、意识系统范围与来源快照、可控时钟 |
| `agent/ai-client` | AiProvider/AiClient、ModelRouter、内置目录、配置类型、协议工厂、协议适配、SSE 分帧 |
| `agent/node` | 独立 llmn-agent、配对、每 AI 本地档案、SQLite 执行检查点与节点预算；模型 Key 仅引用环境变量 |
| `agent/core` | 纯上下文组装、生成预算、摘要提示词；独立 ContextInput，不读取存储或配置 |
| `connector` | 社交订阅编解码；中立节点 DTO、HTTPS/WSS 客户端与独立外部示例 |
| `crates/storage` | SocialStore/SQLite、目录锁、事务/备份与独立旧格式导入边界 |
| `agent/tools` | Tool、ToolRegistry、固定白名单子进程和内置 echo/add |
| `crates/runtime` | 配置/目录缓存/热更新、Feature 生命周期与必需的 SocialResources 注入 |
| `server/social` | 联系人/私聊/群聊用例、修订/参与控制、持久接话队列、意识材料适配/显式记忆/窗口摘要任务/群话题接续、运行协调、共享业务 DTO |
| `server/http` | HTTP/SSE 适配、GUI DTO、静态前端服务 |
| `server/cloud` | 独立云端认证、真人/AI 账号、节点绑定和任务、PostgreSQL 社交记录、附件与持久事件；当前默认客户端的后端 |
| `client/web` | 共用 React GUI、状态管理、HTTP/Tauri backend 适配 |
| `client/web/src/cloud` | 默认云端账号、私聊、草稿与 HTTPS/SSE 界面 |
| `client/web/src/social` | 共用社交样式与基础组件、草稿存储，以及保留的本地社交状态与视图 |
| `client/desktop/src-tauri` | 桌面 IPC、延迟初始化、平台窗口和打包 |
| `scripts` / `docs` | 可执行验收/构建流程与证据 |

旧 SessionManager、ChatFeature、JSON 写入存储、RuntimeEvent、PluginManager 和 app-sdk 空壳已移除。历史 JSON DTO 仅位于 `storage::legacy`，导入实现位于 `storage::social::legacy`；协议适配与当前社交模型不依赖旧运行生命周期。

`server/cloud` 已独立运行并使用 PostgreSQL；`crates/runtime`、`crates/storage` 仍供保留的本地宿主回归使用，`server/social` 仍协调本地模型运行。桌面还链接延迟 Runtime，但云端入口不初始化它。`agent/node` 已有独立入口与节点 SQLite，不依赖旧 Runtime/SocialStore 或服务端；不要把桌面的过渡依赖当成目标架构。`connector` 不依赖 Runtime、存储、模型或 GUI；`builtin-agent` 不依赖存储、Runtime、服务端或连接器；`cloud-server` 不依赖本地 Runtime/SQLite 或模型模块。运行 `python scripts/check-boundaries.py` 检查已落实的依赖边界。

## 架构边界

社交重构遵守总纲中的 [新架构优先与兼容边界](docs/design/overview.md#新架构优先与兼容边界)：旧代码、配置格式与已完成的 M0 接口都不能成为新领域设计的限制。可以重写 Runtime 资源注入、聊天用例、存储接口和 HTTP/IPC；保护历史与迁移路径，兼容只在边界提供，不要求永久保留旧实现或全部旧接口。不得把 Session 直接当作 Conversation、用全局模型配置承载 AI 身份，或用旧的一问一答生命周期约束群聊与独立 AI 运行。

旧产品入口已完全退役，不再为旧会话接口保留双轨运行。兼容只用于历史数据的读取/导入和 SQLite 顺序迁移；不改写旧迁移、不删除用户数据。

```text
React GUI → Web HTTP/SSE / Tauri IPC/Channel
                    ↓
               SocialFeature
      身份 / 聊天 / 意识 / 参与与执行
                    ↓
       Runtime / AiClient / SocialStore
                    ↓
           协议适配器 / SQLite
```

- 前端调用 Feature 的具体业务方法，消费事件，不构造供应商协议 JSON。
- `Feature` 负责 id、initialize、shutdown 和 Arc downcast；`FeatureContext` 只注入 llm 与必需的 SocialResources（store/clock）。Runtime 不依赖具体 SocialFeature；新业务 API 不必塞进统一 Feature trait。
- `AiClient` 按 `(provider, effective protocol)` 选择适配器；同一 provider 的模型可以覆盖默认协议。
- 模型解析/调用使用一致的配置快照。配置更新不能让在飞请求的目录和 provider 路由来自不同版本。
- 协议差异集中在 `ai-client/src/protocols`，共享 SSE 分帧。业务层和 GUI 不应随供应商名称堆条件分支。

## 必须维持的行为

### 模型、配置与凭据

- 模型名、provider、reasoning effort 严格校验，错误返回可用候选。不要把不存在的模型静默换成另一个。
- 内置目录允许省略协议、地址和模型列表；自定义 provider 需要声明这些内容。空文件、仅注释或空 providers 都是有效首次启动配置；错误 TOML 或字段类型仍报错。社交允许先保存真人消息，未配置/失效模型返回执行提示，不生成模拟回答、不替换模型。
- 热更新先整体校验，成功才替换；失败保留旧配置。在飞请求继续使用原快照。
- 模型目录增补通过 `toml_edit` 定点补充模型，保留注释和其他配置；不可悄悄重写整份 TOML。
- GUI“同步模型目录”由 `ai-client/catalog_sync` 适配 Pi 公共数据，Runtime 校验后原子保存到实际数据目录的 `cache/catalog.json`（不能放会话根目录）。优先级为用户显式字段 > 同步缓存 > 内置目录；不重写配置/源码、不导入远端凭据或端点、不改变默认模型。失败保留旧数据；所有配置重建路径必须合并缓存，在飞请求仍使用原快照。
- 图片能力按精确 provider/model 解析为 supported/unsupported/unknown，人工 image_input > 同步目录 > 内置声明 > 未知；目录缺失信息不当作不支持，不按模型名猜测。人工覆盖经 Runtime 定点原子保存，返回跟随目录只删该字段。普通聊天只暴露账号的三态能力，不泄露绑定。AiClient 按冻结快照检查完整请求的图片（含历史与群引用），明确不支持时不调用 provider，社交记录 image_input_unsupported；真人消息照常保存，其他群成员独立执行，自动额度沿用失败不返还规则。未知允许尝试，不主动探测、不静默丢图/替换模型；配置声明不等于实际识图验收。
- `.env` 层级为进程环境 > 配置目录 > 默认数据目录，只填缺失变量。密钥解析只发生在需要凭据的边界。
- 社交 AI 的 temperature/max_tokens 保存于管理员维护的 ExecutionBinding，不读取旧 TOML `[gui]` 参数；旧字段保留但不再提供读写接口。留空分别使用模型服务默认温度与意识系统自动输出预算；显式输出上限按原值预留，按协议/思考模式、模型容量和必要输入校验，不静默丢弃或裁减。运行使用同一配置快照。旧绑定缺失字段按 None 读取，存储候选序列化省略 None，保留旧命令摘要兼容。
- OpenAI 兼容返回同时允许 `reasoning_content` 和 `reasoning`：优先非空前者，否则后者，不重复拼接。

### 会话与恢复

- 采用写穿透：先成功落盘，再替换内存；失败上抛，不能让 UI 展示“已保存”。
- DataDirectoryGuard 持有 `.llmn.lock` 的 OS 排他锁。同目录只允许一个独立 Runtime；不要删除运行中的锁文件或退回 PID 文件锁。
- `Runtime::open(config_path, data_dir)` 取得一次目录锁并直接打开 SQLite，不启动旧会话加载器。SocialStore 在单写线程原子提交实体、事件引用和命令回执，不跨模型请求持有事务。Runtime 关闭等待 Feature 中断收尾和 writer 排空。AI 绑定由获授权管理员经 SocialFeature 和共享 AiClient 严格校验；启动前复核档案/绑定版本，普通配置修改不使已开始运行失效，最终提交仍复核消息来源与参与状态。
- 新消息创建真实 UUID。历史 JSON 仅由设置中的显式导入读取：校验完整目录的重复 ID，所选来源与预览一致，事务中补齐缺失 ID 并保存映射，原 JSON 不改写。导入为只读历史窗口，不自动执行或合并为账号私聊；checksum 重复导入幂等，源变化返回明确错误。Web/IPC 共用当前操作者，不接受任意路径或 operatorId。
- 社交用例先提交真人消息，生成另行开始；成功回答、运行终态、用量/思考详情与持久事件同事务。流式每 500ms 保存有变化的 draft。
- 社交订阅断开不取消生成；EOF/取消/失败保留片段和终态，下一次请求只读 committed 原文。当前社交执行不开放工具。
- 启动遇到 Running 检查点时，幂等恢复片段并标为 Interrupted。不自动重放工具，不声称能恢复 provider 的 token 游标或回滚副作用。
- 社交库保留独立 AiRun 和运行详情，尚不是完整协议/工具审计日志。旧检查点仅在历史导入时转成中断记录，不自动重放。
- 显式记忆归属稳定 AI 账号及当前关系/群范围。仅读取 confirmed；来源消息修改/撤回同事务标 stale，编辑/遗忘提升窗口上下文版本并中断在飞回复，提交时复核记忆版本。新增记忆仅影响新运行。遗忘清空记忆及修订正文，保留来源 tombstone，不删除原聊天或备份。范围由服务端推导，管理员身份不扩大窗口访问；HTTP/IPC 共用用例。
- 窗口摘要由成员显式启用专用模型和输出预算，任务归属窗口，不是 AI 发言或 ParticipationPlan。仅概括 committed 原文；queued/running/ready/failed/interrupted/stale 与来源版本持久化。源编辑/撤回同事务清空依赖摘要及修订正文并标 stale，迟到结果不能覆盖；配置/窗口参与变化停止未完成任务。后台全局并发 1、每次唤醒最多 4 段、120 秒超时，关闭/崩溃后不重放。ContextSnapshot 只记录实际读取的 ready 分段，最终提交复核。HTTP/IPC 共用用例和 summaries_changed 引用事件；摘要不依赖显式记忆，不将遗忘条目等同撤回原文。
- 群话题接续沿用唯一私聊。ContextReference 随真人消息原子保存，仅包含 ID/版本；引用 1–8 条共同群原文，可明确附带覆盖所选原文的一个 ready 摘要。预览/发送/运行开始/提交均复核双方当前成员和来源版本，管理员不扩大访问。普通追问只读取最近 12 条私聊记录的最新引用；显式附带或回复的引用必须完整容纳。源编辑/撤回或相关成员退出同事务标 stale、失效关联消息派生材料并中断运行，重新入群不恢复旧引用；来源预览不持久缓存正文、不接受迟到旧读取。HTTP/IPC 共用用例。
- M3 接话规则按窗口保存，群聊默认只响应结构化 @ / 回复；limitedAuto 显式开启后按稳定账号顺序有限轮转。私聊默认 800ms，仅合并未开始且无引用/回复的普通输入计划，完整保留原文/回执，所有 input_ids 都是必要上下文，任一源修订取消合并计划。在飞快照不变；真人新输入替换尚未开始的自动计划。每窗口生成 1、全局社交并发 4；默认连续 AI 发言 3、Bout 自动调用 12、每日自动调用 48，按窗口/UTC 日预留 262144 tokens，开始事务按整个模型输入/输出容量扣减，失败/取消/重启不返还，墙钟回拨不重开旧日额度。明确点名不计自动额度。规则变化取消自动计划和运行；每窗口一个可取消调度任务，Clock.wait_until 可由 ManualClock 驱动，空闲默认 300 秒结束，排队有效期 5 分钟，过期不补发。HTTP/IPC 共用 participation_changed 与规则用例。
- 一次性 ContactIntent 由真人成员在独立窗口授权开启后明确创建，时间按 UTC 与创建时固定偏移保存；到期通过 participation_tick 原子领取并排入既有执行队列，不伪造真人消息。意图正文不可就地修改，取消后可新建；创建/取消/设置用幂等回执和各自版本。免打扰仅影响约定联系，支持延后或跳过；开始和最终提交复核授权/成员/暂停/有效期，执行中到期停止并保留片段。主动调用与有限自动接话共用额度；一次成功联系不继续自动接话。结束 Bout 保留 pending 未来意图，关闭授权、账号停用、窗口归档或目标退群取消对应意图；恢复不重放。queued/claimed 未开始工作重启后重评，executing 恢复为 uncertain（已过期则 expired），终态/消息/run 同事务完成且不自动重发。Web/IPC 共用 contact_schedule_changed；首版无重复日程、系统唤醒或离线推送。
- 下一次模型请求过滤中断消息；展示元数据（ID、反馈、思考、计时、用量、中断状态）通过 wire 转换剥离。工具调用与结果按各协议规则成对转换。
- Usage 的 prompt_tokens 包含缓存输入；cached_tokens 是命中部分，缓存命中率为 cached / prompt，避免再次加到分母。
- 图片消息通过共享发送用例保存收到的图片字节，支持 PNG/JPEG/WebP，每条最多 4 张，单张 4 MiB、合计 8 MiB，边长 4096；服务端校验真实格式与解码限制，不接受路径或远程 URL。历史 DTO 只返回 index/mime/byteLength，图片按消息 ID、part index 和 revision 读取，普通读取复核当前成员，账号视角复核管理员及目标成员。改文字保留图片，撤回清空图片及旧修订；Web/IPC 共用语义。
- 普通文件沿用发送事务与本机 Blob 草稿：每条最多 4 个，单个 10 MiB、合计 20 MiB，可空文件；文件名/类型/base64 在入口校验，不接受路径。SocialContent::File.name 可选且 None 省略，保留旧序列化与回执。HTTP 发送上限 40 MiB；历史只给文件元信息，下载按消息/part/revision 复核当前权限，统一 attachment/octet-stream/no-store/nosniff。Tauri 仅从原生对话框取得目标，选择后再复核并原子写入，不自动打开文件。意识系统/群引用/摘要只提供名称、类型、大小和未读取内容说明，不将普通文件字节发送模型。正文修订保留附件、撤回清空；聊天导出按下述附件选项决定是否携带原文件。
- 图片缩放/压缩在共用前端本机执行；合规选图默认保留原字节，超限时或主动处理草稿图时进入确认窗口。按比例缩小、不放大小图，WebP/PNG 保留透明、JPEG 明确填白底；输出格式不受支持时报错，不接受 Canvas 静默回退。仅确认当前预览后加入/替换草稿，选项变化使旧预览失效；取消/迟到处理不改草稿、不自动发送。确认时重查当前草稿张数和大小，服务端原有限制不放宽。处理源限制单张 20 MiB、解码后 6400 万像素；临时 URL/画布在结束或取消后释放。未确认选图仅保存在内存，离开时提示、桌面关闭要求确认或取消；确认后沿用完整草稿事务与不可变待确认发送快照，替换生成新图片 ID。原图下载/携图导出保存消息收到的版本，不恢复处理前文件。
- 意识系统按选中的原文携带图片，共同群引用使用原生图片 part，不将二进制转为文本；图片按尺寸估算上下文，实际用量由模型返回，单次请求另限 16 张/16 MiB。必要材料超限报错，普通旧历史按预算截取。四种协议分别转换原生图片格式，不静默丢弃图片或替换模型；摘要首版仍只看文字和附件占位。持久化格式与 provider wire 格式分开。

- 单窗口导出在只读事务中复核实际操作者的当前成员身份，读取完整有序消息；不能用界面的一页历史代替。默认仅当前正文和状态、附件占位及引用 ID/版本；includeImages/includeFiles 分别选择图片/普通文件，任一启用就在同一快照读取选中的原字节并生成 ZIP，不逐附件重新查询拼接。包内含 chat.md/json、选中的 images/{messageId}-{partIndex}.{ext} 与 files/{messageId}-{partIndex}/{safeName}；重名分目录保存，Markdown 路径按 UTF-8 编码。JSON filePath 为实际相对路径：含普通文件选项为 v3，仅含图为 v2，无附件为 v1，不序列化二进制。不展开其他窗口或引用源图，不导出记忆、摘要、思考、角色内部设定或凭据；不改变已读、事件和运行。HTTP 与 IPC 共用格式化；桌面路径只来自原生保存对话框，选择后重新检查权限，临时文件完成后替换，写入失败不报成功。导出文件不等同整库备份，不接入旧历史导入器。
- 原图另存为保留原始字节，文件名由消息 ID、part 索引和识别格式生成，不使用客户端路径。每次保存重新按消息 revision 与当前成员权限读取；账号视角同时复核管理员及目标成员。桌面在原生对话框前后复核，不从已加载预览缓存保存；Web 二进制下载沿用本地访问校验、no-store 与 nosniff，不接受 operatorId。保存失败和取消明确区分，成功下载不等同确认写入磁盘。

### 工具执行

- `Tool::run` 必须明确实现可丢弃、会让出执行权的异步契约。Registry 的超时不能抢占不 yield 的进程内代码。
- 默认 echo/add 通过当前宿主 EXE 的 `--llmn-tool-worker` 入口执行；所有宿主在配置、密钥、日志、UI、存储初始化之前处理此入口。
- 子进程只允许固定编译白名单，清空继承环境，输入/输出各限 64 KiB。Registry 默认 30 秒墙钟超时并捕获 unwind panic。
- Windows 使用 Job Object：最多 1 个进程、256 MiB committed memory、10 秒用户态 CPU，Job 关闭时杀子进程。配置/附加失败须停止，不得回退到进程内。
- Linux 当前依赖子进程终止/kill_on_drop，没有等价的完整资源配额或通用沙盒。打包后须实际验证 worker 的动态库加载和退出行为。
- `register_trusted_tool` / `register_trusted_in_process` 是显式的信任边界，不用于模型指定的任意插件。

### 联系人屏蔽

- ContactEntry.blocked 与 listed 独立，共用条目版本；实际操作者只能改自己的条目。任一方向屏蔽限制双方私聊新消息，不隐藏历史、不改变共同群的完整历史及正常发言，也不停用 AI 账号。
- 屏蔽事务同时结束私聊当前交谈、取消运行/排队和双方尚未完成的约定（包括共同群中本人创建的约定）；不影响其他账号约定。发送、执行开始/提交以及约定创建/到期复核屏蔽。解除只响应新消息，不重放；Relationship 版本防止屏蔽前快照迟到启动。
- 记忆、摘要、原文、已读与置顶保留；HTTP/IPC 共用用例及 contact_changed，界面保留草稿并按后端 directBlocked 禁止发送。移除/重新添加、管理员恢复账号、恢复接话都不能清除屏蔽。

### 消息列表与搜索

- 列表状态按实际操作者与窗口保存，不改变 Conversation/context/participation 版本，不向模型注入已读状态。读取位置使用终态消息的 delivery position，不能用消息 sequence 代替，避免较早开始、较晚完成的 AI 回复漏计。自己的消息、生成中和已撤回消息不计未读；中断片段计一次。v12 升级与显式导入的已有历史默认已读。
- 已读/手动未读/置顶共用版本化命令、事务事件和回执。只确认服务端返回且属于该窗口的位置，读位置单调；自动确认来自同事务读取的最新页，前台聚焦且滚动在底部才提交。搜索定位、翻历史、后台窗口和管理员只读查看不自动改已读。
- 正文搜索覆盖当前成员有权访问的全部已保存正文，中文和符号按字面子串、ASCII 英文不区分大小写，按 delivery position 倒序分页；不查 generating/withdrawn、思考或角色内部设定。直接读当前原文，修订/撤回立即生效。定位重查权限并读取目标前后记录，迟到结果不能覆盖新查询或重新显示失效来源。

### GUI 与平台

- React 源码只有 `client/web` 一套。Tauri 构建输出到忽略的 `client/desktop/dist`，Web 输出 `client/web/dist`；两者均忽略，不提交派生 bundle。
- Web 与 Tauri 默认共用 CloudApp；legacy/demo/social-preview 查询参数不切换产品。云端草稿复用 IndexedDB `llm-nest-social-drafts`，按服务器、实际真人账号与窗口保存文字、图片/文件 Blob 及不可变的待确认发送快照；先保存完整命令再发送，恢复后只允许手动重试同一内容与 ID，不自动发送。附件不写入 localStorage，恢复或写入失败需显示错误，不能用空草稿覆盖；事务版本阻止多页面互相覆盖。订阅游标在对应状态应用成功后推进。保留的本地界面另有提及/回复/引用草稿及旧 localStorage 迁入逻辑；`build:social-runtime` 只用于本地模块回归，写入忽略的 `.social-runtime-dist`。
- IPC 订阅属于调用 WebView，每次推送应用成功后确认；仅一个未确认事件，确认超时释放，临时广播溢出要求 resync。替代订阅、页面重载和窗口销毁释放订阅，不能取消 AI run。主窗口关闭先向已注册页面请求草稿保存确认，等待正在读取的附件及 IndexedDB 写入；确认匹配当前 page/request 后才拒绝新业务命令、释放订阅，再等待 Runtime 运行中断保存与 SQLite writer 关闭。草稿失败或 10 秒超时保持窗口与后端可用，页面重载/销毁释放等待，未注册页面不阻止初始启动失败时退出；有界等待失败保持窗口并显示保存错误，初始化失败不妨碍关闭。
- `client/desktop/src-tauri/gen/schemas` 是 Tauri 自动生成的权限 schema/manifest，按平台重建并由 Git 忽略，不手改或强制提交。实际权限源文件在 `capabilities`，与 `tauri*.conf.json` 一样继续跟踪；不要将整个 `gen` 目录一概忽略。
- `IS_DESKTOP` 表示 Tauri 运行环境；`CUSTOM_WINDOW_CHROME` 控制 Windows/Linux 的内容内窗口按钮与拖动区域。不要把二者混同，Linux 仍使用桌面 IPC。
- 自绘窗口控制独立于聊天后端初始化；初始化/历史/设置失败有提示和重试。社交保存与 AI 执行可用性分开。
- HTTP Headers 使用 Headers.set，避免大小写重复头导致值变成 `1, 1`。修改事件名、字段命名或 DTO 时同步 Web/Tauri 两条链路。
- 当前云端入口提供消息、通讯录和我的账号，认证链接使用 `#/register|verify|reset`；保留的本地设置路由为 `#/settings/models|appearance|config|about`，不是云端已开放页面。详见 [界面说明](docs/design/frontend.md)，切换时保留当前草稿。
- Windows 的窗口参数位于 `tauri.windows.conf.json`；公共配置关闭系统标题栏，Windows/Linux 共用内容内控制，社交页不再预留独立标题栏。Tauri 合并配置中的窗口数组会整体替换，调整公共尺寸时检查平台文件。Linux 新窗口布局尚待 GNOME 实机复验，历史系统装饰验收不能证明当前布局。
- Tauri 公共与 Windows 窗口配置均关闭原生文件拖放拦截（dragDropEnabled=false），由共用 Web File 接收图片拖放，不增加任意路径读取 IPC。
- 无标题栏界面在侧栏空白与页面头部使用 Tauri 原生拖动区域；`deep` 区域排除交互元素，搜索容器显式禁用。空页面只拖动自身空白，聊天/账号正文保留选字；不要用覆盖内容的拖动层扩大命中范围。双击最大化不能替代连续拖动验收。
- Windows 保留 `shadow=false`、透明 WebView 合成表面；启动阶段为空白透明 HTML，露出原生 Acrylic，初始化成功或失败后显示不透明界面。其他平台不声称具有 Windows 原生玻璃效果。`Window.setBackgroundColor` 同步原生底色，不能换成会覆盖 WebView 透明背景的 WebviewWindow 同名 API。
- 普通 Windows 11 窗口由 DWM 绘制圆角/边框；上下占满当前显示器 rcWork（容差 2 物理像素）且未最大化/全屏时，绘制 1 物理像素矩形内描边并关闭原生描边。
- 不恢复 tao 阴影边距，不使用透明外边距、SetWindowRgn、自建阴影窗口或 CSS 裁切整个应用。窗口状态变化发事件，revision 防止迟到事件覆盖新状态。
- Windows 10 不支持 DWM 属性时回退细边框；DWM 返回成功不等同于所有机器视觉验收通过。
- 产品 identifier `com.llmnest.tauri` 和安装路径不得随意更改。默认会话目录与桌面安装版配置目录按用户要求统一为 `~/.llmn`（Windows 使用用户主目录），`LLMN_DATA_DIR` / `LLMN_CONFIG` 覆盖仍有效。宿主在读取配置/.env 前准备数据目录；旧系统 `llmn` 仅在新目录不存在、未显式指定数据目录且旧目录可获得写锁时复制，保留源目录，不自动合并。图标变更时保留 build.rs 对 icons 的依赖，核对实际安装器和 EXE 资源。
- 设置中的路径来自后端启动时冻结的实际路径。打开目录 IPC 只接受 config/data 枚举，不接受任意文件路径或可执行程序；Web 不提供打开服务器文件夹的接口。
- Windows 安装器显示名与内部标识分离：保留 `packaging/windows` 模板里的旧安装目录和注册项，MSI upgradeCode 固定为 `0dc13402-ce85-5dcb-ba1a-1f629fecd8b8`。模板基于 Tauri CLI 2.11.4，升级 CLI 时对照上游合并；打包后执行 `scripts/verify-windows-branding.ps1`。不要全局替换二进制名或历史安装标识。

## 如何验证改动

先跑受影响模块和相关链路，再按改动范围扩大。不要把编译、模拟 API、真实厂商 API 和桌面安装验收混为一谈。

```bash
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked

pnpm --dir client/web typecheck
pnpm --dir client/web test

cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
```

- 全量跨平台检查：`cargo test --workspace --locked`。Linux 需要先安装 GTK/WebKitGTK/OpenSSL 开发依赖，见开发指南。
- 云端测试设置独立 `CLOUD_TEST_DATABASE_URL` 后执行 `cargo test -p cloud-server --locked`；无变量的跳过不计为数据库通过。部署联调用 `scripts/cloud_acceptance.py`，真实地址、测试登录资料与证书保存在忽略目录，具体结果见验证指南。
- Windows 发布使用 MSVC，不能把 GNU 本地构建当作发布包。桌面必须通过 Tauri CLI 构建，确保嵌入前端；裸 cargo release 可能仍访问开发地址。
- `scripts/acceptance.py` 使用独立临时配置与旧格式夹具，验证空配置、旧接口退役、本地访问边界、显式部分导入/重启与底层 worker；`social_acceptance.py` 使用模拟 provider 验证完整社交链路。
- 旧 Harness 的真实 API 脚本已退役；新付费验收须另获明确授权，不放入默认 CI，不得从个人配置生成 CI 密钥或发布资源。
- Windows 图标检查用 `scripts/verify-windows-icons.ps1`。安装退出码 0 不足以证明覆盖正确，核对安装 EXE 和包内容。
- Linux 构建脚本、容器和发布流程见 `scripts/linux`、`.github/workflows` 与 [发布指南](docs/releases.md)。

## 新增功能的落点

- **协议**：更新 Protocol、build_provider 和对应 convert/SSE；添加协议 fixture，覆盖错误、终止和工具调用。新增 provider 品牌但协议相同时优先改目录/配置。
- **社交用例**：当前云端业务落在 `server/cloud`，`server/social` 保留原本地实现供回归和后续迁入；不要将新云端用例接回本地 Feature 生命周期。纯上下文和生成策略放 `agent/core`，节点通信契约放 `connector`，不以跨层本地调用代替节点协议。
- **工具**：先明确权限、副作用、超时和进程边界，再注册能力。不得把“支持工具调用”写成“支持任意安全插件”。
- **GUI 字段**：从持久化/Runtime 契约开始，同步 HTTP、IPC、前端 DTO、保存失败行为与历史恢复。
- **文档**：根 README 介绍应用、主要能力和开始方法；详细操作放 `docs/usage.md`，各模块 README 介绍用途、运行方式和阅读入口。使用自然、简洁的表达，少用术语，不写开发日志、架构规则或防御性清单。当前设计放 `docs/design`，已完成/当前安排/待办与候选分别维护在 `docs/project`，检查方法与平台结果放 `docs/validation`，打包专项放 `docs/packaging`；AGENTS 维护修改约束。每批工作完成后更新相关说明与进度，不默认新建批次报告或再开一份 TODO。过时过程记录从目录移除，Git 保留历史；保留的验收结论注明日期与适用版本，不改写成新版本已通过。直接修订当前规则，合并重复内容。文件名使用简短英文小写连字符，README 除外，不再用日期或阶段编号命名常规文档。最终汇报说明本批结果、当前阶段、剩余范围和下一步。

## 发布与证据

- 当前经过用户确认的 Windows 窗口基线是 `91f9a9b`，包含高速缩放、圆角和上下贴边细边框；2026-09-13 用户确认边框验收通过。
- Linux 的编译/打包结果和 GUI 验收必须分别记录，见 [Linux 验证状态](docs/validation/linux.md)。
- 发布应校验 Cargo/Tauri 版本与 tag 一致，产物包含校验和。默认流程生成草稿 Release，由维护者检查后公开。
- 不提交私有配置、测试目录或个人远程主机地址。源码发布前需要明确项目许可证；不要代替维护者虚构许可证、签名或支持承诺。
