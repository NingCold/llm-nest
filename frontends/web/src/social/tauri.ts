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
  message: id => command("social_message", { id }),
  revise: (id, input) => command("social_revise_message", { id, input }),
  withdraw: (id, input) => command("social_withdraw_message", { id, input }),
  control: (id, input) => command("social_control_conversation", { id, input }),
  pauseMember: (id, input) => command("social_pause_member", { id, input }),
  createGroup: input => command("social_create_group", { input }),
  changeMembership: (id, input) => command("social_change_membership", { id, input }),
  plans: id => command("social_plans", { id }),
  init: () => command("social_init"),
  createContact: input => command("social_create_contact", { input }),
  updateContact: (id, input) => command("social_update_contact", { id, input }),
  controlContact: (id, input) => command("social_control_contact", { id, input }),
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
