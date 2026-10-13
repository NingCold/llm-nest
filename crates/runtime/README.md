# runtime · 应用运行

[共用模块](../README.md)

应用打开时，需要读取配置、连接本地数据库并启动聊天服务；退出时，需要把正在处理的工作保存好。这个模块把这些事情组织在一起，也负责应用运行期间的配置更新。

从 [runtime.rs](src/runtime.rs) 可以看到启动和关闭的过程。[config](src/config/mod.rs) 负责读取与保存设置，[catalog.rs](src/catalog.rs) 负责模型目录缓存。

桌面和 Web 服务会调用这里的代码。详细说明见[运行与配置](../../docs/social-refactor/09-runtime-and-configuration.md)。

在仓库根目录运行测试：

```bash
cargo test -p runtime --locked
```
