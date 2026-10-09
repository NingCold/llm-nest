# M0 领域与存储基础

[重构总纲](README.md) · [实施 TODO](TODO.md) · [存储设计](06-storage-and-migration.md)

日期：2026-10-09。本记录说明 M0 完成时的范围：当时 Runtime 仍使用 JSON，社交 Web 使用独立模拟适配器，应用启动不创建社交库或导入个人历史。后续 [M1 首批私聊](m1-private-chat.md) 已将 GUI 宿主接入 SQLite，并提供 v1 → v2 迁移；旧 JSON 仍不自动导入。以下为 M0 的历史实现与验证记录。

## 实际结构

| 模块 | 已实现的责任 |
| --- | --- |
| `crates/social` | UUID 身份/空间/消息/运行 ID，中立身份、执行绑定候选、成员、消息、Bout/运行状态；意识系统范围、来源、权限依赖与快照；SystemClock/ManualClock |
| `crates/events/src/social_event.rs` | 持久变更引用 DTO；事件不保存原文、草稿 delta 或 provider 请求 |
| `crates/storage/src/guard.rs` | DataDirectoryGuard，JSON 与 SQLite 共享同一 OS 目录锁 |
| `crates/storage/src/social/store.rs` | 异步 SocialStore、类型化命令与操作者/请求 ID |
| `crates/storage/src/social/sqlite.rs` | 容量 64 的单写线程；原子命令、分页历史、回执、事件和启动恢复 |
| `crates/storage/src/social/schema.*` | SQLite schema v1、迁移账本、备份和数据库设置 |
| `crates/storage/src/social/legacy.rs` | 只读预览、来源备份、批次导入、UUID 映射与离线中断收尾 |

`social` 不依赖 Runtime、storage、ai-client 或前端。原协议 Message 与 SocialMessage 分开；社交正文只有文本/图片/文件，发送者由 ActorId 表达。工具和旧 system 消息保存为执行轨迹，不当作社交发言。

revision、sequence、epoch 和事件游标使用共同的有界 Counter，范围为 0..=i64::MAX，传输为十进制字符串；数据库对实体 revision 和消息 sequence 另外约束为正数。消息顺序由空间内 sequence 决定，时钟回退也不会乱序。

## 可调用的原子操作

- 初始化唯一真人；创建 AI 的身份/档案/执行绑定候选。
- 取得或创建关系及唯一私聊；归档后仍返回原空间，恢复须显式执行；群聊单独创建。
- 保存真人消息、修订自己的消息、暂停/恢复参与、归档/恢复聊天；暂停期间仍可保存真人消息。
- 开始运行并创建可见 generating 消息、保存草稿、成功/失败/取消/中断收尾、结束 Bout。
- 分页读取当前历史、补齐持久事件、显式 SQLite 备份、旧记录预览与批次导入。

每个改变业务状态的命令将实体、事件引用和回执一起提交，提交失败不返回成功结果。回执作用域为操作者 + 命令种类 + 请求 ID，保存请求 SHA-256 与实体 ID。相同请求重试先查回执，再查 revision，返回实体的当前可见版本；更改输入复用请求 ID 返回冲突。读取和重试均复核本地操作者与成员权限。

每个空间最多一个 running AI。开始/成功事务复核目标 AI、成员可见范围、档案/绑定/关系版本、参与 epoch、上下文预算声明和所有消息来源版本；群来源需要真人与目标 AI 共同可见。目标空间及来源空间的 context_epoch 或成员版本变化使快照失效。普通后续发送不撤销已经在飞的请求，修订背景会阻止迟到成功提交。暂停、归档和 EndBout 在事务中中断当前运行，保留已保存草稿。实际 token 计算及模型容量匹配由 M1 的 ContextBuilder/Runtime 实现。

这些是存储状态转换，不包含 ContextBuilder、模型调用或参与队列。存储中的 ExecutionBinding 是候选引用，不能根据它声称模型已校验或 executionReady；M1 的用例层须通过 Runtime 的同一配置快照严格解析模型，再调用存储。

## 数据库、锁与迁移

数据库位于调用者已冻结的数据目录的 `social.sqlite3`。foreign_keys=ON、journal_mode=WAL、synchronous=FULL，写事务采用 BEGIN IMMEDIATE，busy 等待上限 500ms。SQLite 错误保留原因和实际数据库路径。

v1 表覆盖身份/档案/绑定/关系、空间/成员、消息/修订/读位置、Bout/运行、事件/回执、导入来源/映射/轨迹。唯一真人、无向关系唯一、包括归档的唯一私聊、私聊成员、空间序列、单活动 Bout/运行以及外键由 SQL 约束。暂不创建尚无用例的记忆、摘要、计划或预算空表；M1/M2/M3 按实际业务增加版本迁移。

