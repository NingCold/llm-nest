<p align="center">
  <img src="assets/branding/app-icon.svg" width="96" height="96" alt="LLM-Nest 图标">
</p>

<h1 align="center">LLM-Nest</h1>

<p align="center"><strong>同一套 Rust 运行时，连接桌面、浏览器和终端。</strong></p>

<p align="center">
  <a href="#开始使用">开始使用</a> ·
  <a href="#为什么做-llm-nest">项目定位</a> ·
  <a href="docs/development.md">开发指南</a> ·
  <a href="docs/releases.md">构建与发布</a> ·
  <a href="https://github.com/NingCold/llm-nest/issues">反馈问题</a>
</p>

LLM-Nest 是一个以本地会话为基础的 AI 工作空间，也是一套正在发展的 AI Harness。你可以在图形界面里配置模型、阅读流式回答，也可以在 CLI 或 TUI 中使用同一套模型路由、会话存储和工具执行逻辑。

**当前处于开发预览阶段。** 聊天、设置、持久化和内置工具循环已经落地；通用编码代理、MCP、知识库和工作流编排尚未实现。

## 为什么做 LLM-Nest

模型回答只是一次任务的一部分。LLM-Nest 关心它之前和之后的事情：选中了哪个模型、任务是否完成、取消后留下了什么，以及应用重启后还能找回多少上下文。

- **一个运行时，多个入口。** 桌面、Web、CLI 和 TUI 共用 Rust 业务实现。桌面和 Web 还共用 React 前端，减少不同入口之间的行为差异。
- **让中断有明确的结果。** 取消、网络错误和异常结束会留下状态与已保存的片段。重启时可以恢复最近检查点，并将未完成任务标为中断；不会悄悄重放工具。
- **把模型差异留在协议边界。** 支持多种 API 协议、模型级协议覆盖和思考强度校验。同一供应商网关里的不同模型可以使用不同协议。
- **让扩展建立在可检查的行为上。** Feature、协议适配器、存储和工具执行有明确边界；离线验收通过真实 Runtime、HTTP 和工具子进程检查整个链路。

这些是当前的工程方向，不代表功能广度或成熟度已经超过现有客户端。我们希望先把“会话和运行过程可理解、可恢复”做好，再围绕真实任务扩展能力。

## 现在能做什么

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
| Web | 本地 HTTP 服务与浏览器界面；已完成离线完整链路和部分真实模型验收 |
| CLI / TUI | 已实现，命令分别为 `llmn` / `llmnt` |
| macOS / 移动端 | 尚未构建和实机验收 |

## 开始使用

### 桌面

1. 从 [GitHub Releases](https://github.com/NingCold/llm-nest/releases) 获取已发布的对应平台安装包。若还没有公开版本，可按 [开发指南](docs/development.md) 自行构建。
2. 打开 LLM-Nest，进入 **设置 → 模型**，添加供应商、API Key 和模型。
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

打开 `http://127.0.0.1:8787`。Web 服务默认仅监听本机，不具备面向公网的完整身份认证与多用户隔离。

## 数据与运行边界

- 会话以 JSON 保存在本机；配置为 TOML。默认数据目录是系统用户数据目录下的 `llmn`，可通过 `LLMN_DATA_DIR` 指定。
- **同一个数据目录只能由一个独立 Runtime 写入。** 多入口共用实现，不等于多个进程可以同时打开同一存储目录；同时运行时应使用不同目录。
- 配置与会话目前未加密。密钥可使用环境变量引用；发送到远程模型的消息会交给所选供应商处理。
- 恢复以最近一次落盘的检查点为准，不承诺零片段丢失，也不提供工具副作用回滚或模型 token 游标续传。
- 默认工具是固定白名单子进程。Windows 有 Job Object 资源约束；Linux 尚未具备等价的完整资源约束，当前不支持运行任意不可信插件。

## 项目结构

```text
frontends/cli · frontends/tui · frontends/tauri · crates/web-server
                         │
                    features/chat
                         │
                     runtime
                    /    |    \
              ai-client storage tools
                    \    |    /
                     common
```

业务事件定义在 `crates/events`。图形界面源代码在 `frontends/web`；Tauri 是桌面壳，Web Server 是 HTTP 入口。详细边界和修改约定见 [AGENTS.md](AGENTS.md)。

## 接下来重点做什么

1. 完成 Linux GNOME 实机验收，并跑通 GitHub 托管构建与发布流程。
2. 完善运行记录和故障诊断，让失败原因与恢复边界更容易理解。
3. 围绕一个可验收的实际任务扩展工具，先完成权限和执行边界，再考虑更广泛的插件接入。

欢迎提供具体任务和复现步骤：你当时想做什么、选了什么模型、期望和实际结果分别是什么。请勿在 Issue、日志或截图中附带 API Key。

## 文档与参考

- [文档索引与验收记录](docs/README.md)
- [开发约定](AGENTS.md) · [配置指南](docs/configuration.md) · [发布指南](docs/releases.md)
- [Cherry Studio](https://github.com/CherryHQ/cherry-studio)：参考其面向用户的功能组织和开发入口。
- [Chatbox](https://github.com/chatboxai/chatbox)：参考其安装、快速开始和平台要求的分层说明。
- [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)：参考其 Harness 定位、开发预览边界与扩展文档入口。

上述项目是设计和文档参考，没有进行功能评分或性能排名。本仓库尚未声明许可证，许可证选择将在正式公开发布前明确。
