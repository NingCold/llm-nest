# 连接器

当前提供已用于社交服务 HTTP/SSE 与桌面 IPC/Channel 的订阅协议类型，供 Rust 消费方独立引用。它是连接器目前已有的协议部分；节点 WSS、节点授权、网络连接与重连尚未实现，不是完整 Agent SDK。

- `SocialEvent` / `SocialChange`：带持久 cursor 的变更引用。消费方重新读取当前获授权实体，事件不携带聊天正文。
- `SocialGenerationEvent` / `LiveEvent`：无持久 cursor 的临时生成片段和执行提示。
- `SubscriptionEvent`：`change`、`generation`、`resync` 三种订阅交付。支持 serde 序列化和反序列化；持久 cursor 与修订号保持十进制字符串。

库只依赖 serde 和中立的 `social` 类型，不依赖 Runtime、存储、模型客户端或 GUI。订阅执行、权限复核、SSE 帧与 IPC 确认仍由各自现有模块负责。

仓库外的 Rust 项目可使用路径依赖：

```toml
[dependencies]
connector = { path = "/path/to/llm-nest/connector" }
serde_json = "1"
```

```rust
let event: connector::SubscriptionEvent =
    serde_json::from_str(r#"{"kind":"resync"}"#)?;
```

运行真实协议解析示例与协议测试：

```sh
cargo run -p connector --example parse_subscription
cargo test -p connector
```

HTTP/SSE 适配器将变更和生成内容分别放入各自 SSE 事件，客户端需先读取 SSE 的事件名与 data；这里的 `SubscriptionEvent` 是共享业务交付封装，不负责解析 SSE 文本帧。
