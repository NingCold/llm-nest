# 完整流程验收与 GUI 设置修复（2026-09-13）

首轮完成核心聊天链路的离线集成验收和 Web GUI 操作验收：运行真实 web-server、ChatFeature、工具子进程与文件存储，模型 HTTP 服务使用本地确定性模拟。随后按用户授权使用 chatecnu/ecnu-max 完成真实 API 与 GUI 验收，并修复实测发现的 reasoning 字段兼容问题，详见 `live-acceptance-2026-09-13.md`。两轮均使用独立临时配置和会话目录。

## 修复内容

- GUI 设置此前在 HTTP/Tauri 两端均为占位实现。现在共用 Runtime 的 GuiConfig，写入原配置文件的 `[gui]` 节；定点编辑、原子替换，保留 provider 与注释。新增 `PUT /api/config` 和真正工作的 Tauri `set_config`。
- 设置界面提供温度（0–2）和最大输出 tokens（正整数或留空），前后端均校验。支持小数温度，清空 maxTokens 会移除旧值。模型必须通过目录解析。
- 设置保存按顺序执行，成功才更新已保存值；失败提示并允许重试。切换会话只恢复该会话的模型，不写回默认模型；延迟完成的保存不会把界面切回旧会话。新会话使用保存的默认模型。删除供应商后会纠正失效选择，无模型时提示先配置。
- 浏览器实测发现 JSON 请求同时包含大小写不同的客户端标识头，浏览器合并为 `1, 1`，导致保存设置、重命名、反馈等请求返回 403。改为使用 Headers.set，回归测试也按浏览器规则检查。安全校验保持原有要求，拒绝响应统一为 JSON 错误。
- 初始化异常现在显示失败页和重新连接按钮，不再误判为启动完成；历史加载失败显示重试，未加载成功前不能发送。
- 会话创建、删除、重命名、列表刷新、停止生成、反馈保存和供应商模板读取的失败均有可见提示。中断回复提供重新生成入口。
- Tauri 使用合并后的 Runtime 模型目录，支持内置模型和模型级协议覆盖；补齐供应商模板、添加/编辑、删除的 IPC 接口。Runtime 延迟初始化，配置/目录锁错误可通过 GUI 查看与重试，不再在窗口启动前直接退出；支持 LLMN_CONFIG。

## 自动验收

| 验收项 | 结果 |
|---|---|
| 设置保存、进程重启读取、非法设置拒绝且原文件不变 | 通过 |
| 会话新建、重命名、删除 | 通过 |
| Unicode 正文、思考内容、usage、参数传入模型请求 | 通过 |
| 反馈持久化、按消息 ID 编辑、过期 revision 拒绝 | 通过 |
| add 工具通过真实子进程执行并接回下一轮模型 | 通过 |
| provider 错误、流式 EOF、失败后再发消息 | 通过 |
| 同会话重复请求拒绝、取消、HTTP 消费端断开 | 通过 |
| 强制结束进程后从非空 checkpoint 恢复正文与思考内容 | 通过 |
| 再次重启恢复幂等，第二写者被 OS 文件锁拒绝 | 通过 |
| 供应商增删保留 GUI 设置 | 通过 |

可重复运行（先构建 web-server，脚本只用 Python 标准库）：

```powershell
cargo build -p web-server --offline --locked
python scripts/acceptance.py
```

脚本每次创建独立临时目录和 loopback 端口。结束时停止服务，保留测试记录与 server.log 供排查。GUI 验收可传 `--serve --dist <前端构建目录>`；输出地址后使用浏览器访问，在输出的 Fixture 目录创建 `STOP` 文件即可停止。

```powershell
# 在 frontends/web 目录构建到临时位置，不改动已跟踪的 dist
pnpm exec vite build --outDir "$env:TEMP/llmn-acceptance-web"
# 在仓库根目录
python scripts/acceptance.py --serve --dist "$env:TEMP/llmn-acceptance-web"
```

## 浏览器人工操作验收

在 Edge 上连接隔离环境，使用真实界面验证：

- 将温度设为 0.25、最大输出设为 2048，看到“已保存”；后续请求正常使用设置。
- 临时移走测试配置文件，保存失败给出明确提示；刷新显示启动失败页。恢复测试配置后点击重新连接，正常进入界面。
- 新建会话、发送 tool，界面显示 add 参数、工具结果、最终答案和用量；反馈、会话重命名成功。
- 通过浏览器调试接口仅对一次历史请求注入 503，界面显示失败与重试，发送被禁用；撤销注入并重试后正常加载历史。
- 发送 slow 后停止，显示“已停止生成”，保留部分正文、思考内容和重新生成入口。
- 强制结束进程恢复后的历史在 GUI 展示部分正文、思考内容、恢复错误原因和重新生成入口。

测试注入已撤销，测试配置已恢复。

## 验证范围与限制

- `cargo test --workspace --exclude tauri-frontend --offline --locked`：205 个测试通过（包含真实接口测试后新增的两项协议回归）。
- `cargo check -p tauri-frontend --offline --locked` 通过；`cargo clippy --workspace --all-targets --offline --locked` 通过，已有风格警告仍存在。
- Web TypeScript 类型检查、执行实际 store/hook/adapter 代码的回归测试、Vite 生产构建通过。
- `cargo fmt --all -- --check`、`git diff --check` 通过。
- Tauri 本轮做编译/IPC 适配检查，未宣称安装包实机验收通过。上一轮 GNU 工具链完整链接遇到 export ordinal 上限；Windows MSVC 打包与启动验收仍需单列进行。
- 已实测 chatecnu/ecnu-max 的普通回答、思考、工具调用、取消与 GUI；其余厂商 API、图像/文件输入和长期负载尚未验收。
- Web 构建仍有大 chunk 提示；应在实际启动性能数据支持下做加载优化。
- 本轮不是完整运行审计、工具副作用回滚或 provider token 游标续传；此前 checkpoint/单写者/工具隔离的边界不变。
