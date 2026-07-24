# AGENTS.md — LLM Nest

## 项目概述

基于 Rust 的模块化 AI 应用平台。CLI 名 `llmn`。

## 架构

```
Apps / Frontends (Chat CLI / TUI / Web / ...)
    ↓
Features (chat / translate / ...)  ← 纯业务逻辑, 无 UI
    │
    │  CompletionRequest { model_selection, messages, options }
    │
    ▼
Runtime ───→ Runtime
    ├── SessionManager        (session CRUD)
    ├── LlmClient             (trait, 接收 CompletionRequest)
    │     └── RuntimeLlmClient (impl, 持有 ModelRouter)
    ├── ModelRouter           (ProviderId → Provider)
    ├── ProviderManager       (持有 Provider map, 支持动态修改)
    ├── EventBus<RuntimeEvent>
    └── FeatureRegistry       (HashMap<id, Arc<dyn Feature>>)
         │
         └── Feature trait (id / initialize / shutdown / as_any)
             Feature 业务方法通过 Arc downcast 获取

provider ──→ Provider trait (接收 ProviderRequest)
    │
    ├── OpenAIProvider
    └── AnthropicProvider (将来)

llm ──────→ LlmClient trait + CompletionRequest + LlmChunk
            流式调用接口定义 + 通用数据结构

common ───→ Message / Role / SessionId / Usage / Config
```

## 目录结构

| 路径 | 角色 |
|---|---|
| `crates/common/` | 公共数据结构（Message, SessionId, MessageId, config 类型） |
| `crates/llm/` | LlmClient trait + CompletionRequest / LlmChunk / 错误类型 |
| `crates/events/` | 前端事件类型定义（ChatEvent 等） |
| `crates/provider/` | Provider trait + ProviderRequest + OpenAI 实现 |
| `crates/runtime/` | 核心运行时：Runtime, SessionManager, ProviderManager, ModelRouter, RuntimeLlmClient, Feature, EventBus |
| `crates/storage/` | 预留，当前空 stub |
| `crates/tools/` | 预留，当前空 stub |
| `features/chat/` | 聊天 Feature：ChatFeature |
| `frontends/cli/` | CLI 前端，仅处理用户交互 |
| `frontends/tui/` | TUI 前端 |

## 依赖关系

```
common ← llm ← provider ← runtime ← features/chat
    ↑_________________________|           |
                     |___________frontends/cli
                                  frontends/tui

common ← events ← frontends/cli
      ← events ← frontends/tui
```

## 数据流

```
Frontend: ChatFeature::chat(session_id, input, model, options, cancel) → Stream<ChatEvent>
    ↓
ChatFeature:
  1. SessionManager.push_message(session_id, user_msg)
  2. 构建 CompletionRequest { model_selection, messages, options }
  3. Arc<dyn LlmClient>::complete_stream(req) → Stream<LlmChunk>
  4. 消费 LlmChunk stream, 转为 ChatEvent (Started/Delta/Finished/Error/Cancelled)
  5. 通过 mpsc::channel(64) 将 ChatEvent 发送给调用方
  6. 完成后 SessionManager.push_message(session_id, assistant_msg)
    ↓
RuntimeLlmClient::complete_stream():
  1. ModelRouter::route(&model_selection) → Arc<dyn Provider>
  2. 构建 ProviderRequest { model, messages, parameters }
  3. Provider::complete_stream(req) → Stream<LlmChunk>
    ↓
Provider (OpenAIProvider):
  1. ProviderRequest → OpenAI API JSON
  2. SSE stream → LlmChunk
```

## 设计要点

### LlmClient trait — `crates/llm/src/traits.rs`

```rust
#[async_trait]
pub trait LlmClient: Send + Sync {
    async fn complete_stream(
        &self,
        req: CompletionRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>>;
}
```

纯接口，定义流式调用方式。实现由 `runtime` 提供（`RuntimeLlmClient`）。

### Provider trait — `crates/provider/src/traits.rs`

```rust
#[async_trait]
pub trait Provider: Send + Sync + Debug {
    async fn complete(&self, req: ProviderRequest) -> Result<ProviderResponse>;
    async fn complete_stream(&self, req: ProviderRequest)
        -> Result<Pin<Box<dyn Stream<Item = Result<LlmChunk>> + Send>>>;
}
```

### ProviderManager — `crates/runtime/src/provider_manager.rs`

运行时概念。持有 `RwLock<HashMap<ProviderId, Arc<dyn Provider>>>`，支持动态 `register` / `remove`。

### ModelRouter — `crates/runtime/src/model_router.rs`

根据 `ModelSelection.provider` 查找 Provider。

### RuntimeLlmClient — `crates/runtime/src/llm_service.rs`

`LlmClient` trait 的实现。持有 `Arc<ModelRouter>`，将 `CompletionRequest` 转为 `ProviderRequest` 后调用 Provider。

### EventBus — `crates/runtime/src/event_bus.rs`

`tokio::sync::broadcast`。

### RuntimeEvent — `crates/runtime/src/event.rs`

```rust
pub enum RuntimeEvent {
    SessionCreated { session_id: SessionId },
    SessionChanged { session_id: SessionId },
    SessionDeleted { session_id: SessionId },
    Error { kind: String, message: String },
}
```

