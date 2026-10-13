# 06 存储与迁移

[总纲](overview.md) · [聊天与消息](02-conversations-and-messages.md) · [API 与事件](07-api-and-events.md)

本文维护所属模块的规则与扩展目标。 实施状态见 [已完成](completed.md) / [尚未完成](pending.md)，当前调用契约见 [API 与事件](07-api-and-events.md)。

## 1. 存储布局与锁

在启动时解析并冻结的实际数据目录中使用唯一运行存储 `social.sqlite3`。默认目录继续为 `~/.llmn`，`LLMN_DATA_DIR`、`LLMN_CONFIG` 的覆盖及原有目录迁移条件继续有效。模型目录缓存仍为实际数据目录的 `cache/catalog.json`。

SQLite 存储身份、聊天、记忆、计划与执行元数据。图片当前随 MessageBody 以原始字节存入 SQLite 的 JSON DTO，沿用已有格式，数据库仍为 v13；存储消息总量按原始字节计（8 MiB 附件加 80000 字节文字），避免把 JSON 数字数组膨胀当作文件大小。未来大文件可独立设计受管理对象存储，不接受任意文件路径。API Key、`.env` 和 provider 凭据不写入业务数据库或导出档案。

保留 `.llmn.lock` 的 OS 排他锁，一个目录只允许一个独立 Runtime。DataDirectoryGuard 由 SQLite 持有，旧 JSON 仅经 writer 的显式导入读取。旧 FileSessionStore 已移除，不再在启动时解析或补写 JSON，也不能用 SQLite 文件锁替代整个目录的应用锁。

现有旧系统目录到新 home 目录的复制先按既有规则完成；JSON 到 SQLite 是随后独立的一次格式导入，两种迁移不可混为自动合并。

个人屏蔽只更新 address_book.blocked；屏蔽/解除提升对应 Relationship 版本，私聊取消与双方约定取消、事件、回执在同一事务完成，不删除原文或意识材料。添加/移除只更新 listed，不能覆盖 blocked。迁移与失败回滚证据见[屏蔽记录](contact-blocking.md)。

## 2. 当前 Schema 与目标关系

当前 schema 版本为 13；v1 基础、v2 run_details、v3 participation_plans、v4 移除成员历史起点、v5 成员 paused、v6 个人通讯录与管理员授权、v7 显式记忆、v8 窗口摘要与后台尝试状态、v9 跨窗口引用依赖索引、v10 接话规则/状态/每日额度，v11 联系设置、意图与计划触发来源，v12 消息完成顺序及个人列表状态，v13 个人屏蔽状态（address_book.blocked，默认 false）。顺序迁移前一致备份，旧 migration 内容与校验值不改写。

| 表组 | 当前表 | 后续按用例扩展 |
| --- | --- | --- |
| 身份 | actors、ai_profiles、execution_bindings、relationships、address_book、account_admins | 关系设置与工具策略 |
| 聊天 | conversations、memberships、messages、message_revisions、message_deliveries、conversation_list_state、bouts | 导出、附件等独立用例 |
| 参与/执行 | participation_plans、participation_policies、participation_state、participation_daily_budget、contact_settings、contact_intents、ai_runs、run_details | 工具/协议审计 |
| 契约 | committed_events、command_receipts | M2/M3 新事件种类 |
| 迁移 | schema_migrations、legacy_imports、legacy_id_map、legacy_entries、legacy_session_data | 按后续 schema 变更迁移 |
| 意识派生数据 | memories、memory_revisions、memory_message_sources；summary_settings、conversation_summaries、summary_revisions、summary_message_sources；context_references、context_reference_sources；保留来源 tombstone | 自动提取与巩固 |

这是一份职责模型，实施时可以合并一对一表，但不能省略事务或来源约束。外键、唯一索引和 CHECK 约束要实际启用并验证；不能只在 Rust 层假设有效。

direct 的 relationship_id 非空且唯一，group 不使用该字段；归档不会释放私聊唯一性。窗口完整消息是权威记录，摘要、近期消息选择和 ContextSnapshot 是派生材料，不因压缩删除原记录。上下文状态或缓存按 ActorId + ConversationId 隔离。

ContextReference 保存在 messages/message_revisions 的可选 context_reference 字段中，旧消息缺失该字段时读取为 None。context_references 按源群/目标 AI 索引关联私聊消息；context_reference_sources 索引全部原文依赖，包括所选摘要覆盖的原文。不重复保存源正文或授予源权限。源修订、撤回和权限变化要覆盖所有依赖该来源的窗口。summaries 不能仅按 conversation_id 唯一保存；分段范围、来源版本和有效状态共同决定是否可复用。

