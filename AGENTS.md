# AGENTS.md — LLM Nest

## 项目概述

基于 Rust 的模块化 AI 应用平台。CLI 名 `llmn`，TUI 名 `llmnt`。

## 架构（按实际代码）

```
Apps / Frontends (cli / tui / tauri)
    │  直接调用 Feature 业务方法（chat.chat(...)），消费 ChatEvent 流渲染
    ▼
Features (chat)  ← 纯业务逻辑, 无 UI
    │  ChatFeature::chat(session_id, input, model, options, cancel) → Stream<ChatEvent>
    │  经 FeatureContext 注入 SessionManager + AiClient
    ▼
Runtime
    ├── SessionManager        (session CRUD + 会话级模型记忆)
    ├── AiClient (ai-client crate) ── LLM 客户端：模型路由 + 协议分发 + 请求
    │     ├── ModelRouter     (模型目录、每 provider 默认模型、严格解析校验)
    │     ├── build_provider  (协议工厂: protocol → 适配器)
    │     └── 协议适配器      (openai / openai_responses / anthropic / gemini / ollama)
    ├── ConfigLoader + 配置热更新 watcher (notify)
    ├── EventBus<RuntimeEvent>
    └── FeatureRegistry       (HashMap<id, Arc<dyn Feature>>)
         │
         └── Feature trait (id / initialize / shutdown / as_any)
             Feature 业务方法通过 Arc downcast 获取

ai-client ──→ AiProvider trait (接收 ChatRequest)
    │
    ├── OpenAIProvider          (chat completions)
    ├── OpenAIResponsesProvider (Responses API)
    ├── AnthropicProvider       (Messages API)
    └── GeminiProvider          (Gemini API; ollama 复用 OpenAIProvider)

common ───→ Message / Role / SessionId / Usage / GenerationOptions
```

> 注：AGENTS.md 曾描述 `crates/llm` + `crates/provider` 三 crate 分层（LlmClient/Provider trait），
> 已按实际代码合并为 `crates/ai-client` 单 crate 承担 LLM 客户端全部职责（接口、协议实现、
> 模型路由、协议工厂）。模块边界仍在（`router.rs` / `protocols/` / `config/` / `reasoning.rs`），
> 未来如需拆分，类型归属已清晰。

## 目录结构

| 路径 | 角色 |
|---|---|
| `crates/common/` | 公共数据结构（Message, Role, SessionId, Usage, GenerationOptions） |
| `crates/ai-client/` | LLM 客户端：AiProvider trait、AiClient（路由+分发）、ModelRouter、协议实现、config（Protocol/ProviderConfig/ModelConfig）、reasoning、catalog、SSE 分帧 |
| `crates/events/` | 前端事件类型定义（ChatEvent 等） |
| `crates/runtime/` | 核心运行时：Runtime, SessionManager, Feature, EventBus, ConfigLoader, 配置热更新 watcher |
| `crates/storage/` | 会话持久化：SessionStore trait、FileSessionStore（每会话一个 JSON 文件，原子写）、SessionRecord DTO、默认数据目录 |
| `crates/tools/` | 预留，当前空 stub |
| `features/chat/` | 聊天 Feature：ChatFeature |
| `frontends/cli/` | CLI 前端（llmn） |
| `frontends/tui/` | TUI 前端（llmnt） |
| `frontends/tauri/` | Tauri 桌面前端 |

## 依赖关系

```
common ← ai-client ← runtime ← features/chat
                   ↑         ↓
              events ←──────┘
                 storage ↑
frontends/cli、tui、tauri 依赖 runtime + features/chat + common + events
runtime 依赖 notify（配置热更新）+ storage（会话持久化）
storage 依赖 common + ai-client（SessionRecord 复用 Message / ModelSelection 的 serde）
```

## 数据流（一次对话）

