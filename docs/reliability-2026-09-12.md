# 四项可靠性修复

## 1. 存储防覆盖

采用单写者模式：FileSessionStore 持有 `.llmn.lock` 的操作系统排他锁，进程崩溃后锁自动释放。第二个 CLI/TUI/Web/桌面实例访问同一数据目录会明确失败。要同时使用多个独立实例，必须指定不同 LLMN_DATA_DIR；共享 Runtime 服务尚未引入。

会话写入先写同目录临时文件、sync_all，再原子 rename。锁文件留在目录是正常现象，是否占用由 OS 锁决定，不取决于文件是否存在。不要手动删除运行中的锁文件。

## 2. 稳定消息 ID

新增持久化 UUID。旧 JSON 首次加载时自动补齐并保存，后续重启保持不变；重复 ID 会拒绝加载。前端不再生成 m-index 作为服务端身份。

编辑使用 userId + expectedRevision；反馈使用 messageId + revision。删除或替换消息后，旧页面无法把操作错用到同位置的新消息。反馈成功重新加载新版本，失败回滚。旧的无版本索引反馈 API 已移除。ID 不会进入模型协议请求。

## 3. Run 检查点与恢复

SessionRecord 增加可选 run，旧文件兼容。开始运行时，用户消息与 Running 记录原子保存。流式每 500ms 检查是否有变化并保存正文/思考草稿；工具调用和结果在执行边界落盘。成功答案与 Succeeded 一起保存。

启动发现 Running，恢复已保存草稿、补齐未完成工具的失败结果，标记 Interrupted。该恢复可重复启动，不重复插入，也不自动重放工具。用户可查看内容后重新生成。

边界：这是检查点恢复，不是模型连接续传；最后一次成功检查点之后的内容仍可能丢失。当前保存最近一条 RunCheckpoint，尚非完整运行轨迹数据库。已测试进程被强制杀死后的恢复，没有声称断电时零数据损失。

## 4. 工具隔离

现有 echo/add 默认运行在独立 worker 子进程。四个前端都先处理 worker 入口，再初始化配置/存储。父进程只发送 JSON 参数，不传递整个环境；只保留 Windows 运行所需 SystemRoot。

Windows Job Object 限制：

- 最多一个进程，阻止 worker 创建子进程；
- 256 MiB committed memory；
- 10 秒用户态 CPU；
- 关闭 Job（包括父进程退出）时杀死 worker。

Registry 保留 30 秒墙钟超时，stdin/stdout 各限制 64 KiB；失败、取消和超时都会关闭子进程控制句柄。创建资源限制失败时拒绝执行，绝不回退。未知工具和 Shell 请求拒绝。

权限边界依靠编译内置允许清单，当前不开放任意程序、文件或 Shell 工具。这不是运行任意恶意二进制的通用沙箱，也没有撤销外部副作用的能力。Windows 已做实际隔离验证；其他平台有独立进程/取消，但 Job 资源配额仅适用于 Windows。

主机扩展接口改名为 register_trusted_tool / register_trusted_in_process，明确声明调用者信任该扩展。该接口是主机代码的显式隔离豁免，模型无法调用；不应用于不可信插件。

## 验证

- Rust workspace（排除 Tauri 链接测试）：202 个测试通过。
- 包含真实跨进程占用测试：杀死持锁进程后重开成功。
- 包含真实强杀恢复测试：子进程写检查点、被杀、父进程加载恢复。
- 包含挂起 worker 的强制终止测试；例程 `cargo run -p tools --example isolation_probe --offline --locked` 验证真实 add/echo 子进程往返和未知工具拒绝。
- 前端类型检查与回归测试通过；回归使用非位置型 ID，检查编辑请求带 userId。
- Tauri cargo check 通过。完整安装包、真实 WebView、收费模型端点尚未验收。

运行要求：Rust 1.89+（标准库文件锁），本环境使用 Rust 1.97。改动延续前两轮，尚未 Git 提交。