上述来源引用、摘要、记忆与 AiRun 中保存的 ContextSnapshot 共同支撑意识系统，存储仍统一使用 SocialStore。原消息由聊天用例写入，意识系统只持久化自身派生材料和必要依赖，不另建独立 consciousness 数据库或完整请求副本。M1 使用消息来源快照；M2 启用摘要、显式记忆与跨窗口引用。

SocialStore 接口放在 `crates/storage`，当前提供 send_message、revise_message、withdraw_message、begin_plan、checkpoint_run、finish_run 和显式控制等原子操作。显式记忆提供 memories/memory/create_memory/update_memory/forget_memory；摘要提供 summaries/configure_summaries/queue_summary/begin_summary/finish_summary/interrupt_summary_tasks；reference_preview 复核双方源群成员及选中原文/摘要，send_message 原子保存引用；参与节奏提供 update_participation/participation_tick，计划在同事务合并或续接；约定联系提供 update_contact_settings/create_contact_intent/cancel_contact_intent，participation_tick 负责到期领取，plan_material 按消息或意图构造输入。v11 重建计划表，使 trigger_id 可空并加入 contact_intent_id，CHECK 保证两者恰有一个，意图索引唯一；旧计划 ID、来源、状态和 payload 保留。前端和 Feature 不直接拼 SQL。

message_deliveries 在消息初次保存为 committed/interrupted 或从 generating 结束时通过 SQLite trigger 同事务分配完成顺序；唯一 message_id 防止重复计数。conversation_list_state 按 conversation_id/actor_id 保存 read_through、marked_unread、pinned、revision。v12 将迁移时已有历史初始化为已读并移除 read_positions；启动恢复生成中断发生在迁移之后，因此仍会提示新中断。显式导入的只读历史同样初始化为已读，原文件保持不变。

搜索直接读取 messages 的当前文本部分，不另存正文索引或过期副本。按当前成员身份过滤，再按完成顺序分页；当前是字面子串检索，大规模索引优化另行安排。列表命令返回当前状态，旧回执不重放旧标记；消息分页与可确认的完成位置在同一读事务中获得。

## 3. 关键事务

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

上表为完整目标事务；货币费用账本与工具轨迹当前尚未接入；联系意图已按上述事务落地。窗口摘要的配置/任务/修订、原消息依赖、事件及命令回执已同事务提交；开始与保存分别复核来源，事务不跨模型调用。显式记忆的实体、修订、来源索引、上下文版本、运行中断、事件和回执同事务提交；原消息修订/撤回在原事务中将对应记忆标 stale。当前发送事务创建计划，Bout 在开始运行事务创建；不通过事件消费重复安排。上表中的事件为持久变更引用，不含完整 provider 请求或私密原文。数据库提交后更新内存派生状态并通知订阅者，继续维持写穿透。广播失败不撤销已提交业务状态，订阅者通过事件补齐恢复。

command_receipts 保存稳定实体引用、请求摘要和结果分类，不保存撤回后仍可取出的原响应正文。重复请求返回同一实体的当前可见状态；幂等检查先于已经成功请求的 revision 冲突判断。

主动调用预算在开始事务按模型输入/输出容量扣减；终态不退款，重启保留账本，不按 provider usage 结算或释放。提交最终消息须检查 participation_epoch、Bout 状态和可撤销来源版本，停止与完成竞争由事务顺序决定。摘要任务保存同样执行来源版本校验，不能让迟到结果覆盖有效新版本。

## 4. 并发、性能与检查点