```
Frontend: ChatFeature::chat(session_id, input, model, options, cancel) → Stream<ChatEvent>
    ↓
ChatFeature (features/chat/src/feature.rs):
  1. SessionManager.push_message(session_id, user_msg)
  2. 构建 ChatRequest { selection, messages, options, resolved: None }
  3. ctx.llm.complete_stream(req) → ChatStream
  4. 消费 ChatChunk 流, 转为 ChatEvent (Delta/Finished/Error/Cancelled)
  5. 完成后 SessionManager.push_message(session_id, assistant_msg)
    ↓
AiClient::complete_stream (crates/ai-client/src/client.rs):
  1. ModelRouter::resolve(&selection) → ResolvedSelection
     —— 校验 provider/model/reasoning_effort，错误带候选清单；首个 await 前快照冻结
  2. route(&selection) → Arc<dyn AiProvider>（按 provider id 查 providers 表）
  3. 填充 ChatRequest.resolved → provider.complete_stream(req)
    ↓
Provider 适配器 (protocols/<name>/):
  1. convert: ChatRequest → 该协议 wire JSON（消息格式、认证头、reasoning 映射、max_tokens 等）
  2. HTTP 请求 + SSE 解析（共享 SseDataStream 分帧，各协议解析自己的事件词汇）→ ChatChunk
```

## 设计要点

### AiProvider trait — `crates/ai-client/src/ai_provider.rs`

```rust
#[async_trait]
pub trait AiProvider: Send + Sync {
    fn id(&self) -> String;
    fn supported_protocols(&self) -> &[Protocol];
    async fn complete(&self, req: ChatRequest) -> Result<ProviderResponse>;
    async fn complete_stream(&self, req: ChatRequest) -> Result<ChatStream>;
    async fn list_models(&self) -> Result<Vec<WireModel>> { Ok(Vec::new()) } // GET /models
}
```

协议适配器边界：一个 provider 类型 = 一个协议实现；请求层（ModelRouter 校验、路由）不感知协议差异。
`list_models`（默认空）供 `/refresh` 自动更新模型清单用：openai/ollama 走 `GET {base}/models`
（OpenRouter 带 `metadata.context_length`）、anthropic 走同端点取 `display_name`、gemini 剥掉
`models/` 前缀。

### AiClient — `crates/ai-client/src/client.rs`

LLM 客户端本体。两张路由表 + 配置快照：
- `routes: HashMap<(ProviderId, Protocol), Arc<dyn AiProvider>>`——目录路径（`from_config` /
  `reload_config` 构建）：**每个 provider 按用到的协议各建一个适配器**（provider 默认协议 + 模型级覆盖
  协议去重），共享该 route 的 key/base_url/headers；`route_resolved` 按模型的有效协议取适配器
- `legacy: HashMap<ProviderId, Arc<dyn AiProvider>>`——手动 `register()` 路径（无目录，按 provider id 路由）
- `catalog: RwLock<Option<ModelRouter>>`——合并后的模型目录（resolve 校验）
- `configs: RwLock<HashMap<ProviderId, ProviderConfig>>`——源配置快照；`refresh_models` 把
  `GET /models` 发现的新 wire 名合并进来后经 reload 重建目录，返回新增模型列表供 Runtime 写回配置文件

职责：
- `resolve` / `route_resolved` / `route` / `complete` / `complete_stream`（请求分发）
- `list_models` / `default_selection` / `resolve_model`（目录与默认模型查询）
- `reload_config`（原子替换 router + routes，配置热更新入口）
- `refresh_models(provider)`（`/refresh` 后端：拉取并合并新模型，返回新增列表）
- 手动 `register()` 的 quickstart 路径（无目录回退行为）

### 刷新持久化 — `crates/runtime/src/config/persist.rs`

`/refresh` 写回配置文件用 `toml_edit` **定点插入**：`persist_new_models(path, provider, models)`
只往 `[providers.<provider>]` 的 `models` 表补缺失条目（wire 名做 key，含 `/` 等字符自动加引号），
注释/格式/其他段落原样保留；`models` 为内联表或 provider 段缺失时报错（不静默改坏配置）。
`Runtime::refresh_models` 流程：拉取合并 → 非空则写回（config_path 缺失时仅内存生效）。

