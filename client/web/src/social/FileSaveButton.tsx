import { useRef, useState } from "react"
import { Download } from "lucide-react"
import { socialClient } from "./client"

export function FileSaveButton({ id, index, revision, account }: { id: string; index: number; revision: string; account?: string }) {
  const running = useRef(false), [busy, setBusy] = useState(false)
  const [status, setStatus] = useState(""), [error, setError] = useState("")
  const save = async () => {
    if (running.current) return
    running.current = true; setBusy(true); setStatus(""); setError("")
    try {
      const result = await socialClient.saveFile(id, index, revision, account)
      setStatus(result.outcome === "cancelled" ? "已取消保存" : `${result.outcome === "saved" ? "已保存" : "已开始下载"}：${result.fileName}`)
    } catch (e) { setError(e instanceof Error ? e.message : String(e)) }
    finally { running.current = false; setBusy(false) }
  }
  return <div className="image-save-actions">
    <button className="social-button secondary" disabled={busy} onClick={() => void save()}><Download />{busy ? "正在保存…" : "保存文件"}</button>
    {status && <p className="form-note" role="status">{status}</p>}
    {error && <p className="form-error" role="alert">{error}</p>}
  </div>
}
