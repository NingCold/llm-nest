import { useRef, useState } from "react"
import { ArrowUp, ImagePlus, SlidersHorizontal, X } from "lucide-react"
import { IconButton } from "./components/common"
import { useLiveSocialStore } from "./live-store"
import { imageUrl } from "./images"
import { ImagePreview } from "./MessageImages"

export function ChatComposer({ id, title, hint }: { id: string; title: string; hint: string }) {
  const state = useLiveSocialStore()
  const draft = state.drafts[id]
  const ready = state.draftsReady && !state.draftsClosing
  const pending = state.pendingSends[id]
  const saved = state.draftSaveStates[id]
  const images = draft?.images ?? []
  const [busy, setBusy] = useState(false)
  const [dragging, setDragging] = useState(false)
  const reading = useRef(false)
  const fileInput = useRef<HTMLInputElement>(null)
  const add = async (files: File[]) => {
    if (!ready || !files.length || reading.current) return
    reading.current = true; setBusy(true)
    await state.action(() => state.addDraftImages(id, files))
    reading.current = false; setBusy(false)
  }
  const send = () => { if (ready && !reading.current && !pending) void state.send(id) }
  return <div className={`composer ${dragging ? "image-drop-active" : ""}`}
    onDragOver={e => { if (Array.from(e.dataTransfer.types).includes("Files")) { e.preventDefault(); e.dataTransfer.dropEffect = "copy"; setDragging(true) } }}
    onDragLeave={e => { if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setDragging(false) }}
    onDrop={e => { setDragging(false); if (e.dataTransfer.files.length) { e.preventDefault(); void add(Array.from(e.dataTransfer.files)) } }}>
    {!!images.length && <div className="draft-images">{images.map(image => <div className="draft-image" key={image.id}><ImagePreview src={imageUrl(image)} label={image.name} /><IconButton label={`移除图片 ${image.name}`} disabled={!ready || busy} onClick={() => state.imageDraft(id, images.filter(i => i.id !== image.id))}><X /></IconButton><IconButton label={`处理图片 ${image.name}`} disabled={!ready || busy} onClick={() => void state.action(() => state.editDraftImage(id, image.id))}><SlidersHorizontal /></IconButton></div>)}</div>}
    <textarea aria-label="消息输入框" rows={2} maxLength={20000} disabled={!ready} placeholder={dragging ? "松开添加图片" : `给 ${title} 发消息…`} value={draft?.text ?? ""}
      onChange={e => state.updateDraft(id, e.target.value)}
      onPaste={e => { const files = Array.from(e.clipboardData.items).filter(i => i.kind === "file").map(i => i.getAsFile()).filter((f): f is File => f !== null); if (files.length) { e.preventDefault(); void add(files) } }}
      onKeyDown={e => { if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing && e.keyCode !== 229) { e.preventDefault(); send() } }} />
    <div className="composer-bottom"><input className="sr-only" ref={fileInput} type="file" aria-label="选择图片文件" accept="image/png,image/jpeg,image/webp" multiple disabled={!ready || busy} tabIndex={-1} onChange={e => { void add(Array.from(e.target.files ?? [])); e.target.value = "" }} />
      <IconButton label="添加图片" disabled={!ready || busy || images.length === 4} onClick={() => fileInput.current?.click()}><ImagePlus /></IconButton>
      <span className="composer-hint">{busy ? "正在读取图片…" : hint}</span><button className="send-button" aria-label="发送消息" disabled={!ready || busy || !!pending || state.sending[id] || (!draft?.text.trim() && !images.length)} onClick={send}><ArrowUp /></button>
    </div>
    {!!images.length && <p className="image-draft-hint">已选 {images.length}/4 张</p>}
    {!ready ? <div className="draft-storage-status" role={state.draftLoadError ? "alert" : "status"}>{state.draftsClosing ? "正在保存草稿并关闭…" : state.draftLoadError ?? "正在恢复本地草稿…"}{state.draftLoadError && <button className="text-button" onClick={() => void state.loadDrafts()}>重试恢复草稿</button>}</div>
      : <div className="draft-storage-status" role={saved?.status === "error" ? "alert" : "status"}>{saved?.status === "error" ? <>草稿尚未保存，请暂时不要关闭页面。{saved.error}<button className="text-button" onClick={() => void state.action(() => state.saveDraft(id))}>重试保存草稿</button></> : saved?.status === "saving" ? "正在保存草稿…" : (images.length || draft?.text || pending) ? "草稿已保存在本机，重开后可继续编辑" : null}</div>}
    {ready && pending && !state.sending[id] && <div className="draft-pending-send"><p>上次发送尚未确认。重试会使用原内容核对，不会自动发送当前新输入。</p><details><summary>查看上次发送的内容</summary><p className="pending-send-text">{pending.text || "（仅图片）"}</p>{!!pending.images?.length && <div className="draft-images">{pending.images.map(image => <div className="draft-image" key={image.id}><ImagePreview src={imageUrl(image)} label={image.name} /></div>)}</div>}</details><button className="text-button" onClick={() => void state.send(id)}>重试上次发送</button><button className="text-button" onClick={() => state.discardPending(id)}>放弃重试</button><small>放弃重试不撤回可能已经送达的消息，当前草稿会保留。</small></div>}
  </div>
}
