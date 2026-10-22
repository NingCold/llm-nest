import type { DraftImage } from "./images"

export interface ImageSource { file: File; url: string; width: number; height: number }
export interface ImageSelection { id: string; conversationId: string; sources: ImageSource[]; reason: string; replaceId?: string }
export interface ImageOptions { maxEdge: number; mime: "image/webp" | "image/jpeg" | "image/png"; quality: number }
export const defaultImageOptions: ImageOptions = { maxEdge: 2560, mime: "image/webp", quality: 0.8 }
export const imageSize = (bytes: number) => bytes >= 1024 * 1024 ? `${(bytes / (1024 * 1024)).toFixed(2)} MiB` : `${Math.ceil(bytes / 1024)} KiB`

export function releaseImages(images: ImageSource[]) { for (const image of images) URL.revokeObjectURL(image.url) }

export async function inspectImages(files: File[]): Promise<ImageSource[]> {
  const sources: ImageSource[] = []
  try {
    for (const file of files) {
      if (!["image/png", "image/jpeg", "image/webp"].includes(file.type)) throw new Error("请选择 PNG、JPEG 或 WebP 图片")
      if (!file.size || file.size > 20 * 1024 * 1024) throw new Error("待处理的单张图片需在 20 MiB 以内，请先用图片编辑器缩小后再选择")
      const url = URL.createObjectURL(file), image = new Image()
      const source = { file, url, width: 0, height: 0 }; sources.push(source)
      try {
        image.src = url
        await image.decode()
        source.width = image.naturalWidth; source.height = image.naturalHeight
        if (source.width * source.height > 64_000_000) throw new Error("待处理图片不超过 6400 万像素，请先缩小图片")
      } finally { image.src = "" }
    }
    return sources
  } catch (error) { releaseImages(sources); throw error }
}

export function imageSelectionIssue(sources: ImageSource[], existing: DraftImage[]): string | null {
  if (sources.length + existing.length > 4) return "每条消息最多 4 张图片，请先移除一些草稿图片"
  if (sources.some(s => s.width > 4096 || s.height > 4096)) return "图片最长边需要缩小到 4096 像素以内"
  if (sources.some(s => s.file.size > 4 * 1024 * 1024)) return "单张图片仍超过 4 MiB，请缩小尺寸或降低质量"
  if (sources.reduce((n, s) => n + s.file.size, existing.reduce((n, i) => n + i.byteLength, 0)) > 8 * 1024 * 1024) return "加上已有草稿图片后仍超过 8 MiB，请继续压缩或先移除一些图片"
  return null
}

export function fittedSize(width: number, height: number, maxEdge: number) {
  const scale = Math.min(1, maxEdge / Math.max(width, height))
  return { width: Math.max(1, Math.round(width * scale)), height: Math.max(1, Math.round(height * scale)) }
}

export async function processImages(sources: ImageSource[], options: ImageOptions): Promise<ImageSource[]> {
  const results: ImageSource[] = []
  try {
    for (const source of sources) {
      const size = fittedSize(source.width, source.height, options.maxEdge)
      const canvas = document.createElement("canvas"), image = new Image()
      canvas.width = size.width; canvas.height = size.height
      try {
        image.src = source.url; await image.decode()
        const context = canvas.getContext("2d")
        if (!context) throw new Error("无法创建图片画布，请稍后重试")
        if (options.mime === "image/jpeg") { context.fillStyle = "#ffffff"; context.fillRect(0, 0, size.width, size.height) }
        context.imageSmoothingEnabled = true; context.imageSmoothingQuality = "high"
        context.drawImage(image, 0, 0, size.width, size.height)
        const blob = await new Promise<Blob>((resolve, reject) => canvas.toBlob(value => value ? resolve(value) : reject(new Error("图片编码失败，请调整选项后重试")), options.mime, options.quality))
        if (blob.type !== options.mime) throw new Error("当前浏览器不支持所选输出格式，请改选 PNG 或 JPEG")
        const extension = { "image/webp": "webp", "image/jpeg": "jpg", "image/png": "png" }[options.mime]
        const stem = (source.file.name || "图片").replace(/\.[^.]+$/, "")
        const file = new File([blob], `${stem}-处理后.${extension}`, { type: options.mime })
        results.push({ file, url: URL.createObjectURL(file), ...size })
      } finally { canvas.width = canvas.height = 0; image.src = "" }
    }
    return results
  } catch (error) { releaseImages(results); throw error }
}

export function draftImageFile(image: DraftImage): File {
  const bytes = Uint8Array.from(atob(image.data), c => c.charCodeAt(0))
  return new File([bytes], image.name, { type: image.mime })
}
