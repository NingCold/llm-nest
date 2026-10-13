import { Channel, invoke } from "@tauri-apps/api/core"
import { SocialApiError, type SocialClient, type Subscribe, type SubscriptionEvent } from "./transport"

function apiError(error: unknown): Error {
  if (error instanceof Error) return error
  if (typeof error === "object" && error !== null && "message" in error && "code" in error) {
    return new SocialApiError(String(error.message), String(error.code))
  }
  return new Error(String(error))
}
async function command<T>(name: string, args?: Record<string, unknown>): Promise<T> {
  try { return await invoke<T>(name, args) }
  catch (error) { throw apiError(error) }
}
export const socialClient: SocialClient = {
  accountViewImage: (account, id, index, revision) => command("social_account_view_image", { account, id, index, revision }),
  messageImage: (id, index, revision) => command("social_message_image", { id, index, revision }),
  saveFile: (id, index, revision, account) => command("social_save_file", { id, index, revision, account: account ?? null }),
  saveImage: (id, index, revision, account) => command("social_save_image", { id, index, revision, account: account ?? null }),
  exportConversation: (id, format, includeImages = false, includeFiles = false) => command("social_export_conversation", { id, format, includeImages, includeFiles }),
  updateConversationList: (id, input) => command("social_update_conversation_list", { id, input }),
  messageContext: id => command("social_message_context", { id }),
  searchMessages: input => command("social_search_messages", { input }),
  previewLegacy: () => command("social_preview_legacy"),
  importLegacy: preview => command("social_import_legacy", { preview }),
  referencePreview: input => command("social_reference_preview", { input }),
  summaries: (id, before) => command("social_summaries", { id, before: before ?? null }),
  configureSummaries: (id, input) => command("social_configure_summaries", { id, input }),
  requestSummary: (id, input) => command("social_request_summary", { id, input }),
  memories: (id, actor) => command("social_memories", { id, actor }),
  createMemory: (id, actor, input) => command("social_create_memory", { id, actor, input }),
  updateMemory: (id, input) => command("social_update_memory", { id, input }),
  forgetMemory: (id, input) => command("social_forget_memory", { id, input }),
  message: id => command("social_message", { id }),
  revise: (id, input) => command("social_revise_message", { id, input }),
  withdraw: (id, input) => command("social_withdraw_message", { id, input }),
  control: (id, input) => command("social_control_conversation", { id, input }),
  pauseMember: (id, input) => command("social_pause_member", { id, input }),
  updateParticipation: (id, input) => command("social_update_participation", { id, input }),
  updateContactSettings: (id, input) => command("social_update_contact_settings", { id, input }),
  createContactIntent: (id, input) => command("social_create_contact_intent", { id, input }),
  cancelContactIntent: (id, intent, input) => command("social_cancel_contact_intent", { id, intent, input }),
  createGroup: input => command("social_create_group", { input }),
  changeMembership: (id, input) => command("social_change_membership", { id, input }),
  plans: id => command("social_plans", { id }),
  init: () => command("social_init"),
  account: id => command("social_account", { id }),
  updateAccount: (id, input) => command("social_update_account", { id, input }),
  setContactBlock: (id, input) => command("social_set_contact_block", { id, input }),
  setContact: (id, input) => command("social_set_contact", { id, input }),
  managedAccounts: () => command("social_managed_accounts"),
  managedAccount: id => command("social_managed_account", { id }),
  createAi: input => command("social_create_ai", { input }),
  updateAi: (id, input) => command("social_update_ai", { id, input }),
  controlAccount: (id, input) => command("social_control_account", { id, input }),
  setAdministrator: (id, input) => command("social_set_administrator", { id, input }),
  accountView: id => command("social_account_view", { id }),
  accountViewMessages: (id, conversation, before) => command("social_account_view_messages", { id, conversation, before: before ?? null }),
  openDirect: (id, requestId) => command("social_open_direct", { id, input: { requestId } }),
  messages: (id, before) => command("social_messages", { id, before: before ?? null }),
  send: (id, input) => command("social_send", { id, input }),
  cancel: id => command("social_cancel_run", { id }),
  runDetails: id => command("social_run_details", { id }),
}

type Delivery = { kind: "event"; sequence: number; event: SubscriptionEvent }
  | { kind: "error"; error: { code: string; message: string } } | { kind: "closed" }

export const subscribe: Subscribe = async (cursor, signal, change, live, resync, ready) => {
  if (signal.aborted) return
  const id = crypto.randomUUID()
  let finish!: (error: Error | null) => void
  const ended = new Promise<Error | null>(resolve => { finish = resolve })
  let finished = false, sequence = 0
  const close = (error: Error | null) => { if (!finished) { finished = true; finish(error) } }
  const abort = () => close(null)
  signal.addEventListener("abort", abort, { once: true })
  let processing = Promise.resolve()
  // Install the callback before invoke: events can arrive before its acknowledgement.
  const channel = new Channel<Delivery>(delivery => {
    // Terminal transport frames must also release a callback awaiting a failed IPC read.
    if (delivery.kind === "closed") { close(new Error("消息订阅已断开")); return }
    if (delivery.kind === "error") { close(apiError(delivery.error)); return }
    processing = processing.then(async () => {
      if (finished) return
      if (delivery.sequence !== sequence + 1) throw new SocialApiError("桌面事件序号不连续", "invalid_input")
      const event = delivery.event
      if (event.kind === "change") await change(event.event)
      else if (event.kind === "generation") live(event.event)
      else await resync()
      if (finished) return
      sequence = delivery.sequence
      await command("social_ack_subscription", { id, sequence })
    }).catch(error => close(apiError(error)))
  })
  try {
    await command("social_subscribe", { id, after: cursor, channel })
    if (!finished) ready()
    const error = await ended
    if (error) throw error
  } finally {
    close(null)
    signal.removeEventListener("abort", abort)
    // Waiting for subscribe above also covers an abort during backend initialization.
    await command("social_unsubscribe", { id })
  }
}
