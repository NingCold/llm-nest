import type { ImageInput } from "./live-types"

export interface DraftImage extends ImageInput { id: string; name: string; byteLength: number }
export const imageUrl = (image: ImageInput) => `data:${image.mime};base64,${image.data}`

export async function readImages(files: File[], existing: DraftImage[]): Promise<DraftImage[]> {
  if (files.length + existing.length > 4) throw new Error("每条消息最多 4 张图片")
  if (files.reduce((n, f) => n + f.size, existing.reduce((n, f) => n + f.byteLength, 0)) > 8 * 1024 * 1024) throw new Error("每条消息的图片合计不超过 8 MiB")
  const images: DraftImage[] = []
  for (const file of files) {
    if (!["image/png", "image/jpeg", "image/webp"].includes(file.type)) throw new Error("请选择 PNG、JPEG 或 WebP 图片")
    if (!file.size || file.size > 4 * 1024 * 1024) throw new Error("单张图片不超过 4 MiB")
    const url = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => resolve(reader.result as string)
      reader.onerror = () => reject(new Error(`无法读取 ${file.name}`))
      reader.readAsDataURL(file)
    })
    const image = new Image()
    image.src = url
    try { await image.decode() } catch { throw new Error(`无法识别图片 ${file.name}`) }
    if (image.naturalWidth > 4096 || image.naturalHeight > 4096) throw new Error("图片边长不超过 4096 像素，请先缩小图片")
    images.push({ id: crypto.randomUUID(), name: file.name || "粘贴的图片", mime: file.type, byteLength: file.size, data: url.slice(url.indexOf(",") + 1) })
  }
  return images
}
