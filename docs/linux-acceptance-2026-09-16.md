# Linux 安装包接续验收（2026-09-16）

## 结论与边界

先验收 [9 月 13 日报告](linux-acceptance-2026-09-13.md) 中的原包，核对两种包的 SHA-256 完全一致，再修复并重建。修复后的 deb 已在宿主安装；deb 的 XWayland、原生 Wayland 和 GTK 2 倍缩放，以及 AppImage 的 FUSE/XWayland 入口均通过下面的自动 GUI 流程。

**本轮没有人工签字通过。** WebDriver 操作、AT-SPI 操作、剪贴板读取、截图审阅、容器测试均属于自动检查。中文字符输入不代表真实输入法候选确认通过；GTK 整数缩放不代表 GNOME 分数缩放通过。Ubuntu 24.04 仍仅完成容器/Xvfb 检查，尚未完成其真实桌面验收。

所有聊天使用绑定 `127.0.0.1` 的模拟 provider；配置、会话、WebKit 配置/数据/缓存均在独立目录。未使用个人配置、会话或真实 API，未推送、打 tag、创建 Release 或发布产物。

## 环境及构建来源

| 项目 | 本轮实际值 |
| --- | --- |
| 修改前源码 | `main`，`3781f4a`，工作区干净；用户已同步另一台机器的提交 |
| 宿主 | Ubuntu 26.04.1 LTS x86_64，GNOME Shell 50.1，已登录 Wayland 会话 |
| 宿主运行库 | GTK 3.24.52、WebKitGTK 2.52.6、XWayland 24.1.10 |
| 桌面驱动 | WebKitWebDriver 2.52.6；调用包内原始 Wry 0.55.1 WebView/Tauri IPC，未替换前端或后端 |
| 显示后端 | 环境默认 `GDK_BACKEND=x11`；另显式运行 `wayland`；AppImage 的 linuxdeploy GTK 启动钩子强制 X11 |
| 发布构建基线 | 原 Ubuntu 24.04 镜像 `8c6fc2f84df2d4269facd5d0acb75416374a716ceb076bdda88441b279572b7f` |
| 工具链 | 容器 Rust 1.97.0、Node 24.18.0、pnpm 11.22.0；8 个 Cargo job |
| 隔离构建 | 从当前 Git 内容导出单独源码目录，再覆盖本轮两个产品源文件；不覆盖原包或原构建目录 |

重建经 Tauri CLI 生成 deb/AppImage，生产前端输出到忽略的 `frontends/tauri/dist`，没有更新已跟踪的 Web dist。构建依赖复用原 Ubuntu 24.04 构建缓存；Cargo.lock 以及两份 pnpm-lock.yaml 按换行归一化后与原构建一致。pnpm 自动依赖重装曾发生崩溃/下载失败，最终使用原容器已安装的相同锁文件依赖，并仅在临时构建目录关闭运行前的自动重装检查。此轮不是全新依赖下载的可重复性证明。

## 发现和修复

### 1. Linux 设置页仍保留 Windows 拖动区域和按钮占位

原包的聊天页已使用 `CUSTOM_WINDOW_CHROME`，但 `SettingsPage` 的两处 `data-tauri-drag-region` 和 `pr-[140px]` 仍受 `IS_DESKTOP` 控制。通过包内 DOM 检查，设置页显示后两处拖动区域可见，Header 保留 140px 的自绘按钮空位。

修复为与聊天页共用 `CUSTOM_WINDOW_CHROME`。Windows 的条件保持为真，Linux 使用系统标题栏且不再预留该空位。自动脚本在原包报告此问题，在修复包报告零失败。原包正常聊天等检查并未因这一问题被误记为失败。

### 2. AppImage 加载宿主 GVfs 时出现 GLib 符号不兼容

原 AppImage 在 Ubuntu 26.04 的真实 FUSE 启动日志出现：

```text
libgvfscommon.so: undefined symbol: g_variant_builder_init_static
Failed to load module: .../gio/modules/libgvfsdbus.so
```

linuxdeploy 仅设置 `GIO_EXTRA_MODULES`，GLib 仍扫描其默认宿主模块目录，宿主模块要求的符号超出了包内 GLib。将默认模块目录限定到实际存在且位于 APPDIR 内的包内目录，探针确认警告消失。

