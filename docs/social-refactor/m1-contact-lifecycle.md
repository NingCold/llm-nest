# M1：联系人生命周期

[身份与关系](01-identity-and-relationships.md) · [当前契约](07-api-and-events.md) · [实施顺序](TODO.md)

日期：2026-10-09。本批接通联系人归档/恢复与移除入口；联系人状态是全局身份状态，窗口归档和成员暂停仍各自独立。

## 实现与规则

- 共享 SocialFeature 用例与 SQLite 事务新增联系人 control；HTTP 与 IPC 使用同一 DTO、联系人 expectedRevision、requestId。已成功命令重试返回当前联系人状态，不用旧归档回执覆盖后来的恢复。
- 归档原子保存 Actor 状态及版本、该 AI 在所有空间的活动运行终态、已保存片段、排队计划、事件和命令回执；保存失败全部回滚。其他 AI 的运行和队列继续。运行开始与取消 token 登记共用短锁，避免跨空间归档漏掉新登记的执行；模型调用不持锁或事务。
- 保留 ActorId、档案、绑定、关系、唯一私聊、群成员关系和历史发送时署名。恢复不重放已取消计划，不解除窗口/成员暂停，不恢复归档窗口或重新加入已移出的群。
- 已归档联系人已有的私聊可打开并保存真人消息，群内 @ / 回复该 AI 也保存消息并给出 contact_archived 提示，不新增它的接话计划。其他活动 AI 按顺序回复。已归档联系人不能创建新私聊、加入新群或编辑档案，但允许移出已有群。
- init 保留全部 AI 联系人并返回 status；界面默认显示活动联系人，“已归档”列表提供找回入口。移除操作确认后执行 archive；当前不提供物理删除或遗忘。群邀请与接话选择显示可参与身份，表单打开后被归档的已选成员保留取消选择入口。

数据库 schema 仍为 v5，本批复用已有 status 列，不新增迁移，不更改群历史可见性。具体接口只在 [API 与事件](07-api-and-events.md#2-当前接口) 维护。

## 自动验证

- Windows MSVC：storage 19 项单元测试和 18 项存储集成测试、social-feature 6 项、web-server 4 项、tauri-frontend 15 项通过。新增三项集成测试覆盖跨空间取消、其他 AI 继续、片段与身份保留、回执/版本/操作者边界、事务失败回滚、独立窗口/成员控制、恢复不重放，以及归档/完成两种提交顺序不覆盖已确定结果。
- 前端 typecheck 和四组回归通过；IPC 命令回归加入联系人控制。cargo fmt 与受影响包 clippy 通过，保留既有 ai-client、runtime、chat 和旧 Web API 警告。
- 更新后的 social_acceptance.py 使用临时目录和 loopback 模拟 provider 通过真实 HTTP 验收，覆盖私聊和群聊同时生成时归档、FIFO 后续 AI 继续、归档期间消息保存、重启状态与唯一私聊、恢复只接新消息；原有私聊、群聊和控制场景仍通过。
- Web 生产构建只写入忽略的 .social-runtime-dist；桌面通过 Tauri CLI 构建 Windows MSVC 嵌入前端 debug EXE（--no-bundle）。

## 界面验证范围

浏览器在隔离 HTTP fixture 中实际检查：移除确认、活动/归档列表、归档详情、原私聊记录、归档期间保存消息与提示、恢复后的联系人和原历史；群邀请表单打开后，已选联系人被归档会提示并禁止提交，取消该选择后可继续。

Windows Tauri 实际通过 IPC 完成移除/归档、归档列表与恢复，界面与 SQLite 状态一致：目标 Actor 的 revision 从 1 到 2（archived）再到 3（active），已有群的四个成员关系保留，恢复后打开原 ConversationId；PRAGMA integrity_check 为 ok，正常关闭后进程退出。桌面使用独立 mock provider、数据和 WebView 目录，不读取个人会话。

本批未使用真实厂商 API，也未进行安装包升级或 Linux GUI/打包验收；M1 完整平台与异常矩阵仍在阶段剩余范围。联系人生命周期以外的独立生成参数、工具授权和 M2/M3 能力未在本批实现。

## 复现

```powershell
cargo test -p storage -p social-feature -p web-server -p tauri-frontend --target x86_64-pc-windows-msvc --locked
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
pnpm --dir frontends/web build:social-runtime
cargo build -p web-server --target x86_64-pc-windows-msvc --locked
python scripts/social_acceptance.py --binary target/x86_64-pc-windows-msvc/debug/web-server.exe --dist frontends/web/.social-runtime-dist
```

桌面 fixture 的构建/启动方式见 [IPC 验收记录](m1-desktop-ipc.md#复现桌面验收)。在联系人详情移除并确认，归档列表恢复后打开私聊，核对原 ConversationId 与历史；群成员身份应始终保留。
