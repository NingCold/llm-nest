export type ExportFormat = "markdown" | "json"
export interface ConversationExport { fileName: string; mediaType: string; messageCount: number; content: string }
export type ExportResult = { outcome: "cancelled" } | { outcome: "saved" | "downloaded"; fileName: string; messageCount: number }
export type ImageSaveResult = { outcome: "cancelled" } | { outcome: "saved" | "downloaded"; fileName: string }

export function downloadExport(file: ConversationExport): ExportResult {
  downloadFile(new Blob([file.content], { type: file.mediaType }), file.fileName)
  return { outcome: "downloaded", fileName: file.fileName, messageCount: file.messageCount }
}

export function downloadFile(blob: Blob, fileName: string): ImageSaveResult {
  const url = URL.createObjectURL(blob)
  const link = document.createElement("a")
  try {
    link.href = url
    link.download = fileName
    document.body.appendChild(link)
    link.click()
  } finally {
    link.remove()
    // Keep the URL alive while the browser starts consuming the download.
    setTimeout(() => URL.revokeObjectURL(url), 60_000)
  }
  return { outcome: "downloaded", fileName }
}
