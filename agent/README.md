# 自带 Agent

[项目首页](../README.md) · [接入计划](../docs/project/cloud-plan.md)

这里放 AI 如何准备上下文、连接模型和生成回复的代码。

- [core](core/README.md)：选取聊天、记忆和摘要，组织一次回复所需的材料和预算。
- [ai-client](ai-client/README.md)：连接模型服务，处理文字、图片和流式回复。
- [tools](tools/README.md)：已有的工具执行基础，当前聊天尚未开放工具。

现有聊天已经调用这些库。它们可以单独构建和测试，但目前还由本地应用启动和调度；独立 Agent 程序、节点授权、私有资料存储和后台运行将在接入计划中完成。

在仓库根目录检查核心：

```bash
cargo test -p builtin-agent -p ai-client --locked
```
