# ai-client · 模型连接

[共用模块](../README.md) · [模型配置指南](../../docs/configuration.md)

这个模块负责向模型服务发送请求，把返回的文字、思考内容和用量整理成应用能使用的回复。它支持 OpenAI Chat Completions、Responses、Anthropic、Gemini，以及 Ollama 的兼容接口。

文字和图片会转换成各个接口所需的格式；实际能否看图取决于所选模型。

日常添加模型服务可以直接使用应用设置。需要接入新接口时，从 [client.rs](src/client.rs) 和 [protocols](src/protocols/mod.rs) 开始阅读。内置模型信息在 [catalog.rs](src/catalog.rs)，公共模型目录的同步在 [catalog_sync.rs](src/catalog_sync.rs)。

在仓库根目录运行本地测试：

```bash
cargo test -p ai-client --locked
```

这些测试使用预设的请求和响应数据；真实模型调用另行安排。