### ModelRouter — `crates/ai-client/src/router.rs`

合并内置目录（`catalog.rs`，含 protocol/base_url/默认模型/模型清单/能力）与配置（字段级覆盖），
提供每 provider 默认模型（`default_model` > 内置默认 > 首个，BTree 确定性）、严格解析与带候选的诊断。
`resolve()` 在请求首个 await 前完成，`ChatRequest.resolved` 快照冻结——切换模型不影响在飞请求。
`ModelSpec.protocol` 是模型的有效协议（模型级覆盖 > provider 默认），请求据此分派适配器。

### 协议工厂 build_provider — `crates/ai-client/src/client.rs`

对应 DSH `PROTOCOLS` 表：`protocol` → 适配器构造器（含 headers/timeout 校验、凭据解析）。
未实现协议启动即报错（fail-fast），不注册会 panic 的 stub。

### reasoning — `crates/ai-client/src/reasoning.rs`

中性级别 `ReasoningEffort{Off,Low,Medium,High,Max}` + `ReasoningFormat`（openai-effort /
deepseek-thinking / deepseek-effort / anthropic-thinking / gemini-thinking）+
`ReasoningCapability{budget_tokens}`。
wire 映射由各协议 convert 决定（级别→wire 对照表见 README）。

### 配置热更新 — `crates/runtime/src/config/watcher.rs`

notify 监听配置文件父目录 + 100ms 防抖 → `Runtime::reload_config` → `AiClient::reload_config`
原子替换。新配置先整体校验，失败保留旧配置；在飞请求不受影响。

### SessionManager — `crates/runtime/src/session_manager.rs`

会话 CRUD + **会话级模型记忆**（`Session.model: Option<ModelSelection>`，读时回退全局默认）。
`/model` `/effort` 绑定当前会话，`/switch`/`/new` 恢复会话模型。

### 会话持久化 — `crates/storage`

`SessionManager` 可选挂载 `storage::SessionStore`，**写穿透**：每次变更（create / push / rename /
model / delete）先落盘成功再改内存，写失败内存不变、错误上抛（chat 流以 `ChatEvent::Error` 呈现，
不会静默丢数据）。
- 启用：`RuntimeBuilder::storage_dir(dir)` 或 `Runtime::from_config_persistent(path, dir)`
  （与 `state()` 互斥）；CLI/TUI/tauri 默认用 `storage::default_data_dir()`——`LLMN_DATA_DIR` 环境变量
  优先，缺省平台数据目录 + `llmn`（`dirs` crate）
- `FileSessionStore`：每会话一个 JSON 文件 `<dir>/<session-id>.json`（`SessionRecord` DTO，
  `version: 1`），**原子写**（同目录临时文件 + rename，崩溃不截断）；`load_sessions` 跳过非 `.json`
  文件，损坏文件启动即报错（fail-fast，错误带文件路径）
- 持久化格式：title / messages（复用 `Message` 的 serde，含 `reasoning` 思维链与
  `ContentPart::ToolCall`/`ToolResult` 工具调用/结果块，均无损往返）/ metadata / model
  （`ModelSelection`）/ created_at / updated_at；`Session ↔ SessionRecord` 转换在 `runtime::session`；
  旧文件零迁移（`reasoning` 字段 serde default）
- 思维链链路：流式 `ReasoningDelta` 边转发前端边收集，`Done` 时以 `Message::assistant_with_reasoning`
  存入；`web-server`/tauri 的 `get_messages` 把 `Message.reasoning` 映射进 `GuiMessage.reasoning`，
  Web 前端 `StoredMessage.reasoning` + ThinkingBlock 刷新后直接渲染（前端零改动）
- 工具调用：工具**执行**流程尚未实现（`crates/tools` 仍为 stub），本次仅落地消息表示与持久化——
  工具执行层实现时把 `ToolCall`/`ToolResult` 映射到各协议 wire（openai tool_calls / anthropic
  tool_use / gemini functionCall）即可，历史已无损可存