修复在 Linux `main` 最开始、创建 GTK/Tauri/应用线程之前设置 `GIO_MODULE_DIR`；保留用户显式指定值，规范化路径并拒绝目录外的路径或符号链接。保留包内 GnuTLS 模块。普通 deb 和空环境 worker 不带 APPDIR，不受此逻辑影响。新增 Rust 测试覆盖包内目录、宿主目录、缺失目录及越界符号链接。

新 AppImage 完整 FUSE GUI 流程不再出现 GVfs/符号错误。仍有原报告已记录的 `GStreamer element appsink not found`；本产品本轮未验收音视频，因此没有为了消除提示引入整套媒体依赖。

## 自动检查结果

复用新增的 [gui_acceptance.py](../scripts/linux/gui_acceptance.py)，通过真实桌面包和本地 SSE provider 操作界面，读取隔离磁盘记录进行断言，不用 Web 页面或 mock IPC 代替 Tauri。

| 运行对象 | 完整自动流程 | 证据目录（均在 `target/linux-acceptance-2026-09-16/`） |
| --- | --- | --- |
| 原 deb / XWayland | 功能流程通过，设置页平台断言失败 | `deb-crash-baseline/` |
| 原 deb / Wayland | 功能流程通过，设置页平台断言失败 | `deb-wayland-baseline-2/` |
| 原 AppImage / FUSE | 功能流程通过，设置页平台断言失败；有 GVfs 警告 | `appimage-baseline/` |
| 新 deb / XWayland / DPR 1 | **通过** | `fixed-deb-x11-final/` |
| 新 deb / Wayland / DPR 1 | **通过** | `fixed-deb-wayland-final/` |
| 新 deb / XWayland / GTK scale 2、DPR 2 | **通过** | `fixed-deb-scale2-verified/` |
| 新 AppImage / FUSE / XWayland / DPR 1 | **通过** | `fixed-appimage-final/` |

最终四组均覆盖：

1. 包启动、前端经桌面 IPC 初始化 Runtime，无自绘拖动区域。
2. 新建会话，输入中文，经真实流式链路回复中文/emoji，持久化正文、思考和 token 用量。
3. 点击复制全文，Ctrl+V 粘贴回输入框，中文/emoji 无损；另用 GTK 独立读取原包的系统剪贴板确认匹配。
4. 生成参数通过设置页保存，后续请求实际携带 `temperature=0.3`、`max_tokens=1234`；浅/深主题切换；切换设置保留未发送草稿。
5. 慢流中点击停止，磁盘记录为 cancelled，保存已生成片段；provider 429 显示错误，之后可正常发送。
6. 调整原生窗口至 800×600，检查输入框仍在视口内，再恢复原尺寸。2 倍缩放时窗口受显示器工作区限制，恢复尺寸允许 GTK 的 1 逻辑像素舍入；记录见 `resize.json`。
7. 结束进程并重启，消息、取消状态、主题和生成参数恢复。
8. 流式生成时等待检查点落盘，SIGKILL 本测试私有进程组内的应用；重启恢复片段、标为 interrupted；再次重启不重复插入。没有重放工具。

| 其他检查 | 结果 |
| --- | --- |
| 宿主 deb 安装 | 原包安装及修复包覆盖安装均成功；已安装新 ELF SHA-256 为 `cf093584238273a123813ec3e2033743a632d4d2756c1f5e4a4f14525e271766` |
| 桌面入口与图标 | `desktop-file-validate` 通过；Name/Exec/Icon 均为 LLM-Nest；32/128px 已安装 PNG 与仓库派生图标哈希一致；X11 窗口含应用图标 |
| `.desktop` 启动 | 通过 Gio 桌面入口、隔离空 providers 配置启动，Runtime 锁创建成功；这不是人工点菜单验收 |
| Ubuntu 24.04 最小运行镜像 | 新 deb/AppImage 解包后 worker 空环境 add=5、拒绝 shell；Xvfb 启动和 IPC 通过；未把该检查表述为新版包在干净桌面上的人工安装 |
| 源码验证 | fmt、前端 typecheck、前端 regressions 通过；容器 `cargo test -p tauri-frontend --locked` 为 10 个库测试 + 1 个新增入口测试通过 |
| Clippy | `cargo clippy -p tauri-frontend --all-targets --locked` 成功；依赖模块的 8 条既有建议未作无关修改 |
| 清理 | 自动组完成后无测试应用/驱动遗留；人工模式另起独立 fixture，关闭其应用后退出 provider |

