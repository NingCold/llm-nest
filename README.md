# LLM Nest

LLM Nest 是基于 Rust 编写的一个模块化的 AI 平台，可以支持聊天、编码和构建智能工作流

- **品牌名**：**LLM Nest**
- **仓库名**：`llm-nest`
- **CLI**：`llmn`

## 设计原则

1. 万物皆插件
2. Core 只负责提供软件基建，永远不知道具体实现
3. APP 之间不能直接依赖

## 软件架构

```
Apps (Chat / Code / CLI / Web / Mobile / Future)
    ↓
Core Runtime (Session / Router / Context / EventBus / PluginManager / Scheduler / Config)
    ↓
Providers / Tools / Storage
    ↓
External Systems (LLM APIs / Filesystem / Git / Browser / DB)
```

## 目录结构

```
llm-nest/
├── crates/
│   ├── common/        → 公共数据结构
│   ├── provider/      → 模型供应协议
│   ├── runtime/       → 程序运行时
│   ├── storage/       → storage (V0.1 预留)
│   └── tools/         → tools (V0.1 预留)
├── features/
│   ├── chat/          → 聊天功能
│   ├── translate/     → 翻译功能（尚未实现）
|   ...                → 更多功能（尚未实现）
├── frontends/
│   ├── cli/           → cli 应用（暂未实现）
|   ...                → 更多前端（暂未实现）
├── .cargo/config.toml  — 使用 GNU 工具链 (MinGW)
└── Cargo.toml          — workspace 根
```

## 构建

```bash
# 构建全部
cargo build

# 构建并运行 Chat (目前暂时集成在chat功能中)
cargo run -p chat
```

## 环境配置

通过 `config/llmn.toml` 配置：

```toml
[providers.chatecnu]
protocol = "openai"                       # 可选：命中内置目录时可省略
api_key = "API-KEY"                       # 或 api_key = { env = "MY_ENV_VAR" }
base_url = "https://chat.ecnu.edu.cn/open/api/v1/"   # 可选：命中内置目录时可省略
default_model = "ecnu-max"                # 可选：默认模型（key 或 wire 名），缺省回退内置默认/首个模型
# headers = { X-Custom = "value" }        # 可选：附加到该 provider 每个请求的头
# timeout_ms = 30000                      # 可选：单请求超时毫秒（含流式读取）

[providers.chatecnu.models.ecnu-max]
model = "ecnu-max"
display_name = "DeepSeek-V4-Flash"
# context_window = 131072                 # 可选：能力信息（仅展示）
# max_tokens = 16384                      # 可选：能力信息（仅展示）
reasoning = { levels = ["off", "low", "high", "max"], format = "deepseek-effort" }
# protocol = "openai_responses"           # 可选：模型级协议覆盖（缺省继承 provider 协议）
```

**内置 provider 目录**（`ai_client::catalog`）：deepseek / openai / gemini / kimi / zhipu / anthropic /
xai / minimax / mimo / openrouter / opencode-zen / opencode-go / siliconflow / tokenrhythm / chatecnu 等。
命中内置目录的 provider 可以**只写 api_key**，protocol / base_url / 模型清单 / 默认模型全部缺省回退：

```toml
[providers.deepseek]
api_key = { env = "DEEPSEEK_API_KEY" }
```

未命中内置目录的自定义 provider 必须显式写 `protocol` + `base_url` + 至少一个模型。

- `providers.<id>` 的 dict key 就是一个模型路由（route），任意自定义 id 均可。支持的协议：
  - `openai` / `openai_chat`：OpenAI 兼容 chat completions（`{base}/chat/completions`）
  - `openai_responses`：OpenAI Responses API（`{base}/responses`，消息走 `input`，上限 `max_output_tokens`，reasoning 走 `reasoning: {effort}`）
  - `anthropic`：Anthropic Messages API（`{base}/messages`，认证 `x-api-key` + `anthropic-version`，`max_tokens` 必填——取自请求选项、模型声明的 `max_tokens`，缺省 4096；system/developer 消息折叠进顶层 `system` 字段）
  - `gemini`：Gemini API（模型名在 URL 路径 `{base}/models/{model}:generateContent` / `:streamGenerateContent?alt=sse`，消息走 `contents`/`parts`，assistant 角色为 `model`，system 走 `systemInstruction`）
  - `ollama`：Ollama 自带 OpenAI 兼容端点，直接复用 chat completions 适配器（`base_url = "http://localhost:11434/v1"`）
