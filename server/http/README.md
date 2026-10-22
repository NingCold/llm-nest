# web-server · 浏览器服务

[服务端](../README.md) · [Web 界面](../../client/web/README.md)

运行这个服务后，就能用浏览器打开 LLM Nest。它提供页面、接收操作，并持续把新消息和 AI 回复传给界面。

首次运行先准备 [config/llmn.toml](../../config/README.md)。然后在仓库根目录执行：

```bash
pnpm --dir client/web install --frozen-lockfile
pnpm --dir client/web build
cargo run -p web-server
```

打开 `http://127.0.0.1:8787`。服务监听本机地址，适合在自己的电脑上使用。

修改界面时可以改用 [Web 开发模式](../../client/web/README.md#开发时运行)。端口、配置和数据位置通过环境变量设置，见[配置指南](../../docs/configuration.md)。接口代码从 [social_api.rs](src/social_api.rs) 开始阅读。

在仓库根目录运行测试：

```bash
cargo test -p web-server --locked
```
