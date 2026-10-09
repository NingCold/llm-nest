# M1：AI 账号管理与联系人关系拆分

[身份与关系](01-identity-and-relationships.md) · [当前契约](07-api-and-events.md) · [实施顺序](TODO.md)

日期：2026-10-09。本批将 AI 账号、管理员和个人通讯录拆开，接通 SQLite、SocialFeature、HTTP、Tauri IPC 与共用 React 界面。当前里程碑仍是 M1 第 4 条业务线“双适配与阶段收尾”，本批完成不代表整个 M1 已完成。

## 实现与行为

- ActorId 同时作为稳定账号 ID。真人与 AI 共用公开 Account DTO 和资料页，展示名字、头像引用、公开简介、账号 ID 和状态；内部 persona、模型与 reasoning effort 只进入授权管理员的管理 DTO，不进入公开 init、联系人资料或账号视角快照。
- 创建 AI 时授予创建者初始管理员权限。初始管理员可授予或撤销其他已有账号的管理权；普通管理员可修改公开资料、内部设定和模型、停用/恢复并查看该 AI 的账号视角。普通管理员不能继续授权或撤销初始管理员，初始管理员不能移除或替换自己。权限在业务/存储边界检查，重试旧回执也须仍有权限。
- 每个账号有自己的 ContactEntry。移除只撤下实际操作者通讯录中的条目，不改变目标账号状态、唯一私聊、共同群成员关系、运行或接话队列。移除后原私聊仍可收发，重新添加沿用原 ConversationId；AI 自己的通讯录不随管理员的个人移除而改变。
- 停用属于管理员的全局账号操作，保留账号、档案、关系与历史，并原子收尾该 AI 在所有空间的运行与排队计划。恢复后仅新消息触发回复，不重放旧计划，不解除独立的窗口或成员暂停。停用期间可修改设定，修改不会自动恢复账号。
- 修改设定或执行绑定不改变账号、关系与历史。准备上下文后、开始运行前仍校验档案/绑定版本；已开始运行使用原快照并可正常提交。来源消息修订、退出群或参与控制仍可使运行失效，账号停用仍会取消执行。
- “登录 AI 账号查看”首版为管理员只读视角：实际操作者保持自己的账号，按 AI 自身的当前成员关系读取聊天和历史，显示它自己的通讯录。查看停用账号仍要求活动管理员；不允许读取该 AI 未参与或已退出的聊天。页面按需分页、手动刷新，无消息输入或管理操作，不切换后台命令身份。

实现分别位于 storage/social 的 accounts、account_control，SocialFeature 对应用例和 wire DTO；前端 AccountPages 承担公开资料、私有管理和只读查看。HTTP 与 IPC 适配同一组业务方法，持续订阅及关闭协议沿用已有实现，不复制业务。

## 数据迁移

SQLite schema 升至 v6，新增 account_admins 与 address_book，迁移前保留一致备份。旧 AI 的本地真人成为初始管理员；旧活动 AI 出现在真人通讯录，旧已归档 AI 保持停用且不列入真人通讯录。AI 自己的通讯录保留本地真人条目。

公开 bio 初始化为空，不将旧内部 persona 复制到公开资料。旧身份、档案、模型绑定、关系、私聊、消息与版本保留；旧 control_contact 回执改名为 control_account，继续遵守当前状态重试语义。旧联系人生命周期批次的归档/恢复记录见 [历史验收](m1-contact-lifecycle.md)，当前联系人移除的语义以本批和身份文档为准。

## 自动验证

- Windows MSVC：social 1、events 1、storage 单元 19/集成 23、social-feature 6、web-server 4、tauri-frontend 15，共 69 项测试通过。最后的存储权限实现整理后再次通过 23 项集成测试。
- 新集成场景覆盖 v5 真数据升级与备份、内部设定不外泄、旧停用与回执保留、关系移除期间运行/队列继续、唯一私聊与个人通讯录独立、授权/撤销及旧回执权限、事务失败回滚、AI 自身成员范围与停用查看、配置更改时在飞运行保持原快照。
- cargo fmt 检查通过；受影响包 all-targets clippy 完成，保留旧 Web API 的 collapsible_if 提示。前端 typecheck 与五组回归通过，新增同一公开资料组件的人/AI 展示及内部字段排除断言，IPC 回归覆盖全部 30 个命令。
- 更新后的 social_acceptance.py 使用独立临时目录、loopback 模拟 provider 和真实 HTTP，新增公开 DTO、个人资料、生成期间移除/继续回复/重新添加、管理员授予/撤销和账号视角成员范围场景。既有私聊、群聊、修订/参与控制、账号停用、异常收尾与崩溃恢复场景继续通过。
- Web 生产构建写入忽略的 .social-runtime-dist；Tauri CLI 完成 Windows MSVC 嵌入前端的 debug EXE 构建（--no-bundle）。前端构建仍有既有大 chunk 提示。

## 实际界面验证

浏览器使用隔离 HTTP fixture 检查公开资料、管理员设定页及 AI 只读视角；实际移除后在原私聊继续发送并收到模拟回复，再次添加沿用原窗口。公开资料只展示公开简介，内部设定和模型在管理页维护；只读视角显示实际管理员与 AI 自身聊天。

Windows Tauri 使用独立 mock provider、配置、SQLite 与 WebView 目录，实际通过 IPC 打开真人公开资料、AI 管理列表和内部详情，进入 AI 只读视角核对其私聊/群聊和独立通讯录。随后在正常私聊发送合成消息并收到模拟回复，再从 AI 视角读取同一段记录：私聊对方显示真人，自己的 AI 回复显示在自身一侧，页面没有发送入口。

正常关闭后桌面进程退出，SQLite 中两条消息均 committed，没有 running 运行；三个初始管理员与六条双方通讯录记录保留，schema 为 v6，PRAGMA integrity_check 为 ok。本批桌面 UI 没有重复执行授权/撤销或关系移除操作，这些边界由存储、真实 HTTP 与 Web 界面验证覆盖。

## 复现与剩余范围

```powershell
cargo test -p social -p events -p storage -p social-feature -p web-server -p tauri-frontend --target x86_64-pc-windows-msvc --locked
cargo clippy -p social -p events -p storage -p social-feature -p web-server -p tauri-frontend --all-targets --target x86_64-pc-windows-msvc --locked
cargo fmt --all -- --check
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
pnpm --dir frontends/web build:social-runtime
cargo build -p web-server --target x86_64-pc-windows-msvc --locked
python scripts/social_acceptance.py --binary target/x86_64-pc-windows-msvc/debug/web-server.exe --dist frontends/web/.social-runtime-dist
```

桌面构建及独立 fixture 启动方式见 [IPC 验收记录](m1-desktop-ipc.md#复现桌面验收)。检查入口为“我的账号 → AI 角色管理 → 登录 AI 账号查看”；固定实际操作者、不超出目标成员范围和没有发送入口均须核对。

本批没有调用真实厂商 API，没有进行安装包升级、Linux GUI 或发布验收。温度/输出上限等独立生成参数是下一批，随后补明确工具授权及 M1 完整双适配与平台验收。账号视角的独立实时订阅、初始管理员转让、第二个真人账号创建、远程认证和多人联网未实现；记忆查看、摘要与跨窗口接续仍归 M2。当前状态分别维护于 [已完成](completed.md)、[正在推进](in-progress.md)、[尚未完成](pending.md)，避免在历史记录中改写当时结果。
