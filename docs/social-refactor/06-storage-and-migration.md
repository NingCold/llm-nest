# 06 存储与迁移

[总纲](README.md) · [聊天与消息](02-conversations-and-messages.md) · [API 与事件](07-api-and-events.md)

状态：完整目标设计；M0 已实现 SQLite 基础、共享目录锁、原子操作、备份与只读旧记录导入；M1 首批新增 v2 run_details 迁移、联系人修改和上下文/分页读取，GUI 宿主已启用 SQLite。下述完整首版表按后续里程碑扩展，当前范围见 [M0](m0-implementation.md) 与 [M1 私聊](m1-private-chat.md)。

## 1. 存储布局与锁

在启动时解析并冻结的实际数据目录中新增 `social.sqlite3`。默认目录继续为 `~/.llmn`，`LLMN_DATA_DIR`、`LLMN_CONFIG` 的覆盖及原有目录迁移条件继续有效。模型目录缓存仍为实际数据目录的 `cache/catalog.json`。

SQLite 存储身份、聊天、记忆、计划与执行元数据。附件可转为数据目录内受管理的文件；引用使用相对对象标识，后端限制大小与解析范围，不接受任意文件路径。API Key、`.env` 和 provider 凭据不写入业务数据库或导出档案。

保留 `.llmn.lock` 的 OS 排他锁，一个目录只允许一个独立 Runtime。新增 DataDirectoryGuard 统一持有锁；新 SQLite 与旧 JSON 读取器共享其所有权。不能同时各自创建 FileSessionStore 再重复获取同一锁，也不能用 SQLite 文件锁替代整个目录的应用锁。

现有旧系统目录到新 home 目录的复制先按既有规则完成；JSON 到 SQLite 是随后独立的一次格式导入，两种迁移不可混为自动合并。

## 2. 目标表与关系

| 表组 | 主要表 | 主要约束 |
| --- | --- | --- |
| 身份 | actors、ai_profiles、execution_bindings、relationships | 身份 UUID、档案版本、无向关系对唯一 |
| 聊天 | conversations、memberships、messages、message_revisions、message_context_refs、read_positions、bouts | 私聊 relationship_id 唯一，含归档；每空间 message_seq 唯一，成员对唯一；跨窗口引用追踪源消息版本 |
| 意识系统 | memories、memory_sources、summaries、summary_sources、forget_suppressions | 来源引用版本，范围显式保存；摘要按窗口分段与可见来源视图缓存 |
| 参与 | participation_plans、contact_intents、consumer_offsets | dedupe_key 和处理进度保证幂等 |
| 执行 | ai_runs、run_checkpoints、run_protocol_messages、tool_executions、budget_reservations | RunId 独立，轨迹归属一个 AI 与 run |
| 契约 | committed_events、command_receipts | 全局事件序列、幂等请求作用域唯一 |
| 迁移 | schema_migrations、legacy_imports、legacy_id_map | 版本迁移与来源映射可核对 |

这是一份职责模型，实施时可以合并一对一表，但不能省略事务或来源约束。外键、唯一索引和 CHECK 约束要实际启用并验证；不能只在 Rust 层假设有效。

direct 的 relationship_id 非空且唯一，group 不使用该字段；归档不会释放私聊唯一性。窗口完整消息是权威记录，摘要、近期消息选择和 ContextSnapshot 是派生材料，不因压缩删除原记录。上下文状态或缓存按 ActorId + ConversationId 隔离。

message_context_refs 关联目标私聊消息、目标 AI、发起者、来源窗口与消息/话题范围，不重复保存源正文或授予源权限。源修订、撤回和权限变化要覆盖所有依赖该来源的窗口。summaries 不能仅按 conversation_id 唯一保存；分段范围、来源版本和可见视图共同决定是否可复用。

上述来源引用、摘要、记忆与 AiRun 中保存的 ContextSnapshot 共同支撑意识系统，存储仍统一使用 SocialStore。原消息由聊天用例写入，意识系统只持久化自身派生材料和必要依赖，不另建独立 consciousness 数据库或完整请求副本。M1 使用消息来源快照；M2 启用摘要、显式记忆与跨窗口引用。

SocialStore 接口放在 `crates/storage`，提供领域查询和类型化原子操作，例如 append_message、revise_message、begin_run、finish_run、interrupt_run、forget_memory、claim_intent。前端和 Feature 不直接拼 SQL。

## 3. 关键事务

