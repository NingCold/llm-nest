# 旧架构退役与历史迁移收尾

日期：2026-10-13。用户明确要求“完全移除旧版，全部按照新版来写”。本批直接在 main 完成；不部署或覆盖已安装程序。

## 最终结构

唯一产品链路为 React → HTTP/SSE 或 IPC/Channel → SocialFeature → Runtime / AiClient / SocialStore。Runtime::open 直接打开 SQLite，FeatureContext 必需注入模型客户端与 SocialResources（store/clock），不再注入旧 sessions、事件总线或工具注册表。M0–M3 社交用例继续使用原身份、窗口与生命周期。

删除旧 ChatFeature、CLI/TUI、Session/SessionManager、FileSessionStore/SessionStore、RuntimeBuilder/旧命令/RuntimeEvent、ChatEvent、全局 GuiConfig 和无调用的插件/SDK 空壳。旧 HTTP /api/init、/api/config、/api/sessions、/api/cancel 及旧 IPC/chat-event 不再可用。

前端删除 LegacyApp、旧消息/会话 store、旧聊天 hook、全局生成参数、demo 与早期 U0 独立模拟产品。旧 legacy/demo/social-preview 参数不切换界面。设置只保留模型服务、外观、配置和关于，/api/settings 与 settings_snapshot 仅返回模型服务和版本，不读取旧会话。

协议/模型目录/配置快照/凭据、Markdown 阅读、SQLite 迁移、工具隔离底层与桌面窗口/安装标识均保留。工具未接入社交 AI。旧已跟踪的 Web dist 删除，Web/Tauri 均从同一源码生成被忽略的产物。旧 Linux Harness GUI 和付费验收脚本退役，共用 WebDriver 独立保留，新社交 GUI 脚本仍可使用。

## 数据与导入

本批不变更 SQLite v11 或任何旧迁移。新宿主启动不枚举、加载或补写旧 JSON，因此损坏的旧文件不阻止社交启动。原 TOML/环境引用/目录缓存与旧 [gui] 字段保留，后者不再被运行读取或提供设置接口。

旧格式 DTO 限定于 storage::legacy，导入在 SocialFeature 与 SocialStore 内执行。设置 → 配置 → 导入历史聊天提供检查、选择和明确导入。HTTP 为 GET/POST /api/social/history-import，IPC 为 social_preview_legacy/social_import_legacy；无任意路径或 operatorId。

预览检查当前锁定目录，导入重新检查全目录的重复 ID 并逐条比对所选 checksum/元数据，拒绝伪造、重复选择与源变化。支持导入子集；来源映射使未知回执重试与重启后重复导入幂等。原 JSON 保留，缺失消息 ID 只在导入事务分配；备份、消息、旧工具/系统材料及检查点延续既有导入规则。

导入结果在新版消息列表展示为独立只读历史窗口，专属历史 AI 停用，不自动调用模型或重放任务。旧历史向新联系人接续、完整附件阅读和导出仍属后续能力。

## 验证

- 全工作区 Rust 测试通过；部分导入修复后重跑 storage/social-feature/Web/Tauri。新增存储用例覆盖只导入所选记录、重复选择/伪造预览拒绝、重复导入与源文件不变。
- 前端 typecheck、9 个回归脚本与生产构建通过；IPC 命令一致性检查包含导入接口。
- 重写 scripts/acceptance.py 为新版宿主/迁移验收：空配置、损坏旧 JSON 与启动隔离、旧 HTTP 404、本地请求限制、供应商事务、子集导入/来源变化/幂等/只读/重启、底层固定 worker。
- 完整 social_acceptance.py 通过：私聊/群聊、管理员/关系、持久事件、修订/取消/恢复、显式记忆、摘要、跨窗口引用、有限自动参与与一次性约定/免打扰。
- 实际浏览器携带 legacy=1 仍进入新版；检查并选择单个未导入来源，成功导入后返回消息列表、打开只读历史，控制台无错误。截图为本地忽略的 target/legacy-retirement-web.png。
- cargo fmt --all -- --check、全工作区 Clippy、4 个发布脚本测试与 Python 脚本语法检查通过。Clippy 仍有既有风格提示，前端构建保留大 chunk/混合导入提示，未把通过等同于零警告。
- 文档检查覆盖 60 个 Markdown、413 个本地链接/锚点，以及 35 个社交 HTTP 路径和 44 个 IPC 命令；历史报告中的已删除源码链接固定到退役前提交。

未执行真实供应商调用、安装包更新、Windows 安装验收或 GNOME 实机检查。原型/旧报告只代表历史版本，不作为当前产品能力。

## 进度

本条退役与迁移线完成，M2/M3 首版范围完成，M1 平台收尾仍按用户要求暂缓。下一批建议是消息列表已读/未读、置顶与正文搜索，先从账号/窗口存储和共享事件契约开始。
