# 当前安排

[已完成](completed.md) · [待验收与候选](backlog.md) · [拆仓计划](repository-split-plan.md) · [使用指南](../usage.md)

**三仓拆分及第三批发布切换已完成。** 三个独立仓库放在同一 `llm-nest` 工作目录下，公开 `main` 已推送，统一采用 Apache-2.0。原客户端仓保留完整历史，服务端和 Agent 保留各自相关历史；用户的模型目录修改仍在 Agent 仓保持未提交。

| 部分 | 当前交付 |
| --- | --- |
| Web / Windows / Linux 客户端 | [0.1.1-alpha.13](https://github.com/NingCold/llm-nest/releases/tag/v0.1.1-alpha.13)；公开版本；本机 Windows 已更新本地 `alpha.14` |
| 独立 Agent | [0.1.1-alpha.13](https://github.com/NingCold/llm-nest-agent/releases/tag/v0.1.1-alpha.13)；桌面附带同版程序 |
| 服务端 | [0.1.1-alpha.12](https://github.com/NingCold/llm-nest-server/releases/tag/v0.1.1-alpha.12)；内网已部署，携带 Web `alpha.13` |

拆仓升级 `alpha.13` 时，升级前后 79 个应用及持久 WebView 文件一致。服务端已完成空库和旧库备份恢复验收；实际切换后 15 张关键表、既有调用数和强制加密设置保持，保留一致备份与旧镜像。没有新增真实模型调用。确切提交、摘要与范围见[第三批验证](../validation/README.md#拆仓第三批验证)及[兼容清单](https://github.com/NingCold/llm-nest-server/blob/main/server/deploy/compatibility.json)。

**本机 `alpha.14` 已完成日常账号重启解锁与原聊天验读。** 保持登录在完整退出再启动后直接恢复到锁屏，用户手动解锁；原身份保持。修复同机历史读取后，本机 Agent 完全停止时，十条原加密消息仍可从云端解密显示，同机发送选择保留。随后从界面启动原 Agent，私聊恢复“已同步云端”，十条正文一致，没有新增模型调用。详见[Windows 实际记录](../validation/windows.md#日常重启与离线节点历史验读2026-10-30)。

下一步是日常试用与反馈。可选自动解锁仍未开启，30 天到期、整机冷启动及离线重启等缺口继续保留。本次仅更新本地源码和安装包，未推送、打标签或公开发布 `alpha.14`；服务端 `alpha.12`、服务器 Web `alpha.13` 和随包官方 Agent `alpha.13` 保持。

这次修正已有聊天行为。Alpha 公开不等于稳定版或安全审计完成；干净 Windows、Linux GNOME 人工验收、实际离线重启和独立安全审查仍按[待验收](backlog.md#待验收)保留。后续批次根据日常试用反馈确定，不自动扩成细项队列。