| 操作 | 同一次提交的内容 |
| --- | --- |
| 创建 AI | Actor、档案、执行绑定、事件、命令回执 |
| 发送真人消息 | 消息、可选上下文引用、空间顺序/版本、Bout 更新、事件、回执 |
| 处理触发事件 | consumer offset、去重后的参与计划 |
| 开始 AI 运行 | 领取计划、预算预留、AiRun、generating 消息、开始事件 |
| 完成 AI 运行 | 最终消息、run 终态、计划/意图终态、预算结算、事件 |
| 中断 AI 运行 | 草稿、未回答工具失败结果、run/计划/意图终态、事件 |
| 暂停或移除成员 | 参与 epoch、取消标记、排队计划状态、事件 |
| 归档空间 | 空间 archived 状态、活动运行取消标记、未执行计划/意图终态、事件 |
| 结束 Bout | Bout 终态、本 Bout 的活动运行取消标记与未执行计划状态、事件 |
| 编辑/遗忘 | 修订或遗忘记录、派生内容失效、上下文版本、撤销标记、事件 |
| 保存/失效摘要 | 来源与权限版本复核、分段摘要版本/状态、派生依赖、变更事件 |

上表中的事件为持久变更引用，不含完整 provider 请求或私密原文。数据库提交后更新内存派生状态并通知订阅者，继续维持写穿透。广播失败不撤销已提交业务状态，订阅者通过事件补齐恢复。

command_receipts 保存稳定实体引用、请求摘要和结果分类，不保存撤回后仍可取出的原响应正文。重复请求返回同一实体的当前可见状态；幂等检查先于已经成功请求的 revision 冲突判断。

预算预留在开始事务创建，终态事务结算或释放；崩溃恢复要处理悬挂预留。提交最终消息须检查 participation_epoch、Bout 状态和可撤销来源版本，停止与完成竞争由事务顺序决定。摘要任务保存同样执行来源版本校验，不能让迟到结果覆盖有效新版本。

## 4. 并发、性能与检查点

M0 使用 rusqlite 0.40.2 bundled，WAL、foreign_keys=ON、synchronous=FULL，已通过 Windows MSVC/Linux 基础测试。SQLite 提供原子事务，具体持久性仍取决于配置和文件系统。[SQLite 事务说明](https://www.sqlite.org/transactional.html)

同步数据库驱动放入受限阻塞执行器或单写线程，不在 async 主执行路径做长时间磁盘工作。不得跨模型网络调用持有事务。busy、磁盘满和 schema 错误按原因返回，限次重试不能破坏幂等。

消息按空间和 sequence 分页；不再启动时加载全部消息到内存。成员、档案和配置可缓存，提交或变更事件后失效。索引至少覆盖空间消息顺序、到期待办、活动运行与记忆范围。

草稿检查点约 500ms 保存有变化的内容；工具调用与结果及时提交。持久事件日志不保存逐 token delta，减少放大写入。

## 5. Schema 升级

每次 schema 变更有顺序版本与校验记录。打开数据库先取得目录锁，再检查版本和执行事务迁移。未知较新版本拒绝写入并提示需要匹配应用，不尝试降级解释。

迁移前验证可写空间并生成一致备份。在线数据库使用 SQLite 备份能力或明确暂停写入后的安全流程，不能仅复制主文件而遗漏 WAL。schema 失败保留原库与可定位错误，不发布半升级状态。

## 6. 旧 JSON 导入

导入器只读取旧会话数据，不读取个人配置生成测试或发布资源。已有 TOML、环境变量引用和目录缓存继续由 Runtime 配置层管理。

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
10. 核对计数与内容后激活新存储，原 JSON 保留且不自动删除。

缺失 ID 的分配在事务失败时可重新生成，因为失败结果未对外发布；已成功导入的映射必须稳定。跨会话 UUID 冲突同样报错，交给显式修复流程。

历史会话默认归档只读，不自动接话、提取记忆或创建意图。用户可明确选择与哪个新联系人接续，并授权使用哪些历史；接续使用该联系人唯一私聊，旧来源作为历史引用保留，不给同一联系人新建额外窗口。读位置初始化到导入末尾，避免把全部旧记录显示为新消息。

## 7. 备份、遗忘与回退

导出须由用户明确触发，说明是否包含私聊、记忆和附件；排除密钥、环境文件和完整配置。备份保留策略单独可见，不能承诺遗忘自动改写用户已经导出的文件。

撤回和遗忘立即从正常查询、摘要和检索排除，必要正文与修订内容进行清除。WAL、备份和文件系统残留的物理擦除不由普通删除保证，不将逻辑遗忘宣传为所有副本安全擦除。

旧程序仍可读取保留的 JSON，但不会看到新社交记录。首版不双写旧格式；降级可能产生独立历史，不能宣称无损回滚。启用新写入前可恢复备份；启用后以新数据库备份和完整导出保障数据，不自动把群聊压回旧 Session。

## 8. 验收

1. 同目录第二个 Runtime 失败，退出后锁可重新获取。
2. 每项关键事务注入失败，内存、消息、终态和事件无部分成功。
3. 杀进程后草稿可恢复，计划和预算不会重复结算。
4. 同一批 JSON 重复导入，身份、消息和计数不增加。
5. 缺失 ID、重复 ID、坏 JSON、源变化、较新 schema 都有明确处理。
6. 导入不改写源文件或用户配置，不自动提取私密记忆或重放工具。
7. WAL 模式下备份与恢复核对所有记录，磁盘满和 busy 不伪报保存成功。