- **模型级协议覆盖**：`models.<id>.protocol` 让单个模型改用其它协议发送，provider 内共享 key/base_url/
  headers——适配"同端点多协议"的网关（如 opencode zen 一家同时暴露 chat completions / responses）。
  请求分派按模型的有效协议走对应适配器（`AiClient` 按 `(provider, protocol)` 建路由表）。
- `reasoning.format`：`openai-effort`（chat wire: `reasoning_effort`；responses wire: `reasoning: {effort}`）、
  `deepseek-thinking`（wire: `thinking`）、`deepseek-effort`（wire: `thinking` + `reasoning_effort`，
  ECNU ecnu-max 风格）、`anthropic-thinking`（wire: `thinking: {type, budget_tokens}`）、
  `gemini-thinking`（wire: `generationConfig.thinkingConfig.thinkingBudget`）。
  中性级别（off/low/medium/high/max）到各协议 wire 的映射：

  | `/effort` | openai-effort (chat) | openai-effort (responses) | deepseek-thinking | deepseek-effort | anthropic-thinking | gemini-thinking |
  |---|---|---|---|---|---|---|
  | `off` | 不输出 | 不输出 | `thinking:{type:"disabled"}` | `thinking:{type:"disabled"}` | `thinking:{type:"disabled"}` | `thinkingBudget: 0` |
  | `low` | `reasoning_effort:"low"` | `reasoning:{effort:"low"}` | `thinking:{type:"enabled"}` | `enabled` + `reasoning_effort:"low"` | `enabled, budget_tokens:1024` | `thinkingBudget: 1024` |
  | `medium` | `"medium"` | `effort:"medium"` | `enabled` | `enabled` + `"medium"` | `enabled, budget_tokens:4096` | `4096` |
  | `high` | `"high"` | `effort:"high"` | `enabled` | `enabled` + `"high"` | `enabled, budget_tokens:16384` | `16384` |
  | `max` | `"max"` | `effort:"max"` | `enabled` | `enabled` + `"max"` | `enabled, 4096*` | `4096*` |

  anthropic/gemini 的逐级别预算可用能力声明里的 `budget_tokens` 统一覆盖（缺省按上表）。`*`：max 级别没有专属
  默认预算，缺省回退 4096。deepseek-thinking 只有开关语义，low/medium/high/max 都映射为 `enabled`；
  deepseek-effort（ECNU ecnu-max）除 thinking 开关外还带 `reasoning_effort` 强度（官方文档称强度仅对开启
  思考模式的请求生效，故 `off` 只发 `thinking:{type:"disabled"}`，不带强度）。
- 模型与 effort：CLI 与 TUI 均支持 `/models`（列出合并目录）、`/model <provider/model>`（切换，也支持裸模型名跨
  provider 唯一匹配）、`/effort <off|low|medium|high|max>`（独立设置当前模型的 reasoning effort，无参显示当前值，
  切换前经路由校验该模型是否支持，无效级别会列出该模型实际支持的级别）、`/current`（显示当前 provider / model / effort）；
  未知 provider/model/effort 会报错并列出候选。
- **思维链显示**：模型返回的 reasoning_content（DeepSeek/ecnu-max/kimi/glm 等）会实时显示——CLI 用浅色（dim）
  打印，TUI 用浅色+斜体渲染；思维链随消息持久化（刷新后可再次显示），但不会回传给 provider。
