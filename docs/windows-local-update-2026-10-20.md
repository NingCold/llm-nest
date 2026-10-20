# Windows 本机程序更新（2026-10-20）

## 结果与范围

现有 Windows 程序已更新，并通过原桌面快捷方式启动，供日常试用。窗口经 IPC 显示“本地聊天 / 记录持续保存”，搜索框细灰焦点边框已复验。随后通过应用关闭按钮正常退出，再从原桌面快捷方式重新打开，仍显示上述状态，实际进程来自安装目录；当前保留程序打开。

本轮通过 Tauri CLI / MSVC release `--no-bundle --ci` 嵌入生产前端，版本仍为 `0.1.1-alpha.3`。源码为 `e45ff3d` 加用户未提交的 `crates/ai-client/src/catalog.rs` 修改；该修改原样参与构建，保留且不纳入提交。未运行 NSIS/MSI、卸载或重装，未恢复 Linux 和干净 Windows 安装任务，未调用模型 API、修改账户或配置。

## 产物与替换

| 产物 | SHA-256 |
| --- | --- |
| 构建 EXE：`target/release/LLM-Nest.exe`，26,611,200 字节 | `A5B8B083BE07E8D2E4C2920F871F790085F5511DA31CB9BB6CC974B89AD52793` |
| 更新后已安装 EXE | `21417598F18E4DF6A145F4972C51331558256B765575DA8BC5E3556DFAB44335` |
| 更新前已安装 EXE | `F55F52CBA14BC803D1FDDE0B2C25C84CACAD3F00ED9F1AFFE7D6DBD79E010216` |

目标为 `C:\Users\16617\AppData\Local\Programs\LLM-Nest\LLM-Nest.exe`。沿用现有安装版流程，仅将构建 EXE 的 `UNK` 标记改回安装版 `NSS`，其余字节完全相同，因此两份新 EXE 哈希不同。

原桌面与开始菜单快捷方式的字节哈希、`uninstall.exe` 和核对的四项安装登记字段均未变。release 与已安装 EXE 图标验证通过，`ProductName` / `FileDescription` 均为 `LLM Nest`；两份 EXE 的空环境 worker 均通过 add=5 与拒绝 shell 检查。

## 备份与数据核对

替换前已在仓库外备份至 `C:\Users\16617\.llmn-backups\daily-trial-20261020-220237`，包含旧 `program/LLM-Nest.exe`、`data/`（`~/.llmn`，不含锁文件）及 `webview.zip`。369 个数据与界面文件逐一核对哈希，旧 EXE 也与备份一致。WebView 原文件带有 EFS 属性，使用 ZIP 备份可读取的文件内容并验证；不声明保留 EFS 属性。

实际个人库由 SQLite v6 升级到 v13，`quick_check` 返回 ok，`foreign_key_check` 无结果；核心 10 张表逐行与备份一致，配置文件 SHA-256 未变。个人库原有 1 个真人、0 个窗口、消息及 run，升级后相同。此次不包含非空个人历史升级验证；完整聊天历史恢复的独立数据证据见[桌面体验验收](social-refactor/windows-desktop-experience-2026-10-20.md)。

## 证据与边界

原桌面快捷方式在正常环境启动了已安装路径的进程（首次确认 PID 34292），正常退出后再次启动也通过。本机证据位于忽略的 `target/desktop-installed-update/`：`build.log`、`backup.json`、`update.json`、`data-before.json`、`data-after.json`、`installed-search-focus.png` 和重开后的 `installed-ready.png`，不提交备份、个人数据或配置。

本次是现有程序的本机更新，不代表安装器、干净系统、Windows 10 或其他缩放环境验收。Linux、干净 Windows 安装及相关卸载重装继续暂缓；当前进入日常试用，不新增功能批次。
