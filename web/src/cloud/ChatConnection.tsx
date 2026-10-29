import type { Conversation } from "./types";
import type { LocalView } from "./local-api";
import { shareConversationHistory } from "./crypto/engine";

export interface ChatConnectionProps {
  cryptoUnlocked?: boolean;
  enableEncryption?: () => Promise<void>;
  localChat?: LocalView;
  localSelected?: boolean;
  prepareLocal?: () => Promise<void>;
  syncLocal?: () => Promise<void>;
  stopLocal?: () => Promise<void>;
  closeLocal?: () => Promise<void>;
}

export function localConnectionLabel(localChat?: LocalView) {
  if (!localChat) return "本机未连接";
  if (localChat.syncState === "conflict") return "同步冲突";
  if (localChat.syncState !== "synced") return "待同步云端";
  return "已同步云端";
}

export function ChatConnectionDetails(p: ChatConnectionProps & {
  conversation: Conversation;
  meId: string;
  requireEncryption?: boolean;
  hasPlaintextMessages: boolean;
  pending: boolean;
  act(fn: () => Promise<unknown>): Promise<void>;
  reconnect(): Promise<unknown>;
}) {
  const { conversation } = p;
  const canEnableEncryption = !conversation.readOnly && !conversation.encryption &&
    (conversation.kind === "direct" || conversation.members.some(m => m.account.id === p.meId && m.role !== "member"));
  return <>
    <section className="cloud-detail-section cloud-chat-connection">
      <h3>端到端加密</h3>
      {conversation.encryption ? <>
        <p>{p.cryptoUnlocked ? "新消息已启用端到端加密。" : "新消息已启用端到端加密；请先在“我的账号 → 可信设备与恢复”解锁此设备。"}</p>
        <p>{conversation.encryption.historyComplete ? "旧历史已转换。" : p.hasPlaintextMessages ? "当前已加载的消息中含尚未转换的明文记录。可在下方预览完整转换范围。" : "旧历史转换尚未确认完成；可在下方预览转换范围。"}</p>
        {conversation.kind === "group" ? <>
          <button className="social-button secondary" disabled={!p.cryptoUnlocked} onClick={() => void p.act(() => shareConversationHistory(conversation))}>分享历史密钥</button>
          <p>新成员或新设备缺少历史密钥时，请由可读取历史的成员分享。{conversation.encryption.historyComplete ? "只发给当前已核验的成员设备，其他成员须先授权。" : "所有当前成员须先完成设备核验。"}</p>
        </> : <p>新设备请在“我的账号 → 可信设备与恢复”使用恢复文件或已有可信设备授权恢复。</p>}
      </> : <>
        <p>{p.requireEncryption ? "服务器已要求加密，请先准备可信设备并开启此窗口加密，再发送新内容。" : "此窗口尚未开启端到端加密。"}</p>
        {canEnableEncryption && p.enableEncryption && <button className="social-button secondary" onClick={() => {
          if (window.confirm("开启后不可回退明文。请确认双方可信设备及恢复文件已准备好，旧历史可在聊天详情的“转换旧历史”中备份并转换。")) void p.act(p.enableEncryption!);
        }}>开启端到端加密</button>}
      </>}
    </section>
    {(p.prepareLocal || p.localSelected || p.localChat) && conversation.kind === "direct" && conversation.peer?.kind === "ai" && <section className="cloud-detail-section cloud-chat-connection">
      <h3>同机私聊</h3>
      {p.localSelected ? <>
        <p role="status">{localConnectionLabel(p.localChat)}{p.localChat?.running ? " · 本机正在回复" : ""}</p>
        <p>发送使用本机 Agent，不会自动改发云端。断开云端后仍可在本机保存，重连核对权限和来源后再同步。</p>
        {!p.localChat ? <button className="social-button secondary" onClick={() => void p.act(p.reconnect)}>重新连接本机</button> : <>
          <p>{conversation.encryption ? "支持文字、图片、文件、回复及已预取的群话题来源。" : "未加密的同机私聊暂不支持附件；可先开启端到端加密。"}{p.localChat.hasMore && "当前仅显示节点已缓存的历史。"}</p>
          {p.localChat.error && <p role="alert">{p.localChat.error}</p>}
          <button className="social-button secondary" onClick={() => void p.act(p.syncLocal!)}>重试云端同步</button>
          <button className="social-button secondary" disabled={p.localChat.running || p.localChat.syncState !== "synced" || p.pending} onClick={() => void p.act(p.closeLocal!)}>返回云端发送</button>
          {p.localChat.running && <button className="social-button secondary" onClick={() => void p.act(p.stopLocal!)}>停止本机回复</button>}
          {(p.localChat.running || p.localChat.syncState !== "synced" || p.pending) && <p>停止本机回复、确认发送结果并同步完成后，可返回云端发送。</p>}
        </>}
      </> : <>
        <p>连接此桌面上的 Agent，在同一私聊中直接发送并保存到节点。</p>
        {p.prepareLocal && <button className="social-button secondary" onClick={() => void p.act(p.prepareLocal!)}>开启同机私聊</button>}
      </>}
    </section>}
  </>;
}