- `Runtime::list_sessions` 按 `updated_at` 倒序——重启后 CLI 自动恢复最近会话
- 存储层同步实现（小 JSON 写入微秒级，无需 async）；`SessionId` 已支持 serde（uuid serde feature）

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

`FeatureContext` 包含 `SessionManager`, `Arc<AiClient>`, `EventBus`。

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

方法内部：存 user message → 构建 ChatRequest → 调 AiClient → 发 ChatEvent → 存 assistant message。
使用 `mpsc::channel(64)` 实现背压，支持 `CancellationToken` 取消。

### ChatEvent — `crates/events/src/chat_event.rs`

```rust
pub enum ChatEvent {
    Started { message_id: MessageId },
    Delta { message_id: MessageId, content: String },
    ReasoningDelta { message_id: MessageId, content: String }, // 思维链，前端浅色显示、随消息持久化
    Finished { message_id: MessageId, usage: Option<Usage>, timings: Option<MessageTimings> },
    Error { message_id: MessageId, error: String },
    Cancelled { message_id: MessageId },
}
```

ChatEvent 定义在 `events` crate 中，前端只依赖 `events` + `common` 即可消费事件，无需依赖 `features/chat`。
流式链路：SSE `delta.reasoning_content` → `ChatChunk::ReasoningDelta` → `ChatEvent::ReasoningDelta`；
CLI 用 ANSI dim（`\x1b[2m`）打印，TUI 用 `CachedLine::Reasoning`（Gray + DIM）渲染。思维链**随 assistant
消息一起持久化**（`Message.reasoning`，见会话持久化小节）——刷新/重启后可再次显示；回传 provider 时
被剥离（`Message::to_wire()`），不污染请求。

### 用量与计时

- `common::Usage`：prompt/completion/total + `cached_tokens`（serde default 零迁移；各协议 convert 归一化：
  openai `prompt_tokens_details.cached_tokens`、anthropic `cache_read + cache_creation`、gemini
  `cachedContentTokenCount`、responses `input_tokens_details.cached_tokens`）
- `common::MessageTimings`：`ttft_ms`（请求开始→首个 token）、`reasoning_ms`（请求开始→首个正文，思考阶段）、
  `total_ms`；`ChatFeature` 在流循环内计时，`ChatChunk::Done { usage }` 时一并持久化到 assistant 消息
  （`Message.created_at/thinking_ms/usage/timings`）并随 `ChatEvent::Finished { usage, timings }` 下发
- 流式 usage 捕获：openai 独立尾块（空 choices + usage）/ finish_reason 块、responses `response.completed`、
  anthropic `message_delta`、gemini 末块 `usageMetadata` → `ChatChunk::Done { usage }`
- `Message::to_wire()`：role/content 保留、展示元数据（reasoning/created_at/thinking_ms/usage/timings）剥离，
  请求侧只用 wire 形式（`ChatFeature` 构建 `ChatRequest.messages` 时 map）
- GUI 透传：web-server/tauri 的 `GuiMessage` 增加 `createdAt/thinkingMs/usage/timings`（camelCase，
  skip_serializing_if None）；会话级聚合（状态栏）由前端从消息级数据求和

### Command — 前端命令

前端命令（/new, /switch, /model, /effort, /current, /models, /reload 等）在 frontend CLI/TUI 中定义；
`Runtime::select_model` / `resolve_model` / `session_model` / `reload_config` 提供后端校验与操作入口。

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
protocol = "openai"        # 可选：命中内置目录时可省略（回退内置协议）
api_key = "..."            # 或 { env = "VAR" }
base_url = "..."           # 可选：命中内置目录时可省略（回退内置 base_url）
default_model = "<model>"  # 可选：默认模型（key 或 wire 名），缺省回退内置默认/首个模型
headers = { X-Name = "v" } # 可选：附加到该 provider 每个请求的头
timeout_ms = 30000         # 可选：单请求超时毫秒（含流式读取）

