# M1 阶段异常、安装包与平台验收（2026-10-09）

[总纲](overview.md) · [当前状态](in-progress.md) · [交付条件](10-delivery-and-validation.md#3-m1身份与私聊群聊)

## 结论与范围

本批完成社交异常链路复验、Windows MSVC / Ubuntu 24.04 安装包构建、Windows 覆盖安装和 Ubuntu 26.04 GNOME 实机检查。GNOME 的自动流程和用户人工确认分别记录。**M1 仍保留干净 Windows 客户系统验收这一收尾项**；安装器生成、MSI 行政解包和已有开发机上的升级不能证明首次安装或卸载重装通过。

所有生成使用 loopback 模拟 provider，配置、SQLite、旧 JSON 和 WebView 数据均隔离；未调用真实付费 API。没有推送、打标签、创建 Release 或公开产物。工作区原有模型目录调整予以保留，不纳入本批提交；本地产物由 `0285a3e` 加当前工作区构建，不描述为某个已发布提交的可复现资产。

## 本批修改

- 社交 HTTP 的 JSON、路径和查询解析失败统一返回 `code/message/retryable`，沿用框架的 400/413/415/422 状态；非法输入不写库、不启动模型。原业务权限、修订冲突和模型错误契约保留。
- 修复聊天列表把空正文中断回复显示成“开始你们的第一段对话”的问题。已有正文、草稿和正在回复的展示仍按各自状态处理。
- 新增实际安装包的社交 GUI/IPC 验收脚本，以及 Windows 包内 EXE / worker 比对脚本；旧 GUI 脚本显式进入 `?legacy=1`，关闭测试调用真实窗口关闭并等待进程退出后再重启。
- CI 与发布流程加入社交离线验收，修正发布命令的参数转发和 PowerShell 多包引号。GitHub 托管 runner 尚未实际执行本次工作流。
- 交付版本递增到 `0.1.1-alpha.3`，MSI 数字版本为 `0.1.1.3`；稳定产品 ID、安装目录、注册项和 upgradeCode 保留。

## 源码与业务验证

| 检查 | Windows MSVC | Ubuntu 24.04 独立构建环境 |
| --- | --- | --- |
| workspace tests | 271 通过，0 失败 / 忽略 | 273 通过，0 失败 / 忽略 |
| fmt / workspace clippy | 通过；既有建议保留 | 通过；既有建议保留 |
| 前端 typecheck / 五组回归脚本 | 通过 | 通过 |
| 真实 HTTP 社交离线验收 | 通过 | 通过 |
| 保留的 Harness HTTP / worker / JSON 验收 | 通过 | 通过 |
| 版本 / 发布脚本 | 版本校验和 4 个发布脚本测试通过 | 两种 Linux 包收集及校验和通过 |

社交 HTTP 新增 14 个非法输入检查，涵盖坏 JSON、缺失/错误字段、错误 Content-Type、超过 2 MiB 的正文、无效 UUID 和查询游标；验证状态码、JSON 错误体、非重试属性、零写入和零模型请求。已有流程继续覆盖三个 AI、唯一私聊、完整本群历史、FIFO、连续输入、修订/撤回、暂停竞争、配置快照、管理员权限、联系人移除/重新添加、错误/EOF/取消、事务回滚和恢复。

| 交付场景 | 本批证据与边界 |
| --- | --- |
| A01/A02/A03/A05/A06/A07 | 既有领域/存储/执行/订阅断言和真实 HTTP 复验；实际 IPC 再覆盖唯一私聊、三 AI FIFO、权限/修订错误和终态展示 |
| A10/A11/A12 | 存储测试与 HTTP 复验；安装版 GUI 用 SQLite trigger 注入写失败，草稿保留、无伪成功、解除故障后原输入重试成功；没有实际填满磁盘 |
| A13/A26/A27/A28/A29 | 既有业务测试、HTTP 和前端 IPC 契约回归；实际 Linux IPC 再检查公开/内部资料隔离、非法模型和账号视角范围 |
| A16/A18/A22 的 M1 范围 | 双平台安装版、新界面和基本意识上下文通过下述检查；干净 Windows 场景仍待验收 |
| M2/M3 及后续工具场景 | 本批不验证尚未实现的记忆/摘要、跨窗口接续、自动参与、未来联系或社交工具执行 |

## Windows 安装与运行

Windows 11 开发宿主使用稳定 MSVC 工具链，通过 Tauri CLI 嵌入生产前端，生成 NSIS 与 MSI。真实 NSIS 从已有 `0.1.0` 升级到本批版本成功，随后覆盖安装最终候选包成功；默认 `~/.llmn` 文件哈希在整个安装/隔离运行前后相同。

`verify-windows-branding.ps1` 与 `verify-windows-icons.ps1` 通过，安装标识、快捷方式及 EXE 资源保持。MSI 行政解包成功；`windows_package_acceptance.py` 核对原始构建 / NSIS 已安装 / MSI 解包 EXE，只允许 Tauri 预期的三字节 `UNK/NSS/MSI` bundle 标记差异，其余字节完全相同。两种包内 worker 均在空环境返回 add=5，并拒绝 shell。

实际已安装 NSIS 版通过：中文私聊及模拟回复、provider 失败保留记录、SQLite 写失败提示与草稿保留、同输入重试。最终候选包在慢流生成中点击正常关闭，进程完成退出，正文片段和运行保存为中断；重启显示该片段且没有重放，SQLite integrity_check 为 `ok`。窗口控制与当前平台实现沿用既有 Windows 基线，本批不据此声称 Windows 10、多显示器或所有任务栏组合已重新人工验收。

本机没有可用 Windows Sandbox，未修改系统功能。两种安装包的 `.wsb` 与当前社交检查清单已各自准备在 `target/sandbox-acceptance-*/`，只映射包目录。首次安装、缺少 WebView2 的在线/离线提示、MSI 实际安装、卸载/重装和干净系统窗口检查仍按 [清单](../windows-clean-install.md) 执行。

## Linux 安装与 GNOME

发布包在独立 Ubuntu 24.04.5 / Rust 1.96.0 / Node 24.16.0 / pnpm 11.22.0 环境构建，GTK 3.24.41、WebKitGTK 2.52.6。源码快照不含个人配置、密钥或会话，未复用 Windows node_modules/target。该环境的 deb 实际安装与社交 GUI/IPC 通过；这是 Xvfb 自动检查。

实机为 Ubuntu 26.04.1、GNOME Shell 50.1、GTK 3.24.52、WebKitGTK 2.52.6，用户登录 Wayland 桌面后继续验收。包传输后 SHA-256 校验通过；deb 从宿主已有 `0.1.0` 升级成功，`desktop-file-validate` 通过，显示名称为 `LLM Nest`，Exec / Icon 保持 `LLM-Nest`。测试准备只补充缺少的 Xvfb、dbus-x11 和匹配的 WebKitWebDriver；新版 Ubuntu 包名为 `webkitgtk-webdriver`。

| 运行对象 | 自动社交 GUI/IPC | 证据目录（位于 `target/m1-platform-qa/remote/`） |
| --- | --- | --- |
| deb / Xvfb | 通过 | `deb-final/` |
| AppImage / FUSE / Xvfb | 通过 | `appimage-final/` |
| deb / GNOME 原生 Wayland | 通过 | `deb-gnome-wayland/` |
| deb / GNOME XWayland | 通过 | `deb-gnome-x11/` |
| AppImage / FUSE / GNOME XWayland | 通过 | `appimage-gnome-fuse/` |
| deb / GNOME Wayland / 请求 GTK scale 2 | 通过，但实际 DPR 为 1，不作为 2 倍缩放证明 | `deb-gnome-scale2/` |
| deb / GNOME XWayland / GTK scale 2、DPR 2 | 通过 | `deb-gnome-x11-scale2/` |

每组覆盖七类结果：默认社交启动与实际 IPC/Unicode；设置双主题和草稿；三 AI FIFO 与权限/过期修订拒绝；429、EOF、禁止工具和取消；SQLite 写失败后原草稿重试；800×600 可见输入框且无 Windows 自绘区域；SIGKILL 后片段恢复一次、再次正常关闭/重启不重放。AppImage 的上述实机组直接使用 FUSE 入口；`smoke.py` 的解包 worker 检查单独通过。旧 Harness 的八类 GUI/剪贴板/保存/恢复流程在显式旧入口复验通过。

自动脚本曾遇到 React 草稿遮盖列表断言、WebDriver 删除 AppImage session 未等待实际应用关闭、崩溃后会话未释放等驱动问题；已修正测试步骤并保留最终报告。驱动插入 astral emoji 有限制，因此社交脚本用实际 IPC 检查 emoji，无损中文输入另经键盘事件验证。上述问题不通过修改产品的锁或生命周期来规避。Xvfb 的 portal / EGL 和已有 GStreamer 提示保留，不把音视频支持写为通过。

### 用户人工确认

为用户打开独立模拟 provider、合成三 AI 与测试群的原生 Wayland 窗口。用户对下列清单回复 **“已列项目均通过”**：

- 中文输入法候选用空格/回车确认不误发送，Enter 发送、Shift+Enter 换行。
- 与其他应用复制/粘贴中文、emoji 和多行文本。
- 原生窗口拖动、缩放、最小化、最大化和还原。
- 应用菜单、Dock、任务切换器的名称及图标。
- 清单同时列出可选 GNOME 150% 显示缩放；人工结论来自用户回复，没有另行采集显示器参数。

这份确认覆盖该 GNOME 宿主与本批包，不外推为 Ubuntu 24.04 实际桌面或其他输入法/硬件组合全部通过。GTK scale 2 的自动检查与 GNOME 分数缩放的人工结论分别计量。

## 本地产物和证据

最终包位于忽略目录 `target/m1-final-artifacts/windows/` 与 `linux/`，各自包含 SHA256SUMS 清单。不要使用早期 `target/m1-artifacts/` 候选包替代最终包。

| 文件 | SHA-256 |
| --- | --- |
| `LLM Nest_0.1.1-alpha.3_x64-setup.exe` | `ca9c15f9cea7cb0588ce72bc964502042e12212e00793bf98cbcd25736027ff7` |
| `LLM Nest_0.1.1-alpha.3_x64_en-US.msi` | `267f9905b3814d68f4369632de3e4f51b795558a2139a5e1e4704316cf87f32d` |
| `LLM Nest_0.1.1-alpha.3_amd64.deb` | `62e1dbc5df51214d6f053f79fbebd8930b217338f69c76a68134f44dd2492e23` |
| `LLM Nest_0.1.1-alpha.3_amd64.AppImage` | `370a2665fe8f880acc4b04dcb4d7c7b03c8dba21969fd1fc9d3f670bfec51577` |

Windows 日志、最终安装版生成/恢复截图和 Linux 报告保留于 `target/`，未提交合成库、临时配置或用户远程地址。复验命令见 [发布指南](../releases.md#本地-linux-打包)；当前里程碑及下一批工作仅在 [正在推进](in-progress.md) 维护。

后续独立完成的本机 NSIS 卸载重装见 [2026-10-09 重装记录](../windows-reinstall-2026-10-09.md)，不改写本阶段当时未执行该项的结论。
