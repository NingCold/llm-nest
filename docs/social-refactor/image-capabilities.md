# 图片消息第五批：模型图片能力与发送提示

日期：2026-10-13

[总纲](overview.md) · [界面说明](08-frontend-and-desktop.md) · [配置指南](../configuration.md) · [实施顺序](TODO.md)

本批让模型设置、管理员模型选择和聊天输入区显示图片能力，并在实际模型调用前检查。Web 与 Tauri 共用 Runtime 和 AiClient；不新增 SQLite 迁移，仍为 v13。

## 能力来源与保存

能力属于确定的 provider/model，分为支持、不支持和未知。优先级是人工配置 > 同步目录 > 内置目录 > 未知。按完整标识匹配，不通过名称中的 vision、VL 或供应商品牌推断能力。

Pi 目录的 `input` 包含 `image` 时为支持，包含 `text` 而不包含 `image` 时为不支持；字段缺失、null、空数组或只有未识别项时不产生能力声明。旧缓存缺失此字段仍可读取。来源类型见 [Pi 模型定义](https://github.com/earendil-works/pi/blob/main/packages/ai/src/types.ts)。这次核对了 [Anthropic 目录](https://pi.dev/api/models/providers/anthropic)，内置离线基线只列入该来源明确声明图片输入的五个精确 ID：claude-haiku-4-5、claude-opus-4-5、claude-opus-4-6、claude-sonnet-4-5、claude-sonnet-5；其余条目继续读取同步目录，缺少信息时显示未知。

设置中的“模型图片能力”提供跟随目录、支持图片、不支持图片，并显示当前来源。人工选择写入对应模型的 `image_input = true/false`；返回“跟随目录”只移除这个字段。Runtime 用 toml_edit 定点修改，保留模型别名、其他字段与注释，整体校验和原子保存成功后发布新快照。同步、热更新和重启都保留人工优先级。正在执行的请求沿用原快照。

自动读取是读取目录元数据，目录同步仍由用户点击触发。没有后台识图探测或付费调用；声明“支持”不表示已经验证特定账号、中转服务、图片格式或识图质量。模型报出的鉴权、限流和网络错误不会被写成“不支持图片”。

## 聊天与实际执行

- 私聊显示对方的能力；群聊有结构化 @ 或回复 AI 时显示对应对象，否则显示当前所有 AI 成员。提示不改变群接话规则，也不禁用真人发送。
- 草稿含图片、群引用或回复图片时显示提示。普通聊天只收到账号 ID 对应的三态标识，不收到内部绑定、模型参数或角色设定。
- 后端以本次实际请求的全部 `ContentPart::Image` 为准，包括意识系统选中的旧图片和共同群引用；前端不重新构造上下文。
- 明确不支持且请求包含图片时，AiClient 在调用 provider 前返回 `ImageInputUnsupported`。社交运行按既有失败流程保存 `image_input_unsupported`，界面说明记录已保存、该 AI 未调用模型；其他群成员继续独立运行。
- 支持或未知时保留原图片并照常请求；不丢弃图片、不代做 OCR、不更换模型。明确不支持的模型仍可处理不含图片的请求。

这项检查位于模型分发边界，AiRun 已经开始时沿用原有失败与额度规则：失败记录保留，自动调用预留额度不返还，不把“未调用 provider”写成生成成功。窗口摘要当前仅使用文字及附件占位，不受原文图片数量直接影响。

## 共享接口

| 内容 | 契约 |
| --- | --- |
| 保存人工覆盖 | `POST /api/models/image-input` / IPC `set_model_image_input` |
| 参数 | `{provider, model, imageInput}`；imageInput 为 true、false 或 null（跟随目录） |
| 成功返回 | 更新后的 ProviderInfo 数组 |
| 设置 ModelInfo 与社交 ModelOption | `imageInput: {support, source}`；support 为 supported/unsupported/unknown，source 为 config/catalog/builtin/unknown |
| 普通社交 Init | `imageInputs: {账号ID: support}`；只读账号视角无输入区，返回空映射 |
| 执行错误 | `RunDetails.error.code = image_input_unsupported` |

设置读取和保存复用既有本地访问边界；HTTP/IPC 传入相同 DTO。普通账号公开资料不增加内部配置。能力设置不产生聊天事件，离开设置回到社交页会刷新快照；实际执行始终按后端本次冻结的配置检查。

## 验证

- `cargo fmt --all -- --check`、`cargo test --workspace --locked` 通过，工作区 289 项测试包含 Tauri；`cargo clippy --workspace --all-targets --locked` 完成，仍有既有警告，不声称无警告。
- 前端 typecheck、16 组测试与 `build:social-runtime` 通过。新测试覆盖实际设置组件、保存失败后重试、恢复跟随目录、私聊/群聊提示以及 HTTP/IPC 的 true/false/null 传递。
- `cargo build -p web-server --locked`、`scripts/acceptance.py`、完整 `scripts/social_acceptance.py` 与 `scripts/social_acceptance.py --only-image-capabilities` 通过。专项使用本机模拟 provider，核对未知允许请求、历史图片与群引用拦截、真人消息保留、混合群独立回复、纯文字窗口、重启、非法配置拒绝及实际请求次数。
- Runtime 测试覆盖人工覆盖与目录缓存的优先级、同步/重载/重启、移除覆盖、注释与别名保留、非法模型不改文件以及旧运行快照不变。
- 真实浏览器界面检查了人工保存/恢复自动、群中三种状态、@ 对象变化、图片发送和混合群回复；浏览器无 warning/error。截图在忽略的 `target/image-capabilities-settings.png` 与 `target/image-capabilities-chat.png`。

本批使用隔离配置与临时数据，没有调用付费模型或更新本机安装程序。浏览器验证不替代 Windows/Linux 原生 WebView 实机验收；安装与平台待办仍见 [尚未完成](pending.md)。

## 后续范围

图片消息前五批基础链路已完成。下一批建议细化通用文件附件的选择、发送、保存和下载。动态图、图像描述/OCR、视觉能力筛选、真实模型识图验收与超长多图窗口的流式导出继续作为独立扩展，不纳入本批完成范围。
