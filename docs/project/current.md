# 当前安排

[已完成](completed.md) · [待验收与候选](backlog.md) · [拆仓计划](repository-split-plan.md) · [使用指南](../usage.md)

**三仓拆分及第三批发布切换已完成。** 三个独立仓库放在同一 `llm-nest` 工作目录下，公开 `main` 已推送，统一采用 Apache-2.0。原客户端仓保留完整历史，服务端和 Agent 保留各自相关历史；用户的模型目录修改仍在 Agent 仓保持未提交。

| 部分 | 当前交付 |
| --- | --- |
| Web / Windows / Linux 客户端 | [0.1.1-alpha.13](https://github.com/NingCold/llm-nest/releases/tag/v0.1.1-alpha.13)；本机 Windows 已覆盖安装 |
| 独立 Agent | [0.1.1-alpha.13](https://github.com/NingCold/llm-nest-agent/releases/tag/v0.1.1-alpha.13)；桌面附带同版程序 |
| 服务端 | [0.1.1-alpha.12](https://github.com/NingCold/llm-nest-server/releases/tag/v0.1.1-alpha.12)；内网已部署，携带 Web `alpha.13` |

本机升级前后 79 个应用及持久 WebView 文件一致。服务端已完成空库和旧库备份恢复验收；实际切换后 15 张关键表、既有调用数和强制加密设置保持，保留一致备份与旧镜像。没有新增真实模型调用。确切提交、摘要与范围见[第三批验证](../validation/README.md#拆仓第三批验证)及[兼容清单](https://github.com/NingCold/llm-nest-server/blob/main/server/deploy/compatibility.json)。

**下一步先做日常账号重启后的登录、解锁与历史验读。** 本机已通过原“LLM Nest 内网试用”入口连接服务器，当前停在登录页。沿用已有账号、可信设备及恢复资料，确认“小巢”和原聊天可读，再继续试用保持登录、手动锁定及可选自动解锁；不用重新创建身份。两项自动选项默认关闭，各自最多有效 30 天。

这批没有新增产品功能。Alpha 公开不等于稳定版或安全审计完成；干净 Windows、Linux GNOME 人工验收、实际离线重启和独立安全审查仍按[待验收](backlog.md#待验收)保留。后续批次根据日常试用反馈确定，不自动扩成细项队列。
