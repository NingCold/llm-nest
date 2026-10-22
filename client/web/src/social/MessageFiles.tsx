import { FileIcon } from "lucide-react"
import type { Message } from "./live-types"
import { fileSize } from "./files"
import { FileSaveButton } from "./FileSaveButton"
export function MessageFiles({ message, account }: { message: Message; account?: string }) {
  if (message.status === "withdrawn" || !message.files.length) return null
  return <div className="message-files">{message.files.map(file => <div className="message-file" key={`${message.id}/${message.revision}/${file.index}`}><FileIcon /><div><strong>{file.name}</strong><small>{file.mime} · {fileSize(file.byteLength)}</small><FileSaveButton id={message.id} index={file.index} revision={message.revision} account={account} /></div></div>)}</div>
}
