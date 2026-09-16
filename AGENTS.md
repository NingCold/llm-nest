# AGENTS.md — LLM-Nest

本文件面向修改仓库的开发者和编码代理，记录当前架构、必须维持的行为和验证方式。用户入门见 [README](README.md)，环境搭建见 [开发指南](docs/development.md)，历史验收见 [文档索引](docs/README.md)。

## 项目定位与当前范围

LLM-Nest 是共用 Rust 运行时的本地 AI 工作空间与 Harness。产品/桌面二进制名为 `LLM-Nest`，CLI 为 `llmn`，TUI 为 `llmnt`；内部 Tauri Cargo 包名为 `tauri-frontend`。

已实现聊天、协议路由、会话持久化、运行中断/恢复、内置工具循环，以及桌面/Web 设置。MCP、通用编码代理、Shell/文件工具、插件市场、知识库和工作流编排尚未实现。不要根据目录名、预留 trait 或界面按钮声称功能已经可用。

## 工作前先确认

- 读取 `git status --short`，保留用户的未提交修改。不要混入无关生成文件、环境配置或测试数据。
- 通过实际调用链确认类型、签名和行为；过往报告描述的是当时的版本，不应覆盖当前代码。
- 修改只落在负责该行为的模块。优先复用现有接口，不为单一前端复制业务逻辑。
- 正常验证使用独立临时配置/数据和模拟 provider。真实 API 验收会产生费用，仅在已有明确授权时执行。
- `config/llmn.toml`、`.env`、用户会话、API Key 不得提交、打包、上传到 CI 或打印到日志。保留环境变量引用，不把它替换为明文。
- 图标的唯一编辑源是 `assets/branding/app-icon.svg`；不要单独修改某一个派生尺寸。

## Git 工作约定

- 普通功能修改、问题修复和文档更新默认直接在 `main` 上开发并提交，不自动新建 `codex/*` 分支。
- 仅在用户明确要求，或较大的实验性改动确需隔离时新建分支；创建前说明原因。
- PR 不作为日常默认流程。需要创建时，单独说明用途和目标分支，不把推送分支当作已经合入 `main`。
- 多台机器接续工作前，确认工作区状态并同步 `main`；保留未提交修改，不能用强制覆盖解决分歧。
- 汇报时分别说明本地提交、远端推送与合并的实际状态；用户要求提交不等于要求推送、创建 PR 或发布版本。

## 代码地图

| 路径 | 责任 |
| --- | --- |
| `crates/common` | Message、ContentPart、ID、Usage、计时、中断与运行检查点等公共类型 |
| `crates/ai-client` | AiProvider/AiClient、ModelRouter、内置目录、配置类型、协议工厂、协议适配、SSE 分帧 |
| `crates/events` | ChatEvent 等业务事件的序列化契约 |
| `crates/storage` | SessionStore、JSON DTO、原子文件写入、目录锁和默认数据目录 |
| `crates/tools` | Tool、ToolRegistry、固定白名单子进程和内置 echo/add |
| `crates/runtime` | 配置/热更新、SessionManager、Feature 生命周期、事件总线、工具注册 |
| `features/chat` | 聊天与工具循环、取消、检查点、结束收尾、消息计时 |
| `crates/web-server` | HTTP/SSE 适配、GUI DTO、静态前端服务 |
| `frontends/cli` / `frontends/tui` | 命令解析与终端展示 |
| `frontends/web` | 共用 React GUI、状态管理、HTTP/Tauri backend 适配 |
| `frontends/tauri/src-tauri` | 桌面 IPC、延迟初始化、平台窗口和打包 |
| `scripts` / `docs` | 可执行验收/构建流程与证据 |

`sdk/app-sdk`、PluginManager 等预留部分不代表已经支持动态第三方插件。不存在旧文档提到的 `crates/provider` / `crates/llm` 分层。

## 架构边界

```text
CLI / TUI / Tauri IPC / Web HTTP
               ↓
        ChatFeature → Stream<ChatEvent>
               ↓
Runtime / SessionManager / AiClient / ToolRegistry
               ↓
   协议适配器 / SessionStore / 工具子进程
```

