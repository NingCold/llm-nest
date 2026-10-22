import { useEffect, useRef, useState, type ReactNode } from "react"
import { ImageIcon } from "lucide-react"
import { Dialog, DialogContent, DialogDescription, DialogTitle } from "@/components/ui/dialog"
import { socialClient } from "./client"
import type { Message } from "./live-types"
import { imageUrl } from "./images"
import { ImageSaveButton } from "./ImageSaveButton"

export function ImagePreview({ src, label, onError, actions }: { src: string; label: string; onError?: () => void; actions?: ReactNode }) {
  const [open, setOpen] = useState(false)
  return <><button className="image-thumbnail" aria-label={`放大图片 ${label}`} onClick={() => setOpen(true)}><img src={src} alt={label} onError={onError} /></button><Dialog open={open} onOpenChange={setOpen}><DialogContent className="social-dialog image-preview-dialog"><DialogTitle>图片预览</DialogTitle><DialogDescription>{label}</DialogDescription><img src={src} alt={label} />{actions}</DialogContent></Dialog></>
}

function MessageImage({ message, index, account }: { message: Message; index: number; account?: string }) {
  const element = useRef<HTMLDivElement>(null)
  const [visible, setVisible] = useState(false)
  const [retry, setRetry] = useState(0)
  const [result, setResult] = useState<{ key: string; src?: string; error?: string } | null>(null)
  const key = `${account ?? "self"}/${message.id}/${message.revision}/${index}`
  useEffect(() => {
    const observer = new IntersectionObserver(entries => { if (entries.some(e => e.isIntersecting)) { setVisible(true); observer.disconnect() } }, { rootMargin: "160px" })
    observer.observe(element.current!)
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    if (!visible) return
    let current = true
    const read = account ? socialClient.accountViewImage(account, message.id, index, message.revision) : socialClient.messageImage(message.id, index, message.revision)
    read.then(image => { if (current) setResult({ key, src: imageUrl(image) }) }, error => { if (current) setResult({ key, error: String(error) }) })
    return () => { current = false }
  }, [key, visible, retry])
  const value = result?.key === key ? result : null
  return <div className="message-image" ref={element}>{value?.src ? <ImagePreview src={value.src} label={`${message.senderName} 发送的图片`} actions={<ImageSaveButton key={key} id={message.id} index={index} revision={message.revision} account={account} />} onError={() => setResult({key, error:"图片无法显示，请重新加载"})} /> : <div className="image-placeholder"><ImageIcon /><span>{value?.error ?? "正在加载图片…"}</span>{value?.error && <button className="text-button" onClick={() => setRetry(n => n + 1)}>重新加载</button>}</div>}</div>
}

export function MessageImages({ message, account }: { message: Message; account?: string }) {
  if (message.status === "withdrawn" || !message.images?.length) return null
  return <div className="message-images">{message.images.map(image => <MessageImage key={`${message.id}/${message.revision}/${image.index}`} message={message} index={image.index} account={account} />)}</div>
}
