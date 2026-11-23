# LLM Nest 客户端工作约定

Web 与桌面共用 CloudApp；Android 是[独立原生工程](android/README.md)。跨仓联调先查[版本组合](docs/project/current.md#现在使用的是哪个版本)。

## 协作与提交

- 先看 `git status --short`，保留用户修改。跨机器接续先同步 `main`，不强制覆盖。日常改动直接在 `main` 提交；用户要求或大型实验需隔离时，先说明再建分支。本地提交不包含推送、开 PR 或发布授权；交付时区分提交、推送、合并和安装状态。
- 可按需使用子代理；主代理负责分工、验收和提交。跨模块先定共享接口；按可体验的结果划定范围和完成条件，新想法进候选清单。
- 按当前调用链在负责的模块修改，不为兼容旧接口改变生命周期或复制业务逻辑。校验放在外部输入、存储事务、权限和模型协议边界，内部沿用已成立的条件，不吞错或静默回退。
- 一件事维护一处说明：使用指南写操作，设计文档写契约，项目进度写安排，验收记录写日期、版本、环境、结果与缺口。

## 客户端契约

- Cargo workspace 只含 `desktop/src-tauri`。连接器固定 Git 提交；桌面按版本、平台及 SHA-256 消费独立 Agent，用 `catalog` 取目录、`check` 验配置，不编译 Agent、不读节点 SQLite。协议改动同步 Web 与 Tauri 两条链路，不恢复旧本地 Runtime。
- 保留[安装标识与图标源](docs/packaging/windows-branding.md)。[本机节点目录](desktop/README.md#从源码运行)按服务器和真人账号隔离；保留目录锁、实例 UUID 和后台托盘行为。
- [草稿](docs/design/frontend.md#本机未发送草稿)使用 `llm-nest-social-drafts`，按服务器、账号和窗口隔离，默认本机明文。发送前保存完整命令；结果未知只手动重试同一请求。[关闭窗口](docs/design/frontend.md#桌面窗口与关闭)前等待保存，失败则保留窗口；选择转入后台或取消关闭时发出 `desktop-close-retained`。自绘窗口保留透明启动、边框和交互元素不触发拖动的行为。
- 沿用[消息权限、已读、搜索与附件](docs/design/frontend.md#默认云端页面)及[历史转换规则](docs/usage.md#把旧聊天转为加密记录)。`sealedV1` 使用官方 Matrix SDK，失败不降级为明文。
- [保持登录与本机解锁](docs/usage.md#注册和登录)分开处理。自动解锁默认关闭；开启需可信设备、已确认保存恢复文件及手动口令验证。Windows 用 DPAPI，Web 用不可导出 AES 密钥；保持登录不授予新设备信任。锁屏先存草稿，再关闭 SDK、卸载聊天与订阅，仅保留账号和导航；Agent 继续运行。暂停跨刷新、重启生效，迟到的解锁结果须复核会话、服务器、账号和设备。退出清除自动解锁材料，保留 SDK 库。

## 验证与清理

- 按[客户端验证清单](docs/validation/README.md#常用检查命令)运行检查；桌面安装包用 Tauri CLI 构建，正式打包传入实际 Agent 产物清单。测试用临时配置、数据和模拟 provider；真实 API 调用须明确授权。
- `config/llmn.toml`、`.env`、用户会话、模型 Key 不进提交、安装包、CI 或日志。保留环境变量引用；Key 不出现在命令行、读取配置的 IPC 或 DTO。
- 每批编译后确认无 Cargo/rustc 运行，按[清理步骤](docs/development.md#检查改动)用 `scripts/clean-build-artifacts.py` 预览完整清单，再逐文件、逐空目录删除并核验。不递归删除，不全量 `cargo clean`。Windows 测试复用 MSVC 默认 `target/debug`，打包使用对应 release target。
- 安装包保留当前版和上一可回退版，不重复复制。服务器只清理本项目已核验的旧镜像和无用构建缓存；备份不重复收录可再生浏览器缓存，账号、聊天、密钥及恢复材料不按日期删除。操作受限时说明影响；可脚本化的步骤先核对预览，再交付脚本和用法，不绕过限制。
