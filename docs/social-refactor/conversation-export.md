# 单窗口聊天导出

[聊天与消息](02-conversations-and-messages.md) · [接口](07-api-and-events.md) · [进度](in-progress.md)

日期：2026-10-13。本批属于 M0–M3 核心完成后的日常社交体验补齐。

## 使用方式与范围

打开一个聊天，在“聊天详情 → 聊天 → 导出聊天记录”选择 Markdown 或 JSON。浏览器下载文件，桌面打开系统“另存为”。支持私聊、群聊、归档窗口和已导入的只读历史；屏蔽联系人后仍可导出既有记录。

导出读取时已保存的完整窗口历史，不受界面已加载页数限制。桌面在选择保存位置后读取快照；等待对话框期间收到的新消息也会包含在内。

| 内容 | 文件中的表示 |
| --- | --- |
| 消息 | 当前版本的正文、消息 ID、顺序、历史发言人名字与账号 ID、发送时间、修订号和状态 |
| 生成中 | 标记为生成中，仅包含此时已保存的片段，未落盘增量不包含在内 |
| 中断 | 保留片段并标记中断，不把片段写成完整成功回复 |
| 撤回 | 保留消息位置与状态，不恢复正文或附件 |
| 回复与提及 | 保留对应消息或账号 ID |
| 群话题引用 | JSON 保留来源 ID、版本与失效标记；Markdown 提示来源群及是否失效，不附带源群正文或摘要 |
| 图片与文件 | 仅包含类型、MIME 与字节数占位，不包含附件二进制 |

记忆、窗口摘要、思考过程、用量与运行内部详情、角色内部设定、模型绑定和凭据不包含在导出中。文件不等同整库备份；本批不提供重新导入，不与旧 JSON 历史导入格式混用。之后的编辑、撤回或遗忘不会改写已保存的导出文件。

## 读取与格式

`SocialStore::conversation_transcript` 在一次只读事务中检查实际操作者和当前成员身份，再按 sequence 读取完整消息。当前群成员可导出入群前的历史；离开后不能导出。管理员身份不扩大窗口范围。读取不标已读，不改变窗口、运行、事件或命令回执；无需数据库迁移，仍为 SQLite v13。

存储层逐行投影消息，附件只留下类型与大小，不在最终快照中持有二进制；不读取历史修订、模型配置或其他窗口。`SocialFeature::export_conversation` 负责两种格式，HTTP 和 IPC 共用这条路径。

- Markdown 使用 UTF-8，按消息分别列出发言人、RFC3339 时间与状态。正文保存在长度足够的代码围栏中，按原文阅读；消息里的 Markdown/HTML 不会变成文件的发言人标题。
- JSON 使用 UTF-8，顶层为 `format: "llm-nest.conversation"`、`version: 1`、`capturedAt`、`conversationId`、`title`、`kind`、`legacy` 和 `messages`。每条消息含 `parts`；序号和版本为十进制字符串，避免大整数精度丢失。引用内部字段沿用领域类型的 serde 形式，不把来源正文复制进文件。
- 建议文件名为 `LLM-Nest-{窗口名}-{窗口ID}.md/json`，移除文件名非法字符并限制标题长度，支持中文与 emoji。

## Web 与桌面

HTTP `GET /api/social/conversations/{id}/export?format=markdown|json` 返回 `{fileName,mediaType,messageCount,content}`，设置 `Cache-Control: no-store`。沿用本地客户端校验与结构化错误；非法格式、缺少格式或重复参数被拒绝。Web 使用 Blob 下载并释放临时链接，只显示“已开始下载”，不声称文件已写入用户磁盘。

IPC `social_export_conversation` 只接受 `id` 和 `format`，不接受文件路径、正文或 operatorId。宿主先检查窗口可读，再打开绑定当前窗口的原生保存对话框；选定位置后复核当前权限并读取快照。同目录临时文件写完并同步后才替换目标；失败保留既有文件，不显示成功。取消返回 `outcome: cancelled`，保存成功返回 `outcome: saved`、实际 `fileName` 和 `messageCount`。

桌面使用 Rust 侧 `tauri-plugin-dialog`，没有增加 WebView 的通用 dialog/fs 命令权限。前端导出时禁用按钮，失败保留格式并允许重试，取消单独提示；按窗口重建控件，旧窗口的结果不会显示到新窗口。

## 验证

验证使用独立临时数据库和本地模拟 provider，没有读取个人聊天或调用付费模型。

- `cargo test -p storage -p social-feature -p tauri-frontend -p web-server --locked` 通过，共 114 项。新增存储场景覆盖 225 条完整有序历史、空窗口、只读/屏蔽窗口、当前成员与管理员边界、编辑/撤回、生成和中断片段、附件占位、跨窗口引用不展开；旧历史导入测试补充导出检查。
- 格式测试覆盖中文/emoji、多行文本、大整数、Markdown 围栏与标题、空记录、状态和文件名。桌面文件测试覆盖 UTF-8、替换保存、失败保留及临时文件清理。
- `cargo fmt --all -- --check`、受影响包 Clippy、Web `typecheck`、12 个前端测试脚本、`build:social-runtime` 与 Web 宿主构建通过。保留 7 条既有 Clippy 风格提示及前端大分块提示，本批未新增。
- HTTP 验收覆盖 207 条记录、当前修订和撤回、空窗口/群聊、非法格式、不同窗口隔离、导出前后状态不变；同批运行社交全链路和宿主/旧历史导入回归。
- 浏览器实际从聊天详情下载 Markdown 和 JSON，并检查下载文件的中文、emoji、消息数与正文。截图位于本地忽略目录 `target/conversation-export-web.png`。

桌面已通过 Windows MSVC 编译、IPC 参数映射和文件保存测试；原生“另存为”的实际鼠标操作、系统覆盖确认及 Linux 对话框仍待平台实机复验，不将这些自动测试视为安装验收。本批未更新本机安装程序，原有暂缓的安装任务保持原安排。

本批导出范围完成。后续可在图片/文件消息链路完成后增加附件文件导出；当前建议的下一批是图片消息，见[正在推进](in-progress.md)。