- 前端调用 Feature 的具体业务方法，消费事件，不构造供应商协议 JSON。
- `Feature` 负责 id、initialize、shutdown 和 Arc downcast；`FeatureContext` 注入 sessions、llm、events、tools。新业务 API 不必塞进统一 Feature trait。
- `RuntimeEvent` 只包含会话与运行时层面的通知；聊天增量、思考、工具结果和终止事件走 `ChatEvent`。不要使用不存在的 `RuntimeEvent::Feature`。
- `AiClient` 按 `(provider, effective protocol)` 选择适配器；同一 provider 的模型可以覆盖默认协议。
- 模型解析/调用使用一致的配置快照。配置更新不能让在飞请求的目录和 provider 路由来自不同版本。
- 协议差异集中在 `ai-client/src/protocols`，共享 SSE 分帧。业务层和 GUI 不应随供应商名称堆条件分支。

## 必须维持的行为

### 模型、配置与凭据

- 模型名、provider、reasoning effort 严格校验，错误返回可用候选。不要把不存在的模型静默换成另一个。
- 内置目录允许省略协议、地址和模型列表；自定义 provider 需要声明这些内容。空 providers 是有效的 GUI 首次启动状态，但不能发送聊天。
- 热更新先整体校验，成功才替换；失败保留旧配置。在飞请求继续使用原快照。
- `/refresh` 通过 `toml_edit` 定点补充模型，保留注释和其他配置；不可悄悄重写整份 TOML。
- `.env` 层级为进程环境 > 配置目录 > 默认数据目录，只填缺失变量。密钥解析只发生在需要凭据的边界。
- GUI 生成设置通过 Runtime 校验并原子持久化。会话模型恢复不得覆盖用户保存的全局默认值。
- OpenAI 兼容返回同时允许 `reasoning_content` 和 `reasoning`：优先非空前者，否则后者，不重复拼接。

### 会话与恢复

- 采用写穿透：先成功落盘，再替换内存；失败上抛，不能让 UI 展示“已保存”。
- FileSessionStore 持有 `.llmn.lock` 的 OS 排他锁。同目录只允许一个独立 Runtime；不要删除运行中的锁文件或退回 PID 文件锁。
- JSON 写入使用同目录临时文件和原子替换。坏文件错误须带路径，不可静默丢弃。
- 新消息创建真实 UUID；旧记录加载时补齐并持久化，重复 ID 报错。编辑按 userId、反馈按 messageId + revision 定位，不能恢复旧索引 API。
- 一次 run 的开始和用户消息同事务，成功回答和 Succeeded 同事务。流式每 500ms 保存有变化的 draft，工具调用/结果写穿透。
- 取消、EOF、provider 错误、消费端断开均先完成中断持久化，再发送终止事件；补齐未回答工具调用的失败 ToolResult。
- 启动遇到 Running 检查点时，幂等恢复片段并标为 Interrupted。不自动重放工具，不声称能恢复 provider 的 token 游标或回滚副作用。
- 只保存最近一次 RunCheckpoint，尚不是完整任务审计日志。
- 下一次模型请求过滤中断消息；展示元数据（ID、反馈、思考、计时、用量、中断状态）通过 wire 转换剥离。工具调用与结果按各协议规则成对转换。
- Usage 的 prompt_tokens 包含缓存输入；cached_tokens 是命中部分，缓存命中率为 cached / prompt，避免再次加到分母。
- 附件以持久化 DTO 保存二进制；持久化格式与 provider wire 格式分开。

### 工具执行

- `Tool::run` 必须明确实现可丢弃、会让出执行权的异步契约。Registry 的超时不能抢占不 yield 的进程内代码。
- 默认 echo/add 通过当前宿主 EXE 的 `--llmn-tool-worker` 入口执行；所有宿主在配置、密钥、日志、UI、存储初始化之前处理此入口。
- 子进程只允许固定编译白名单，清空继承环境，输入/输出各限 64 KiB。Registry 默认 30 秒墙钟超时并捕获 unwind panic。
- Windows 使用 Job Object：最多 1 个进程、256 MiB committed memory、10 秒用户态 CPU，Job 关闭时杀子进程。配置/附加失败须停止，不得回退到进程内。
- Linux 当前依赖子进程终止/kill_on_drop，没有等价的完整资源配额或通用沙盒。打包后须实际验证 worker 的动态库加载和退出行为。
- `register_trusted_tool` / `register_trusted_in_process` 是显式的信任边界，不用于模型指定的任意插件。

### GUI 与平台