RuntimeEvent 只包含平台级事件，不携带 Feature 业务事件。

### Feature trait — `crates/runtime/src/feature.rs`

```rust
pub trait Feature: Send + Sync {
    fn id(&self) -> &'static str;
    fn as_any(self: Arc<Self>) -> Arc<dyn Any + Send + Sync>;
    fn initialize(self: Arc<Self>, ctx: FeatureContext) -> BoxFuture<'static, Result<()>>;
    fn shutdown(self: Arc<Self>) -> BoxFuture<'static, Result<()>>;
}
```

`FeatureContext` 包含 `SessionManager`, `Arc<dyn LlmClient>`, `EventBus`。

Feature 业务方法不统一，各 feature 暴露自己的 API。前端通过 `FeatureRegistry::get_by_id::<ChatFeature>("chat")` 获取。

### FeatureRegistry — `crates/runtime/src/feature.rs`

```rust
pub struct FeatureRegistry {
    features: HashMap<&'static str, Arc<dyn Feature>>,
}

impl FeatureRegistry {
    pub fn register(&mut self, feature: Arc<dyn Feature>);
    pub fn get_by_id<T: Feature + 'static>(&self, id: &str) -> Option<Arc<T>>;
    pub async fn initialize_all(&self, ctx: FeatureContext) -> Result<()>;
}
```

方案 B 动态注册：通过 `Arc::as_any()` + `downcast` 获取具体类型。

### ChatFeature — `features/chat/src/feature.rs`

```rust
impl ChatFeature {
    pub async fn chat(
        &self,
        session_id: SessionId,
        input: String,
        model: ModelSelection,
        options: GenerationOptions,
        cancel: CancellationToken,
    ) -> Result<impl Stream<Item = ChatEvent>>;
}
```

方法内部：存 user message → 构建 CompletionRequest → 调 LlmClient → 发 ChatEvent → 存 assistant message。
使用 `mpsc::channel(64)` 实现背压，支持 `CancellationToken` 取消。

### ChatEvent — `crates/events/src/chat_event.rs`

```rust
pub enum ChatEvent {
    Started { message_id: MessageId },
    Delta { message_id: MessageId, content: String },
    Finished { message_id: MessageId },
    Error { message_id: MessageId, error: String },
    Cancelled { message_id: MessageId },
}
```

ChatEvent 定义在 `events` crate 中，前端只依赖 `events` + `common` 即可消费事件，无需依赖 `features/chat`。

### Command — `crates/runtime/src/command.rs`

三层命令体系：
- 前端命令（/new, /switch 等）在 frontend CLI/TUI 中定义
- 平台命令（CompactSession, ExportSession 等）在 runtime 中定义
- Feature 业务命令（/retry, /regenerate）在各 feature 中定义

## 构建与运行

```bash
cargo build                    # 构建全部
cargo run -p cli               # 运行 CLI（可执行文件 llmn）
cargo run -p tui               # 运行 TUI（可执行文件 llmnt）
cargo test                     # 运行所有测试
cargo test -p <crate名>         # 运行单个 crate 测试
```

## Config

配置文件: `config/llmn.toml`

```toml
[providers.<provider_id>]
protocol = "openai"
api_key = "..."
base_url = "..."

[providers.<provider_id>.models.<model_id>]
model = "model-name"
display_name = "显示名称"
```

`Protocol` 当前只支持 `OpenAI`。

## CLI 命令

| 命令 | 说明 |
|---|---|
| `/new` | 创建新会话并切换到该会话 |
| `/switch <id\|标题>` | 切换到指定会话，支持 UUID 或标题 |
| `/rename <标题>` | 重命名当前会话 |
| `/delete <id>` | 删除指定会话 |
| `/list` | 列出所有会话 |
| `/help` | 显示帮助 |
| `/quit` | 退出程序 |

## Frontend — CLI 示例

```rust
let runtime = Runtime::from_config("config/llmn.toml")?;

let chat = Arc::new(chat::ChatFeature::new());
runtime.register_feature(chat.clone()).await;
runtime.initialize_features().await?;

let cancel = CancellationToken::new();
let mut stream = chat.chat(session_id, input, model, options, cancel).await?;

while let Some(event) = stream.next().await {
    match event {
        ChatEvent::Delta { content, .. } => print!("{}", content),
        ChatEvent::Finished { .. } => println!(),
        _ => {}
    }
}
```

## Frontend — TUI 示例

```rust
let chat = Arc::new(ChatFeature::new());
runtime.register_feature(chat.clone()).await;
runtime.initialize_features().await?;

runner::run(app, chat).await?;
```

## 注意事项

- `config/llmn.toml` 含 API Key，**不要提交到仓库**
- 使用 `cargo fmt` 和 `cargo clippy` 默认行为
- 添加新 Provider：
  1. 在 `common::config::Protocol` 加 variant
  2. 在 `ProviderFactory::create()` 加 match arm
  3. 实现 `Provider` trait
- 添加新 Feature：
  1. 创建 `features/<name>/`，实现 `Feature` trait
  2. 暴露业务方法
  3. 前端通过 `Arc<Feature>` 直接调用
  4. 事件通过 `RuntimeEvent::Feature` 广播