- **用量与计时（后端提供，前端渲染）**：`/models` 显示每个模型的能力时带上 wire 格式
  （如 `[reasoning: off/low/high/max · deepseek-effort]`）；每次回复后端持久化并随事件下发：
  - `Usage`：prompt/completion/total + **cached_tokens**（缓存命中，各 provider 归一化：openai
    `prompt_tokens_details.cached_tokens`、anthropic `cache_read + cache_creation`、gemini
    `cachedContentTokenCount`；缓存命中率 = cached / (prompt + cached)，前端算）
  - `MessageTimings`：`ttft_ms`（请求开始→首个 token）、`reasoning_ms`（请求开始→首个正文，即思考阶段）、
    `total_ms`（请求开始→流结束）
  - 消息级 `created_at` / `thinking_ms` / `usage` / `timings` 随 assistant 消息持久化，
    `ChatEvent::Finished` 实时携带 `usage` + `timings`；GUI 消息（web-server/tauri）原样透传，
    会话级聚合（状态栏的轮数/LLM 用时/平均速度等）由前端从历史+实时数据求和
- **附件无损持久化**：持久化格式与 wire 格式分离——`ContentPart::Image/File` 的二进制以 base64 直存
  （`{type:"image"/"file", mime, data}`），刷新/重启后无损还原；`Message::to_wire_value()` 才转成
  provider 的 `image_url` data URL（旧持久化文件里的 `image_url` 块也会被还原）；GUI 消息带
  `attachments`（dataUrl 数组）供前端渲染缩略图/附件回显
- **feedback 持久化**：`Message.feedback`（up/down，serde default 零迁移）+ GUI 透传；
  更新接口：`PATCH /api/sessions/{id}/messages/{idx}`（body `{"feedback":"up"|"down"|null}`）/
  tauri `set_message_feedback(sessionId, idx, feedback)`（idx 与 GUI 消息 id `m-{i}` 的索引一致，
  按 user/assistant 过滤后计数）
- **模型清单自动刷新**：`/refresh <provider>` 调用该 provider 的 `GET /models` 拉取最新模型，合并进内存目录
  并**写回 config/llmn.toml**（toml_edit 定点插入，只往该 provider 的 models 表补缺失条目，注释/格式/其他
  内容不动）；`/refresh` 无新模型时输出提示。底层接口：`Runtime::refresh_models`（返回新增模型列表）。
  若 provider 刚写进 `config/llmn.toml`、内存快照还没有（热更新未触发），`/refresh` 会先自动重载配置再拉取；
  但 provider 的注册信息（protocol/base_url/api_key）必须来自配置——`/refresh` 只负责模型清单，不凭空注册 provider。
- **会话记住模型**：`/model` 与 `/effort` 的选择绑定到当前会话；切换会话（`/switch`/`/new`）时恢复该会话记住的
  模型，新会话回退全局默认；TUI 状态栏常驻显示当前会话的 `provider/model@effort`。
- 配置在启动时整体校验（未知 `default_model`、空 `reasoning.levels`、非法 header 名、`timeout_ms = 0` 都会
  在启动阶段失败并指名）。

### 配置热更新

- 运行中修改 `config/llmn.toml` 会被自动识别（100ms 防抖）并热更新：CLI/TUI 都会在文件变化后重新
  加载并原子替换 provider 路由，成功时提示"已热更新"，失败时**保留旧配置继续服务**并显示原因。
- 也可以手动触发：`/reload`。
- 热更新同样走启动时的完整校验（未知 `default_model`、空 `reasoning.levels`、未设置的环境变量 key、
  非法 header、`timeout_ms = 0` 等都会拒绝这次更新，运行中的配置不受影响）。
- 在飞请求不受影响：路由解析在请求的第一个 await 前冻结，配置替换只影响下一个请求。

## 版本状态

- 当前版本：V0.1
- 已完成：Workspace 骨架、Provider Trait + OpenAI 实现、Core Runtime
- 全部静态链接 (V0.1)