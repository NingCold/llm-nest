# 存储与迁移

[总纲](overview.md) · [聊天与消息](conversations.md) · [API 与事件](api.md)

## 当前云端存储

`server/cloud` 直接使用 PostgreSQL，不经过旧 SocialStore。`0001_accounts.sql` 保存 accounts、credentials、邀请、待验证注册、会话、密码重置和认证限流；`0002_chat.sql` 保存 cloud_contacts、cloud_conversations、cloud_list、cloud_messages、cloud_receipts、cloud_events。`0003_nodes.sql` 增加 AI 管理员、节点绑定、配对、任务及节点事件；`0004_groups.sql` 增加真实群与成员、结构化提及/回复、多 AI 任务回执、联系人屏蔽及账号启停。启动自动运行顺序迁移，不改写旧迁移。

双方 UUID 有序配对和唯一约束保证唯一私聊；群的 kind 为 group，first_id/second_id 均为 null，以 name 和 cloud_members 表达真实群，不使用 AI 账号占位。账号、窗口和消息 ID 均可沿用稳定 UUID。消息 position 是全局交付顺序，个人 read_position 不回退；消息发送、编辑/撤回、列表修改与相关账号的持久事件及回执在同一事务提交。写事务在分配事件 ID 前取得同一 advisory lock，保证可重放游标的提交顺序。init 和历史页的消息/读位置使用一致快照。

`cloud_members(conversation,account_id,role)` 是私聊与群的当前成员授权表，role 为 owner/admin/member；0004 为既有私聊补入双方 member。每群至多一位 owner 的部分唯一索引配合群管理事务保持群主不变量。成员变化提升窗口 revision，保留已提交聊天，取消未完成任务及其失效派生片段，并向旧成员和新成员提交事件；移出成员后每次读取均检查当前成员表。

0004 为 accounts 增加 status/status_revision，为 cloud_contacts 增加 listed/blocked/revision；通讯录列出与私聊屏蔽独立，移除联系人保留条目。cloud_messages 增加 sender_name（既有消息按发送者账号补齐）、reply_to、mentions UUID 数组与 ai_responses JSONB 数组；旧 ai_response 合并 aiId 迁为数组后删除旧列。历史署名不要求发送者仍是成员，mentions 保留明确选择的顺序，reply_to 由业务层复核同窗 committed 来源。ai_responses 按对应任务的 queue_position 重建，不将多个 AI 的状态覆盖成一个值。

cloud_node_tasks 新增 queue_position（唯一 bigserial）与 conversation_revision；窗口未完成任务的索引按 `(conversation,queue_position)` 支持跨节点 FIFO。材料选择限定于触发位置以内原文，另允许本窗更早 queue_position 的 completed 任务输出，且该任务的触发位置不得晚于当前触发；排除后续无关真人输入，最近 100 条之外仍补足必要触发/回复来源。开始事务复核队首、成员/账号/屏蔽、窗口 revision 与材料来源，持久保存 sources 后，后续 delta/result 逐条检查冻结来源，不重新选最近历史。`cloud_group_receipts` 按实际 owner/request 保存群与关系命令的 fingerprint 和窗口引用；不保存命令正文。AI 启停仍使用账号状态版本，取消只影响目标 AI 的未完成任务；屏蔽只影响对应私聊，二者保留有效片段为 interrupted，停用期间仍保存真人私聊消息但不为目标 AI 排任务，恢复/解除不重排。

附件元数据在消息 JSONB，原字节位于独立受管理目录的 `{messageId}/{attachmentIndex}`。先校验、独占创建文件、同步落盘，成功后提交数据库引用；数据库失败可能留下不可见的无引用文件，当前没有自动孤儿清理器。正文编辑保留原字节，撤回事务清空正文及元数据后删除文件。下载持有消息共享锁直至原字节读取完成；导出在一致快照中锁定完整消息及读取所选附件，避免撤回与读取拼出不一致记录。备份须配套保存数据库与附件，不将独立目录复制视为已实现的在线备份工具。

