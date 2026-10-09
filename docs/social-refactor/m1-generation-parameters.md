# M1 独立生成参数实现与验证

日期：2026-10-09。所属阶段：M1 第 4 条业务线“双适配与阶段收尾”。本文记录本批证据，当前规则分别维护于 [身份](01-identity-and-relationships.md)、[意识系统](03-consciousness.md)、[执行](05-ai-execution.md)、[存储](06-storage-and-migration.md) 和 [API](07-api-and-events.md)。

## 本批交付

每个 AI 的 ExecutionBinding 增加可选 temperature、max_tokens（共享 DTO 为 temperature、maxTokens）。创建与修改复用既有管理员接口，公开 Account、普通 init 账号列表和只读账号视角不返回内部绑定；未增加传输路由，仍为 24 个 HTTP 路径、30 个 IPC 命令。私聊与各群使用同一 AI 绑定，身份、关系和完整历史保持独立且稳定。

管理员在自己的 AI 角色管理页面编辑参数。前端通过 ModelOption.generationLimits 读取当前协议与思考模式的温度范围、最小/最大输出量；非法组合提示原因并禁用保存。切换模型保留已填参数，温度不可调时提供“清空温度”，不静默丢弃设置。失败保留表单与重试身份，成功详情显示实际保存值。

只接入已有协议转换支持的温度和输出上限；top_p、惩罚项、任意工具与完整历史请求审计不属于本批。

## 默认值与约束

| 设置 | 留空或旧绑定缺失 | 显式值 |
| --- | --- | --- |
| temperature | 请求不带该字段，采用模型服务默认值 | 有限数值；一般 0–2，当前 Anthropic 适配为 0–1；部分思考模式不可覆盖 |
| max_tokens | 意识系统自动分配输出预算 | 正整数，满足模型声明的输出能力、上下文容量与当前思考预算 |

社交生成不读取全局 Harness GUI 参数。自动回答预算取 min(1024, context_window / 4)；当前 Anthropic 显式思考模式再加上思考预算，最终受模型输出上限约束。未声明窗口时沿用既有 8192 容量。显式输出按原值预留，4096 不压回 1024；输入预算等于窗口减去实际输出预留。必要角色/空间背景、当前触发与回复对象无法容纳时返回 budget_exhausted，历史原记录完整保存，近期材料按预算选择。

约束由 AiClient 按 effective protocol 与已有 ReasoningFormat 计算，业务与前端不按供应商品牌分支：

