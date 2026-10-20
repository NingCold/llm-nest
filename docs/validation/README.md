# 如何验证

[开发指南](../development.md) · [Windows](windows.md) · [Linux](linux.md) · [设计](../design/README.md)

先验证改动所属模块及相关调用链，再按范围扩大。使用独立临时配置、数据和模拟 provider；不要把个人配置、会话或密钥放进 fixture、CI 或发布资源。真实模型验收会产生费用，必须已有明确授权。

在仓库根目录执行：

```bash
cargo fmt --all -- --check
cargo test -p <受影响的包> --locked
cargo clippy -p <受影响的包> --all-targets --locked
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
cargo build -p web-server --locked
python scripts/acceptance.py
python scripts/social_acceptance.py
```

全量 Rust 检查用 `cargo test --workspace --locked`。Linux 先安装开发依赖，Windows 发布使用 MSVC；桌面须经 Tauri CLI 构建以嵌入生产前端，详见[发布指南](../releases.md)。上述是可执行入口，不表示每次文档更新都重新运行。

`acceptance.py` 检查空配置、旧接口退役、本地访问、显式导入/重启与 worker；`social_acceptance.py` 以模拟 provider 驱动真实业务。编译不证明安装通过，模拟模型不证明真实模型能力，自动输入中文不证明真实输入法候选正常。

## 必要场景

| 改动范围 | 应核对的结果 |
| --- | --- |
| 账号与聊天 | AI 身份及唯一私聊稳定，当前成员共享完整群历史；移除联系人不等于停用；屏蔽、管理员与只读账号视角权限正确 |
| 消息与执行 | 生成中连续发送、修订/撤回、暂停/取消、断线重连与重启不丢终态、不重复发布、不重放执行 |
| 意识与来源 | 记忆/摘要/群引用按角色与窗口隔离；源版本或成员变化使派生材料失效，迟到结果不能覆盖 |
| 配置与额度 | 在飞请求使用同一配置快照；非法模型和参数明确报错；自动轮数、额度、约定到期和停止规则正确 |
| 存储 | 事务失败无伪成功，目录冲突有排他锁，迁移与重复导入保留历史、原文件和幂等关系 |
| 附件和界面 | 图片/文件选择、取消、处理、剪贴板/拖放、草稿重开、原生保存/覆盖/失败与导出字节正确；生成及附件处理中关闭不丢材料 |
| 平台与包 | 真实目标设备的窗口/缩放/输入、实际路径、快捷方式、安装标识及 worker；两条 HTTP/IPC 适配保持同一业务语义 |

测试检查用户可见结果和边界，不只镜像实现。当前尚未开放的工具执行与多人联网，不作为已实现功能的验收场景。