cloud_receipts 只保存命令 SHA256 与实体 ID，不重复保存正文或 base64；重复请求返回当前权威状态，旧回执不能恢复撤回内容。密码使用 Argon2 哈希，邀请、验证/重置链接、会话只存校验值和有效性信息。SMTP 秘密属于部署环境。云端保存 AI 社交任务、来源修订、开始/结果指纹与确认 ID、累计草稿及终态、用量报告和节点事件；配对/绑定只保存节点 secret 的哈希。云端没有模型 Key、persona、内部模型参数、记忆或摘要正文。

客户端草稿复用 IndexedDB，owner 包含服务器与实际账号，再按窗口隔离；文字、mentions/replyTo、附件 Blob 和不可变待确认发送命令同事务保存在本机，不是服务器正式历史。刷新不自动发送，未知发送结果只手动重试原 ID/原内容；首次明确拒绝才恢复可编辑草稿，已存在 pending 的后续权限拒绝不能证明旧发送未提交。浏览器来源隔离与桌面内存会话说明见[界面设计](frontend.md)。

[本地 payload 映射探针](../../server/cloud/tests/fixtures/README.md) 用合成数据和真实共享类型核对 UUID、公开资料、消息相对顺序及图片/文件原字节。旧 per-window sequence 需映射到云端 global position，旧正文 part index 需映射到云端附件索引。它不是迁移器；头像、停用状态、账号版本、AI、群、意识及来源依赖仍需正式备份预览与迁移设计，不读取或修改用户旧库。

`0005_consciousness.sql` 在云端增加群话题引用及其来源索引、按稳定 AI 账号补齐的来源变更、节点材料版本和独立摘要调用确认。正式消息只保存 `context_reference` 的 ID、版本和失效标记，不复制群原文；摘要调用表只有来源、配置版本、状态和用量。在线管理请求与响应只经内存转发，不写入上述表或日志。

`0006_participation.sql` 增加云端窗口规则与状态、每日自动次数、一次性约定及其授权设置。社交任务区分消息和约定触发，保存合并输入、最早执行时间及规则版本。真人消息、任务与事件同事务；开始确认与自动次数同事务；成功结果、约定终态及后继接话同事务。约定命令回执只保存指纹和目标 ID，授权正文保存在意图记录中。

## 当前节点存储

`agent/node` 独立使用 `node.sqlite`，与旧 `social.sqlite3` 无关。bindings 保存每 AI 的绑定版本和原始节点凭据，profiles 保存每 AI 的角色、模型选择和参数，tasks 保存开始意图、开始回执、草稿及原始最终提交，budget/meta 保存每日保守预留及不回退的日期。节点目录有独占进程锁和当前用户私有权限；凭据数据库不能提交、分享或打包。

节点的 `conscious_memory`、`conscious_summaries` 和 `conscious_settings` 按 AI/窗口保存记忆、摘要和设置；`conscious_scopes` 保存上下文版本，`conscious_cursors` 保存来源同步位置。写操作回执只存请求编号与指纹，读取不产生回执，避免轮询累积记录。遗忘或来源失效直接清空当前派生正文，回执中没有旧正文可复活。未完成摘要及其调用确认保存在节点，重启收尾而不重复调用。

