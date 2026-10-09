<p align="center">
  <img src="assets/branding/app-icon.svg" width="96" height="96" alt="LLM Nest 图标">
</p>

<h1 align="center">LLM Nest</h1>

<p align="center"><strong>同一套 Rust 运行时，连接桌面、浏览器和终端。</strong></p>

<p align="center">
  <a href="#开始使用">开始使用</a> ·
  <a href="#为什么做-llm-nest">项目定位</a> ·
  <a href="docs/development.md">开发指南</a> ·
  <a href="docs/releases.md">构建与发布</a> ·
  <a href="https://github.com/NingCold/llm-nest/issues">反馈问题</a>
</p>

LLM Nest 正从本地 AI 工作空间转向人与 AI 的社交空间。Web 已采用“消息”和“通讯录”，支持稳定 AI 联系人、独立模型配置和持续私聊、独立群聊；桌面、CLI/TUI 暂时保留原有 Harness 入口，共用 Rust 运行时和模型协议能力。

**当前处于开发预览阶段。** 真实 Web 已接通私聊、群聊和消息控制；桌面社交界面、摘要、长期记忆与群话题接续尚未完成。实际能力见 [已完成](docs/social-refactor/completed.md)，剩余范围与顺序见 [TODO](docs/social-refactor/TODO.md)。

## 为什么做 LLM Nest

我们希望 AI 成为有稳定身份、能延续共同经历、懂得参与时机的聊天对象。更换模型不更换联系人，结束一次交谈不删除长期窗口，群聊中每个 AI 以自己的身份发言。

软件管理的上下文、摘要、记忆、召回与遗忘统一称为“意识系统（Consciousness）”。目前它按目标 AI 和窗口准备近期上下文；后续加入有来源、可编辑的记忆和历史摘要。产品范围与架构见 [重构总纲](docs/social-refactor/README.md)。

## 现在能做什么

### 真实社交 Web

| 能力 | 当前实现 |
| --- | --- |
| 联系人与窗口 | 独立身份和模型绑定；每个 AI 唯一长期私聊，各群独立 |
| 群聊 | 成员管理、结构化 @ / 回复、多 AI 持久 FIFO 队列；当前成员共享入群前后的完整群历史 |
| 消息与控制 | 生成期间继续发送；编辑/撤回保留后续记录；停止回复、成员/窗口暂停、结束交谈、归档/恢复 |
| 上下文与恢复 | 按角色和窗口选择近期原文；完整历史保存在 SQLite；重启保留中断片段并继续未开始的计划 |
| 展示与设置 | 流式正文、思考、Markdown、用量与耗时；模型服务与外观设置 |

未配置或失效的模型不会阻止保存真人消息，也不会产生模拟回答。自动接话、摘要、长期记忆和跨窗口接续尚未接入；社交 AI 当前不开放工具。

### 保留的 Harness 入口

桌面、CLI/TUI 与 Web 的 `?legacy=1` 仍使用以下行为，和社交消息用例分别维护：

| 能力 | 当前实现 |
| --- | --- |
| 多模型对话 | OpenAI Chat Completions、Responses、Anthropic Messages、Gemini；Ollama 通过 OpenAI 兼容接口接入 |
| 会话管理 | 新建、切换、重命名、删除；会话记住模型与思考强度 |
| 阅读与展示 | 流式正文、模型返回的思考内容、Markdown、代码、公式、Mermaid、用量与耗时 |
| 设置 | 独立设置页、供应商和模型管理、生成参数、浅色/深色主题 |
| 消息操作 | 编辑后重新生成、重新生成回答、消息反馈；通过持久消息 ID 定位 |
| 工具循环 | 模型调用工具、接收结果并继续回答；默认仅开放编译内置的 `echo` 和 `add` |
| 中断与恢复 | 取消、错误、断流收尾；运行检查点；重启恢复已保存片段 |
| 配置更新 | TOML 配置、环境变量密钥、配置热更新、模型清单刷新 |

界面里的联网搜索尚未接入。附件的数据表示与存储已实现，但完整上传、文档解析和跨协议多模态体验还没有完成。

### 平台状态

| 入口 | 状态 |
| --- | --- |
| Windows 桌面 x64 | NSIS 实机安装、图标、设置及窗口边框已验收；干净系统、缺少 WebView2 和 MSI 系统级安装仍待验收 |
| Linux 桌面 x64 | Ubuntu 24.04 基线的 deb/AppImage 已构建；干净容器安装、包内 worker 和无界面启动通过，GNOME 实机操作待验收；见 [记录](docs/linux-acceptance-2026-09-13.md) |
| Web | 真实社交 HTTP/SQLite 与浏览器界面通过模拟 provider 验收；保留的 Harness 另有历史真实 API 验收 |
| CLI / TUI | 已实现，命令分别为 `llmn` / `llmnt` |
| macOS / 移动端 | 尚未构建和实机验收 |

## 开始使用

### 桌面

