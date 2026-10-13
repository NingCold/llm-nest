<p align="center">
  <img src="assets/branding/app-icon.svg" width="96" height="96" alt="LLM Nest 图标">
</p>

<h1 align="center">LLM Nest</h1>

<p align="center"><strong>同一套 Rust 运行时，连接桌面与浏览器。</strong></p>

<p align="center">
  <a href="#开始使用">开始使用</a> ·
  <a href="#为什么做-llm-nest">项目定位</a> ·
  <a href="docs/development.md">开发指南</a> ·
  <a href="docs/releases.md">构建与发布</a> ·
  <a href="https://github.com/NingCold/llm-nest/issues">反馈问题</a>
</p>

LLM Nest 是人与 AI 的本地社交空间。Web 与桌面共用“消息”和“通讯录”，支持独立 AI 账号、持续私聊和群聊、意识记忆与约定联系。旧 Harness、CLI/TUI 和旧界面入口已退役，当前只有一套社交架构。

**当前处于开发预览阶段。** Web 与桌面已接通真实私聊、群聊、消息控制和实时订阅；显式记忆、窗口分段摘要和群话题接续唯一私聊已接入。实际能力见 [已完成](docs/social-refactor/completed.md)，剩余范围与顺序见 [TODO](docs/social-refactor/TODO.md)。

## 为什么做 LLM Nest

我们希望 AI 成为有稳定身份、能延续共同经历、懂得参与时机的聊天对象。更换模型不更换联系人，结束一次交谈不删除长期窗口，群聊中每个 AI 以自己的身份发言。

