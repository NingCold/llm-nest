# 验证记录

日期：2026-09-10；基线 `26d527e`；Windows；Rust 1.97.0，默认 GNU toolchain；Node 24.9.0；项目 pnpm 11.22.0。

| 检查 | 结果 |
|---|---|
| `cargo fmt --all -- --check`（原始代码） | 通过 |
| `cargo test --workspace --all-targets --locked` | 初始受失效本地代理阻挡；临时清除该子进程代理后依赖下载成功 |
| 全 workspace 测试，补齐 MinGW PATH 后 | Tauri DLL 链接失败：`export ordinal too large: 130328`，因此没有通过全 workspace 验证 |
| `cargo test --workspace --exclude tauri-frontend --lib --bins --examples --offline --locked` | 173 个测试通过，0 失败；不包含桌面和 doctest |
| `cargo clippy --workspace --exclude tauri-frontend --all-targets --offline --locked` | 退出 0，有常规风格警告；没有开启 `-D warnings`，不等于零警告 |
| `pnpm typecheck` | 通过 |
| `pnpm exec vite build --outDir <临时目录>` | 通过，未覆盖仓库 dist；主 JS 2,547.40 kB，gzip 754.27 kB，有大 chunk 警告 |
| 协议探针 | 最终 7/7 观察断言通过，证实 7 种缺陷表现 |
| 前端探针 | 执行真实 store/hook 源码并模拟 React/传输；重生成、编辑语义和缓存比例观察均得到确认 |

现有 173 个测试分布：ai-client 82、chat 4、common 29、runtime 42、storage 8、tools 5、web-server 3；其余被测 binary/example 为 0。这不是覆盖率。

## 协议探针输出摘要

```text
response.failed was mapped to Done
resolved wire=actual-wire-model, sent model=alias
CRLF frames: []
comment followed by data in the same buffer: []
usage tail: [Ok(Done { usage: None })]
two requested tools: [Ok(ToolCall { id: "a", name: "add", arguments: "{}", thought_signature: None })]
split Unicode: "���"
test result: ok. 7 passed; 0 failed
```

探针没有修复业务代码；这里的“通过”意味着代码确实表现出预期要验证的缺陷。初次组合帧测试返回零个工具，暴露出另一个 buffer 排空问题；最终多工具探针把网络帧分开发送，隔离验证 finished/队列顺序错误。不是删去失败条件以声称实现通过。

## 重跑

从项目根目录运行前端探针：

```powershell
node docs/audit-2026-09-10/frontend_probes.cjs
```

Rust 探针依赖 ai-client 测试环境，可临时将本目录 `protocol_probes.rs` 复制到 `crates/ai-client/tests/audit_probe.rs` 后运行：

```powershell
cargo test -p ai-client --test audit_probe -- --nocapture
```

审查运行时临时加入 `C:\msys64\mingw64\bin` 到子进程 PATH；没有修改系统环境变量。Rust 测试访问的 HTTP fixture 仅监听随机 loopback 端口，并禁用 reqwest 代理，没有调用真实供应商。探针文件已从 crate 的正式 tests 目录移回审查资料，避免把“断言错误行为”的探针混入日常正确性测试。

安装依赖时最初离线缓存不足；随后按冻结锁文件联网补齐。没有更改依赖清单、锁文件或降低安装策略。前端构建输出在系统临时目录。Tauri 构建产生的 schema 变更已恢复。

## 尚未验证

- Tauri 在 MSVC 环境的完整构建与桌面操作回归；本次 GNU 链接失败不是所有平台都无法构建的证据。
- 真实模型/代理商的逐协议端到端兼容性、网络中断恢复与持续任务能力。
- 热更新和同会话并发的可控调度测试；报告对此使用“代码确认的竞争窗口”。
- 恶意 Origin 在不同浏览器本地网络策略下的可利用性；本次只确认服务侧 permissive CORS 和缺少认证。
- 安装包体积、启动内存、长会话与大附件压力；没有宣称 Rust/Tauri 性能优于竞品。
- 竞品性能、用户留存、付费意愿；建议路线仍须实际验证。
