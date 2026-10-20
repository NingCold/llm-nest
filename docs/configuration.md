# 模型配置

[使用指南](usage.md) · [开发指南](development.md) · [全部文档](README.md)

日常使用从 **设置 → 模型服务** 添加服务和密钥，再到 **我的账号 → AI 角色管理** 为每位 AI 选择模型。本页说明配置文件、环境变量和自建服务的写法。

## 配置和数据的位置

| 运行方式 | 配置文件选择顺序 |
| --- | --- |
| Web | `LLMN_CONFIG` 指定的文件，否则查找项目 `config/llmn.toml` |
| 桌面开发版 | `LLMN_CONFIG`，项目 `config/llmn.toml`，最后是数据目录内的 `llmn.toml` |
| 桌面安装版 | `LLMN_CONFIG`，否则使用数据目录内的 `llmn.toml`，首次自动创建 |

默认数据目录为用户主目录下的 `.llmn`：Windows 是 `%USERPROFILE%/.llmn`，Linux 是 `~/.llmn`。聊天保存在其中的 SQLite 数据库中。设置 `LLMN_DATA_DIR` 可更换数据目录，也会改变桌面默认配置位置；`LLMN_CONFIG` 只指定配置文件，优先级更高。

在 **设置 → 配置 → 配置与数据** 查看实际路径。桌面版可以打开目录；Web 的文件位于服务器，浏览器只显示路径。

### 旧默认目录

首次启动时，如果新目录不存在且没有设置 `LLMN_DATA_DIR`，应用会从旧系统数据目录的 `llmn` 子目录复制配置、`.env` 和旧聊天 JSON。Windows 旧目录通常是 `%APPDATA%/llmn`，Linux 通常是 `~/.local/share/llmn`（遵循 XDG 设置）。

