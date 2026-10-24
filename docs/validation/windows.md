# Windows 已知结果

[验证入口](README.md) · [待验收](../project/backlog.md) · [安装清单](../packaging/windows-clean-install.md)

## 存量加密转换版本安装：2026-10-24

本机已使用 `0.1.1-alpha.4` NSIS 安装器以 `/S` 更新，安装器退出码为 0，注册表安装登记版本为 `0.1.1-alpha.4`。这次是已有安装的更新，不是干净系统安装。MSI 安装包已构建，尚未实际安装或验证覆盖升级。

安装后的程序使用隔离 `LLMN_DATA_DIR` 启动，实际窗口截图确认正常显示登录页。配套 Agent 已更新，但维持停止；本机 Agent 构建沿用用户未提交的模型目录修改，该修改保留且不纳入本次提交。没有用日常账号完成可信设备授权、恢复文件保管或旧资料转换，也没有由此次启动触发模型调用。自动链路、恢复和备份证据统一见[存量加密转换验证](README.md#存量加密转换验证)。

日常服务仍保留旧资料且未开启强制加密，待用户准备设备和恢复材料后再切换。本轮安装成功与登录页检查不能替代这些后续验收，也不替代干净 NSIS/MSI 安装、其他 Windows 版本或缩放检查。

本机另提供“LLM Nest 内网试用”快捷方式，为应用进程设置测试 CA；原快捷方式保留，不关闭 TLS 验证，也不修改系统证书库。已从该入口启动最终安装版，并在实际界面确认“服务器已连接”，注册页和邀请填写正常。用户确认尚未注册日常账号，已准备七天单次邀请，账号提交与邮箱验证由用户接续；现有验收账号和记录不自动迁入新账号。

## 云端管理与本机 Agent：2026-10-23

本轮使用独立的 Tauri 开发构建、WebView 目录、节点数据和云端测试账号，模型连接本机模拟服务。实际窗口验证本机模型配置、密钥本地保存、节点配对与授权、启动、取消关闭及后台继续回复。用户实测通过托盘恢复窗口、继续输入和停止本机 Agent，随后云端确认节点离线。自动检查与联调结果统一记录在[第 4 步验证](README.md#云端管理与本机运行验证)。

验收构建 GUI SHA-256 为 `A1727D1849739A13A5B18E05D6184242FB88567EECAC64F7B3848DBE0C806CC9`，配套 Agent 为 `1E75919539155EE782E9BE69F5E58C52A867863E9B72FE45A030F469C31A0C38`。产物与证据保留在忽略的 `target/management-work/`；构建沿用用户未提交的模型目录修改，该修改不纳入本次提交。

没有替换下面记录的已安装程序，也没有迁移旧数据或改动日常 Agent。干净安装、安装包和其他设备仍按原清单安排。

## 本机日常版本验收：2026-10-20

Windows 11 家庭中文版 26H2、build 26300.9711、150% 缩放。本机日常使用流程与搜索焦点修复完成，现有程序已更新，供日常试用。

用户实测通过中文输入法候选空格/回车确认、Enter/Shift+Enter、侧栏/顶部连续拖动、尺寸与窗口控制、正文选字/搜索、跨应用中文/emoji/多行和图片剪贴板，以及 Explorer 图片/文件拖放。搜索双层焦点外框已修复；新 EXE 深浅主题鼠标点击及键盘切换焦点均复验。

独立配置、SQLite 和 WebView 环境连接本地模拟 provider，通过原生选图/选文件及取消、混合附件草稿重开且不自动发送、超限图片处理、未确认处理时关闭拦截、透明及 EXIF 方向处理、原图/文件保存取消与覆盖、Markdown/JSON 携附件 ZIP、中断片段恢复。透明输出保留 alpha；EXIF 6 输出尺寸 200×320、无方向元数据，角点与转正参考每通道差不超过 1。JSON v3 导出含 16 消息、8 附件，CRC、路径及附件字节与 SQLite 一致。ZIP 被文件锁阻止覆盖时明确报错，旧文件完整且无临时残留，解除锁后重试成功。

本次 `tauri-frontend` 库测试 13/13 及现有 `tests/image-drafts.cjs` 通过；后者覆盖读写时关闭等待与存储失败，不代表真实 WebView 故障注入。未调用真实模型 API。

## 2026-10-20 历史试用程序

以下记录对应当日更新，已由上面的 `alpha.4` NSIS 更新替代，不是当前安装版本。

Tauri CLI / MSVC release `--no-bundle --ci`，嵌入生产前端，版本仍为 `0.1.1-alpha.3`。源码 `e45ff3d` 加用户未提交的 `catalog.rs` 修改，该修改原样参与构建、保留且不纳入提交。

| 产物 | 大小 / SHA-256 |
| --- | --- |
| release 构建 EXE | 26,611,200 字节；`A5B8B083BE07E8D2E4C2920F871F790085F5511DA31CB9BB6CC974B89AD52793` |
| 已安装 EXE | `21417598F18E4DF6A145F4972C51331558256B765575DA8BC5E3556DFAB44335` |
| 此前独立验收 debug EXE | `ACABAFD89D2EF517FB2C8CF724EEEC6DC5FF04F90107B716176F321808D76374`；不与 release 混用 |

更新目标为 `C:\Users\16617\AppData\Local\Programs\LLM-Nest\LLM-Nest.exe`。仅把构建 EXE 的 `UNK` 标记改回现有安装版 `NSS`，其余字节相同。原快捷方式、卸载程序及核对的安装登记未变；EXE 图标、名称、空环境 worker add=5 与拒绝 shell 检查通过。

完整备份在 `C:\Users\16617\.llmn-backups\daily-trial-20261020-220237`，包含旧 `program/LLM-Nest.exe`、`data/`（`~/.llmn`，排除锁文件）与 `webview.zip`。369 个数据及界面文件逐一核对哈希，旧 EXE 也一致。WebView 按可读内容压入 ZIP 并验证，不声明保留原 EFS 属性。

个人库 v6→v13，`quick_check` 为 ok、外键检查无结果，核心 10 表逐行与备份一致，配置哈希不变。原个人库只有 1 个真人、0 个窗口/消息/run，升级后相同；本次迁移结果限这个个人空库，非空历史恢复来自独立验收。

经原桌面快捷方式正常启动，IPC 显示“本地聊天 / 记录持续保存”；关闭按钮正常退出后重新打开成功，进程来自安装目录，窗口保留供试用。未运行安装器、卸载重装，未改账户/配置或调用模型 API。

本机证据在忽略的 `target/desktop-platform-build/`、`target/desktop-platform-output/` 和 `target/desktop-installed-update/`，包括构建日志、图片/导出字节验证、备份/更新及数据前后记录、截图 `installed-ready.png`；不提交个人备份或配置。

## 安装结果及待验范围

| 日期与操作 | 已有结果或安排 |
| --- | --- |
| 2026-10-09，本机 NSIS 实际卸载重装 | 备份后普通卸载，程序/快捷方式/卸载登记移除；原路径重装与开始菜单启动通过。个人配置、原 GUI 数据及隔离非空合成库均保留，SQLite 全表指纹一致、完整性检查 ok；合成库含 4 账号、2 窗口及真人消息/AI 中断片段，启动后可读且不重放运行 |
| 2026-10-20，历史 release EXE 更新 | 直接更新已有程序并正常启动/重开，本次未运行安装器 |
| 2026-10-24，alpha.4 NSIS 更新 | `/S` 退出码 0，安装登记版本正确；隔离数据目录下实际启动到登录页，Agent 保持停止 |
| 干净安装与 MSI | alpha.4 MSI 只完成构建；缺少 WebView2、干净 NSIS/MSI 安装、MSI 实际安装/覆盖及卸载重装继续按原安排待验 |
| 其他环境与专项故障 | Windows 10、其他缩放，以及真实 WebView 极端草稿存储失败/附件读写瞬间关闭待验 |

## 包检查入口

在仓库根执行，参数指向本次实际产物：

```powershell
./scripts/verify-windows-branding.ps1 -BuildDirectory <构建目录>
./scripts/verify-windows-icons.ps1 -ArtifactPath <EXE或安装包路径>
python scripts/windows_package_acceptance.py --build <release构建EXE> --installed-nsis <已安装EXE> --msi-extracted <MSI解包后的EXE>
```

先用各脚本的帮助或参数定义确认输入路径。`windows_package_acceptance.py` 使用已经安装的 NSIS 程序和 MSI 解包结果检查产物，不代为完成干净安装。实际安装、覆盖及卸载按[安装清单](../packaging/windows-clean-install.md)另行安排。