AI 已拥有独立账号，与真人共用公开资料结构；内部设定、模型、思考强度、温度与输出上限在自己账号下的 AI 角色管理页维护。创建者成为初始管理员，可授予/撤销其他管理员；管理员可停用/恢复账号，并以只读视角查看它参与的聊天。移除联系人只撤下个人通讯录条目，仍可继续私聊与共同群互动，重新添加沿用原窗口。规则见 [身份与关系](docs/social-refactor/01-identity-and-relationships.md#0-已确认的账号与管理边界)。

软件管理的上下文、摘要、记忆、召回与遗忘统一称为“意识系统（Consciousness）”。目前它按目标 AI 和窗口准备近期原文、有效显式记忆、窗口分段摘要与明确选中的共同群话题引用，保存来源并在修改/撤回后复核。产品范围与架构见 [重构总纲](docs/social-refactor/README.md)。

## 现在能做什么

### 真实社交 Web 与桌面

| 能力 | 当前实现 |
| --- | --- |
| 账号与联系人 | 共同公开资料、管理员自己的 AI 管理页、授予/撤销与只读查看；联系人移除与管理员停用分开 |
| 独立生成配置 | 每个 AI 自己的模型、思考强度、温度与输出上限；管理员修改，新运行生效，不继承全局 Harness 参数 |
| 聊天窗口 | 每个 AI 唯一长期私聊，各群独立；重新添加或更换模型保留身份与历史 |
| 群聊 | 成员管理、结构化 @ / 回复、多 AI 持久 FIFO 队列；当前成员共享入群前后的完整群历史 |
| 消息与控制 | 生成期间继续发送；编辑/撤回保留后续记录；停止回复、成员/窗口暂停、结束交谈、归档/恢复 |
| 上下文与恢复 | 按角色和窗口选择近期原文；完整历史保存在 SQLite；重启保留中断片段并继续未开始的计划 |
| 展示与设置 | 流式正文、思考、Markdown、用量与耗时；模型服务与外观设置 |

未配置或失效的模型不会阻止保存真人消息，也不会产生模拟回答。显式记忆和窗口摘要可在聊天/联系人详情管理；摘要需先明确选择专用模型并开启，后台调用可能产生费用。群消息的“私聊继续”会先加入唯一私聊的引用草稿，确认发送后带入共同群来源。群聊可在聊天详情显式开启有限自动接话，设置轮数和每日额度；私聊默认等待 800ms 合并尚未开始的普通输入，空闲默认 5 分钟结束本次交谈。聊天详情现已支持明确授权的一次性约定联系：选择 AI、时间和失效时间，设置免打扰期间延后或跳过。应用运行时调度，关闭后不会后台唤醒；失败或已开始但结果不确定的联系不会自动重发。自动记忆整理尚未接入；社交 AI 当前不开放工具。

模型协议支持 OpenAI Chat Completions、Responses、Anthropic Messages、Gemini 与 Ollama 兼容接口。完整附件输入、自动记忆整理与工具授权仍需后续实施。

### 平台状态

| 入口 | 状态 |
| --- | --- |
| Windows 桌面 x64 | MSVC 社交 NSIS/MSI 已构建；NSIS 覆盖升级、安装版 IPC/保存/恢复与资源检查通过，见 [阶段记录](docs/social-refactor/m1-stage-validation.md)；[本机 NSIS 卸载重装](docs/windows-reinstall-2026-10-09.md)通过，干净系统与 MSI 实际安装验收仍待完成 |
| Linux 桌面 x64 | 既有 Ubuntu 24.04 基线 deb/AppImage、包内 worker 与社交 IPC 通过；Ubuntu 26.04 GNOME 的 Wayland/XWayland/FUSE 自动检查和用户人工确认通过，见 [记录](docs/social-refactor/m1-stage-validation.md)；本批无系统标题栏的新布局尚待构建和实机复验 |
| Web | 真实社交 HTTP/SQLite 与浏览器界面通过模拟 provider 验收 |
| macOS / 移动端 | 尚未构建和实机验收 |

## 开始使用

### 桌面

1. 从 [GitHub Releases](https://github.com/NingCold/llm-nest/releases) 获取已发布的对应平台安装包。若还没有公开版本，可按 [开发指南](docs/development.md) 自行构建。
2. 打开 LLM Nest，进入 **设置 → 模型**，添加供应商、API Key 和模型。
3. 在「我的账号 → AI 角色管理」创建 AI 并选择其回复模型，从消息或通讯录进入持续私聊；也可建立独立群聊。未配置模型时仍可创建账号和保存真人消息，界面提示该 AI 需要配置。

桌面后端随应用在同一进程内启动，无需另开 Web Server。Windows 打开时先显示空白磨砂玻璃，再加载消息/通讯录；窗口按钮位于内容右上角，没有独立标题栏。

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
# 桌面：先安装两份前端依赖
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/tauri install --frozen-lockfile
pnpm --dir frontends/tauri exec tauri dev
```

也可以使用其他供应商或本地 Ollama，见 [配置指南](docs/configuration.md)。首次运行桌面安装版时无需手写配置；上述源码配置主要供Web 开发使用。

### Web

```bash
pnpm --dir frontends/web install --frozen-lockfile
pnpm --dir frontends/web build
cargo run -p web-server
```

打开 `http://127.0.0.1:8787`，默认进入消息/通讯录；在设置中添加模型服务，再创建 AI 联系人并选择其回复模型。旧查询参数不再切换到旧产品。Web 服务默认仅监听本机，不具备面向公网的完整身份认证与多用户隔离。

离线检查真实存储和 HTTP 链路可运行 `python scripts/social_acceptance.py`；使用 `--serve` 保留隔离测试页面。该脚本使用临时配置与本机模拟 provider，不调用付费模型。Windows MSVC 单独 target 的二进制通过 `--binary` 指定，页面产物通过 `--dist` 指定；详见 [开发指南](docs/development.md#运行入口)。

### 不调用真实模型的界面验证

运行 `python scripts/social_acceptance.py --dist frontends/web/.social-runtime-dist --serve`，使用脚本打印的地址。它以隔离临时目录与本机模拟 provider 驱动真实界面、HTTP 和 SQLite。早期独立浏览器模拟预览已退役。

## 数据与运行边界

- 社交联系人、私聊、群聊及运行记录保存到 `social.sqlite3`；配置为 TOML。旧 JSON 不自动加载或导入，可在“设置 → 配置 → 导入历史聊天”检查、选择并导入只读窗口，保留原文件与既有消息 ID。默认数据目录与桌面安装版的配置目录为用户主目录下的 `.llmn`（Windows：`%USERPROFILE%/.llmn`；Linux：`~/.llmn`），可通过 `LLMN_DATA_DIR` 指定。桌面版在“设置 → 配置 → 配置与数据”显示实际路径并可打开目录。
- **同一个数据目录只能由一个独立 Runtime 写入。** 多入口共用实现，不等于多个进程可以同时打开同一存储目录；同时运行时应使用不同目录。
- 配置与会话目前未加密。密钥可使用环境变量引用；发送到远程模型的消息会交给所选供应商处理。
- 恢复以最近一次落盘的检查点为准，不承诺零片段丢失，也不提供工具副作用回滚或模型 token 游标续传。
- 默认工具是固定白名单子进程。Windows 有 Job Object 资源约束；Linux 尚未具备等价的完整资源约束，当前不支持运行任意不可信插件。

## 项目结构

```text
frontends/web → frontends/tauri / crates/web-server
                         │
               features/social
                         │
                     runtime
                    /    |    \
              ai-client storage tools
                    \    |    /
                     common
```

业务事件定义在 `crates/events`。图形界面源代码在 `frontends/web`；Tauri 是桌面壳，Web Server 是 HTTP 入口。详细边界和修改约定见 [AGENTS.md](AGENTS.md)。

## 接下来重点做什么

1. M1 业务、界面与启动修复已完成。干净 Windows 与 MSI 实际安装/卸载重装验收按用户要求暂缓，Linux 新窗口布局待实机复验，范围见 [剩余工作](docs/social-refactor/pending.md)。
2. M2 三批意识系统与 M3 两批参与节奏、一次性约定联系已完成；旧架构退役与显式历史导入已完成，下一批建议补齐消息列表的已读/未读、置顶与搜索。工具授权按具体需求作为独立链路实施。
3. 维持平台与发布基线，按对应阶段继续安装和实机验收。

欢迎提供具体任务和复现步骤：你当时想做什么、选了什么模型、期望和实际结果分别是什么。请勿在 Issue、日志或截图中附带 API Key。

## 文档与参考

- [文档索引与验收记录](docs/README.md)
- [开发约定](AGENTS.md) · [配置指南](docs/configuration.md) · [发布指南](docs/releases.md)
- [Cherry Studio](https://github.com/CherryHQ/cherry-studio)：参考其面向用户的功能组织和开发入口。
- [Chatbox](https://github.com/chatboxai/chatbox)：参考其安装、快速开始和平台要求的分层说明。
- [DeepSeek Harness](https://github.com/deepseek-ai/deepseek-harness)：参考其 Harness 定位、开发预览边界与扩展文档入口。

上述项目是设计和文档参考，没有进行功能评分或性能排名。本仓库尚未声明许可证，许可证选择将在正式公开发布前明确。