- AnthropicThinking 开启时温度不可覆盖，max_tokens 必须大于实际 thinking budget；思考预算转换与参数校验使用同一函数。未设置 effort 不发 thinking 块，off 发既有关闭语义。[Anthropic 扩展思考说明](https://platform.claude.com/docs/en/build-with-claude/extended-thinking)
- DeepSeekThinking/DeepSeekEffort 通过当前 Chat 适配开启思考时温度不可覆盖；显式 off 后可设置。[DeepSeek 思考模式说明](https://api-docs.deepseek.com/guides/thinking_mode/)
- 当前 OpenAIEffort 适配未表达显式 reasoning-none，off 仍省略 reasoning 字段，因此该格式的社交温度覆盖保持禁用。这是现有适配器的边界，不宣称所有供应商或未来模型均不支持温度。
- Gemini 和其他非受限模式复用现有温度与输出字段转换；新参数校验只由社交用例调用，不改变旧 Harness 入口的行为。

模型声明与模式约束在保存及运行启动时校验；输入实际容量在意识构造时检查。明确参数越界返回错误，不裁减数值、不换模型、不生成模拟回复。无绑定仍可先创建 AI 和保存真人消息。

## 存储、快照与兼容

字段保存到既有 execution_bindings JSON payload，schema 保持 v6，不添加迁移。领域对象与 BindingCandidate 的 None 省略序列化，缺失字段按 None 读取，旧 payload 和省略参数的命令摘要保持兼容；明确 0 温度与 null 分别表达。旧 JSON 导入不复制全局生成配置。

模型解析、协议路由和 GenerationOptions 使用同一个冻结 AiClient。开始事务复核绑定/档案版本，保存后新运行使用新值；已经开始的运行继续原请求快照。成功提交仍检查消息来源与参与状态。运行快照持久化绑定版本与实际预算，尚未形成完整历史参数/请求审计日志。

存储边界检查有限温度、基本范围及正输出量；模型/协议校验由共享 Feature/AiClient 承担。管理员授权、账号/绑定修订、持久事件与命令回执同事务提交，失败没有部分修改。未引入第二套 Runtime、上下文缓存或前端业务实现。

## 验证

本轮使用 Windows MSVC stable、离线 locked 依赖、独立临时配置/数据库和模拟 provider，未调用真实付费 API。

| 检查 | 结果与范围 |
| --- | --- |
| Rust 受影响链路 | 232 项通过：ai-client 97、storage 43、social-feature 8、chat 8、events 1、runtime 55、social 1、tauri-frontend 15、web-server 4 |
| 前端 | typecheck 与五套测试通过；包含留空、显式零温度、思考约束、输出冲突及 IPC 参数传输 |
| 模拟 HTTP | 完整 social_acceptance.py 通过；同模型两个 AI 在私聊/群聊分别发送自己的温度与 4096/2048 输出量；SQLite 记录相同输出预留及 4096/6144 输入预算 |
| 配置与错误 | 全局 GUI 温度 1.75/输出 7 不影响社交默认；非法温度、零/小数/超模型/超窗口输出拒绝保存；非管理员、事务故障、命令重试/冲突、独立绑定、关闭重开均有业务断言 |
| 在飞请求 | 修改中的旧运行保持原模型、默认温度和 1024；下一运行与重启后的运行使用新模型、0.5 与 4096 |
| 实际 Web | Edge 真实 HTTP 表单：零输出报错并禁用保存；温度 0/输出 2048 保存成功，重新打开保持值，截图在本地验收产物中 |
| 实际 Windows IPC | 嵌入前端 debug EXE 的独立桌面 fixture：温度 0.6/输出 768 保存、重新打开一致；正常关闭后 SQLite integrity_check=ok，绑定 revision=2，其他 AI 保持默认，原私聊/群 ID 保留 |
| 构建与静态检查 | Web 社交生产构建、web-server、Tauri CLI 的 MSVC debug --no-bundle 构建通过；fmt 与受影响 all-targets clippy 通过 |

复现入口为 [social_acceptance.py](../../scripts/social_acceptance.py)（HTTP）、[social_desktop_fixture.py](../../scripts/social_desktop_fixture.py)（独立桌面数据）、受影响包的 cargo test/clippy、Web typecheck/test/build:social-runtime 及 Tauri CLI build --debug --no-bundle。Windows 本轮指定 stable-x86_64-pc-windows-msvc 与对应 target，Cargo 使用 --locked --offline；未修改仓库工具链配置。

现有 Web 大 chunk 提示、旧 web-server api.rs 的 collapsible_if 提示及 MSVC linker 输出不属于本批新增失败。桌面正常退出还记录了 WebView2 的 Chrome_WidgetWin_0 注销提示，进程已退出且数据库检查通过。编译与本地 debug 验证不代表安装包、Linux GUI、干净系统或 M1 全阶段验收已完成。

## 后续

本批完成独立生成参数。仍位于 M1 第 4 条业务线；下一批明确管理员工具授权，默认拒绝，先验证已有 echo/add 白名单的 worker、协议配对、撤销与中断收尾。社交执行当前仍不开放工具。随后完成双适配异常场景、桌面打包和受影响平台完整验收，再进入 M2 的显式记忆、分段摘要与群话题接续唯一私聊，顺序见 [TODO](TODO.md)。
