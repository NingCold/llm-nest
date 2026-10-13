import { useRef, useState } from "react"
import { ArrowUp, ImagePlus, X } from "lucide-react"
import { IconButton } from "./components/common"
import { useLiveSocialStore } from "./live-store"
import { imageUrl, readImages } from "./images"
import { ImagePreview } from "./MessageImages"

export function ChatComposer({ id, title, hint }: { id: string; title: string; hint: string }) {
  const state = useLiveSocialStore()
  const draft = state.drafts[id]
  const images = draft?.images ?? []
  const [busy, setBusy] = useState(false)
  const [dragging, setDragging] = useState(false)
  const reading = useRef(false)
  const fileInput = useRef<HTMLInputElement>(null)
  const add = async (files: File[]) => {
    if (!files.length || reading.current) return
    reading.current = true; setBusy(true)
    await state.action(async () => {
      const selected = await readImages(files, useLiveSocialStore.getState().drafts[id]?.images ?? [])
      const store = useLiveSocialStore.getState()
      store.imageDraft(id, [...(store.drafts[id]?.images ?? []), ...selected])
    })
    reading.current = false; setBusy(false)
  }
  const send = () => { if (!reading.current) void state.send(id) }
  return <div className={`composer ${dragging ? "image-drop-active" : ""}`}
    onDragOver={e => { if (Array.from(e.dataTransfer.types).includes("Files")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; setDragging(true) } }}
    onDragLeave={e => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false) }}
    onDrop={e => { setDragging(false); if (e.dataTransfer.files.length) { e.preventDefault(); void add(Array.from(e.dataTransfer.files)) } }}>
    {!!images.length && <div className="draft-images">{images.map(image => <div className="draft-image" key={image.id}><ImagePreview src={imageUrl(image)} label={image.name} /><IconButton label={`移除图片 ${image.name}`} disabled={busy} onClick={() => state.imageDraft(id, images.filter(i => i.id !== image.id))}><X /></IconButton></div>)}</div>}
    <textarea aria-label="消息输入框" rows={2} maxLength={20000} placeholder={dragging ? "松开添加图片" : `给 ${title} 发消息…`} value={draft?.text ?? ""}
      onChange={e => state.updateDraft(id, e.target.value)}
      onPaste={e => { const files = Array.from(e.clipboardData.items).filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => f !== null); if (files.length) { e.preventDefault(); void add(files) } }}
      onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); send() } }} />
    <div className="composer-bottom"><input className="sr-only" ref={fileInput} type="file" aria-label="选择图片文件" accept="image/png,image/jpeg,image/webp" multiple disabled={busy} tabIndex={-1} onChange={e => { void add(Array.from(e.target.files ?? [])); e.target.value = "" }} />
      <IconButton label="添加图片" disabled={busy || images.length === 4} onClick={() => fileInput.current?.click()}><ImagePlus /></IconButton>
      <span className="composer-hint">{busy ? "正在读取图片…" : hint}</span><button className="send-button" aria-label="发送消息" disabled={busy || state.sending[id] || (!draft?.text.trim() && !images.length)} onClick={send}><ArrowUp /></button>
    </div>
    {!!images.length && <p className="image-draft-hint">已选 {images.length}/4 张 · 刷新或关闭后需重新选择未发送图片</p>}
  </div>
}
