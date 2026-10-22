# web-server · 本地 API 回归宿主

[服务端](../README.md) · [当前云端服务](../cloud/README.md)

这里保留旧本地 AI 业务的 HTTP/SSE 适配，用于 Runtime、SQLite、协议和本地社交用例的回归检查。当前默认 Web 和桌面界面使用 `cloud-server`；运行 `web-server` 不会提供可登录当前界面的云端账号服务。

本地 API 检查通常通过[验收脚本](../../scripts/README.md#保留的本地-api-回归)运行，脚本准备独立临时配置、数据和模拟模型。需要单独调试 API 时，在仓库根目录执行：

```bash
cargo run -p web-server --locked
```

宿主默认监听 `http://127.0.0.1:8787`。启动需要本地模型配置，参考[配置说明](../../docs/configuration.md)；用 `LLMN_CONFIG` 与 `LLMN_DATA_DIR` 指向独立测试文件和目录，避免占用用户正在使用的本地库。`LLMN_PORT` 可更换端口。保留的静态文件服务不意味着当前 CloudApp 与这些旧 API 兼容，不要把这里作为云端部署入口。

接口代码从 [social_api.rs](src/social_api.rs) 开始阅读。运行模块测试：

```bash
cargo test -p web-server --locked
```
