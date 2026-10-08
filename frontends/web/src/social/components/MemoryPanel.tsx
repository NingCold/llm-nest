import { Brain, FileText, Pencil, Plus, RotateCcw, Trash2 } from "lucide-react"
import { useSocialStore } from "../store"
import { memoryScope, sameScope } from "../types"
import type { Conversation } from "../types"
import type { Editor } from "./EditorDialogs"
import { shortTime } from "./common"
import { navigate } from "../router"

export function MemoryPanel({ conversation, edit }: { conversation: Conversation; edit(editor: Editor): void }) {
  const snapshot = useSocialStore(s => s.snapshot!)
  const action = useSocialStore(s => s.action)
  const scope = memoryScope(conversation)
  const memories = snapshot.memories.filter(m => sameScope(m.scope, scope) && m.status !== "forgotten")
  const summary = snapshot.summaries.find(s => s.conversationId === conversation.id)
  return <div className="memory-panel">
    <div className="section-title"><span><Brain />{conversation.kind === "group" ? "群聊记忆" : "共同记忆"}</span><button className="text-button" onClick={() => edit({ kind: "memory", scope })}><Plus />添加</button></div>
    <p className="section-description">{conversation.kind === "group" ? "属于这个群的约定与共同经历。" : "你与这位朋友的偏好、约定与共同经历。"}</p>
    {memories.length ? memories.map(record => <article key={record.id} className="memory-card">
      <div className="memory-card-top"><span className={record.status === "stale" ? "status-tag warning" : "status-tag"}>{record.status === "stale" ? "来源已变化，待核对" : "已确认"}</span><span>{shortTime(record.updatedAt)}</span></div>
      <p>{record.status === "stale" ? "来源消息已编辑或撤回，请重新确认这条记忆。" : record.text}</p>
      <div className="memory-card-bottom">{record.sourceMessageId ? <button className="text-button" onClick={() => navigate("messages", conversation.id)}><FileText />来自聊天消息</button> : <span>手动记录</span>}<div><button aria-label="编辑这条记忆" onClick={() => edit({ kind: "memory", scope, record })}><Pencil /></button><button aria-label="遗忘这条记忆" onClick={() => edit({ kind: "confirm", title: "遗忘这条记忆？", description: "移除记忆条目，原聊天消息仍会保留。要移除原文，请在聊天中撤回对应消息。", confirm: async () => !!await action(async api => { await api.forgetMemory(record.id, record.revision); return true }) })}><Trash2 /></button></div></div>
    </article>) : <div className="quiet-empty"><Brain /><p>还没有共同记忆</p><small>可以手动添加，也可以从聊天消息选择“记住”。</small></div>}
    <div className="section-title summary-heading"><span><FileText />历史摘要</span><button className="text-button" disabled={!snapshot.messages.some(m => m.conversationId === conversation.id && m.status === "complete")} onClick={() => void action(api => api.refreshSummary(conversation.id))}><RotateCcw />整理预览</button></div>
    <p className="section-description">摘要按聊天窗口归属，完整聊天记录始终保留。当前整理功能只展示原文摘录。</p>
    {summary ? <article className="summary-card"><div><span className={summary.status === "stale" ? "status-tag warning" : "status-tag"}>{summary.status === "stale" ? "来源已变化" : "预览摘要"}</span><small>覆盖 {summary.sourceIds.length} 条消息</small></div><p>{summary.status === "stale" ? "来源消息已编辑或撤回，请重新整理摘要。" : summary.text}</p><small>更新于 {shortTime(summary.updatedAt)}</small></article> : <div className="quiet-empty"><FileText /><p>还没有历史摘要</p><small>聊天积累后，可以在这里查看整理结果。</small></div>}
    <div className="scope-note">意识系统 · Consciousness<span>当前只预览条目、摘要与来源管理，尚未接入后台记忆整理。</span></div>
  </div>
}
