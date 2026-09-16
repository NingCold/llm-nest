# Windows 显示名与安装标识

应用显示名是 **LLM Nest**，但已安装版本使用 `LLM-Nest` 作为安装目录、注册项和升级标识的输入。Tauri 默认模板把这些用途绑定在 `productName` 上；仅改名称会影响旧安装的识别，所以这里保留安装标识，单独更新用户看到的名字。

`installer.nsi` 和 `main.wxs` 来自 Tauri **tauri-cli-v2.11.4** 标签，分别对应上游 [NSIS 模板](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) 和 [WiX 模板](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/msi/main.wxs)。原代码的 MIT 许可见 `LICENSE.tauri`，该许可只适用于引入的 Tauri 模板，不代表本项目整体的许可证选择。

相对于上游的改动：

- NSIS 的安装目录、卸载注册项及安装位置注册项继续使用原来的 `LLM-Nest`；安装器、快捷方式和系统应用列表使用配置中的显示名。
- NSIS 检测 MSI 时兼容更名前后的显示名/发布者组合。`hooks.nsh` 在成功安装后处理旧快捷方式，只改动目标仍指向当前安装的快捷方式；不会覆盖指向其他程序的同名新快捷方式。
- WiX 保留安装目录和持久化注册项，使用新的快捷方式与菜单文件夹名称，并清理对应旧快捷方式；旧菜单文件夹只在为空时移除。
- `tauri.conf.json` 显式固定 MSI upgradeCode 为 `0dc13402-ce85-5dcb-ba1a-1f629fecd8b8`，与更名前的实际 MSI 一致。二进制名及应用 identifier 保持原值。

升级 Tauri CLI 时必须对照上游模板合并修改，并重新构建两种包。`scripts/verify-windows-branding.ps1` 检查实际 EXE/MSI 元数据与生成的 NSIS 脚本；构建和检查通过后，仍应分别验收旧 NSIS → 新 NSIS、旧 MSI → 新 MSI 的覆盖安装、快捷方式及卸载。不要把 NSIS/MSI 交叉安装当成已经验证的升级路径。

2026-09-16 的 `0.1.1-alpha.2` 已完成 Windows MSVC 前端/桌面构建、NSIS/MSI 打包、上述名称与安装标识检查、EXE/NSIS 图标检查，以及前端类型/回归测试和版本校验。WiX 仍报告上游模板的 ICE03、ICE40、ICE57、ICE61 警告，构建成功不能替代缺少 WebView2、系统级安装和覆盖升级验收。本轮没有安装、卸载或自动启动新包；Linux 包未重新构建。