[providers.<provider_id>.models.<model_id>]
model = "model-name"
display_name = "显示名称"
context_window = 131072    # 可选：能力信息（仅展示）
max_tokens = 16384         # 可选：能力信息（仅展示）
reasoning = { levels = [...], format = "openai-effort" }  # 可选能力声明
protocol = "openai_responses"  # 可选：模型级协议覆盖（缺省继承 provider 协议）
```

- **分层 `.env` 加载**（`runtime::config::env`，DSH `loadLayeredEnv` 同款）：`ConfigLoader::load`
  启动时把可选 `.env` 填进进程环境（只填缺失项，**环境变量优先、永不被覆盖**）——`{ env = "VAR" }`
  的 key 不必事先 export。层级：进程环境 > `<config 目录>/.env`（项目层，如 `config/.env`，
  gitignored）> `<llmn 数据目录>/.env`（用户层）；文件缺失静默，读失败/格式错误仅 stderr 警告
  不阻止启动。示例见 `config/.env.example`。语法：`KEY=VALUE`、`export ` 前缀、`#` 注释、
  单/双引号（双引号支持 `\n \r \t \" \\`）、无引号值支持 ` #` 行内注释

- **内置 provider 目录**（`ai_client::catalog::BUILTIN_PROVIDERS`）：deepseek / openai / gemini / kimi /
  zhipu / anthropic / xai / minimax / mimo / openrouter / opencode-zen / opencode-go / siliconflow /
  tokenrhythm / chatecnu。命中内置目录的 provider 可只写 `api_key`（protocol/base_url/模型清单/默认模型
  全部缺省回退）；未命中则必须显式 `protocol` + `base_url` + 至少一个模型。`opencode-zen` 是模型级协议
  覆盖的旗舰用例：同一 base_url 下按模型绑定 4 种协议（responses / anthropic / gemini / chat completions）。
- `reasoning.format`: `openai-effort` / `deepseek-thinking` / `deepseek-effort` / `anthropic-thinking` /
  `gemini-thinking`；levels: `off|low|medium|high|max`；`budget_tokens`（anthropic/gemini thinking 预算）；
  级别→wire 映射见 README 对照表——
  anthropic/gemini 逐级别预算 1024/4096/16384（可用 `budget_tokens` 覆盖），deepseek-thinking 只有开关语义（级别扁平），
  deepseek-effort（ECNU ecnu-max）thinking 开关 + reasoning_effort 强度（off 只发 thinking disabled 不带强度）
- 模型路由：`ai_client::ModelRouter` 合并内置表与配置（字段级覆盖），提供每 provider 默认模型、严格解析与带候选的诊断；
  请求经 `resolve` 校验后才 dispatch，`ChatRequest.resolved` 在首个 await 前快照冻结
- 协议实现：`ai_client::client::build_provider` 是协议工厂（对应 DSH `PROTOCOLS` 表）——`openai`/`openai_chat`
  （chat completions）、`openai_responses`（Responses API）、`anthropic`（Messages API，`x-api-key` 认证、
  `max_tokens` 必填、system 折叠进顶层字段、thinking 走 `budget_tokens`）、`gemini`（模型名在 URL 路径、
  contents/parts、assistant 角色 `model`、thinkingConfig.thinkingBudget）、`ollama`（OpenAI 兼容端点，复用
  chat completions 适配器）
- 流式：共享 `ai_client::protocols::sse::SseDataStream` 负责 SSE 分帧，各协议解析自己的事件词汇
  （responses: `response.output_text.delta`；anthropic: `content_block_delta`/`message_stop`；
  gemini: `candidates[].content.parts[]`）

## CLI 命令

| 命令 | 说明 |
|---|---|
| `/new` | 创建新会话并切换到该会话 |
| `/switch <id\|标题>` | 切换到指定会话，支持 UUID 或标题 |
| `/rename <标题>` | 重命名当前会话 |
| `/delete <id>` | 删除指定会话 |
| `/list` | 列出所有会话 |
| `/models` | 列出所有可用模型（含能力/reasoning 标记） |
| `/model <provider/model>` | 切换模型（支持裸模型名跨 provider 唯一匹配），绑定当前会话 |
| `/effort <off\|low\|medium\|high\|max>` | 设置当前模型的 reasoning effort（无参显示当前值，经路由校验，无效级别列出模型实际支持的级别），绑定当前会话 |
| `/current` | 显示当前模型的 provider / model / effort |
| `/reload` | 手动重载 `config/llmn.toml` |
| `/refresh <provider>` | 从该 provider 的 `GET /models` 拉取新模型，合并进内存目录并写回 config/llmn.toml（toml_edit 定点插入） |
| `/help` | 显示帮助 |
| `/quit` | 退出程序 |

