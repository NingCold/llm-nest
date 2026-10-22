import type { FileInput } from "./live-types"
export interface DraftFile extends FileInput { id: string; byteLength: number }
export function validateFiles(files: { byteLength: number }[]) {
  if (files.length > 4) throw new Error("每条消息最多 4 个文件")
  if (files.some(file => file.byteLength > 10 * 1024 * 1024)) throw new Error("单个文件不超过 10 MiB")
  if (files.reduce((total, file) => total + file.byteLength, 0) > 20 * 1024 * 1024) throw new Error("每条消息的文件合计不超过 20 MiB")
}
export async function readFiles(files: File[], existing: DraftFile[]): Promise<DraftFile[]> {
  validateFiles([...existing, ...files.map(file => ({ byteLength: file.size }))])
  return Promise.all(files.map(async file => {
    if (!file.name || new TextEncoder().encode(file.name).length > 240 || /[\\/:*?"<>|\x00-\x1f\x7f]/.test(file.name) || /[ .]$/.test(file.name) || /^(CON|PRN|AUX|NUL|COM[1-9]|LPT[1-9])(?:\.|$)/i.test(file.name)) throw new Error("文件名不能超过 240 字节或包含路径、控制字符及系统保留名称")
    const url = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => resolve(reader.result as string)
      reader.onerror = () => reject(new Error(`无法读取 ${file.name}`))
      reader.onabort = () => reject(new Error(`读取已取消：${file.name}`))
      reader.readAsDataURL(file)
    })
    return { id: crypto.randomUUID(), name: file.name, mime: file.type || "application/octet-stream", byteLength: file.size, data: url.slice(url.indexOf(",") + 1) }
  }))
}
export const fileSize = (bytes: number) => bytes < 1024 ? `${bytes} B` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KiB` : `${(bytes / 1024 / 1024).toFixed(1)} MiB`
