# Windows 安装模板

[桌面应用](../../../README.md)

这里保存 Windows 安装包使用的模板：[installer.nsi](installer.nsi) 用于 NSIS，[main.wxs](main.wxs) 用于 MSI，[hooks.nsh](hooks.nsh) 处理安装后的快捷方式。

安装时显示的应用名是 **LLM Nest**。内部安装目录和升级标识沿用已有值，让新安装包能找到用户之前安装的程序。

模板来自 Tauri 2.11.4，随附许可见 [LICENSE.tauri](LICENSE.tauri)。模板维护的说明在[安装标识说明](../../../../../docs/packaging/windows-branding.md)，构建命令在[发布指南](../../../../../docs/releases.md)。
