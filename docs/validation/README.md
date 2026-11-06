# 如何验证客户端

从改动影响的用户操作开始：网页与桌面共用聊天界面，窗口、文件保存、本机 Agent 和打包由桌面端负责。Android 的原生界面、密文互通和 APK 使用[独立模拟器验收](../../android/testing.md)。使用独立测试资料和模拟模型；真实模型验收需另有授权。

## 常用检查命令

按[开发指南](../development.md)准备依赖，在客户端仓库根目录运行：

```bash
python scripts/check-boundaries.py
cargo fmt --all -- --check
cargo test -p tauri-frontend --locked
cargo clippy -p tauri-frontend --all-targets --locked
corepack pnpm@11.22.0 --dir web typecheck
corepack pnpm@11.22.0 --dir web test
python scripts/test_build_artifacts.py
node --test desktop/scripts/prepare-agent.test.mjs scripts/web-notices.test.cjs
python scripts/test_release.py
```

安装包通过 Tauri CLI 构建，参数和 Agent 产物见[发布指南](../releases.md)。数据库、迁移与跨仓联调见[服务端验证指南](https://github.com/NingCold/llm-nest-server/blob/main/docs/validation.md)；节点、模型调用和记忆处理见[Agent 验证指南](https://github.com/NingCold/llm-nest-agent/blob/main/docs/validation/README.md)。

## 关键回归场景

| 改动 | 检查用户能否完成什么 |
| --- | --- |
| 账号与聊天 | 找到原来的唯一私聊；群成员按权限查看历史，普通联系人不能代替管理员操作。 |
| 执行与意识 | 连续输入、暂停、撤回或断线后不会重复回复；失效的来源不再进入记忆、摘要和后续对话。 |
| 角色与额度 | 修改影响新的模型调用；已开始的回复沿用原设定，自动接话和一次性约定遵守额度。 |
| 存储与附件 | 重开后找回草稿；保存失败或取消时保留输入。检查下载和导出的文件能否打开，窗口关闭时正在读写的附件是否保留。 |
| 桌面与安装 | 在目标系统实际使用输入法、快捷方式、窗口控件和托盘；覆盖安装后用原账号读取原来的记录。 |

## 解锁等待优化

排查实际等待时，先关闭开发者工具，再手动解锁。列表出现后打开 **设置 → 关于 → 解锁耗时**（试用版 `alpha.21`），读取本次运行的阶段和毫秒数。开库、本机信任、联网校验与首列表各有计时，部分步骤并行，不能直接相加。界面记录的绘制时机也不同于实际屏幕呈现。环境及已有测量见[Windows 记录](windows.md#解锁与设备目录同步优化2026-11-01)。

### 用隔离 WebView 检查 SDK 耗时

`scripts/sdk-open-benchmark.cjs` 提供只在本机监听的测试页面和结果收集器，`sdk_open_benchmark` example 使用单独的浏览器资料目录，不启动产品账号、云端连接或 Agent。页面只加载固定的官方 SDK 18.9.0，所有身份和口令均为合成测试数据。

Windows 先执行 `cargo +stable-x86_64-pc-windows-msvc build -p tauri-frontend --example sdk_open_benchmark --locked`。建立一个临时工作目录，然后运行 `node scripts/sdk-open-benchmark.cjs 1437 <结果文件绝对路径>`。收集器保持运行，另启 `target/debug/examples/sdk_open_benchmark.exe <专用资料目录绝对路径> <测试URL>`；资料目录末级必须叫 `synthetic-profile`。

- 首次 URL 为 `http://127.0.0.1:1437/?mode=seed`，建立并核验合成可信设备。
- 程序完全退出后，以同一目录和端口运行 `?mode=measure`，记录新进程首次开库和本进程随后三次重开。
- 需要验证较大设备内部资料时，先运行一次 `?mode=grow`。它模拟 100 批公钥被领取后的正常补货，保留相应合成私钥，不修改 SDK 或存储格式；完成后再测量。
- 调试干扰对照使用 `?mode=measure&manual=1&devtools=1`，并给 EXE 增加第三个参数 `devtools`。等待工具显示后，点击测试窗口的 **开始合成测量**。正常组应关闭 DevTools，且没有并行编译。

结果写入指定文件。每组结束后等待测试程序和它的 WebView 退出，再测下一组。只使用 `success` 和全部 `valid` 都为真的样本；合成资料适合比较同一条件下的成本。

## 查看已有结果

- [集成与版本验收](records.md)：社交、加密、恢复、迁移及拆仓。
- [Windows](windows.md) · [Linux](linux.md)：实际安装和桌面交互。
- [待验收与候选](../project/backlog.md)：仍缺的环境与使用证据。
