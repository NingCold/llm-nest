import { useEffect, useRef, useState } from "react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { defaultImageOptions, imageSelectionIssue, imageSize, processImages, releaseImages, type ImageOptions, type ImageSelection, type ImageSource } from "./image-processing"
import { useLiveSocialStore } from "./live-store"
import { ImagePreview } from "./MessageImages"

function Editor({ selection }: { selection: ImageSelection }) {
  const state = useLiveSocialStore()
  const [options, setOptions] = useState(defaultImageOptions)
  const [results, setResults] = useState<ImageSource[] | null>(null)
  const [busy, setBusy] = useState(false)
  const [accepting, setAccepting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const generation = useRef(0), owned = useRef<ImageSource[]>([])
  useEffect(() => () => { generation.current++; releaseImages(owned.current) }, [])
  const remaining = (state.drafts[selection.conversationId]?.images ?? []).filter(image => image.id !== selection.replaceId)
  const issue = results ? imageSelectionIssue(results, remaining) : null
  const change = (next: ImageOptions) => {
    generation.current++; releaseImages(owned.current); owned.current = []
    setResults(null); setError(null); setOptions(next)
  }
  const process = async () => {
    const attempt = ++generation.current
    releaseImages(owned.current); owned.current = []; setResults(null); setBusy(true); setError(null)
    try {
      const images = await processImages(selection.sources, options)
      if (attempt !== generation.current) { releaseImages(images); return }
      owned.current = images; setResults(images)
    } catch (e) { if (attempt === generation.current) setError(`处理失败：${String(e)}`) }
    finally { if (attempt === generation.current) setBusy(false) }
  }
  const accept = async () => {
    if (!results || issue || accepting) return
    setAccepting(true); setError(null)
    try { await state.acceptImageSelection(selection, results) }
    catch (e) { setError(String(e)) }
    finally { setAccepting(false) }
  }
  const total = (images: ImageSource[]) => imageSize(images.reduce((n, image) => n + image.file.size, 0))
  return <Dialog open onOpenChange={open => { if (!open && !accepting) state.cancelImageSelection() }}>
    <DialogContent className="social-dialog image-processing-dialog">
      <DialogTitle>发送前处理图片</DialogTitle>
      <DialogDescription>{selection.reason}。原文件不会被修改；确认后加入草稿，再由你发送。</DialogDescription>
      <fieldset className="image-processing-options" disabled={busy || accepting || state.draftsClosing}>
        <label>最长边<select value={options.maxEdge} onChange={e => change({ ...options, maxEdge: Number(e.target.value) })}>{[4096, 2560, 1600, 1024].map(edge => <option key={edge} value={edge}>{edge} 像素</option>)}</select></label>
        <label>输出格式<select value={options.mime} onChange={e => change({ ...options, mime: e.target.value as ImageOptions["mime"] })}><option value="image/webp">WebP · 保留透明</option><option value="image/jpeg">JPEG · 白色背景</option><option value="image/png">PNG · 无损编码</option></select></label>
        <label>压缩质量<select value={options.quality} disabled={options.mime === "image/png"} onChange={e => change({ ...options, quality: Number(e.target.value) })}><option value={0.9}>90% · 更清晰</option><option value={0.8}>80% · 均衡</option><option value={0.65}>65% · 更小</option></select></label>
      </fieldset>
      <p className="image-processing-note">等比例缩小，不放大小图。WebP / JPEG 使用有损压缩；PNG 保留透明。处理后为静态图片，照片拍摄信息不保留。</p>
      <div className="image-processing-list">{selection.sources.map((source, index) => {
        const result = results?.[index]
        return <section className="image-processing-item" key={source.url}>
          <strong>{source.file.name}</strong>
          <div className="image-comparison">
            <figure><ImagePreview src={source.url} label={`原图 ${source.file.name}`} /><figcaption>原图 · {source.width} × {source.height} · {imageSize(source.file.size)}</figcaption></figure>
            <figure>{result ? <ImagePreview src={result.url} label={`处理结果 ${result.file.name}`} /> : <div className="image-processing-placeholder">{busy ? "正在生成预览…" : "处理结果将在这里显示"}</div>}<figcaption>{result ? `处理后 · ${result.width} × ${result.height} · ${imageSize(result.file.size)}` : "先生成预览，再确认画面"}</figcaption></figure>
          </div>
        </section>
      })}</div>
      <div className="image-processing-status" role="status">本次选择 {total(selection.sources)}{results && ` → ${total(results)}`} · 已有草稿图片 {imageSize(remaining.reduce((n, image) => n + image.byteLength, 0))}</div>
      {(error || issue) && <p className="form-error" role="alert">{error ?? issue}</p>}
      <p className="image-processing-note">{selection.replaceId ? "确认前保留原有草稿，处理预览不会单独保存。" : "确认前的新选图和预览尚未保存，刷新后需重新选择。"}确认后的图片沿用本机草稿保存。</p>
      <div className="form-actions">
        <button className="social-button secondary" disabled={accepting} onClick={() => state.cancelImageSelection()}>取消</button>
        <button className="social-button secondary" disabled={busy || accepting || state.draftsClosing} onClick={() => void process()}>{busy ? "处理中…" : results ? "重新生成预览" : "生成处理预览"}</button>
        <button className="social-button primary" disabled={!results || !!issue || busy || accepting || state.draftsClosing} onClick={() => void accept()}>{accepting ? "正在保存…" : selection.replaceId ? "使用处理结果替换" : "使用处理结果加入草稿"}</button>
      </div>
    </DialogContent>
  </Dialog>
}

export function ImageProcessingDialog() {
  const selection = useLiveSocialStore(state => state.imageSelection)
  return selection ? <Editor key={selection.id} selection={selection} /> : null
}
