import { useRef, useState } from "react"
import { Download } from "lucide-react"
import { socialClient } from "./client"
import type { ExportFormat } from "./export"

export function ConversationExportPanel({ conversationId }: { conversationId: string }) {
  const [format, setFormat] = useState<ExportFormat>("markdown")
  const [includeImages, setIncludeImages] = useState(false)
  const [busy, setBusy] = useState(false), running = useRef(false)
  const [error, setError] = useState<string | null>(null), [status, setStatus] = useState("")
  const save = async () => {
    if (running.current) return
    running.current = true
    setBusy(true); setError(null); setStatus("")
    try {
      const result = await socialClient.exportConversation(conversationId, format, includeImages)
      setStatus(result.outcome === "cancelled" ? "已取消导出" : `${result.outcome === "saved" ? "已保存" : "已开始下载"} ${result.messageCount} 条消息：${result.fileName}`)
    } catch (e) { setError(e instanceof Error ? e.message : String(e)) }
    finally { running.current = false; setBusy(false) }
  }
  return <section className="live-conversation-controls conversation-export"><h3>导出聊天记录</h3>
    <p>保存这个窗口的完整记录，包含发言人、时间、正文和消息状态。</p>
    <fieldset className="social-form social-form-fields" disabled={busy}>
      <label>文件格式<select value={format} onChange={e => { setFormat(e.target.value as ExportFormat); setError(null); setStatus("") }}>
        <option value="markdown">Markdown · 方便阅读</option><option value="json">JSON · 结构化记录</option>
      </select></label>
      <label className="export-images-option"><input type="checkbox" checked={includeImages} onChange={e => { setIncludeImages(e.target.checked); setError(null); setStatus("") }} />包含图片 · 保存为 ZIP</label>
      {includeImages && <p className="form-note">解压后可查看聊天记录和 images 文件夹中的原图；Markdown 中的图片可以直接显示。</p>}
      <button className="social-button secondary" onClick={() => void save()}><Download />{busy ? "正在导出…" : "导出聊天记录"}</button>
    </fieldset>
    <p className="form-note">未勾选时，图片仅保留占位说明。生成中的回复只包含已保存的片段，撤回消息保留占位。记忆、思考过程和角色内部设定不随聊天导出。</p>
    {error && <p className="form-error" role="alert">{error}，可以重试。</p>}
    {status && <p className="form-note export-status" role="status">{status}</p>}
  </section>
}
