# storage · 数据保存

[共用模块](../README.md)

这里负责保存账号、私聊、群聊、记忆和约定联系，也会记住联系人屏蔽、聊天的已读位置和置顶状态，让你下次打开应用时还能接着聊。主要数据保存在数据目录里的 `social.sqlite3`，已发送的图片和普通文件也随消息保存在其中。

阅读代码时，可以先看 [social/store.rs](src/social/store.rs) 中提供的保存和读取操作，再看 [social/schema.rs](src/social/schema.rs) 中的数据表。历史聊天的导入在 [social/legacy.rs](src/social/legacy.rs)，用户入口位于应用的“设置 → 配置”。

[social/transcript.rs](src/social/transcript.rs) 为聊天导出读取一个窗口的完整记录，需要时一并读取原图。

备份前关闭应用，再复制完整数据目录即可。目录位置可以在桌面应用的设置里查看；保存方式和历史导入的细节见[存储与迁移](../../docs/social-refactor/06-storage-and-migration.md)。

在仓库根目录运行测试：

```bash
cargo test -p storage --locked
```
