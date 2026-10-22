# Windows 显示名与安装标识

[发布](../releases.md) · [安装检查](windows-clean-install.md)

用户看到的名称为 **LLM Nest**，二进制、现有安装目录和注册项仍用 `LLM-Nest`，应用 identifier 保持 `com.llmnest.tauri`。显示名不应改变旧安装识别；不要全局替换名称或随意修改产品标识。

Windows 模板在 `client/desktop/src-tauri/packaging/windows`，来自 Tauri CLI **2.11.4** 的 [NSIS](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/nsis/installer.nsi) 和 [WiX](https://github.com/tauri-apps/tauri/blob/tauri-cli-v2.11.4/crates/tauri-bundler/src/bundle/windows/msi/main.wxs)。引入模板的许可见 [LICENSE.tauri](../../client/desktop/src-tauri/packaging/windows/LICENSE.tauri)，不代表项目整体许可证。

- NSIS 保留 `LLM-Nest` 安装目录、卸载注册项和安装位置注册项，安装器/快捷方式/应用列表使用显示名。检测 MSI 兼容新旧显示名与发布者组合。
- `hooks.nsh` 仅处理仍指向当前安装的旧快捷方式，不覆盖指向其他程序的同名快捷方式。
- WiX 保留安装目录与持久注册项，更新快捷方式及菜单名称；旧菜单文件夹只在为空时移除。
- MSI upgradeCode 固定为 `0dc13402-ce85-5dcb-ba1a-1f629fecd8b8`，不可随版本或显示名重算。

升级 Tauri CLI 时对照上游模板合并并重建 NSIS/MSI。执行 `scripts/verify-windows-branding.ps1` 检查 EXE/MSI 元数据与生成 NSIS 脚本，图标执行 `scripts/verify-windows-icons.ps1`。图标唯一源为 `assets/branding/app-icon.svg`，保留 build.rs 对派生 icons 的依赖，核对包内和实际安装 EXE。

构建和静态检查不能代替实际覆盖安装、快捷方式及卸载验证，也不能证明缺少 WebView2 或系统级安装通过。当前结果见 [Windows 验证](../validation/windows.md)。