- React 源码只有 `frontends/web` 一套。Tauri 构建输出到忽略的 `frontends/tauri/dist`；不要用桌面打包改写已跟踪的 Web dist。
- `IS_DESKTOP` 表示 Tauri 运行环境；`CUSTOM_WINDOW_CHROME` 只控制 Windows 自绘窗口区域。不要把二者混同，Linux 仍使用桌面 IPC。
- 自绘窗口控制独立于聊天后端初始化；初始化/历史/设置失败有提示和重试；历史未成功加载或无模型时禁止发送。
- HTTP Headers 使用 Headers.set，避免大小写重复头导致值变成 `1, 1`。修改事件名、字段命名或 DTO 时同步 Web/Tauri 两条链路。
- 设置路由为 `#/settings/models|generation|appearance|about`，聊天为 `#/chat`。切换保留输入和表单；隐藏聊天 Header 时卸载其菜单 portal。
- Windows 的窗口参数位于 `tauri.windows.conf.json`；Linux 首版使用系统装饰。Tauri 合并配置中的窗口数组会整体替换，调整公共尺寸时检查平台文件。
- Windows 保留 `shadow=false`、透明 WebView 合成表面、不透明 HTML 背景。`Window.setBackgroundColor` 同步原生底色，不能换成会覆盖 WebView 透明背景的 WebviewWindow 同名 API。
- 普通 Windows 11 窗口由 DWM 绘制圆角/边框；上下占满当前显示器 rcWork（容差 2 物理像素）且未最大化/全屏时，绘制 1 物理像素矩形内描边并关闭原生描边。
- 不恢复 tao 阴影边距，不使用透明外边距、SetWindowRgn、自建阴影窗口或 CSS 裁切整个应用。窗口状态变化发事件，revision 防止迟到事件覆盖新状态。
- Windows 10 不支持 DWM 属性时回退细边框；DWM 返回成功不等同于所有机器视觉验收通过。
- 产品 identifier `com.llmnest.tauri`、数据目录 `llmn` 和安装路径关系不得随意更改。图标变更时保留 build.rs 对 icons 的依赖，核对实际安装器和 EXE 资源。

## 如何验证改动

先跑受影响模块和相关链路，再按改动范围扩大。不要把编译、模拟 API、真实厂商 API 和桌面安装验收混为一谈。

```bash
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked

pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test

cargo build -p web-server --locked
python scripts/acceptance.py
```

- 全量跨平台检查：`cargo test --workspace --locked`。Linux 需要先安装 GTK/WebKitGTK/OpenSSL 开发依赖，见开发指南。
- Windows 发布使用 MSVC，不能把 GNU 本地构建当作发布包。桌面必须通过 Tauri CLI 构建，确保嵌入前端；裸 cargo release 可能仍访问开发地址。
- `scripts/acceptance.py` 使用模拟 provider 和独立临时目录，验证真实 HTTP、工具子进程、持久化及错误收尾。
- `scripts/live_acceptance.py` 是真实 API 付费测试，不放入默认 CI；不得从个人配置生成 CI 密钥或发布资源。
- Windows 图标检查用 `scripts/verify-windows-icons.ps1`。安装退出码 0 不足以证明覆盖正确，核对安装 EXE 和包内容。
- Linux 构建脚本、容器和发布流程见 `scripts/linux`、`.github/workflows` 与 [发布指南](docs/releases.md)。

## 新增功能的落点

- **协议**：更新 Protocol、build_provider 和对应 convert/SSE；添加协议 fixture，覆盖错误、终止和工具调用。新增 provider 品牌但协议相同时优先改目录/配置。
- **Feature**：在 features 下实现生命周期并暴露业务方法；通过 FeatureContext 获得运行时能力，事件放到 events。
- **工具**：先明确权限、副作用、超时和进程边界，再注册能力。不得把“支持工具调用”写成“支持任意安全插件”。
- **GUI 字段**：从持久化/Runtime 契约开始，同步 HTTP、IPC、前端 DTO、保存失败行为与历史恢复。
- **文档**：README 讲用途与入口，AGENTS 讲修改约束，细节进 docs。直接修订当前规则，避免继续追加互相矛盾的“以此节为准”。

## 发布与证据

- 当前经过用户确认的 Windows 窗口基线是 `91f9a9b`，包含高速缩放、圆角和上下贴边细边框；2026-09-13 用户确认边框验收通过。
- Linux 的编译/打包结果和 GUI 验收必须分别记录，见 [本轮记录](docs/linux-acceptance-2026-09-13.md)。
- 发布应校验 Cargo/Tauri 版本与 tag 一致，产物包含校验和。默认流程生成草稿 Release，由维护者检查后公开。
- 不提交私有配置、测试目录或个人远程主机地址。源码发布前需要明确项目许可证；不要代替维护者虚构许可证、签名或支持承诺。
