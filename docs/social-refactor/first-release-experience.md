# 首版本地社交体验收尾

日期：2026-10-13。按[路线](TODO.md)完成现有功能的组合验收和必要修复；本批未增加文档解析、自动记忆或联网能力。当前状态见[正在推进](in-progress.md)，平台未测范围见[剩余验收](pending.md#m1-剩余范围)。

## 本批修复

| 实际问题 | 修复后的行为 |
| --- | --- |
| 无模型时，发送回执的执行提示被自动已读或后台读取清除 | 消息照常保存，成功的自动已读、历史/运行/计划读取保留提示；后台失败仍显示，显式新操作可清除旧提示 |
| 搜索定位超过最新一页的群消息后，“私聊继续”从最新历史取候选，漏掉所点消息 | 从当前显示的定位页选择来源，带入原来的唯一私聊；纯文件来源显示文件占位 |
| 记忆来源的迟到读取可在原文修订后重新显示旧正文，或覆盖刚选择的另一条来源 | 结果绑定读取时的事件游标，列表仅显示最近一次选择的来源 |
| 群成员变更遇到明确版本冲突后一直使用旧版本，部分重试会更换命令 ID | 复用现有版本化操作：结果未知时重试原命令，明确拒绝后使用新版本重新操作 |
| 带普通文件的消息不能清空附带正文 | 可以保留原文件并将正文清空，沿用后端已有的附件修订语义 |

修改集中在共用 React 界面、现有测试与验收脚本。HTTP/IPC DTO、领域模型、SQLite v13 和服务端业务规则未变。主/子代理均沿用现有边界与工具，没有新增备用架构或配置开关。

## 完整使用流程

| 流程 | 本轮验证 |
| --- | --- |
| 空配置 → 创建 AI → 保存消息 → 配置模型 → 继续私聊与共同群 | Edge 实际填写模型设置，创建三位 AI，沿用原私聊并在群中依次点名回复；HTTP 补充三位 AI 各自私聊后进群。无模型保存与提示、重启幂等重试均通过 |
| 群话题 → 唯一私聊 → 记忆与摘要 | Edge 中添加、编辑、遗忘群记忆并开启独立摘要，观察 ready 分段；群累计超过 200 条，刷新确认原话题不在最新页，搜索定位后带引用到已有私聊并成功回复。实际请求选材、范围与来源失效由完整 HTTP 验收复核 |
| 参与控制 → 一次性约定与免打扰 | 浏览器验证群暂停/恢复、设置免打扰、创建/取消约定和结束交谈；HTTP 验证连续输入合并、有限自动接话、到期/延后/跳过、额度与重启恢复 |
| 搜索与个人列表 → 联系人移除/屏蔽 → 恢复联系 | 浏览器验证置顶、手动未读、刷新恢复；移除/重新添加沿用原私聊，屏蔽时历史保留、发送入口关闭，共同群仍正常回复；解除后草稿恢复且可发新消息。HTTP 补充附件窗口列表状态经重启/下载保持不变 |
| 图片/文件草稿 → 刷新与重启 → 发送与导出 | 独立 Edge 自动化会话实际选取合成图片和文件，混合草稿经页面刷新、关闭/重开浏览器及 Runtime 重启恢复；发送后清理草稿，实际下载 JSON ZIP，核对两种附件与原字节完全一致；普通文件清空正文保留附件实测通过 |

普通 Edge 扩展未开放本地文件 URL 访问，无法经该扩展传入文件；附件流程使用本机已安装的 Python Playwright 启动独立 Edge 配置完成。没有更改用户浏览器权限、个人配置或聊天。上述浏览器关闭/重开不代表 Tauri WebView 关闭验收。

## 验证与证据

- 前端类型检查与全部 **18 个测试脚本**通过，包含 HTTP/IPC 契约、草稿事务、此次来源乱序/失效、执行提示、群成员重试与文件修订回归。
- 共用前端 `build:social-runtime` 通过，保留既有大 bundle 提示；依赖与锁文件未更改。本机 pnpm 为 11.25.0，按开发指南用 `--pm-on-fail=ignore` 执行锁定项目的本地验证。
- `acceptance.py`、完整 `social_acceptance.py`、`--only-image-capabilities`、`--only-files` 全部通过。默认完整脚本已包含图片消息与携图导出，无需再重复执行 `--only-images`。
- 三个修改的 Python 脚本语法检查通过。复用上批 MSVC `web-server.exe`，本批无 Rust 修改，不重建庞大的编译目录。
- 文档检查通过：94 份 Markdown、676 个本地链接与锚点；API 表与当前 46 个 HTTP 路由、53 个 IPC 命令一致。`git diff --check` 通过。
- 浏览器新建临时配置、SQLite 和合成附件，模型由本机模拟 provider 响应；没有真实厂商模型调用。此结果证明功能组织与交互，不评价模型的记忆质量或识图效果。

复现共用检查：

```bash
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
pnpm --dir frontends/web build:social-runtime
python scripts/acceptance.py --binary target/debug/web-server.exe
python scripts/social_acceptance.py --binary target/debug/web-server.exe
python scripts/social_acceptance.py --binary target/debug/web-server.exe --only-image-capabilities
python scripts/social_acceptance.py --binary target/debug/web-server.exe --only-files
```

本机忽略的 `target/first-release-*` 保存 frontend typecheck/tests、Web build、host/social/image-capabilities/files 日志，以及浏览器结果、下载 ZIP 和截图。关键截图为 `first-release-old-topic.jpg`、`first-release-attachment-draft.jpg`、`first-release-attachments-export.jpg`、`first-release-model-notice.jpg` 与 `first-release-file-edit.jpg`。测试辅助脚本与临时数据未提交。

## 交付边界

首版本地社交体验收尾完成，既定流程中的问题已修复并复验。下一条线是桌面平台收尾：优先在可用 Windows 环境集中检查窗口、输入、附件与保存，再按设备条件完成 Linux 和干净 Windows 检查；用户暂停的安装任务继续暂缓。

本批没有制作安装包、更新本机安装程序或完成 GNOME / Tauri 原生文件对话框验收。M1 平台阶段仍未完成，不能将本批 Web 与 IPC 回归写成桌面实机通过。候选扩展不自动接续为下一批。
