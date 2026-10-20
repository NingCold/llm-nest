# Windows 桌面体验验收（2026-10-20）

## 结论与范围

本机 Windows 日常使用流程验收与搜索焦点修复完成。测试 EXE 已完成草稿恢复、图片处理、附件保存、聊天 ZIP 导出及生成中关闭检查；用户在 150% 缩放下确认窗口、真实输入法、剪贴板与拖放等操作正常。用户反馈的搜索聚焦外框问题已修复，并在新 EXE 的深浅主题下复验通过。本结论不代表全平台验收完成。

本轮使用独立配置、SQLite 数据及 WebView 环境，连接本地模拟 provider；不读取个人配置或会话，不调用真实厂商 API。产品修改限搜索框聚焦样式，无安装、覆盖安装或卸载操作。用户明确暂缓 Linux，本报告不提供 Linux 桌面结论。

## 环境与构建

| 项目 | 实际记录 |
| --- | --- |
| 系统 | Windows 11 家庭中文版，26H2，build 26300.9711 |
| 显示缩放 | 150%，用户确认 |
| 源码 | `c1f721f` 加本轮搜索样式修改及用户未提交的 `crates/ai-client/src/catalog.rs` 修改；用户修改保留且不纳入本轮提交 |
| 构建 | Tauri CLI / MSVC，debug `--no-bundle`，嵌入生产前端；首轮与搜索样式修改后重建均成功 |
| EXE | `target/debug/LLM-Nest.exe`，40,907,264 字节 |
| 最终 EXE SHA-256 | `ACABAFD89D2EF517FB2C8CF724EEEC6DC5FF04F90107B716176F321808D76374` |
| 首轮 EXE SHA-256 | `543056BA03737FF52BBA3CCD15A4878C21378C2C32C06632E55E955C3FEC77BF`，搜索样式修改前，大小同为 40,907,264 字节 |
| 单元测试 | `tauri-frontend` 库测试 13/13 通过 |
| 日志 | `target/desktop-platform-build/build.log`、`target/desktop-platform-build/build-search-focus.log`、`target/desktop-platform-build/native-tests.log` |

构建与单元测试是独立证据，不替代下述桌面操作。

现有 `node tests/image-drafts.cjs` 再次通过，覆盖附件读取/写入时的关闭等待，以及 quota、读取和清理失败；日志为 `target/desktop-platform-build/draft-contract-tests.log`。这些测试不是原生 WebView 故障注入。

## 桌面检查结果

| 场景 | 实际结果 | 状态 |
| --- | --- | --- |
| 窗口与输入 | 用户确认中文输入法候选空格/回车确认、Enter/Shift+Enter、侧栏及顶部连续拖动、调整尺寸、最小化/最大化/还原、正文选字与搜索行为正常 | 用户实测通过；搜索外观另列修复 |
| 跨应用剪贴板及拖放 | 用户确认中文、emoji、多行文本及图片剪贴板正常，Explorer 图片和普通文件拖放正常 | 用户实测通过 |
| 原生附件选择 | 通过系统文件对话框分别选择透明 PNG 和 TXT，草稿显示图片预览、文件名与大小，随后实际发送成功；图片及普通文件选择器分别按 Escape 取消，草稿未变 | 通过 |
| 草稿关闭与重开 | 文字、透明 PNG 和 TXT 草稿关闭重开后完整恢复，不自动发送；成功发送后清空草稿 | 通过 |
| 超限图片处理 | 5000×3000 图片处理为 2560×1536 WebP，8,338 字节；预览确认后加入草稿，关闭重开后恢复 | 通过 |
| 透明图片处理 | 多选透明 PNG 与方向 JPEG 后分别以默认 WebP 80% 处理、确认并发送；透明图输出为 320×200、RGBA、1,124 字节，右下角 alpha 为 0、红块 alpha 为 255，保留透明 | 界面及字节检查通过 |
| EXIF 方向处理 | EXIF 6 原图为 320×200，显示为 200×320；处理后 WebP 为 200×320、RGB、1,116 字节，无方向元数据，界面前后方向一致；四角对比 PIL `exif_transpose` 参考图，每通道差不超过 1 | 界面及字节检查通过 |
| 图片处理期间关闭 | 未确认处理结果时按 Alt+F4，应用拒绝退出并提示先确认或取消；随后继续生成预览正常，确认后正常退出 | 通过 |
| 原图及普通文件另存为 | 两种保存均实际操作原生对话框取消、成功保存及覆盖已有目标；覆盖均弹出系统确认后成功，PNG/TXT 哈希仍与首次一致 | 通过 |
| Markdown 聊天 ZIP | 原生对话框取消、成功保存、覆盖确认及覆盖成功均已操作；包内含 6 条消息、原 PNG、TXT 及处理后的 WebP，CRC、路径和附件内容哈希正确 | 通过 |
| JSON 携附件导出 | 新 EXE 通过原生对话框导出，`chat.json` 为 v3，含 16 条消息、8 个附件；CRC 正确，全部 `filePath` 存在，8 个附件逐字节与 SQLite 一致 | 通过 |
| ZIP 保存失败与重试 | 对既有 ZIP 持有不允许删除的文件锁后确认覆盖，界面提示“保存失败：拒绝访问（os error 5）”；旧文件正文及导出时间完好，目录无残留临时文件；释放锁后重试覆盖成功 | 通过 |
| 生成中关闭 | 模拟慢流中按 Alt+F4，重开后显示中断片段；SQLite 为 1 个 interrupted、2 个 succeeded；约 8 分钟后仍为 6 条消息、3 个 run，无自动重发 | 通过 |

