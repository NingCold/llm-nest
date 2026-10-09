# M1 第二批：真实群聊与持久接话队列

[总纲](README.md) · [已完成](completed.md) · [实施 TODO](TODO.md)

日期：2026-10-09。创建群空间 → 管理成员与共享群历史 → 保存真人消息与参与计划 → 按队列构造目标 AI 的意识上下文 → 独立模型执行 → 原子收尾 → HTTP/Web 展示与重启恢复已接通。私聊也使用同一持久队列，不再依赖临时任务争抢执行锁决定顺序。

2026-10-09 后续简化：移除成员历史起点与加入历史选项。v3 → v4 先备份，再移除 SQLite 列和成员 payload 中的旧字段；保留消息、成员状态/版本、运行、计划和回执。当前规则是所有当前成员共享完整本群历史，按回复对象选材与是否发言分别由意识系统和参与服务负责。

## 当前行为

- 每个群有独立 ConversationId 和完整历史；相同成员可创建不同群，私聊仍保持唯一。
- 普通群消息只保存。结构化 `mentions: ActorId[]` 按选择顺序指定接话者，去重保留顺序；正文里的 `@名字` 不解析为权限或执行目标。
- 回复 AI 的群消息会将原发送者加入目标列表；与显式 @ 合并去重。回复真人不会自动叫醒其他 AI。跨窗口回复和非成员目标在事务边界拒绝。
- 真人消息、接话计划、持久事件引用和命令回执同事务提交。重试同一 requestId 返回原消息和计划，修改请求内容会冲突；失败落盘不产生模型调用。
- 一个空间最多一个运行。计划按 `(trigger_sequence, ordinal)` 顺序执行；输入可以在生成期间继续保存，不删除、合并或覆盖前一条消息。
- 未配置或失效的模型使对应计划失败并保留原因，不伪造回答；其他计划继续执行。已开始运行的正文、思考、用量和失败遵循原有检查点及终态事务。
- 重启将 Running 的运行和对应计划标为 Interrupted，保留片段，不重发已经开始的请求；Queued 计划在 SocialFeature 初始化后继续执行。没有自动重试 Failed 计划或 provider token 游标恢复。

当前群创建需要至少一个 AI，最多 64 位成员（含本地真人）。可以移出全部 AI，窗口和历史仍保留；本地真人不能移出。所有当前成员共享本群完整历史，包括加入前的记录。是否被 @ 或暂停接话不改变可见范围。重新加入会产生新成员版本，不复活先前已取消的计划。

成员变更停止活动回复并保留已落盘片段；移出 AI 同时取消它尚未开始的计划。其他成员的排队计划在执行时依据当前权限构造新快照。已有存储级暂停/归档/结束操作同步取消队列；完整产品控制界面属于下一批。

## 意识上下文

每次请求结合目标角色设定、当前群的成员名字与本空间可见的 committed 原文；成员名单不包含其他 AI 的私有角色设定。最近历史以触发真人消息为截止点；同一条消息中，后续接话者额外读取前面已经完成的回答。较晚发送的真人消息留给自己的计划处理，不改变旧计划的回应对象。

软件为触发消息及其有权读取的 committed 回复引用预留预算，再选择能放入上下文的近期记录；引用可超出最近 200 条的读取窗口，仍须是本群当前成员。预算不足返回错误，不删原始历史。提示标注发送者、时间、序号和回复关联；目标 AI 自己的记录转换为 assistant，真人及其他 AI 转换为带身份标签的 user。其他 AI 的角色设定、私聊或其他群记录不自动注入，执行思考和展示元数据不进入消息正文。

来源消息版本、目标角色/模型版本、成员版本与空间 epoch 在开始和成功收尾时复核。没有新建第二套记忆或聊天副本。当前仍是基本近期上下文，没有摘要、长期记忆或跨窗口接续。

## 结构与接口

| 落点 | 已实现的责任 |
| --- | --- |
| `crates/social/src/participation.rs` | 中立 ParticipationPlan 与状态；消息保存结构化 mentions |
| `crates/storage/src/social/schema-v3.sql` | 持久计划表、顺序索引、唯一运行约束和外键；v2 → v3 前一致备份 |
| `crates/storage/src/social/participation.rs` | 消息与计划原子提交、FIFO 开始、成员管理、撤销和终态关联 |
| `features/social` | 群用例、共享队列、按身份/空间准备上下文、独立执行和初始化恢复 |
| `frontends/web/src/social/GroupControls.tsx` / `LiveChat.tsx` | 创建群、管理成员、@ 选择、回复引用、队列/接话历史及独立草稿 |

新增 HTTP：`POST /api/social/groups`、`POST /api/social/conversations/{id}/members`、`GET /api/social/conversations/{id}/plans`。成员输入的 `action` 为 `add` 或 `remove`，没有历史范围选项；成员 DTO 不返回历史起点。发送输入新增可省略的 `mentions`，保留 `replyTo`；消息返回 mentions，窗口返回 members/pendingPlans。业务 DTO 使用 camelCase，计数器保持十进制字符串。

新增 `plan_changed` 持久引用事件，沿用 HTTP SSE 回放与游标规则。基础 Tauri 命令 `social_create_group`、`social_change_membership`、`social_plans` 调用相同 Feature 和 DTO；桌面社交界面及实时 IPC 尚未接入。

## 验证与范围

Windows MSVC 的 ai-client/chat/runtime/storage/social-feature/events/web-server 共 198 项相关 Rust 测试通过；Linux 的 storage/social-feature/events/web-server 共 40 项通过。受影响业务模块 strict clippy、格式检查、前端类型检查和回归通过；Tauri MSVC 命令编译通过。真实 Web 构建输出到忽略的 `.social-runtime-dist`，没有修改已跟踪 Web dist。

Windows/Linux 的 `scripts/social_acceptance.py` 使用独立临时 SQLite 和本机模拟 provider，验证三个 AI、选择顺序、重复请求、各自模型绑定、群内协作上下文、空间隔离、连续输入、混合失败、共享群历史与成员撤销、订阅断开及崩溃队列恢复。Linux 的旧 Harness `scripts/acceptance.py` 通过。

Chrome 实际操作创建三个 AI 的群、普通消息保存、指定顺序回复、单独回复引用、成员移出/重新加入及刷新恢复；浏览器未记录错误。没有执行付费 API、桌面安装或本批 Linux GUI 验收。

M1 整体仍未完成：消息修订/撤回、完整暂停/结束控制、实时 IPC 和 Tauri 界面待后续；自动 AI 互聊、连续输入合并、计划过期/调度预算属于 M3，摘要/记忆/群话题接续属于 M2。

共享群历史简化的新增验证：Windows MSVC/Linux 各 41 项相关 Rust 测试通过；迁移保留成员状态/版本，备份保留旧历史起点，重复打开不重复迁移。Windows 模拟 HTTP 验收覆盖新加入和重新加入成员读取早期群消息；strict clippy、前端类型检查/回归/构建通过。