M0 使用 rusqlite 0.40.2 bundled，WAL、foreign_keys=ON、synchronous=FULL，已通过 Windows MSVC/Linux 基础测试。SQLite 提供原子事务，具体持久性仍取决于配置和文件系统。[SQLite 事务说明](https://www.sqlite.org/transactional.html)

同步数据库驱动放入受限阻塞执行器或单写线程，不在 async 主执行路径做长时间磁盘工作。不得跨模型网络调用持有事务。busy、磁盘满和 schema 错误按原因返回，限次重试不能破坏幂等。

消息按空间和 sequence 分页；不再启动时加载全部消息到内存。成员、档案和配置可缓存，提交或变更事件后失效。索引至少覆盖空间消息顺序、到期待办、活动运行与记忆范围。

草稿检查点约 500ms 保存有变化的内容；社交当前不执行工具。持久事件日志不保存逐 token delta，减少放大写入。

## 5. Schema 升级

v9 增加两张跨窗口引用索引表，不改写 v1–v8。引用正文仍是消息 payload 中的类型化来源，旧发送命令的 reference=None 序列化省略，保留旧命令摘要。源编辑/撤回或相关成员退出时，同事务更新目标引用 stale、消息/context 版本、依赖记忆/摘要、运行/计划和引用事件；索引包含选中摘要的全部原文。升级前保留一致的 v8 备份；旧程序不能打开 v9。


v8 增加四张摘要表，原有账号、记忆和聊天均保留；每窗口只允许一个 queued/running 摘要。分段重建保持范围与 ID，修订表保存尝试和终态；编辑/撤回来源清空当前及修订的摘要正文，留下来源与 stale 状态。启动/关闭中断未完成任务，不重放模型调用。升级前备份保持 v7，可明确回退，不允许旧程序打开 v8。

v7 增加记忆、修订和消息来源索引表，保留既有账号、关系、聊天及命令回执。记忆正文在遗忘事务中从当前条目及所有修订中清空，范围与来源仍作为 tombstone 保存；原聊天、已发布回答、备份和 SQLite 物理页不属于安全擦除承诺。旧 v6 程序不能打开 v7 数据库；回退须明确使用升级前备份，不能自动覆盖新数据。

v6 为既有 AI 补入本地真人的初始管理权与各自的通讯录条目。旧 archived Actor 仍保持停用，真人通讯录条目设为未列出，不自动恢复；公开简介初始化为空，内部 persona 不复制为公开内容。账号、关系、私聊 ID、群成员与历史保留，升级前的 v5 备份可核对原文。实现与验证见 [账号管理记录](m1-account-management.md)。

独立生成参数沿用 execution_bindings 的 JSON payload，该批沿用 v6，无新增迁移。temperature/max_tokens 缺失时按 None 读取；领域与命令候选序列化省略 None，保持旧 payload 与已保存命令摘要的语义一致。参数与账号/档案修改同事务提交，权限、失败回滚和关闭重开验证见 [参数记录](m1-generation-parameters.md)。旧 JSON 导入的绑定不复制 Harness 全局温度或输出上限。

每次 schema 变更有顺序版本与校验记录。打开数据库先取得目录锁，再检查版本和执行事务迁移。未知较新版本拒绝写入并提示需要匹配应用，不尝试降级解释。

迁移前生成一致备份，失败返回实际错误，不继续升级；可用磁盘容量不通过估算假装得到保证。在线数据库使用 SQLite 备份能力或明确暂停写入后的安全流程，不能仅复制主文件而遗漏 WAL。schema 失败保留原库与可定位错误，不发布半升级状态。

## 6. 旧 JSON 导入

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

## 7. 备份、遗忘与回退

单窗口导出由用户在聊天详情明确触发。SocialStore 在一次只读事务中检查实际操作者和当前成员身份，读取完整有序消息并投影为导出快照；不分页拼接、不读历史修订和其他窗口正文，不改变消息列表状态或事件。只包含本窗口的记录和附件占位，排除记忆、摘要、思考、角色内部设定、密钥及配置。无需新增迁移，SQLite 仍为 v13。

Markdown / JSON 是聊天记录文件，不是整库备份，也不用于现有旧历史导入器。桌面在原生保存对话框选择位置，之后复核访问权并读取快照，用同目录临时文件写入、同步后替换目标；保存失败不能显示成功。用户取消不创建导出文件。Web 使用浏览器下载。导出、撤回和遗忘都不改写用户已经保存的文件，备份保留策略另行说明。

撤回和遗忘立即从正常查询、摘要和检索排除，必要正文与修订内容进行清除。WAL、备份和文件系统残留的物理擦除不由普通删除保证，不将逻辑遗忘宣传为所有副本安全擦除。

旧产品已退役；历史版本只认识保留的 JSON，不能读取新社交记录。首版不双写旧格式；降级可能产生独立历史，不能宣称无损回滚。启用新写入前可恢复备份；启用后以新数据库备份和完整导出保障数据，不自动把群聊压回旧 Session。

## 8. 验收

1. 同目录第二个 Runtime 失败，退出后锁可重新获取。
2. 每项关键事务注入失败，内存、消息、终态和事件无部分成功。
3. 杀进程后草稿可恢复，计划不重放，额度不重复扣减。
4. 同一批 JSON 重复导入，身份、消息和计数不增加。
5. 缺失 ID、重复 ID、坏 JSON、源变化、较新 schema 都有明确处理。
6. 导入不改写源文件或用户配置，不自动提取私密记忆或重放工具。
7. WAL 模式下备份与恢复核对所有记录，磁盘满和 busy 不伪报保存成功。
