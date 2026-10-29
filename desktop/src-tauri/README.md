# 桌面运行部分

[桌面应用](../README.md)

这里负责桌面窗口、云端网络请求、本机 Agent 的启动与停止，以及关闭前的草稿保存与托盘后台运行。修改页面内容请到 [Web 界面](../../web/README.md)。

- [src/local_agent.rs](src/local_agent.rs)：按实际登录身份隔离本机配置、配对和独立进程控制。
- [src/local_agent_tray.rs](src/local_agent_tray.rs)：首次关闭说明、托盘打开与停止。
- [src/lib.rs](src/lib.rs)：应用启动和关闭。
- [src/draft_close.rs](src/draft_close.rs)：关闭前等待页面保存未发送的草稿。
- [src/cloud.rs](src/cloud.rs)：云端请求、消息更新和文件保存。
- [src/local_chat.rs](src/local_chat.rs)：与本机 Agent 直接聊天。
- [src/config_path.rs](src/config_path.rs)：配置和数据的保存位置。
- [src/window_frame.rs](src/window_frame.rs)：Windows 窗口边框。
- [tauri.conf.json](tauri.conf.json)：窗口和打包设置。

在仓库根目录运行 `cargo test -p tauri-frontend --locked` 检查这部分代码。实际打开应用使用[桌面启动命令](../README.md)，安装包步骤见[发布指南](../../docs/releases.md)。
