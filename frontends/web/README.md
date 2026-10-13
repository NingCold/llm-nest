# Web 界面

[应用界面](../README.md) · [开发指南](../../docs/development.md)

这里是 LLM Nest 的全部页面：消息、通讯录、聊天窗口、AI 角色管理和设置。桌面应用也使用这套界面。屏蔽与解除可以在联系人资料或私聊详情中操作。

聊天输入框支持选择、拖入和粘贴图片，发送前可预览、移除，历史图片可以点开放大并保存原图。聊天详情中可以导出记录，勾选“包含图片”会一起下载原图。相关界面在 [ChatComposer.tsx](src/social/ChatComposer.tsx)、[MessageImages.tsx](src/social/MessageImages.tsx) 和 [ConversationExportPanel.tsx](src/social/ConversationExportPanel.tsx)。

图片超限时可以先缩放或压缩，预览结果后加入草稿。已有草稿中的图片也能处理，取消后保留原草稿。相关代码在 [ImageProcessingDialog.tsx](src/social/ImageProcessingDialog.tsx) 和 [image-processing.ts](src/social/image-processing.ts)。

输入区还会提示 AI 是否支持看图。模型设置可以跟随目录或人工修正，相关界面在 [ModelImageInput.tsx](src/components/settings/ModelImageInput.tsx) 和 [ImageCapabilityHint.tsx](src/social/ImageCapabilityHint.tsx)。

未发送的文字、图片和回复对象会自动保存在当前浏览器，刷新或重新打开同一地址后可以继续编辑。保存失败时，输入框下方会提示重试。上次发送结果尚未确认时，可以查看原内容并手动重试。草稿保存代码在 [draft-storage.ts](src/social/draft-storage.ts)。

## 开发时运行

先按[配置说明](../../config/README.md)准备 `config/llmn.toml`。下面的命令都在仓库根目录执行。

安装依赖：

```bash
pnpm --dir frontends/web install --frozen-lockfile
```

打开两个终端，分别运行：

```bash
# 终端一：本地服务
cargo run -p web-server

# 终端二：页面开发服务器
pnpm --dir frontends/web dev
```

打开 `http://localhost:1420`。修改页面后，浏览器会自动更新。

## 找到要改的页面

消息和通讯录从 [src/social/App.tsx](src/social/App.tsx) 开始；未读、置顶和搜索在 [ConversationList.tsx](src/social/ConversationList.tsx)；聊天窗口在 [LiveChat.tsx](src/social/LiveChat.tsx)，账号页面在 [AccountPages.tsx](src/social/AccountPages.tsx)。设置页面在 [components/settings](src/components/settings/SettingsPage.tsx)。

检查和构建也在仓库根目录执行：

```bash
pnpm --dir frontends/web typecheck
pnpm --dir frontends/web test
pnpm --dir frontends/web build
```

构建结果保存在本目录的 `dist`，由 [web-server](../../crates/web-server/README.md) 提供给浏览器。使用临时聊天数据检查界面的方法见[开发指南](../../docs/development.md#运行入口)。
