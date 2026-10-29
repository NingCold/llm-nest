import type { DraftImage } from "./images"

export interface Draft<R = unknown> {
  route?: "local";
  messageId?: string;
  expectedConversationRevision?: string;
  encryptedAttachments?: { index: number; data: string }[];
  sealed?: import("../cloud/crypto/content").Sealed;
  keyMessages?: Record<string, unknown>[];
  text: string
  requestId: string | null
  images?: DraftImage[]
  files?: import("./files").DraftFile[]
  mentions?: string[]
  replyTo?: string | null
  reference?: R | null
}
export type PendingDraft<R = unknown> = Draft<R> & { requestId: string }
export interface DraftRecord<R = unknown> { draft: Draft<R>; pending: PendingDraft<R> | null }
export const emptyDraft = <R = unknown>(): Draft<R> => ({ text: "", requestId: null, mentions: [], replyTo: null })
