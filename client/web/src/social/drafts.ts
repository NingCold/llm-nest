import type { DraftImage } from "./images"
import type { ReferenceInput, SendInput } from "./live-types"

export interface Draft {
  text: string
  requestId: string | null
  images?: DraftImage[]
  files?: import("./files").DraftFile[]
  mentions?: string[]
  replyTo?: string | null
  reference?: ReferenceInput | null
}
export type PendingDraft = Draft & { requestId: string }
export interface DraftRecord { draft: Draft; pending: PendingDraft | null }
export const emptyDraft = (): Draft => ({ text: "", requestId: null, mentions: [], replyTo: null })

export function sendInput(draft: PendingDraft): SendInput {
  return {
    requestId: draft.requestId, text: draft.text, replyTo: draft.replyTo ?? null, mentions: draft.mentions ?? [],
    ...(draft.files?.length ? { files: draft.files.map(({ name, mime, data }) => ({ name, mime, data })) } : {}),
    ...(draft.images?.length ? { images: draft.images.map(({ mime, data }) => ({ mime, data })) } : {}),
    ...(draft.reference ? { reference: draft.reference.selection } : {}),
  }
}

export function acknowledgeDraft(current: Draft, sent: PendingDraft): Draft {
  if (current.requestId === sent.requestId) return emptyDraft()
  return { ...current, files: current.files?.filter(file => !sent.files?.some(previous => previous.id === file.id)), images: current.images?.filter(image => !sent.images?.some(previous => previous.id === image.id)) }
}
