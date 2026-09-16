# chatecnu/ecnu-max 真实验收（2026-09-13）

按用户授权调用真实接口 `https://chat.ecnu.edu.cn/open/api/v1/`。配置来自 `config/config.example.toml` 的 chatecnu 段（测试时项目没有 config/llmn.toml），密钥来自其引用的 `API_KEY_CHATECNU` 环境变量，支持读取 Windows 用户环境变量。测试配置只保留 env 引用，不写入密钥。

## 结论

普通流式回答、low 思考、high 工具调用、max 思考、取消后继续对话、重启后恢复历史，以及真实 Web GUI 选择强度/保存设置/刷新恢复均通过。所有测试仅发送合成问题和简单算术，没有发送用户会话历史或项目源码。

## 实测发现并修复的 BUG

ecnu-max 的部分 OpenAI 兼容响应使用 `delta.reasoning`，原实现只接收 `delta.reasoning_content`。因此，第一轮 low 模式能显示思考，而 max 和 high 工具调用中的部分思考被丢弃；首 token 时间也错误地等到了正文。

用一次直接 API 探测确认：max 响应包含 14 个带 `reasoning` 的 delta，未出现 `reasoning_content`。这是解析兼容问题，不是模型未思考或 GUI 选择未生效。

修复了流式和非流式转换：

- 支持 `reasoning_content` 与 `reasoning`。
- 两者同时出现时优先选择非空 `reasoning_content`，否则回退到 `reasoning`，只显示一次。
- 不直接采用 serde alias，避免两字段同在时出现 duplicate field 错误。
- 补充两项协议回归，覆盖 Unicode、缺失/null/空字符串、两字段同时出现、思考和正文同帧。

修复后 max 模式收到 21 个思考事件、持久化 265 个字符；high 工具调用也可在 GUI 展示工具调用前和结果返回后的思考内容。

## 样本结果

耗时是单次样本，受网络和服务端调度影响，不代表性能基准。工具调用统计包含多次模型请求。

| 流程 | 结果 | 耗时 | 返回的 total_tokens |
|---|---|---:|---:|
| off 普通回答 | 连接成功🙂 | 0.77 s | 376 |
| low 算术 | 37×49=1813，有思考事件 | 1.17 s | 471 |
| high 工具调用（修复后） | 实际执行 add(123,456)，最终答 579，有思考事件 | 0.66 s | 999 |
| max 算术（修复后） | 23×47=1081，有思考事件 | 10.09 s | 493 |
| 收到正文后取消 | Cancelled，保存已有部分正文 | 0.91 s | 未返回 |
| 取消后继续发消息 | 恢复成功 | 0.78 s | 394 |
| GUI 选择 low 后发送 | 41×43=1763；可见思考、usage、timings | 9.65 s | 431 |

两轮脚本均重启真实 web-server，并逐项比较历史消息：正文、思考内容、工具块、消息 ID、状态和 revision 完全一致。

GUI 还验证：保存温度 0.2 / maxTokens 1024；通过界面选择 low；刷新后后端保存的 `reasoning_effort=low`、生成设置、答案和思考内容仍在。密钥仅在本地后端使用，不传给浏览器。

## 可重复运行

需要 Python 3.11+（tomllib），以及已经构建的 web-server；此脚本会调用真实 API 并消耗额度，不属于 cargo test / 离线验收的一部分。

```powershell
cargo build -p web-server --offline --locked
python scripts/live_acceptance.py --config config/config.example.toml
# 定点重测 max 和工具调用，并保留后端供 GUI 验收
python scripts/live_acceptance.py --cases=reasoning_max,tool --serve --dist <前端构建目录>
```

输出独立 Fixture 目录，内含会话 JSON、report.json、已脱敏的 server.log；在 Fixture 中创建 STOP 文件可退出 `--serve`。测试临时配置只含 chatecnu，单次请求超时 120 秒，每条测试限制输出 tokens；不改动用户原配置和会话目录。

本次修复后的真实脚本报告和 GUI 检查报告保留在系统临时目录 `llmn-live-ecnu-wo51aw1u`；修复前的字段探测结果保留在 `llmn-live-ecnu-ac9tvbqh/wire-max.json`。这些临时产物未纳入 Git。

## 回归与边界

- Rust 全工作区（排除 Tauri 可执行链接）205 个测试通过。
- 离线完整链路验收再次通过。
- 本次只修改 OpenAI 兼容响应解析，GUI 源码沿用上一轮已通过类型检查和构建的版本。
- 不推断其他厂商/模型均兼容；没有测试视觉、文件输入、长上下文或高并发。
- 取消流程没有返回 usage，因此无法据此计算完整账单，也不能据本地停止断言服务端立即停止计费。
- Tauri 安装包实机验收仍待完成；本轮真实 GUI 验收使用 Web + Edge。
