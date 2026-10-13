# 模型服务配置

[项目首页](../README.md) · [完整配置指南](../docs/configuration.md)

日常使用可以在应用的“设置 → 模型服务”里添加模型。这个目录提供从源码运行时使用的配置示例。

首次运行 Web 服务时，新建 `config/llmn.toml`，写入：

```toml
[providers]
```

这样可以先打开应用，再从设置里添加服务。如果喜欢编辑文件，可以参考 [config.example.toml](config.example.toml)，它演示了用环境变量 `DEEPSEEK_API_KEY` 读取密钥。

桌面安装版会自动在用户数据目录创建配置。配置的实际位置可以在“设置 → 配置”查看，也可以用 `LLMN_CONFIG` 指定其他文件。

`llmn.toml` 和 `.env` 是本机文件，Git 已忽略它们。模型地址、本机 Ollama 和密钥的写法见[完整配置指南](../docs/configuration.md)。