会话与模型：`/model` `/effort` 的选择**绑定到当前会话**；`/switch`/`/new` 恢复该会话记住的模型
（无记忆回退全局默认）。TUI 状态栏常驻显示 `provider/model@effort`。

配置热更新：`runtime::config::watcher`（notify + 100ms 防抖）监听配置文档，变化后经
`Runtime::reload_config` 重新加载并原子替换（`AiClient::reload_config`）；新配置先整体校验，失败保留旧配置。
在飞请求不受影响（resolve 在首个 await 前冻结）。

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
  1. 在 `ai_client::config::Protocol` 加 variant
  2. 在 `ai_client::client::build_provider` 加 match arm（DSH `PROTOCOLS` 表风格）
  3. 实现 `AiProvider` trait（放在 `ai_client::protocols/<name>/`，流式复用 `protocols::sse::SseDataStream`）
  4. 若协议有专属 reasoning 格式，在 `reasoning::ReasoningFormat` 加 variant 并在 convert 里映射
- 添加新 Feature：
  1. 创建 `features/<name>/`，实现 `Feature` trait
  2. 暴露业务方法
  3. 前端通过 `Arc<Feature>` 直接调用
  4. 事件通过 `RuntimeEvent::Feature` 广播


## 2026-09-12 中断与工具执行补充（以此处为准）

- `Message.interruption: Option<Interruption>` 保存 `Cancelled` / `Failed(String)`；旧记录缺字段默认 None，`to_wire` 剥离此元数据。
- ChatFeature 先完成中断历史保存，再发送终止事件；取消、EOF、provider 错误、消费端断开均走这一收尾。部分正文/思考链保留，空响应也保存状态。构建下一次请求时过滤带 interruption 的消息。
- `SessionManager::finish_interrupted_turn` 以一次写穿透保存补齐未回答的 ToolCall（失败 ToolResult）和中断消息；写失败内存不变。此机制不提供崩溃检查点或工具副作用回滚。
- `Tool::run` 现在必须显式实现，并遵守非阻塞、可丢弃的异步执行契约。`run_sync` 仅作为内置短计算的辅助方法，不再自动回退调用。
- `ToolRegistry::run` 默认 30 秒协作式超时，捕获 unwind panic，返回 JSON 限制 64 KiB；`run_with_timeout` 可指定时限。不能抢占不 yield 的代码，也不能撤销已发生的外部副作用。Shell/文件等高风险工具仍需进程隔离和权限设计。
- Web/Tauri 历史接口透传 status/error；Web SSE 消费端断开时主动取消上游。


## 四项可靠性修复（2026-09-12，以此节为准）