复制保留源文件；新目录已存在时不会合并。旧目录正被应用占用时，需要先关闭旧实例。复制后的 JSON 还需在设置中[导入为历史窗口](usage.md#导出和导入聊天记录)。

## 最小配置与密钥

空文件、只有注释或下面的空表都能启动，之后可以在界面添加服务：

```toml
[providers]
```

内置服务可省略协议、地址和模型列表，例如：

```toml
[providers.deepseek]
api_key = { env = "DEEPSEEK_API_KEY" }
```

在配置文件同目录创建 `.env`，写入：

```dotenv
DEEPSEEK_API_KEY=填入自己的密钥
```

也可以在启动应用的终端设置：

```powershell
# PowerShell
$env:DEEPSEEK_API_KEY = "填入自己的密钥"
```

```bash
# Linux shell
export DEEPSEEK_API_KEY="填入自己的密钥"
```

变量优先级为 **进程环境 > 配置文件同目录的 .env > 默认数据目录的 .env**，只补充尚未设置的变量。GUI 直接保存的 API Key 目前写入本地配置文件，没有使用系统凭据库加密。`llmn.toml`、`.env` 和密钥应留在本机。

## 同步公共模型目录

在 **设置 → 模型服务 → 同步模型目录**，可从 [Pi 公共目录](https://github.com/earendil-works/pi) 更新模型 ID、名称、上下文容量、输出上限和图片能力。同步不需要 Pi 密钥，也不会调用模型生成内容；目前只在点击时联网。

缓存位于实际数据目录的 `cache/catalog.json`，重启后继续使用。优先级为 **用户显式配置 > 同步目录 > 内置目录**。同步保留配置文件、默认模型和已配置的服务，模型列表也不代表你的账号有调用权限。

当前适配 Anthropic、OpenAI、Gemini、DeepSeek、Kimi、MiMo、MiniMax、xAI、OpenRouter、OpenCode Zen/Go 的对应来源。区域、订阅或协议不匹配的条目会跳过；其他服务仍可使用内置目录或手动配置。协议和思考能力仍取决于本项目适配器。

界面显示同步时间、变更数量和失败提示。某个来源失败时保留它的旧条目，本地保存失败时保留旧缓存；已经开始的请求继续使用原配置。

目录接口为 `https://pi.dev/api/models/providers/<provider>`，结构参考上游提交 `e4c75a73222ae2c72abb5f5314fa35ee8effc508`。该数据由第三方汇总，许可见 [Pi MIT License](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/LICENSE)。

## 图片输入能力

在 **设置 → 模型服务 → 模型图片能力** 查看声明及来源：人工设置、同步目录、内置目录或未知。默认跟随目录；自建或中转服务能力不同时，可以改为“支持图片”或“不支持图片”，也能恢复“跟随目录”。

人工设置优先级最高，重新同步和重启后仍保留。未知允许尝试；这些声明没有通过自动调用模型验收识图效果。

手动配置时，在模型下添加布尔字段，删除字段即可恢复跟随目录：

```toml
[providers.gateway.models.chat]
model = "填写服务商实际的模型 ID"
image_input = true
```

设置影响之后开始的请求。同一模型被多个 AI 使用时，共用这项声明。明确不支持时，聊天图片仍保存，但模型不会收到包含图片的请求；检查范围包括请求所需的历史和群引用。发送和处理图片见[使用指南](usage.md#发送图片和文件)。

## 自定义供应商

```toml
[providers.gateway]
protocol = "openai"
base_url = "https://your-gateway.example/v1"
api_key = { env = "GATEWAY_API_KEY" }
default_model = "chat"
timeout_ms = 120000

[providers.gateway.models.chat]
model = "填写服务商实际的模型 ID"
display_name = "我的模型"

# 同一服务上的另一个模型可使用不同协议
[providers.gateway.models.responses]
model = "填写支持 Responses 的模型 ID"
protocol = "openai_responses"
```

把示例地址和模型名替换为实际值。`gateway` 是配置中的服务 ID，`chat` 是本地模型别名，`model` 是发送给服务商的模型名。自定义供应商需要声明协议、地址和模型列表。

| protocol | 接口 |
| --- | --- |
| `openai` | Chat Completions |
| `openai_responses` | Responses API |
| `anthropic` | Messages API |
| `gemini` | Gemini generateContent / streamGenerateContent |
| `ollama` | Ollama 的 OpenAI 兼容接口 |

供应商可设置 `headers`、`timeout_ms`；模型可设置 `context_window`、`max_tokens` 和 `reasoning`。协议适配范围见[模型执行设计](design/execution.md)。

### Ollama

先在本机 Ollama 中准备模型，再把实际模型 ID 填入：

```toml
[providers.local]
protocol = "ollama"
base_url = "http://localhost:11434/v1"
api_key = ""
default_model = "local"

[providers.local.models.local]
model = "替换为本地模型 ID"
```

## 思考强度

界面统一使用 `off / low / medium / high / max`，可选项以模型声明为准。自建服务可配置：

```toml
[providers.gateway.models.chat.reasoning]
levels = ["off", "low", "high"]
format = "deepseek-effort"
```

支持的格式为 `openai-effort`、`deepseek-thinking`、`deepseek-effort`、`anthropic-thinking`、`gemini-thinking`，部分格式支持 `budget_tokens`。各服务对相同强度的实际预算不同，字段定义见 [reasoning.rs](../crates/ai-client/src/reasoning.rs)。

## AI 账号参数与配置更新

AI 的模型、思考强度、温度和输出上限在角色管理中保存，属于该账号。温度留空使用模型服务默认值；输出上限留空由意识系统分配。显式填写的上限按原值校验和预留。

旧 TOML `[gui]` 参数已不再使用，升级时仍保留原字段。供应商的 `default_model` 用于模型目录解析，不会替每位 AI 自动选择模型。

配置文件变化后会整体校验；有效配置应用于新请求，校验失败则保留旧配置。已经开始的请求继续使用开始时的配置。
