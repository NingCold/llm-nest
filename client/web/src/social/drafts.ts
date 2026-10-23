import type { DraftImage } from "./images"
import type { ReferenceInput, SendInput } from "./live-types"

export interface Draft<R = ReferenceInput> {
  text: string
  requestId: string | null
  images?: DraftImage[]
  files?: import("./files").DraftFile[]
  mentions?: string[]
  replyTo?: string | null
  reference?: R | null
}
export type PendingDraft<R = ReferenceInput> = Draft<R> & { requestId: string }
export interface DraftRecord<R = ReferenceInput> { draft: Draft<R>; pending: PendingDraft<R> | null }
export const emptyDraft = <R = ReferenceInput>(): Draft<R> => ({ text: "", requestId: null, mentions: [], replyTo: null })

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