- `FileSessionStore` 打开目录中的 `.llmn.lock` 并持有 OS 排他文件锁，第二个独立实例报错。不要删除运行中的锁文件。进程退出/被杀后 OS 自动释放，不使用容易遗留的 PID 锁文件。当前选择单写者模式，不支持多个独立 Runtime 同写目录。
- `Message.id: Option<MessageId>`：新消息创建 UUID，旧会话缺 ID 时由存储加载器补齐并立即原子写回；重复 ID 报错。`to_wire()` 剥离 ID。GUI 返回真实 ID，编辑用 userId，反馈用 messageId + revision；旧索引反馈 API 已移除。
- `Session.run` / `SessionRecord.run` 保存最近一次 `RunCheckpoint { id, status, partial }`。开始 run 与 user 消息同一事务，流式每 500ms 检查并保存变化的 draft，工具调用/结果沿用同步写穿透。成功答案和 Succeeded 同一事务。
- 启动加载发现 Running 时，使用 draft 补回中断消息、为未完成工具调用补失败结果并标记 Interrupted。恢复幂等，不自动重放工具，也不是从 provider token 游标续传。最多保留最近一个 RunCheckpoint，不是完整运行审计日志。
- 默认 echo/add 通过当前宿主程序的 `--llmn-tool-worker` 子进程执行；CLI/TUI/Web/Tauri 在加载配置/存储前处理 worker 入口。worker 只允许这两个编译内置工具，无 Shell/任意可执行路径入口，不继承 API Key 环境变量。
- Windows worker 加入 Job Object：关闭 Job 杀进程、最多 1 个进程、256 MiB committed memory、10 秒用户态 CPU；30 秒墙钟超时仍在 Registry，输入输出各 64 KiB。Job 配置/挂载失败时不发送任务、不回退进程内执行。
- 主机代码若确需信任进程内插件，必须显式使用 `Runtime::register_trusted_tool` / `ToolRegistry::register_trusted_in_process`，这会绕过进程隔离；模型不能修改注册表。不要把它用于不可信插件。
- 标准库 File::try_lock 要求 Rust 1.89+；本机以 Rust 1.97 验证。


## GUI 设置与流程验收（2026-09-13）

- Runtime 共用 `config::GuiConfig`，`gui_config`/`set_gui_config` 读取、校验并原子写入配置文件 `[gui]`。字段为 `temperature`、可选 `maxTokens`、`[gui.currentModel]`；会话恢复不覆盖默认模型。
- Web 新增 `PUT /api/config`，Tauri `set_config` 和供应商管理 IPC 已实现；Tauri 模型列表来自 Runtime 合并目录。Tauri 延迟初始化，配置/目录锁错误交给 GUI 展示与重试，支持 `LLMN_CONFIG`。
- Web Headers 必须通过 Headers.set 统一，避免同名不同大小写导致客户端头变成 `1, 1`。
- GUI 初始化/历史失败有重试入口；历史未加载成功或模型未配置时禁止发送；设置失败保留旧值并可重试。
- `python scripts/acceptance.py` 是离线真实链路验收（先 build web-server），每次使用独立临时配置/存储和本地模拟 provider。`--serve --dist <目录>` 保留服务供浏览器验收，Fixture 目录内的 STOP 文件用于停止。
- 本轮结果和未覆盖范围见 `docs/acceptance-2026-09-13.md`；不要把本地模拟、Tauri check 或编译通过说成真实厂商 API / 安装包验收通过。


## ecnu-max 真实接口验收补充（2026-09-13）

- OpenAI 兼容适配器同时接收 `reasoning_content` 和 `reasoning`（流式 delta 与非流式 message）。两字段同在时选非空 reasoning_content，否则回退 reasoning，避免重复显示与 serde duplicate field 错误。
- `scripts/live_acceptance.py` 是显式调用真实 API 的验收脚本，不属于离线测试；读取所选配置的 chatecnu 段与密钥环境变量，只写独立临时配置/会话。不会把密钥写进配置或报告。
- 真实 chatecnu/ecnu-max 已验收普通回答、思考、工具调用、取消后继续、重启历史以及 Web GUI。结果和边界见 `docs/live-acceptance-2026-09-13.md`。

## Tauri Windows 打包与首次启动（2026-09-13）

- Windows 发布包使用 MSVC 工具链；在 `frontends/tauri` 运行 `pnpm exec tauri build --target x86_64-pc-windows-msvc --bundles nsis --ci`。`RUSTUP_TOOLCHAIN=stable-x86_64-pc-windows-msvc` 可覆盖本机默认 GNU 工具链。
- Tauri 前端独立构建到 `frontends/tauri/dist`，每次清理该输出目录，配置 `frontendDist=../dist`，不改 Web 前端已跟踪的 dist。NSIS 工具缓存使用项目 target 目录（`useLocalToolsDir=true`）。
- Release 默认从 `<storage::default_data_dir()>/llmn.toml` 读取配置；缺失时只创建一次初始 `[providers]` 文件。显式 `LLMN_CONFIG` 不存在仍报错；debug 构建可优先使用项目配置。
- 空 providers 是有效的首次启动/删除最后一个供应商状态；模型解析仍拒绝无模型的聊天请求。不要用示例密钥或开发配置填充安装包。
- 桌面实机验收记录与未覆盖范围见 `docs/tauri-acceptance-2026-09-13.md`。

