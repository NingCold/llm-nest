# AGENTS.md — LLM Nest 客户端

本仓维护 `web/` 共用聊天界面和 `desktop/` Tauri 客户端。产品说明见 README，使用见 docs/usage.md，总进度见 docs/project。服务端与连接器由 https://github.com/NingCold/llm-nest-server 维护，Agent 由 https://github.com/NingCold/llm-nest-agent 维护；两份新公开仓已创建，发布状态以总进度和各仓 Release 为准。

## 工作前先确认

- 读取 `git status --short`，保留用户的未提交修改。不要混入无关生成文件、环境配置或测试数据。
- 通过实际调用链确认类型、签名和行为；过往报告描述的是当时的版本，不应覆盖当前代码。
- 修改只落在负责该行为的模块。复用成熟的协议与基础能力；社交用例以新架构约定为准，不为适配旧接口改变领域或生命周期，不为单一前端复制业务逻辑。
- 避免过分防御性编程：在外部输入、存储事务、权限和模型协议边界校验；内部使用已经成立的类型与不变量。不重复校验、不吞错或静默回退，不为假设性场景提前增加空框架、备用路径和配置开关。
- 正常验证使用独立临时配置/数据和模拟 provider。真实 API 验收会产生费用，仅在已有明确授权时执行。
- `config/llmn.toml`、`.env`、用户会话、API Key 不得提交、打包、上传到 CI 或打印到日志。保留环境变量引用，不把它替换为明文。
- 图标的唯一编辑源是 `assets/branding/app-icon.svg`；不要单独修改某一个派生尺寸。
- 已授权的操作若因工具或权限限制无法执行，可脚本化的部分应提供脚本和运行方法交给用户；说明影响范围，能预览的先验证预览，不把未执行写成完成。
- 每批编译结束后，代理主动检查并清理无用产物，不等用户再次提醒。确认没有并行 Cargo/rustc 后，用 `scripts/clean-build-artifacts.py` 先生成完整清单、再逐文件删除，目录确认空后逐个删除并核验；禁止递归删除命令/API。保留可复用的依赖和构建缓存，不用全量 `cargo clean` 代替整理。Windows 普通测试复用 MSVC 的默认 `target/debug`，安装包沿用明确的 release target，避免重复生成另一套 debug。
- 安装包保留当前和上一可回退版本，不在工作目录复制同一包；服务器只清理本项目已核验的旧镜像及无用构建缓存。备份按实际内容判断，账号、聊天、密钥和恢复材料不能按日期自动删除；新备份不重复包含可再生浏览器缓存。删除被策略拒绝时不换方式绕过，交付已核对的脚本并标明待用户执行。

## 模型分工约定

- 用户已授权：需求和边界明确的日常功能、局部修复、测试与文档整理，默认委派给 GPT-6.1-Sol 子代理执行，无需逐次询问。
- 主代理负责明确架构与行为约定、划分互不冲突的修改范围、审核实现和验证结果，并完成文档一致性检查及最终交付。跨模块工作先确定共享契约，再分派实现。
- 计划按完整可体验的结果安排批次，写明范围和结束条件；前端、后端、验证与文档是批次内的工作，不逐个升级为产品阶段。新想法放入候选清单，不自动延长当前批次或成为下一批；依赖明确且互不冲突的工作可并行。
- 委派时提供必要的项目背景、约束、文件范围与验收要求；保留用户未提交修改。子代理负责自己范围内的实现与相关检查，统一提交由主代理完成。
- 主代理和子代理都遵守上面的“避免过分防御性编程”约定；任务交代与代码审查明确检查这一点。边界校验保留，内部直接使用已成立的不变量，不叠加重复检查、静默兜底或假设性扩展。

## Git 工作约定

- 普通功能修改、问题修复和文档更新默认直接在 `main` 上开发并提交，不自动新建 `codex/*` 分支。
- 仅在用户明确要求，或较大的实验性改动确需隔离时新建分支；创建前说明原因。
- PR 不作为日常默认流程。需要创建时，单独说明用途和目标分支，不把推送分支当作已经合入 `main`。
- 多台机器接续工作前，确认工作区状态并同步 `main`；保留未提交修改，不能用强制覆盖解决分歧。
- 汇报时分别说明本地提交、远端推送与合并的实际状态；用户要求提交不等于要求推送、创建 PR 或发布版本。

## 客户端边界和不变量

- 根 Cargo workspace 只含 desktop/src-tauri；连接器使用固定 Git 提交，不引用兄弟仓源码。桌面消费显式版本、平台和 SHA-256 的 Agent 产物，不编译 Agent，不读取节点 SQLite。目录由 Agent catalog 提供，配置由 Agent check 校验。
- Web 与桌面共用 CloudApp；不得恢复旧本地 Runtime。客户端只实现界面、HTTPS/SSE、原生 IPC、窗口、文件保存和本机 Agent 生命周期。
- 保留 identifier com.llmnest.tauri、LLM-Nest 二进制、安装识别和 ~/.llmn 及 LLMN_DATA_DIR；保持 desktop-agents 按服务器及真人隔离、目录锁及实例 UUID、后台和托盘行为。
- 保持草稿 IndexedDB 名 llm-nest-social-drafts、按服务器/账号/窗口隔离，完整命令先落盘再发送；未知结果只手动重试同一请求。关闭先等待草稿确认，失败保持窗口；后台/取消发 desktop-close-retained。
- 加密使用官方 Matrix SDK，sealedV1 启用后失败不能退明文。登录会话与 SDK 解锁分开，自动解锁默认关闭；可信设备和恢复文件确认及手动口令成功后才允许开启。Windows DPAPI；Web 不可导出 AES 包装；不得因保持登录建立新设备信任。
- 锁屏先保存草稿，关闭 SDK，卸载聊天 DOM、订阅和状态；保留导航和账号，独立 Agent 继续运行。暂停跨刷新重启，迟到解锁必须复核会话、服务器、账号和设备。退出清理自动材料，不删 SDK 库。默认草稿是本机明文。
- 保持当前消息权限、读位置、搜索、密文附件和历史转换语义；协议变动同步 Web/Tauri 链路。模型 Key 不进入参数、IPC 读取、DTO 或日志。
- assets/branding/app-icon.svg 是图标唯一编辑源。Windows/Linux 自绘窗口保持拖动排除交互元素、透明启动与当前边框约定；平台历史不代表新版本实机验收。

## 验证与交付

运行 pnpm --dir web typecheck、pnpm --dir web test、cargo fmt --all -- --check、cargo test -p tauri-frontend --locked、cargo clippy -p tauri-frontend --all-targets --locked，以及 scripts/check-boundaries.py、scripts/test_release.py、scripts/test_build_artifacts.py 和 desktop/scripts/prepare-agent.test.mjs。桌面构建使用 Tauri CLI；不以裸 cargo release 代替安装包验收。正式 Agent 发布前 CI/打包必须显式提供实际产物清单，不能伪造 URL、版本或摘要。

前端代码在 web/src/cloud 和 web/src/chat；桌面原生代码在 desktop/src-tauri。修改只落在实际负责模块，维持既有生命周期，不增加重复校验、静默回退或假设性框架。文档直接修订当前说明，平台验证保留日期与版本，最终报告区分本地提交、推送和安装。
