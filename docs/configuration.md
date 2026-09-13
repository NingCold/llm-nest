# 配置与模型路由

[返回首页](../README.md) · [开发指南](development.md)

## 配置放在哪里

| 入口 | 配置文件 |
| --- | --- |
| CLI / TUI | 当前工作目录的 `config/llmn.toml` |
| Web Server | `LLMN_CONFIG` 指定文件，未设置时查找项目配置 |
| Tauri 开发版 | `LLMN_CONFIG` 优先，其次项目配置，再回退用户配置 |
| Tauri 安装版 | `LLMN_CONFIG` 优先，否则使用默认数据目录内的 `llmn.toml`；首次创建空 providers |

默认数据目录来自系统用户数据目录：Windows 通常是 `%APPDATA%/llmn`，Linux 通常是 `~/.local/share/llmn`（遵循 XDG 数据目录）。`LLMN_DATA_DIR` 可以覆盖它。不要让两个独立进程同时写入同一目录。

## 最小配置

```toml
[providers.deepseek]
api_key = { env = "DEEPSEEK_API_KEY" }
```

这里使用内置目录的协议、地址和模型默认值。设置变量的方式：

```powershell
# PowerShell，当前进程
$env:DEEPSEEK_API_KEY = "填入自己的密钥"
```

```bash
# Linux shell，当前进程
export DEEPSEEK_API_KEY="填入自己的密钥"
```

也可以创建被 Git 忽略的 `config/.env`：

```dotenv
DEEPSEEK_API_KEY=填入自己的密钥
```

读取优先级：进程环境 > 配置文件同目录的 .env > 默认数据目录的 .env。只补充尚未设置的变量；不要把密钥写进 README、验收日志或 CI。

GUI 也支持直接保存 API Key，目前写入的是本地配置文件，并未使用系统凭据库加密。

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

# 同一网关上的另一个模型可改用不同协议
[providers.gateway.models.responses]
model = "填写支持 Responses 的模型 ID"
protocol = "openai_responses"
```

地址和模型名必须来自你使用的服务商。示例中的 example 域名不可用于真实请求。provider 的键是配置里的供应商 ID；模型的键是本地别名，`model` 是实际发送的模型名。

| protocol | 对应接口 |
| --- | --- |
| `openai` | Chat Completions |
| `openai_responses` | Responses API |
| `anthropic` | Messages API |
| `gemini` | Gemini generateContent / streamGenerateContent |
| `ollama` | Ollama 的 OpenAI 兼容接口 |

可选的 `headers`、`timeout_ms` 作用于该供应商的请求。模型可以声明 `context_window`、`max_tokens` 和 `reasoning`。协议实现支持不等于所有第三方端点、所有模型能力都已经实测。

### Ollama

先在 Ollama 中准备模型，再把 `model` 改成当地实际可用的模型 ID：

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

LLM-Nest 使用 `off / low / medium / high / max` 中性级别，但只允许选择模型声明支持的级别。没有能力声明不意味着支持所有级别。

```toml
[providers.gateway.models.chat.reasoning]
levels = ["off", "low", "high"]
format = "deepseek-effort"
```

支持的 format：`openai-effort`、`deepseek-thinking`、`deepseek-effort`、`anthropic-thinking`、`gemini-thinking`。部分格式支持 `budget_tokens`。具体 wire 映射以 [reasoning 类型](../crates/ai-client/src/reasoning.rs) 和 [协议转换实现](../crates/ai-client/src/protocols) 为准，不能将 UI 的“高”理解成各供应商相同的计算预算。

## 会话、热更新和常用命令

模型与思考强度绑定到当前会话。没有会话选择时使用全局默认值；GUI 的全局生成设置保存在 `[gui]`。

| 命令 | 用途 |
| --- | --- |
| `/new` / `/list` / `/switch <id或标题>` | 新建、列出、切换会话 |
| `/rename <标题>` / `/delete <id>` | 修改或删除会话 |
| `/models` / `/current` | 查看可选模型或当前选择 |
| `/model <provider/model>` | 为当前会话选模型 |
| `/effort <级别>` | 调整当前会话的思考强度 |
| `/reload` | 校验并重新加载配置 |
| `/refresh <provider>` | 拉取模型目录，将新条目定点补入配置 |
| `/help` / `/quit` | 帮助与退出 |

配置文件变化会自动触发重载。新配置非法时保留旧配置；在飞请求继续使用原来的快照。`/refresh` 不会凭空创建一个未配置凭据的供应商。
