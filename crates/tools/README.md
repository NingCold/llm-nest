# tools · 工具执行

[共用模块](../README.md)

这里保留了工具执行的基础代码：接收参数、运行工具、返回结果。现有的两个小工具是 `echo`（返回输入）和 `add`（求和）。AI 聊天目前尚未开放工具使用。

[tool.rs](src/tool.rs) 描述一个工具需要提供什么，[registry.rs](src/registry.rs) 管理调用，[process.rs](src/process.rs) 让内置工具在单独的进程里执行。

在仓库根目录运行测试：

```bash
cargo test -p tools --locked
```

后续安排见[工具功能计划](../../docs/social-refactor/pending.md#明确工具授权独立链路)。