初期试跑包含驱动等待历史加载不充分和缩放尺寸未等待/未容许工作区舍入造成的失败。脚本已修正并留下最终运行目录；没有据此修改应用窗口尺寸或宣称应用曾崩溃。早期探索性驱动曾报告 session/page 丢失，完整隔离脚本未复现，不能据此认定产品缺陷。

## 截图证据

以下均是自动采集的测试窗口，内容为本地模拟数据。WebDriver 图像只包含 WebView；系统标题栏图单独通过 X11 窗口采集。

- [原包系统标题栏](evidence/linux-2026-09-16/original-system-titlebar.png)：原生控制按钮，没有叠加 Windows 自绘按钮。
- [修复后设置保存](evidence/linux-2026-09-16/fixed-settings.png)。
- [中文/emoji 剪贴板回填](evidence/linux-2026-09-16/fixed-clipboard.png)。
- [原生 Wayland 崩溃恢复](evidence/linux-2026-09-16/wayland-recovery.png)。

## 本轮本地产物

产物位于忽略目录 `target/linux-rebuild-2026-09-16/source/target/release/bundle/`。它们由 `3781f4a` 加本轮 SettingsPage/main.rs 修复构建，版本仍为 0.1.0，是本地验收包，不是新的公开版本。

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `deb/LLM-Nest_0.1.0_amd64.deb` | 8,051,900 | `a591d159b5dc0d41bf55c5377b28a6bd8a830ef35edd63bdca2251607410de7b` |
| `appimage/LLM-Nest_0.1.0_amd64.AppImage` | 84,728,312 | `29eecea8d83e8a99285f5cf1c979e0bab454e37e464dc29d51c23cd06bb8856f` |

自动报告、包运行日志、模拟请求和磁盘记录仅保留于 `target/`，不提交。报告与精选截图不含配置密钥或个人会话。

## 复验与待人工项

已登录图形会话中，安装与系统 WebKit 匹配的 WebKitWebDriver 后运行：

```bash
python3 scripts/linux/gui_acceptance.py --binary /usr/bin/LLM-Nest \
  --driver /usr/bin/WebKitWebDriver --backend wayland

# AppImage 请传真实 AppImage 路径；脚本移除 extract-and-run 环境开关。
python3 scripts/linux/gui_acceptance.py --binary /path/to/LLM-Nest.AppImage \
  --driver /usr/bin/WebKitWebDriver --backend x11

# 不需要 WebDriver；只准备隔离环境，供真人操作，关闭窗口结束。
python3 scripts/linux/gui_acceptance.py --binary /usr/bin/LLM-Nest --manual
```

每次使用全新的目录，`--output` 指定的目录如已存在会拒绝覆盖。自动模式会操作剪贴板和测试窗口；人工模式没有自动通过判定。普通输入返回固定中文回复，`slow`、`error`、`tool` 分别测试慢流、错误、内置 add。

尚待人类在真实桌面记录结论：

- [ ] GNOME 应用菜单/Dock/任务切换器的名称、图标观感；人工点击入口。
- [ ] 系统标题栏拖动、高速调整大小、最小化、最大化、还原和贴边体验。
- [ ] Fcitx/IBus 中文候选、空格/回车确认不误发送、中英切换、长文本与 Shift+Enter。
- [ ] 与外部应用双向复制/粘贴、多行及长文本、选区行为和滚动体验。
- [ ] GNOME 100%/150%/200% 显示缩放，尤其 150% 清晰度和命中位置。此轮未改用户的显示器缩放。
- [ ] Ubuntu 24.04 的真实桌面安装与上述人工项；AppImage 的不同输入法/桌面组合。

只有这些项目获得实际人工证据后，才能写“Linux 桌面人工验收通过”。
