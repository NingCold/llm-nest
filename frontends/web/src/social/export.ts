export type ExportFormat = "markdown" | "json"
export interface ConversationExport { fileName: string; mediaType: string; messageCount: number; content: string }
export type ExportResult = { outcome: "cancelled" } | { outcome: "saved" | "downloaded"; fileName: string; messageCount: number }

export function downloadExport(file: ConversationExport): ExportResult {
  const url = URL.createObjectURL(new Blob([file.content], { type: file.mediaType }))
  const link = document.createElement("a")
  try {
    link.href = url
    link.download = file.fileName
    document.body.appendChild(link)
    link.click()
  } finally {
    link.remove()
    // Keep the URL alive while the browser starts consuming the download.
    setTimeout(() => URL.revokeObjectURL(url), 60_000)
  }
  return { outcome: "downloaded", fileName: file.fileName, messageCount: file.messageCount }
}
