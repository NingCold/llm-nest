# 桌面运行部分

[桌面应用](../README.md)

这里负责打开桌面窗口、启动本地聊天服务，以及在关闭窗口时保存数据。修改页面内容请到 [Web 界面](../../web/README.md)。

- [src/lib.rs](src/lib.rs)：应用启动和关闭。
- [src/social_commands.rs](src/social_commands.rs)：页面调用的聊天操作。
- [src/social_subscription.rs](src/social_subscription.rs)：向页面传递新消息和回复进展。
- [src/config_path.rs](src/config_path.rs)：配置和数据的保存位置。
- [src/window_frame.rs](src/window_frame.rs)：Windows 窗口边框。
- [tauri.conf.json](tauri.conf.json)：窗口和打包设置。

在仓库根目录运行 `cargo test -p tauri-frontend --locked` 检查这部分代码。实际打开应用使用[桌面启动命令](../README.md)，安装包步骤见[发布指南](../../../docs/releases.md)。
