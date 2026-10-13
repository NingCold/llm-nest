# 配置与模型路由

[返回首页](../README.md) · [开发指南](development.md)

## 配置放在哪里

| 入口 | 配置文件 |
| --- | --- |
| Web Server | `LLMN_CONFIG` 指定文件，未设置时查找项目配置 |
| Tauri 开发版 | `LLMN_CONFIG` 优先，其次项目配置，再回退用户配置 |
| Tauri 安装版 | `LLMN_CONFIG` 优先，否则使用默认数据目录内的 `llmn.toml`；首次创建空 providers |

桌面安装版的默认配置/数据目录统一为用户主目录下的 `.llmn`：

- Windows：`C:/Users/<用户名>/.llmn/llmn.toml`（即 `%USERPROFILE%/.llmn/llmn.toml`）。
- Linux：`~/.llmn/llmn.toml`，不再随 `XDG_DATA_HOME` 改变默认位置。

会话 JSON 与桌面默认配置放在同一目录。`LLMN_DATA_DIR` 仍可覆盖数据目录，并改变桌面默认配置的位置；`LLMN_CONFIG` 单独覆盖桌面/Web 的配置文件，优先级更高。源码运行的项目配置优先规则见上表。

桌面版进入 **设置 → 配置 → 配置与数据**，可查看实际配置文件和会话目录，点击“打开配置目录”或“打开数据目录”在文件管理器中打开。自定义路径也会按实际位置显示。Web 版文件位于服务所在机器，浏览器不提供打开服务器文件夹的按钮。

### 从旧默认目录迁移

默认 `.llmn` 不存在且未设置 `LLMN_DATA_DIR` 时，启动会从旧系统数据目录的 `llmn` 子目录复制 `llmn.toml`、`.env` 和会话 JSON。Windows 旧目录通常为 `%APPDATA%/llmn`；Linux 通常为 `~/.local/share/llmn`（旧目录仍遵循 XDG）。复制完成后才启用新目录，原文件保留，不复制运行锁和临时文件。

旧目录被运行中的应用占用时会报错，请关闭旧实例后重试。新目录已经存在时不自动合并或覆盖，避免把旧记录重新导入；需要手动合并时应先备份。迁移后使用新版继续工作，旧目录只是保留副本，不会自动同步后续改动。不要让两个独立进程同时写入同一目录。

## 最小配置

没有模型服务时，空文件、只有注释或 `[providers]` 空表均可启动。桌面后端随应用在同一进程内初始化，无需另开 Web Server；配置语法、字段类型或数据目录错误仍会显示具体原因及重试入口。

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

## 同步公共模型目录

桌面和 Web 版进入 **设置 → 模型 → 同步模型目录**，按需从 [Pi](https://github.com/earendil-works/pi) 的公共目录下载模型 ID、显示名称、上下文容量和输出上限。无需提供 Pi 密钥，也不会调用模型生成内容。界面显示上次同步时间、变更数量，以及未适配条目或网络失败的提示。

- 缓存写入实际数据目录的 `cache/catalog.json`，默认是 `~/.llmn/cache/catalog.json`。Web 版写在服务器；桌面可从 **设置 → 配置 → 打开数据目录** 找到。重启后继续使用缓存。
- 优先级为 **用户显式配置 > 同步目录 > 内置目录**。同步本身不修改 `llmn.toml` 或源码 `catalog.rs`，不更换默认模型、不添加供应商账号、不删除历史模型。
- 首版适配 Anthropic、OpenAI、Gemini、DeepSeek、Kimi、MiMo、MiniMax、xAI、OpenRouter、OpenCode Zen/Go 的对应公共来源；区域、订阅和协议不匹配的新条目会跳过。其他提供商继续使用内置目录或用户配置。
- 新目录只应用于已配置的对应供应商，并补充“添加供应商”的模板。模型列表不代表账号权限；同步也不检查余额或试用每个模型。
- 协议与思考能力仍受本项目适配器约束，不下载或执行远程代码，不导入远程地址、密钥和自定义请求头。目录出现新模型不等于已支持它的全部能力。
- 全部下载失败或本地保存失败时保留旧目录；部分来源失败时保留对应旧条目，更新成功部分并显示提示。在飞请求继续使用原快照。当前只在点击时同步，没有后台定时联网。

来源接口为 `https://pi.dev/api/models/providers/<provider>`；结构参考 Pi 提交 `e4c75a73222ae2c72abb5f5314fa35ee8effc508`。Pi 项目由 Mario Zechner 等维护，许可见 [上游 MIT License](https://github.com/earendil-works/pi/blob/e4c75a73222ae2c72abb5f5314fa35ee8effc508/LICENSE)；目录是第三方汇总数据，并非各厂商的可用性承诺。

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

## AI 账号配置与热更新

模型、思考强度、温度和输出上限由获授权管理员在“我的账号 → AI 角色管理”保存到该账号的 ExecutionBinding。它们不读取旧 TOML `[gui]`，不在切换聊天时更换，不覆盖其他 AI。修改只影响新运行；未配置模型仍可保存真人消息。

旧 `[gui]` 字段留在原配置中但不再提供读写接口，不因升级自动清除。供应商配置、环境变量引用、注释与目录缓存继续保留。供应商级 `default_model` 仍用于协议目录解析，不代表 AI 自动采用全局模型。

配置文件变化自动触发整体校验与热更新；非法新配置保留旧快照，在飞请求继续原快照。设置中的模型服务管理和目录同步仍通过 Runtime 原子保存。旧斜杠命令随 CLI/TUI 退役，不再作为产品接口。
