# events · 变化通知

[共用模块](../README.md)

新消息到了、账号资料改了、AI 正在逐字回复时，界面需要及时知道。这个模块定义这些通知的内容。

[social_event.rs](src/social_event.rs) 用于已经保存的变化，界面断线重连后可以继续补齐。[social_generation.rs](src/social_generation.rs) 用于回复过程中的文字片段和进度。通知的读取与发送由[聊天服务](../../features/social/README.md)处理。

在仓库根目录运行测试：

```bash
cargo test -p events --locked
```

完整的通知说明见[接口与事件](../../docs/social-refactor/07-api-and-events.md)。