1. 从 [GitHub Releases](https://github.com/NingCold/llm-nest/releases) 获取已发布的对应平台安装包。若还没有公开版本，可按 [开发指南](docs/development.md) 自行构建。
2. 打开 LLM Nest，进入 **设置 → 模型**，添加供应商、API Key 和模型。
3. 新建对话，选择模型后开始聊天。没有模型时，界面会显示设置入口。

安装包不附带 API Key，也不需要安装 Node.js 或 Rust。Windows 需要 WebView2 Runtime；当前安装器在缺失时联网下载。

### 从源码运行

开发环境以 **Rust 1.97.0、Node.js 24.18.0、pnpm 11.22.0** 验证；桌面构建还需要相应平台的系统依赖。详细步骤见 [开发指南](docs/development.md)。

```bash
git clone https://github.com/NingCold/llm-nest.git
cd llm-nest
cp config/config.example.toml config/llmn.toml
```

示例配置通过 `DEEPSEEK_API_KEY` 环境变量读取密钥。设置该变量，或在被忽略的 `config/.env` 中填写它，再启动一个入口：

```bash
cargo run -p cli        # llmn
cargo run -p tui        # llmnt

# 桌面：先安装两份前端依赖
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
pnpm --dir frontends/tauri exec tauri dev
```

也可以使用其他供应商或本地 Ollama，见 [配置指南](docs/configuration.md)。首次运行桌面安装版时无需手写配置；上述源码配置主要供终端入口和开发使用。

### Web

```bash
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/web build
cargo run -p web-server
```

打开 `http://127.0.0.1:8787`，默认进入消息/通讯录；在设置中添加模型服务，再创建 AI 联系人并选择其回复模型。`?legacy=1` 可进入迁移期旧聊天。Web 服务默认仅监听本机，不具备面向公网的完整身份认证与多用户隔离。

离线检查真实存储和 HTTP 链路可运行 `python scripts/social_acceptance.py`；使用 `--serve` 保留隔离测试页面。该脚本使用临时配置与本机模拟 provider，不调用付费模型。Windows MSVC 单独 target 的二进制通过 `--binary` 指定，页面产物通过 `--dist` 指定；详见 [开发指南](docs/development.md#运行入口)。

### 社交 Web 交互预览

```bash
pnpm --dir frontends/web dev:social --host 127.0.0.1
```

打开 `http://127.0.0.1:1420`，无需启动 Rust 后端或配置 API Key。可体验消息/通讯录、联系人创建与编辑、唯一长期私聊、独立群聊、邀请 AI 接话、停止/暂停/结束，以及详情中的记忆与摘要、群话题接续私聊。

页面标注“Web 交互预览”，回复为固定模拟内容，摘要整理为原文摘录；记录与草稿保存在当前浏览器的独立 localStorage 中，不能作为正式数据存储或真实记忆验收。它与普通 Web 的真实社交数据独立；也可在普通 Web 开发页显式添加 `?social-preview=1`，Tauri 不启用预览。更多边界与验证见 [界面说明](docs/social-refactor/08-frontend-and-desktop.md#当前-web-交互预览)。

## 数据与运行边界

- 社交联系人、私聊、群聊及运行记录保存到 `social.sqlite3`；旧 Harness 会话仍为 JSON，配置为 TOML，旧记录不自动导入社交库。默认数据目录与桌面安装版的配置目录为用户主目录下的 `.llmn`（Windows：`%USERPROFILE%/.llmn`；Linux：`~/.llmn`），可通过 `LLMN_DATA_DIR` 指定。桌面版在“设置 → 配置 → 配置与数据”显示实际路径并可打开目录。
- **同一个数据目录只能由一个独立 Runtime 写入。** 多入口共用实现，不等于多个进程可以同时打开同一存储目录；同时运行时应使用不同目录。
- 配置与会话目前未加密。密钥可使用环境变量引用；发送到远程模型的消息会交给所选供应商处理。
- 恢复以最近一次落盘的检查点为准，不承诺零片段丢失，也不提供工具副作用回滚或模型 token 游标续传。
- 默认工具是固定白名单子进程。Windows 有 Job Object 资源约束；Linux 尚未具备等价的完整资源约束，当前不支持运行任意不可信插件。

## 项目结构

```text
frontends/cli · frontends/tui · frontends/tauri · crates/web-server
                         │
               features/social · features/chat
                         │
                     runtime
                    /    |    \
              ai-client storage tools
                    \    |    /
                     common
```

业务事件定义在 `crates/events`。图形界面源代码在 `frontends/web`；Tauri 是桌面壳，Web Server 是 HTTP 入口。详细边界和修改约定见 [AGENTS.md](AGENTS.md)。

## 接下来重点做什么

1. 接入桌面实时 IPC 和共用社交界面，补齐 M1 剩余能力与双适配验收。
2. M2 完善摘要、显式记忆和群话题接续；M3 加入有限自动接话与约定联系。
3. 维持平台与发布基线，按对应阶段继续安装和实机验收。

欢迎提供具体任务和复现步骤：你当时想做什么、选了什么模型、期望和实际结果分别是什么。请勿在 Issue、日志或截图中附带 API Key。

## 文档与参考

- [文档索引与验收记录](docs/README.md)
- [开发约定](AGENTS.md) · [配置指南](docs/configuration.md) · [发布指南](docs/releases.md)
- [Cherry Studio](https://github.com/CherryHQ/cherry-studio)：参考其面向用户的功能组织和开发入口。
- [Chatbox](https://github.com/chatboxai/chatbox)：参考其安装、快速开始和平台要求的分层说明。
- [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)：参考其 Harness 定位、开发预览边界与扩展文档入口。

上述项目是设计和文档参考，没有进行功能评分或性能排名。本仓库尚未声明许可证，许可证选择将在正式公开发布前明确。
