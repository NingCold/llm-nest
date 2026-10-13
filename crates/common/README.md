# common · 通用数据

[共用模块](../README.md)

这里定义与模型交流时使用的消息内容，以及回复用量、耗时和工具结果等通用数据。不同模型服务共用这些表达方式。

阅读代码可以从 [message.rs](src/message.rs) 开始，看一条消息能包含哪些内容；[usage.rs](src/usage.rs) 记录模型用量，[timings.rs](src/timings.rs) 记录耗时。账号和群聊的定义在 [social](../social/README.md)。

在仓库根目录运行测试：

```bash
cargo test -p common --locked
```
