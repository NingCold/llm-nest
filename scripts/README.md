# 检查与发布脚本

[项目首页](../README.md) · [开发指南](../docs/development.md) · [发布指南](../docs/releases.md)

这里的脚本帮助检查聊天功能、制作安装包和准备新版本。以下命令都在仓库根目录执行。

## 检查聊天功能

```bash
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
```

第一个脚本检查应用启动、配置和历史导入；第二个检查私聊、群聊、记忆、摘要与约定联系。它们创建临时数据，模型回复由本机测试服务提供。

只检查图片消息时，运行 `python scripts/social_acceptance.py --only-images`。这会验证发送、重开、修改与撤回、群引用，以及较大图片的保存和读取。

想打开测试页面，可以先构建界面，再运行：

```bash
pnpm --dir frontends/web build:social-runtime
python scripts/social_acceptance.py --dist frontends/web/.social-runtime-dist --serve
```

使用脚本打印的地址。检查结束后，在它打印的临时目录中创建 `STOP` 文件即可关闭测试服务。

## 其他常用脚本

| 要做的事 | 脚本或说明 |
| --- | --- |
| 准备桌面聊天测试数据 | [social_desktop_fixture.py](social_desktop_fixture.py) |
| 更新应用图标 | [generate-icons.mjs](generate-icons.mjs)，用法见[图标说明](../assets/branding/README.md) |
| 构建和检查 Linux 安装包 | [Linux 脚本](linux/README.md) |
| 检查 Windows 包内程序 | [windows_package_acceptance.py](windows_package_acceptance.py) |
| 检查 Windows 显示名和图标 | [verify-windows-branding.ps1](verify-windows-branding.ps1)、[verify-windows-icons.ps1](verify-windows-icons.ps1) |
| 准备 Windows Sandbox 安装测试 | [prepare-windows-sandbox.ps1](prepare-windows-sandbox.ps1) |
| 校对版本、整理发布文件 | [release.py](release.py)，步骤见[发布指南](../docs/releases.md) |

`social_memory_acceptance.py` 等专项脚本由完整聊天检查调用，用来分别检查记忆、摘要、话题接续和参与节奏。
