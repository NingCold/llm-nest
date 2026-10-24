# 云端聊天服务

[服务端](../README.md) · [开发指南](../../docs/development.md) · [API](../../docs/design/api.md)

`cloud-server` 独立运行，使用 PostgreSQL 保存账号、认证、联系人、唯一私聊、群与成员、消息、个人读态、AI 节点绑定与执行任务及事件。图片和普通文件字节保存在独立持久目录。它不依赖本地 Runtime、SocialStore、模型客户端或 AI 执行器。

服务提供邀请注册、邮箱验证、密码登录与找回、联系人、真人与 AI 的私聊和群聊。消息支持附件、编辑、撤回、搜索和导出；群内可管理成员、提及或回复 AI，也可以开启有限自动接话；聊天详情支持暂停、结束和一次性约定联系。私聊可屏蔽，管理员可停用或恢复 AI。

模型调用和意识资料保存在独立 [Agent 节点](../../agent/node/README.md)。服务负责节点配对、任务与来源权限、约定到期和自动调用次数，并转发在线记忆和摘要操作；群里的原文也能按权限带回私聊继续讨论。完整接口见 [API](../../docs/design/api.md)，后续范围见[当前安排](../../docs/project/current.md)。

## 启动

私聊、群聊、附件和 AI 资料可用密文保存，服务保管公开设备信息、密钥投递和加密恢复资料，不保管解密秘密。旧聊天可由有权客户端转换，保留作者、顺序和来源关系；客户端从恢复文件重新核对完整历史后才完成。部署时可开启 `CLOUD_REQUIRE_ENCRYPTION`，拒绝旧客户端发送明文；切换前须完成备份和资料准备。详见[加密契约](../../docs/design/crypto-contract.md)，联调见[当前安排](../../docs/project/current.md)。

先构建静态页面并准备 PostgreSQL：

```bash
pnpm --dir client/web build
cargo run -p cloud-server --locked
```

启动前通过进程环境配置以下内容，不将真实值提交到仓库：

| 环境变量 | 用途 |
| --- | --- |
| `CLOUD_DATABASE_URL` | PostgreSQL 连接串；启动自动运行顺序迁移 |
| `CLOUD_PUBLIC_URL` | 页面公开来源；HTTPS，本机开发允许 localhost HTTP |
| `CLOUD_ATTACHMENTS_DIR` | 附件持久目录，与静态网页和测试收件箱分开 |
| `CLOUD_WEB_DIR` | 静态目录，默认 `client/web/dist` |
| `CLOUD_BIND` | 监听地址，默认 `127.0.0.1:8787` |
| `CLOUD_SMTP_HOST`、`CLOUD_SMTP_FROM` | 邮件发送配置；可配 `CLOUD_SMTP_PORT`、`CLOUD_SMTP_USERNAME`、`CLOUD_SMTP_PASSWORD` |
| `CLOUD_MAIL_OUTBOX` | 绝对路径的私有测试收件箱；设置后使用文件邮件，不发送 SMTP |

管理员生成邀请：`cargo run -p cloud-server --locked -- invite 7`；撤销使用 `revoke-invite 邀请码`。命令只需要数据库连接。邀请码和邮件链接属于秘密，不应贴入公开日志。

容器入口见 [Dockerfile](Dockerfile) 和 [Compose](deploy/compose.yaml)。当前测试配置在 Tailscale 地址的 8443 提供 HTTPS，使用测试 CA 和私有 outbox；测试收件箱不放进静态目录。数据库和附件需要配套持久化与备份，单窗口导出不能代替整库备份。

## 验证与迁移边界

设置独立测试库的 `CLOUD_TEST_DATABASE_URL` 后运行 `cargo test -p cloud-server -- --nocapture`。测试为每例建立隔离 schema，未设置变量时数据库测试会跳过，不能当成完整验证。当前验证结果集中在[验证指南](../../docs/validation/README.md)。

[本地数据样本](tests/fixtures/README.md) 继续验证旧 DTO 映射。正式迁移接收使用 `connector::migration` 的版本 1 DTO：登录真人通过 `POST /api/migration/preview` 提交 `MigrationSnapshot`，核对预览后，以 `POST /api/migration/import` 提交 `{snapshot, previewDigest}`。两者沿用会话和 `X-LLMN-Client`/来源检查，上限为 128 MiB，反向代理同样允许 128 MiB。

快照只能包含一个来源真人，其身份映射为当前登录真人；每个来源 AI 明确新建云端身份，并将当前真人登记为初始管理员，不按名称、模型或旧 UUID 匹配既有账号。云端产生所有新 ID；返回账号、窗口、消息修订/position 和附件索引映射，供节点重写资料来源。`(owner, sourceId)` 和快照摘要保证同源重复请求返回原映射，同源快照变化返回 409；不会重写已经修改或撤回的云端消息。

聊天、附件、联系人 listed/blocked、账号停用、当前群成员及个人置顶/未读状态在同一数据库事务保存；附件先暂存、同步并发布，数据库失败时清理本批文件。已退出的 AI 不恢复成员身份，其历史发送者仍保留；来源真人已经退出的窗口不接受导入。归档和 legacy 窗口以 `readOnly` 提供读取和导出，服务器拒绝发言、修订、撤回、群管理、参与及约定写入；个人读态与置顶仍可修改。

旧读态由 `message_deliveries.position` 导出为精确的 `readMessageIds`，专用 `cloud_import_reads` 保留旧已读集合；不会把旧 sequence 当成已读游标。新消息沿用正常云端游标，旧已读集合只排除本批已读消息。历史及导出带 `importOrigin`，节点材料同样标明来源；旧 AI 发言不产生新节点运行记录。旧生成中消息保留片段并转为 interrupted，撤回不带原正文或附件。新窗口均暂停且约定授权关闭，旧队列、自动设置和未来约定仅供预览，不恢复执行。模型凭据、角色、记忆与摘要正文不进入此云端接口，应另行导入经重新授权的目标节点。

## AI 在线管理与只读查看

`GET /api/ai` 保留实际真人身份，返回管理角色、公开资料/管理员版本及节点声明的能力。`PUT /api/ai/{id}/profile` 按 `expectedProfileRevision` 修改名称/简介；`GET /api/ai/{id}/administrators` 读取管理员，只有初始管理员可用 `PUT /api/ai/{id}/administrators/{accountId}` 按 `expectedRevision` 授予或撤销真人管理员，不能移除初始管理员。写操作使用 `requestId` 去重，重放先复核当前权限并返回当前权威结果。

`POST /api/ai/{id}/management` 转发 `view`/`saveProfile` 给声明 `profileManagement` 的当前在线节点，使用节点档案的独立版本。正文仅在内存转发；离线不排队，返回前再次复核会话、管理权与当前绑定。返回 DTO 仅有角色、模型选择与运行参数，不含服务地址和凭据，模型 Key 在节点本地配置。

`GET /api/ai/{id}/view` 读取该 AI 当前窗口列表；其 `/conversations/{cid}/messages` 与 `/messages/{mid}/attachments/{index}?revision=...` 仅提供分页读取和下载。每次读取检查真人管理权及 AI 当前成员身份，不更换登录身份，不改变已读，不提供代发或全局事件订阅。
