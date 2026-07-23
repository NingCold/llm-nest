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

创建 `config/llmn.toml`：

```toml
[providers.chatecnu]
protocol = "openai"
api_key = "API-KEY"
base_url = "https://chat.ecnu.edu.cn/open/api/v1/"

[providers.chatecnu.models.ecnu-max]
model = "ecnu-max"
display_name = "DeepSeek-V4-Flash"
```

## 版本状态

- 当前版本：V0.1
- 已完成：Workspace 骨架、Provider Trait + OpenAI 实现、Core Runtime
- 全部静态链接 (V0.1)