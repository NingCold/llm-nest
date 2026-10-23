# 迁移聊天与 AI 资料

旧版聊天迁入云端，角色、记忆和摘要迁入你自己的 Agent。迁移会留下完整旧库备份，先展示内容，再由你确认。首批使用命令行工具，尚无网页向导。

## 从旧本地版进入云端

迁移工具为 `python scripts/migrate_local.py`，只支持旧本地 SQLite v13。先退出旧程序（含后台进程），停止旧历史的写入，再运行：

```powershell
python scripts/migrate_local.py prepare --source <旧数据目录> --output <新的私有迁移目录>
python scripts/migrate_local.py preview --directory <迁移目录> --server https://<服务域名> --ca <测试CA.pem> --email <真人邮箱>
python scripts/migrate_local.py apply --directory <迁移目录> --server https://<服务域名> --ca <测试CA.pem> --email <真人邮箱> --apply-digest <预览返回的previewDigest>
python scripts/migrate_local.py node-packages --directory <迁移目录>
```

正常公共 CA 可省略 `--ca`。密码通过终端隐藏输入，会话 Cookie 仅在进程内存。工具不关闭 TLS 校验。`apply` 必须显式提供服务器预览摘要；摘要与账号及内容绑定，变化后重新预览。相同来源重试使用原 `cloud.json`，不能以新 sourceId 绕过冲突。

`prepare` 取得原目录 `.llmn.lock` 的操作系统锁，通过只读连接及 SQLite backup 保存一致的 `local.sqlite3`，保留旧库和配置。输出目录必须不存在；失败后的部分输出保留供检查，不能直接复用。Unix 输出目录为 0700，Windows 移除继承权限并仅授权当前用户。目录内含私密资料，请勿放进仓库、共享目录或上传备份服务。

只有白名单生成的 `cloud.json` 发往云端：账号、联系人、成员、原文、附件、引用及个人已读集合/置顶。原本地真人映射到登录真人，每个 AI 创建新账号，同名不合并。`sourceId` 为本地真人稳定 UUID。归档和旧导入窗口保留只读状态，AI 退出保留历史；本地真人已退出的窗口和不可访问引用不上传，并在预览列明。停用状态映射为云端 disabled。生成中消息作为中断片段导入，不恢复旧运行。

`local.sqlite3` 是完整私密备份；`deferred-local.json` 含旧参与规则和约定的完整本地预览，便于人工重新确认时间和内容。它们不上传。云端预览仅含这些事项的提示，不携带内部提示词或未来约定正文。迁移后不启动旧自动规则或约定。每次 HTTP 请求限制 128 MiB，超限应另行设计分批迁移，工具不会截断附件或消息。

`mapping.json` 保存新旧账号、窗口、消息和附件的对应关系。`node-packages` 根据这份对应关系，为每个 AI 生成独立资料包。输出子目录 `node-packages` 必须尚不存在，工具不会覆盖已有包。包只含角色、模型选择参数、记忆和摘要，不读取 TOML、模型 Key、环境变量值或节点授权。

接下来先在目标 Agent 配好模型、填写本机密钥，并在“我管理的 AI”用配对码授权它；然后运行下方的预览与导入命令。模型名称和参数需要与目标配置相符，工具不会替你换成另一个模型。

旧共享群摘要仅分发给当前仍在该窗口的 AI；来源有效且当前 committed 时保留 ready，否则清正文并标 stale。序列范围按服务端实际完成位置映射，不把旧 sequence 当完成位置。

当前节点记忆模型仅支持一个消息来源。旧记忆的 Manual 作者不算消息依赖；单一可映射消息来源按版本保留。多消息、记忆/摘要依赖、无效来源及不匹配范围的条目清正文并标 stale，需人工重新整理；旧完整正文、作者、范围及修订历史保留在 SQLite 备份，不声称这些旧字段已全部成为节点在线能力。工具不付费重新概括。

完成云端与节点验证后明确切换到云端正式记录，保留旧库作为只读备份，不继续两套可写历史。

## 为 AI 更换运行设备

先停止源节点，导出这个 AI 的资料。新文件会包含角色、模型参数、记忆和各窗口摘要：

```text
llmn-agent export-ai <原节点目录> <AI账号ID> <新的资料包.json>
```

在新设备按[节点说明](../agent/node/README.md)配置模型和密钥，再为同一个 AI 重新配对。账号和云端聊天继续沿用，旧节点的授权失效。将资料包复制到新设备的私有目录，运行：

```text
llmn-agent preview-import-ai <新节点目录> <AI账号ID> <资料包.json>
llmn-agent import-ai <新节点目录> <AI账号ID> <资料包.json> <预览返回的expectedProfileRevision>
llmn-agent run <新节点目录>
```

旧本地迁移生成的 `node-packages/<AI账号ID>.json` 也使用相同的预览和导入命令。目标 AI 需要尚无意识资料；导入已经有记忆的节点会明确报冲突。同一包重复提交不会覆盖后续编辑，也不会恢复已经遗忘的内容。

预览和导入都要连接云端核对来源。已经撤回或无权访问的原文不会重新成为有效记忆；预览也会同步目标节点已有资料的失效状态。自动摘要始终先关闭，未完成的旧调用不重做。确认资料正确后，再在聊天详情恢复接话、开启需要的摘要，按本地预览重新安排仍有需要的约定。

## 验证工具

`python scripts/test_migrate_local.py` 使用临时旧库检查备份和转换。完整的云端迁移、更换节点、备份恢复运行方法见[验证指南](validation/README.md#数据迁移与联网试用验证)，测试都使用专用账号和模拟模型。