`node.toml` 保存服务器、nodeId、CA 路径、预算与新绑定的档案模板，provider Key 只允许环境变量引用，解析出的模型 Key 不进入 SQLite。每 AI 档案由本机 profile CLI 保存；正式资料迁移包仍待第 5 步。事务与恢复规则见[执行设计](execution.md#当前云端私聊与群聊执行)。只有同一进程收到云端开始接口明确 409 `task_not_ready`/`conversation_busy` 未开始响应时，才在本地事务删除尚无开始回执的 intent，并撤销该次预留的调用数/token，保留日期单调。普通冲突、断线、确认丢失、已开始或重启后的不确定状态均不适用该撤销路径。

节点的 `automatic_budget` 按 AI、窗口和 UTC 日记录自动调用的 token 容量，`model_reservations` 将预留与任务关联。开始意图、整体额度和自动额度在同一 SQLite 事务保存；仅当前进程取得明确未开始的队列冲突后，才一起撤销这次预留。已开始、确认丢失和重启后的不确定调用不退还。

## 完整云端与 Agent 存储目标（部分待实现）

[云端改造计划](../project/cloud-plan.md)确定了完整数据归属。真人/AI 私聊与群聊、成员与角色、屏蔽和 AI 启停、节点绑定/任务、节点意识资料，以及接话与约定已接入；角色/模型在线管理及正式迁移仍是目标，不能据此声称已经有对应管理界面或迁移工具。

### 数据归属

| 位置 | 权威数据与责任 |
| --- | --- |
| 云端 PostgreSQL | 真人/AI 账号、管理员、邀请与邮箱密码认证资料及会话、联系人与屏蔽、窗口成员与权限、消息与修订、附件元数据、已读置顶、事件和命令回执；节点绑定授权、发言规则、任务与执行终态 |
| 云端附件持久目录 | 经服务端校验的正式图片和文件字节；数据库记录附件 ID、所属消息与版本、格式、大小及完整性信息，不保存用户设备路径 |
| 自带 Agent 本地 SQLite | 按稳定 AI 账号保存角色设定、模型选择与生成参数、显式记忆、各 AI 各窗口的摘要配置/分段/来源版本、执行检查点；缓存原文不是正式历史 |
| 节点本机秘密配置 | 模型 Key 与连接秘密；当前 Key 仅从环境解析，平台节点原始凭据保存在私有节点 SQLite；不写入业务导出、迁移资料包、云端存储或日志 |
| 客户端缓存与草稿 | 按服务器 + 实际登录账号 + 窗口隔离；缓存、附件草稿和待确认发送快照不作为正式聊天，也不因离线成为已发送消息 |

云端不持久保存节点的角色设定、记忆、摘要正文和模型参数。管理请求经云端复核实际管理员授权后转发在线节点，已有关系/窗口显式记忆操作沿用成员与来源范围校验；节点提交本地存储成功才返回保存成功，离线时不排队修改这些资料。普通模型参数修改仅影响新运行，遗忘、来源撤销等操作仍使相关在飞运行失效。模型 Key 只在节点本机填写。外部 Agent 可自行选择存储，不强制使用 SQLite 或自带 Agent 的内部格式。

账号密码保存为密码哈希，邮箱验证、找回密码及节点连接凭据在服务端保存校验值与过期/撤销信息，原始秘密不写入日志或业务导出。云端自己的邮件服务凭据属于部署配置，与用户节点的模型凭据分开。

### 提交与备份边界

云端一次数据库事务提交消息/修订、来源依赖、正式附件元数据、任务变更、事件与回执；权限、屏蔽、任务及当前有效节点的复核与相应状态提交在同一事务内完成。领取任务和提交结果各自事务化，不跨模型请求持有事务。节点在本地事务中保存意识资料、来源版本、失效状态与检查点；两个数据库之间通过协议同步版本与状态，不假设跨库原子提交。云端复核社交来源和发言资格，节点复核内部资料版本。

附件先接收至服务端临时区域并完成格式、大小及完整性校验，再在同一受管理文件系统中持久写入正式位置，成功后才提交引用它的数据库事务。落盘失败不保存消息或成功回执；文件成功而数据库失败时留下未被引用的文件，由服务端清理，不向用户暴露为已发送。清理只处理已结束上传且无有效引用的字节，不能与尚在提交的事务竞争。消息修订/撤回先在事务中解除或更新引用，备份期间暂停清理。客户端只上传字节，不提供本机路径。

数据库与附件目录不构成分布式事务。备份采用短暂暂停写入和附件清理的维护窗口：排空正在提交的写入，取得 PostgreSQL 一致快照并复制该快照所引用的附件，记录配套备份标识及完整性清单后恢复写入。恢复必须成套使用数据库和附件，检查所有正式引用均有完整字节；不能把互不对应的数据库备份与目录复制声称为可恢复备份。节点意识资料另行备份，不包含本机秘密。

### 旧数据迁移与节点迁移

先备份旧库与附件并预览账号对应关系，再明确本地真人的云端身份、AI 身份及管理员；不得按同名自动合并。聊天、附件、关系与成员迁入云端，保留已读置顶、屏蔽和停用状态；角色、显式记忆及摘要迁入对应 AI 的节点。保存账号、窗口、消息和来源映射及导入记录，重复执行不产生重复实体。旧 SQLite 与顺序迁移保持不变、不删除；正式使用经过一次明确的单向切换，不长期双写。

旧私聊和群聊摘要仅在来源有效且目标 AI 仍有源访问权时，标明导入来源后作为该 AI 的初始摘要；此后各自维护。失效摘要不启用，无权读取的内容不分发，也不自动付费重新概括。旧未完成模型调用不重放，未来约定经预览后重新确认才启用，旧队列不因导入自动发言。

自带 Agent 换节点按 AI 导出/导入角色设定、模型参数、记忆、摘要及来源关联资料包，云端身份与聊天历史沿用；资料包不含模型 Key 或平台节点凭据，新节点重新配置并授权。导入检查目标 AI、ID/来源映射、版本和完整性；来源需按当前云端权限再核对。外部 Agent 自行适配资料迁移。具体字段随相应实现批次落实，不预建统一迁移框架。

迁移验收须证明：预览与实际导入范围一致；重复导入无重复消息/附件；附件完整性及来源映射可核对；权限不足的摘要未分发，失效内容未启用；旧库可独立保留并从配套备份恢复；新节点无旧秘密也能重新授权接续身份，旧调用和未经确认的约定不会执行。切换失败时保留旧库和导入记录供核对，不能自动把两边历史重新合并。

## 本地 SQLite 实现参考（待迁移）

以下所有章节只描述保留的本地 SQLite/SocialFeature 实现，不描述默认 CloudApp 或 cloud-server。旧库及顺序迁移保留，用于回归和后续 Agent 迁移；不要把其共享群摘要、意识资料和参与执行机制视为已接入云端；云端群当前仅采用本文顶部的成员及显式提及/回复契约。

### 存储布局与锁

在启动时解析并冻结的实际数据目录中使用唯一运行存储 `social.sqlite3`。默认目录继续为 `~/.llmn`，`LLMN_DATA_DIR`、`LLMN_CONFIG` 的覆盖及原有目录迁移条件继续有效。模型目录缓存仍为实际数据目录的 `cache/catalog.json`。

SQLite 存储身份、聊天、记忆、计划与执行元数据。图片和普通文件随 MessageBody 以原始字节存入 SQLite 的 JSON DTO，数据库仍为 v13；SocialContent::File.name 为可省略的可选字段，旧数据不改写。存储消息总量按原始字节计（图片 8 MiB 加文件 20 MiB，加 80000 字节文字），避免把 JSON 数字数组膨胀当作文件大小，业务入口分别检查各自数量/大小限制。未来大文件可独立设计受管理对象存储，不接受任意文件路径。API Key、`.env` 和 provider 凭据不写入业务数据库或导出档案。

保留 `.llmn.lock` 的 OS 排他锁，一个目录只允许一个独立 Runtime。DataDirectoryGuard 由 SQLite 持有，旧 JSON 仅经 writer 的显式导入读取。旧 FileSessionStore 已移除，不再在启动时解析或补写 JSON，也不能用 SQLite 文件锁替代整个目录的应用锁。

现有旧系统目录到新 home 目录的复制先按既有规则完成；JSON 到 SQLite 是随后独立的一次格式导入，两种迁移不可混为自动合并。

个人屏蔽只更新 address_book.blocked；屏蔽/解除提升对应 Relationship 版本，私聊取消与双方约定取消、事件、回执在同一事务完成，不删除原文或意识材料。添加/移除只更新 listed，不能覆盖 blocked。屏蔽的业务范围见[身份与关系](accounts.md)。

#### 本机未发送草稿

Web 与桌面 WebView 共用 IndexedDB `llm-nest-social-drafts` v1，`drafts` 表以 `[owner, conversation]` 为键。每条记录包含修订版本、可编辑草稿和待确认发送快照；文字、图片/文件 Blob、名称/MIME/稳定 ID、提及、回复、群话题引用及 requestId 同事务保存。files 可省略，旧图片草稿无需迁移。客户端按窗口串行写入，事务比较版本防止多页覆盖；只有 transaction complete 才显示保存成功。附件读取/写入错误保留页面输入，不静默降级到内存草稿。

发送前保存不可变命令快照，成功后原子清理或保留发送期间新增内容；失败或未知结果沿用原快照手动重试。清空记录移除二进制，只留空记录和版本，避免迁移中断后旧草稿复活。旧 localStorage 文字草稿只迁移当前账号可见窗口，数据库事务完成后再删除对应旧条目，其他窗口的旧条目保留。

发送前处理的源文件与候选预览只留在内存，不进入草稿库；用户确认后只将所选处理结果按普通图片保存。取消不改变原草稿，主动替换生成新图片 ID，正在等待发送回执的旧快照仍使用原内容。确认后保存失败保留处理结果并提示重试；未确认时桌面关闭提示先确认或取消，Web 请求离开提示。

草稿不写入 SQLite、不生成消息/事件、不供模型读取；SQLite 仍为 v13。浏览器来源（协议、主机、端口）及桌面 WebView 各自隔离，不提供跨设备同步或整库备份。浏览器清理/回收站点数据仍可能删除草稿，异常退出前未完成的事务不能视为已保存。

### Schema 与实体关系

当前 schema 版本为 13；v1 基础、v2 run_details、v3 participation_plans、v4 移除成员历史起点、v5 成员 paused、v6 个人通讯录与管理员授权、v7 显式记忆、v8 窗口摘要与后台尝试状态、v9 跨窗口引用依赖索引、v10 接话规则/状态/每日额度，v11 联系设置、意图与计划触发来源，v12 消息完成顺序及个人列表状态，v13 个人屏蔽状态（address_book.blocked，默认 false）。顺序迁移前一致备份，旧 migration 内容与校验值不改写。

| 表组 | 当前表 |
| --- | --- |
| 身份 | actors、ai_profiles、execution_bindings、relationships、address_book、account_admins |
| 聊天 | conversations、memberships、messages、message_revisions、message_deliveries、conversation_list_state、bouts |
| 参与/执行 | participation_plans、participation_policies、participation_state、participation_daily_budget、contact_settings、contact_intents、ai_runs、run_details |
| 契约 | committed_events、command_receipts |
| 迁移 | schema_migrations、legacy_imports、legacy_id_map、legacy_entries、legacy_session_data |
| 意识派生数据 | memories、memory_revisions、memory_message_sources；summary_settings、conversation_summaries、summary_revisions、summary_message_sources；context_references、context_reference_sources；保留来源 tombstone |

上表对应当前 schema 的实体分组；外键、唯一索引和 CHECK 约束共同落实关系与来源不变量。附件保存在消息 payload 中，导出读取现有数据，不另建导出表。

direct 的 relationship_id 非空且唯一，group 不使用该字段；归档不会释放私聊唯一性。窗口完整消息是权威记录，摘要、近期消息选择和 ContextSnapshot 是派生材料，不因压缩删除原记录。上下文状态或缓存按 ActorId + ConversationId 隔离。

ContextReference 保存在 messages/message_revisions 的可选 context_reference 字段中，旧消息缺失该字段时读取为 None。context_references 按源群/目标 AI 索引关联私聊消息；context_reference_sources 索引全部原文依赖，包括所选摘要覆盖的原文。不重复保存源正文或授予源权限。源修订、撤回和权限变化要覆盖所有依赖该来源的窗口。summaries 不能仅按 conversation_id 唯一保存；分段范围、来源版本和有效状态共同决定是否可复用。

上述来源引用、摘要、记忆与 AiRun 中保存的 ContextSnapshot 共同支撑意识系统，存储仍统一使用 SocialStore。原消息由聊天用例写入，意识系统只持久化自身派生材料和必要依赖，不另建独立 consciousness 数据库或完整请求副本。摘要、显式记忆与跨窗口引用均已启用。

SocialStore 接口放在 `crates/storage`，当前提供 send_message、revise_message、withdraw_message、begin_plan、checkpoint_run、finish_run 和显式控制等原子操作。显式记忆提供 memories/memory/create_memory/update_memory/forget_memory；摘要提供 summaries/configure_summaries/queue_summary/begin_summary/finish_summary/interrupt_summary_tasks；reference_preview 复核双方源群成员及选中原文/摘要，send_message 原子保存引用；参与节奏提供 update_participation/participation_tick，计划在同事务合并或续接；约定联系提供 update_contact_settings/create_contact_intent/cancel_contact_intent，participation_tick 负责到期领取，plan_material 按消息或意图构造输入。v11 重建计划表，使 trigger_id 可空并加入 contact_intent_id，CHECK 保证两者恰有一个，意图索引唯一；旧计划 ID、来源、状态和 payload 保留。前端和 Feature 不直接拼 SQL。

message_deliveries 在消息初次保存为 committed/interrupted 或从 generating 结束时通过 SQLite trigger 同事务分配完成顺序；唯一 message_id 防止重复计数。conversation_list_state 按 conversation_id/actor_id 保存 read_through、marked_unread、pinned、revision。v12 将迁移时已有历史初始化为已读并移除 read_positions；启动恢复生成中断发生在迁移之后，因此仍会提示新中断。显式导入的只读历史同样初始化为已读，原文件保持不变。

搜索直接读取 messages 的当前文本部分，不另存正文索引或过期副本。按当前成员身份过滤，再按完成顺序分页；当前是字面子串检索，大规模索引优化另行安排。列表命令返回当前状态，旧回执不重放旧标记；消息分页与可确认的完成位置在同一读事务中获得。

### 关键事务

| 操作 | 同一次提交的内容 |
| --- | --- |
| 创建 AI | Actor、内部档案、执行绑定、初始管理员、创建者与 AI 各自的通讯录条目、事件、命令回执 |
| 修改公开资料 | Actor.bio/display_name/revision、事件、回执；不修改内部档案或绑定 |
| 修改 AI 设定/绑定 | 管理权限检查、Actor/档案/绑定版本、事件、回执；已开始运行仍使用原快照 |
| 添加/移除联系人 | address_book 的 owner/account/listed/revision、contact_changed 事件、回执；不改变账号、空间、运行或队列 |
| 授予/撤销管理员 | 初始管理员权限检查、account_admins、Actor.revision、事件、回执 |
| 已读/未读/置顶 | 实际账号权限、个人列表版本、状态、conversation_list_changed 事件和回执；不改变 AI 运行 |
| 发送真人消息 | 消息、空间顺序/版本、接话计划、事件、回执、可选 ContextReference 与来源索引 |
| 更新接话规则 | policy.revision、取消现有自动计划/运行、参与状态、引用事件及回执；不补发旧输入 |
| 创建/取消约定及修改联系设置 | 意图/设置、撤销对应计划和运行、引用事件、回执 |
| 领取到期意图 | pending→claimed、唯一 ParticipationPlan、引用事件，无模型调用 |
| 开始 AI 运行 | 计划转 running、取得/创建 Bout、AiRun、generating 消息、开始事件、自动调用每日次数/容量预留 |
| 完成 AI 运行 | 最终消息、run/计划终态、参与计数、合格的后继自动计划、事件与对应意图终态；一次性联系不产生后继自动计划 |
| 中断 AI 运行 | 草稿、run/计划终态、活动时间、意图终态、事件；工具未接入 |
| 暂停成员 | 该成员 paused/revision、它的运行/队列终态、事件、回执；不提高全群 epoch |
| 加入/移除成员 | 成员版本、空间 context/participation epoch、运行/计划撤销、事件、回执 |
| 归档空间 | 空间 archived 状态、活动运行取消标记、未执行计划/意图终态、事件 |
| 管理员停用账号 | Actor.status/revision、该 AI 跨空间的运行/片段终态与排队计划、事件、回执；保留关系/成员/窗口设置，恢复不重放取消计划 |
| 结束 Bout | Bout 终态、本 Bout 的活动运行取消标记与未执行计划状态、事件 |
| 编辑/遗忘 | 修订或遗忘记录、派生内容失效、上下文版本、撤销标记、事件 |
| 保存/失效摘要 | 来源与权限版本复核、分段摘要版本/状态、派生依赖、变更事件 |

上表描述当前事务边界；货币费用账本与工具轨迹当前尚未接入；联系意图已按上述事务落地。窗口摘要的配置/任务/修订、原消息依赖、事件及命令回执已同事务提交；开始与保存分别复核来源，事务不跨模型调用。显式记忆的实体、修订、来源索引、上下文版本、运行中断、事件和回执同事务提交；原消息修订/撤回在原事务中将对应记忆标 stale。当前发送事务创建计划，Bout 在开始运行事务创建；不通过事件消费重复安排。上表中的事件为持久变更引用，不含完整 provider 请求或私密原文。数据库提交后更新内存派生状态并通知订阅者，继续维持写穿透。广播失败不撤销已提交业务状态，订阅者通过事件补齐恢复。

command_receipts 保存稳定实体引用、请求摘要和结果分类，不保存撤回后仍可取出的原响应正文。重复请求返回同一实体的当前可见状态；幂等检查先于已经成功请求的 revision 冲突判断。

主动调用预算在开始事务按模型输入/输出容量扣减；终态不退款，重启保留账本，不按 provider usage 结算或释放。提交最终消息须检查 participation_epoch、Bout 状态和可撤销来源版本，停止与完成竞争由事务顺序决定。摘要任务保存同样执行来源版本校验，不能让迟到结果覆盖有效新版本。

### 并发、性能与检查点

存储使用 rusqlite 0.40.2 bundled，WAL、foreign_keys=ON、synchronous=FULL，已通过 Windows MSVC/Linux 基础测试。SQLite 提供原子事务，具体持久性仍取决于配置和文件系统。[SQLite 事务说明](https://www.sqlite.org/transactional.html)

同步数据库驱动放入受限阻塞执行器或单写线程，不在 async 主执行路径做长时间磁盘工作。不得跨模型网络调用持有事务。busy、磁盘满和 schema 错误按原因返回，限次重试不能破坏幂等。

消息按空间和 sequence 分页；不再启动时加载全部消息到内存。成员、档案和配置可缓存，提交或变更事件后失效。索引至少覆盖空间消息顺序、到期待办、活动运行与记忆范围。

草稿检查点约 500ms 保存有变化的内容；社交当前不执行工具。持久事件日志不保存逐 token delta，减少放大写入。

### Schema 升级

v9 增加两张跨窗口引用索引表，不改写 v1–v8。引用正文仍是消息 payload 中的类型化来源，旧发送命令的 reference=None 序列化省略，保留旧命令摘要。源编辑/撤回或相关成员退出时，同事务更新目标引用 stale、消息/context 版本、依赖记忆/摘要、运行/计划和引用事件；索引包含选中摘要的全部原文。升级前保留一致的 v8 备份；旧程序不能打开 v9。

v8 增加四张摘要表，原有账号、记忆和聊天均保留；每窗口只允许一个 queued/running 摘要。分段重建保持范围与 ID，修订表保存尝试和终态；编辑/撤回来源清空当前及修订的摘要正文，留下来源与 stale 状态。启动/关闭中断未完成任务，不重放模型调用。升级前备份保持 v7，可明确回退，不允许旧程序打开 v8。

v7 增加记忆、修订和消息来源索引表，保留既有账号、关系、聊天及命令回执。记忆正文在遗忘事务中从当前条目及所有修订中清空，范围与来源仍作为 tombstone 保存；原聊天、已发布回答、备份和 SQLite 物理页不属于安全擦除承诺。旧 v6 程序不能打开 v7 数据库；回退须明确使用升级前备份，不能自动覆盖新数据。

v6 为既有 AI 补入本地真人的初始管理权与各自的通讯录条目。旧 archived Actor 仍保持停用，真人通讯录条目设为未列出，不自动恢复；公开简介初始化为空，内部 persona 不复制为公开内容。账号、关系、私聊 ID、群成员与历史保留，升级前的 v5 备份可核对原文。实现与验证见 [验证入口](../validation/README.md)。

独立生成参数沿用 execution_bindings 的 JSON payload，不单独增加 schema 版本。temperature/max_tokens 缺失时按 None 读取；领域与命令候选序列化省略 None，保持旧 payload 与已保存命令摘要的语义一致。参数与账号/档案修改同事务提交，权限、失败回滚和关闭重开验证见 [AI 执行](execution.md)。旧 JSON 导入的绑定不复制 旧全局温度或输出上限。

每次 schema 变更有顺序版本与校验记录。打开数据库先取得目录锁，再检查版本和执行事务迁移。未知较新版本拒绝写入并提示需要匹配应用，不尝试降级解释。

迁移前生成一致备份，失败返回实际错误，不继续升级；可用磁盘容量不通过估算假装得到保证。在线数据库使用 SQLite 备份能力或明确暂停写入后的安全流程，不能仅复制主文件而遗漏 WAL。schema 失败保留原库与可定位错误，不发布半升级状态。

### 旧 JSON 导入

入口为设置中的“导入历史聊天”，HTTP/IPC 共用 SocialFeature 与实际本地操作者；只能选择当前锁定目录内的合法旧文件。导入器只读取旧会话数据，不读取个人配置生成测试或发布资源。已有 TOML、环境变量引用和目录缓存继续由 Runtime 配置层管理。

流程：

1. 取得目录锁，识别合法旧会话文件并进行只读解析，不调用会补写源文件的旧加载流程。
2. 校验版本、消息 ID、重复与损坏，错误带源路径；生成导入预览与一致备份。
3. 为每个旧 Session 建立独立历史 Conversation 和迁移来源映射。
4. user 消息映射到本地 human；assistant 消息映射到该来源会话专用的“历史 AI”身份，不按模型名合并。
5. 原 system 内容作为旧执行设定保留，不自动变为公开人格；tool 内容进入迁移的执行轨迹，不伪造成真人或普通群消息。无法确定旧运行边界的内容标为 legacy trace，不虚构完整 AiRun 审计。
6. 保留已有 UUID；缺失 ID 在导入事务生成并写入 legacy_id_map。重复 ID 报错，不能静默重编号。
7. 旧模型选择保存为执行绑定候选，无法解析则标需配置，不替换模型。旧中断片段、用量、计时和反馈保留。
8. 每个来源以稳定来源标识、SessionId、checksum 和映射记录幂等提交；已导入但源文件变化时报告冲突，不重复合并。
9. 旧 Running 检查点导入为 interrupted，补齐未回答工具结果，不自动调用模型或重放工具。
10. 导入后直接在新版消息列表查看只读历史，原 JSON 保留且不自动删除。

缺失 ID 的分配在事务失败时可重新生成，因为失败结果未对外发布；已成功导入的映射必须稳定。跨会话 UUID 冲突同样报错，交给显式修复流程。

历史会话导入为带 legacy 标记的只读独立窗口，不自动接话、提取记忆或创建意图。历史 AI 保持停用；读位置初始化到导入末尾。与新联系人明确接续旧历史仍为后续能力，不能把旧模型选择当成已有 AI 身份。

### 备份、遗忘与回退

单窗口导出由用户在聊天详情明确触发。SocialStore 在一次只读事务中检查实际操作者和当前成员身份，读取完整有序消息并投影为导出快照；不分页拼接、不读历史修订和其他窗口正文，不改变消息列表状态或事件。`include_images` 和 `include_files` 分别决定在同一快照保留图片与普通文件字节，未选类型只保留占位；二进制字段不序列化进记录 JSON。排除记忆、摘要、思考、角色内部设定、密钥及配置。无需新增迁移，SQLite 仍为 v13。

Markdown / JSON 及附件 ZIP 是聊天记录文件，不是整库备份，也不用于现有旧历史导入器。ZIP 在读取事务结束后由 SocialFeature 的 blocking 任务生成；图片位于 images，普通文件位于 files/{消息ID}-{part索引}/{安全文件名}，稳定标识分目录避免同名覆盖，见[聊天与消息](conversations.md)。桌面在原生保存对话框选择位置，之后复核访问权并读取快照，用同目录临时文件写入、同步后替换目标；原图和普通文件另存为共用这一写入流程。保存失败不能显示成功，用户取消不创建文件。Web 使用浏览器下载。导出、撤回和遗忘都不改写用户已经保存的文件，备份保留策略另行说明。

撤回和遗忘立即从正常查询、摘要和检索排除，必要正文与修订内容进行清除。WAL、备份和文件系统残留的物理擦除不由普通删除保证，不将逻辑遗忘宣传为所有副本安全擦除。

旧产品已退役；历史版本只认识保留的 JSON，不能读取新社交记录。首版不双写旧格式；降级可能产生独立历史，不能宣称无损回滚。启用新写入前可恢复备份；启用后以新数据库备份和完整导出保障数据，不自动把群聊压回旧 Session。