v0 仅接受空数据库，升级前通过 SQLite backup API 创建一致备份，schema 与账本/version 在同一事务写入。v1 检查迁移账本与 SHA-256，校验时归一化 SQL 的 CRLF/LF；未知较新版本和不明非空 v0 拒绝处理。备份包含 WAL 中已提交记录，不覆盖已有目标。备份 busy/locked 有限等待；不通过复制主文件代替备份。初始备份和失败导入留下的来源备份不会自动删除。

调用者先取得一次 `Arc<DataDirectoryGuard>`，再分别传给 `FileSessionStore::from_guard` 和 `SqliteSocialStore::open`。不能分别重新加锁，也不能把 SQLite 内部锁替代 `.llmn.lock`。已有 FileSessionStore::new 与旧 Runtime 的行为保留。未来 Runtime 生命周期必须在关闭时等待 `SqliteSocialStore::close`，排空已接收任务并释放目录锁；普通 drop 发起线程关闭，锁在线程结束后释放。

启动将 running 记录和相应已保存草稿原子转换为 interrupted，发出终态引用；再次启动不重复修订或发事件。不自动调用模型、重放工具或恢复 token 游标。工具协议轨迹、参与计划和预算的恢复规则在后续对应业务加入时实现。

## 旧 JSON 处理

预览只读取目录根部的规范 UUID 会话文件，跳过配置/缓存等非会话 JSON；不调用会补写 ID 的旧 load_sessions。源文件须为普通文件，版本、文件名/记录 ID、消息 ID、跨文件冲突和附件块均校验；Running 检查点与已提交消息冲突会报错，不覆盖原回答。

导入前重新读取预览并备份源字节至 `legacy-backups`，整批数据库变更在一个事务中执行。每个 Session 对应独立归档历史空间及归档的“历史 AI”；历史容器采用 group + legacy=true，不创建新联系人的 Relationship 或额外私聊。后续接续应由 M2 的显式授权引用完成。历史空间不能直接恢复为活跃聊天。

已有消息 UUID 保持不变；缺失 ID 在事务内分配并记录，trace-only 条目也参加冲突检查。原附件、思考、反馈、用量、计时、时间及模型选择保存在 legacy_entries/legacy_session_data。原 system/工具内容不成为公开人格或普通消息；未知运行边界不生成虚构 AiRun。旧 Running 转为 interrupted，缺失工具结果补成失败轨迹。读位置初始化到末尾。

同一文件名/SessionId/checksum 重复导入返回已有空间，身份和消息不增加；已经导入的文件发生变化时报告冲突。原 JSON 与配置始终不改写、不删除。本轮没有对个人数据执行导入。

## 验证范围

基础测试使用 tempfile、合成记录、模拟模型引用和 ManualClock，不调用真实 provider。覆盖并发重试、当前版本回执、唯一私聊与显式恢复、持续输入、跨窗口来源失效、停止竞争、末尾失败回滚、外键、busy、WAL 备份、较新/不一致 schema、迁移失败、旧附件/轨迹/缺失 UUID/重复导入/源变化，以及杀进程后的锁释放、草稿恢复和未提交 WAL 回滚。

桌面窗口和安装逻辑没有改动，本轮不替代桌面 GUI 或安装验收。

2026-10-09 验证结果：

| 检查 | 结果 |
| --- | --- |
| Windows MSVC / Rust 1.97.0：social/storage/events/runtime/ai-client/tools/chat 测试 | 通过 |
| WSL Ubuntu / Rust 1.96.0：同一组 Rust 测试 | 通过 |
| 两平台 M0 包 clippy --no-deps --all-targets -- -D warnings | 通过 |
| 包含旧 runtime/chat 的普通 clippy | 通过；旧模块原有 lint 警告保留，本轮未修改这些代码 |
| cargo fmt --all -- --check | 通过 |
| Web typecheck / test | 通过，包含旧 GUI 与社交预览回归 |
| Linux cargo build -p web-server --locked + scripts/acceptance.py | 通过，真实本地 HTTP、模拟 provider、临时数据与工具子进程 |

M0 包测试可独立复现：

```bash
cargo test -p social -p storage -p events --locked
cargo clippy --no-deps -p social -p storage -p events --all-targets --locked -- -D warnings
```

本机默认 Rust 为 GNU 且缺少 dlltool，Windows 验证显式使用 `cargo +stable-x86_64-pc-windows-msvc ... --target x86_64-pc-windows-msvc`。Cargo.lock 只增加新包及其必需依赖。用户原有模型目录改动保留且不纳入 M0 提交；未使用个人配置或真实历史。