## 桌面图标、窗口与独立设置页（2026-09-13）

- Web 和 Tauri 共用 `frontends/web`；`lib/desktop.ts` 在初始化前识别桌面壳。Tauri 使用无原生标题栏窗口，`WindowControls` 独立于后端启动状态。窗口 IPC 权限在 capabilities/default.json。
- 设置路由为 `#/settings/models|generation|appearance|about`，聊天为 `#/chat`。切换页面保留 ChatView/InputBar 和设置表单，Header 需卸载以移除菜单 portal。
- `build.rs` 必须跟踪 `icons` 目录，避免增量构建只更新窗口图标却保留 EXE 的旧 Windows 资源。安装器/卸载器使用同一 ICO。
- 发布后运行 `scripts/verify-windows-icons.ps1` 核对实际 EXE 和安装器资源；可用 `-ArtifactPath` 检查已安装 EXE、卸载器、MSI 解包 EXE。界面截图、文件存在或 NSIS 返回 0 均不足以证明安装图标或覆盖安装正确。
- 本轮验收与边界见 `docs/desktop-settings-acceptance-2026-09-13.md`；干净系统、缺 WebView2、MSI 系统级安装仍未验收。

## 窗口与模型入口修整（2026-09-13）

- 产品名与 Rust 二进制名为 `LLM-Nest`，内部 Cargo 包名仍为 `tauri-frontend`。Windows 产物为 `LLM-Nest.exe` / `LLM-Nest_0.1.0_x64-setup.exe`；保持 `com.llmnest.tauri` identifier 和 `llmn` 数据目录。旧 NSIS 产品名到新名称的升级需要一次保留数据的安装迁移，本机已完成，没有通用自动迁移钩子。
- 无边框窗口 `shadow=false`、WebView `transparent=true`，CSS 页面不透明。主题切换只能用 `@tauri-apps/api/window` 的 `Window.setBackgroundColor` 同步原生底色，不要用会覆盖 WebView 透明背景的 `WebviewWindow` 同名 API。关闭系统阴影也会移除 Windows 11 系统圆角。
- 模型与思考菜单空状态通过 `ModelSetupOptions` 提供说明和键盘可达的模型设置入口；无效思考能力不显示为可用级别。输入框底部使用 `clampEffort` 显示实际有效级别。
- 修改、最终包哈希与验收边界见 `docs/window-polish-acceptance-2026-09-13.md`。尺寸调整后的画面已检查，整个高速拉伸过程和垂直贴边黑框仍需用户复核。

## Windows 原生圆角与边框（2026-09-13）

- 用户已确认上一版快速缩放和上下贴边的黑影消失。保留 `shadow=false`、透明 WebView、随主题同步的原生底色，不重新启用 tao 的阴影边缘布局。
- `window_frame.rs` 使用 Windows 11 的 `DWMWA_WINDOW_CORNER_PREFERENCE` 与 `DWMWA_BORDER_COLOR` 独立请求系统圆角和细描边；圆角在最大化/贴边时由 DWM 决定，不使用 `SetWindowRgn`、CSS 圆角裁切、透明外边距或自建阴影窗口。
- `set_window_appearance` 仅同步外观，不依赖聊天后端初始化。边框随应用主题和原生焦点变化；Windows 10 不支持这些属性时返回 false，由不拦截鼠标的 CSS 内描边回退，最大化/全屏时隐藏回退描边。
- 不把 DWM API 返回成功等同于所有机器的视觉验收。原生圆角与测试范围见 `docs/native-window-frame-2026-09-13.md`。
