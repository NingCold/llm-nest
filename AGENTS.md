# AGENTS.md — LLM Nest

## 项目概述

基于 Rust 的模块化 AI 应用平台。CLI 名 `llmn`。

## 架构

```
Apps / Frontends (Chat CLI / TUI / Web / ...)
    ↓
Features (chat / translate / ...)  ← 纯业务逻辑, 无 UI
    ↓
Core Runtime (Session / Router / EventBus / PluginManager / ...)
    ↓
Providers / Tools / Storage
    ↓
External Systems (LLM APIs / Filesystem / ...)
```

## 目录结构

| 路径 | 角色 |
|---|---|
| `crates/common/` | 公共数据结构（Message, ChatRequest, SessionId, config 类型） |
| `crates/provider/` | Provider trait + OpenAI 实现 + Anthropic 骨架 |
| `crates/runtime/` | 核心运行时：Session, EventBus, Router, Command |
| `crates/storage/` | 预留，当前空 stub |
| `crates/tools/` | 预留，当前空 stub |
| `features/chat/` | 聊天业务逻辑（library），通过 `ChatService` 暴露 API |
| `frontends/cli/` | CLI 前端，仅处理用户交互 |

## 依赖关系

`common` ← `provider` ← `runtime` ← `chat` ← `cli`

核心规则：
- Core（runtime）不知道具体实现，只面向 `dyn Provider` / `dyn Plugin`
- Features 之间不能直接依赖
- Frontends 只依赖 Features，不直接依赖 Runtime

## 设计要点

- **Provider trait** — `crates/provider/src/traits.rs`，`ProviderFactory` 根据 `Protocol` 枚举创建具体实现
- **EventBus** — `tokio::sync::broadcast`，Runtime 通过事件对外通信
- **ChatService** — `features/chat/src/service.rs`，封装所有聊天业务逻辑；通过 `mpsc` 桥接将 Runtime 事件转为 `ChatEvent`，对外暴露 `Stream<Item = ChatEvent>`
- **ChatEvent** — `features/chat/src/event.rs`，前端关心的 5 个变体：`ResponseStarted` / `ResponseDelta` / `ResponseFinished` / `Info` / `Error`
- **Plugin trait** — 在 `plugin_manager.rs` 中定义，但**尚未集成进 Runtime**
- **RuntimeBuilder** — 支持 config 构建或注入 mock 用于测试
- `SessionId` / `RequestId` 定义在 `common`，提供 `Display` + `FromStr`，前端可 parse/display 但不能通过 `new()` 构造（`#[doc(hidden)]`）
- Rust Edition 2024

## 构建与运行

```bash
cargo build                    # 构建全部
cargo run -p cli               # 运行 CLI（可执行文件 llmn）
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

## 注意事项

- `config/config.toml` 含 API Key，**不要提交到仓库**
- 没有 CI 配置，没有 formatter/linter 配置文件 — 使用 `cargo fmt` 和 `cargo clippy` 默认行为
- 所有 crate 使用 `[dependencies]` 的 `workspace = true` 统一版本管理
- 添加新 Provider：1）在 `Protocol` 加 variant 2）在 `ProviderFactory::create()` 加 match arm 3）实现 `Provider` trait
- 添加新 Feature：创建 `features/<name>/`，暴露 Service API；`frontends/cli` 等可通过 `cargo add` 引用