附件与 ZIP 内文件核对使用以下 SHA-256；三个导出附件还逐字节对照了 SQLite 中已保存的消息内容，结果一致。核对记录在 `target/desktop-platform-output/verification.json`。

图片透明与方向字节验证见 `target/desktop-platform-output/image-processing-verification.json`；新 EXE 的 JSON 导出验证见 `target/desktop-platform-output/json-export-verification.json`。两次导出的消息及附件数量对应各自操作时刻。

| 文件 | SHA-256 |
| --- | --- |
| 原 PNG | `5bb4dbace5597426c31ba5162f9e65db9a88267ea28be8a183ce9fd46991193d` |
| TXT | `6ff640d53b851a5b72f27c68040ae8f2993f6e41199693c231343a31bb5eeed3` |
| 处理后 WebP | `377335266207d9d5649d800da2da1a2537dbf52b1a1c0d81c39b93c691322c19` |

截图保存在本机忽略的 `target/`，未随报告提交：

| 证据 | 文件 |
| --- | --- |
| 草稿及恢复 | `desktop-platform-draft.png`、`desktop-platform-restored.png` |
| 关闭保护及图片处理 | `desktop-platform-close-guard.png`、`desktop-platform-processing.png` |
| 中断恢复 | `desktop-platform-interrupted.png` |
| 保存失败及导出成功 | `desktop-platform-save-failure.png`、`desktop-platform-export-success.png` |
| 透明处理及 EXIF 预览 | `desktop-platform-alpha-preview.png`、`desktop-platform-exif-preview.png` |
| 图片覆盖及搜索问题 | `desktop-platform-image-overwrite.png`、`desktop-platform-search-before.png` |
| 搜索修复后深浅主题 | `desktop-platform-search-dark.png`、`desktop-platform-search-light.png` |
| JSON 携附件导出 | `desktop-platform-json-export.png` |

## 搜索聚焦样式修复

搜索框的输入框受到 `.social-shell input:focus-visible` 规则影响，覆盖了原有 `outline: none`，与外层 `.list-search` 的聚焦蓝框重复显示。已在 `social.css` 使用更具体的搜索输入框选择器取消内框，外层改为 1px `--s-muted` 灰边及 paper 背景。前端 typecheck 已通过，Tauri 重建成功。

新 EXE 在 150% 缩放下，深浅主题鼠标点击搜索框均显示细灰外边，无内蓝框。Shift+Tab 到“添加联系人”按钮仍显示蓝色键盘焦点，再按 Tab 回到搜索框显示细灰边，复验通过。测试窗口已正常退出，本地模拟服务已停止。

## 尚未完成及结论边界

- 附件正在读取/写入时关闭，以及草稿存储失败的原生界面尚未实测；现有关闭协调和草稿事务测试不替代这些实际操作。
- 本机结果限 Windows 11、150% 缩放，不替代 Windows 10 或其他显示缩放；Linux 与安装任务继续按用户要求暂缓。上述边界保留记录，不扩展为新的功能批次。
- 以上使用模拟 provider，不证明真实模型能力；测试 EXE 不构成安装包、发布构建或安装卸载验收。代码静态审查不列为桌面实测通过项。
