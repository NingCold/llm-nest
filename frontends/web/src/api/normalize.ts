import type { GuiTimings, GuiUsage } from "./types"

/**
 * Wire 归一化：后端 `common::Usage` / `common::MessageTimings` 的 serde 输出
 * 是 snake_case（`prompt_tokens`/`ttft_ms`），前端契约是 camelCase。两个
 * 适配层（http/tauri）在边界统一转换；两个命名都兼容（容忍未来后端改）。
 */

type RawTimings = Partial<Record<"ttftMs" | "reasoning_ms" | "ttft_ms" | "reasoningMs" | "reasoning_ms" | "totalMs" | "total_ms", number | null | undefined>>

export function toGuiTimings(t: unknown): GuiTimings | undefined {
  if (!t || typeof t !== "object") return undefined
  const r = t as RawTimings
  const out: GuiTimings = {}
  const v1 = r.ttftMs ?? r.ttft_ms
  if (v1 != null) out.ttftMs = v1
  const v2 = r.reasoningMs ?? r.reasoning_ms
  if (v2 != null) out.reasoningMs = v2
  const v3 = r.totalMs ?? r.total_ms
  if (v3 != null) out.totalMs = v3
  return Object.keys(out).length > 0 ? out : undefined
}

type RawUsage = Partial<Record<"promptTokens" | "prompt_tokens" | "completionTokens" | "completion_tokens" | "totalTokens" | "total_tokens" | "cachedTokens" | "cached_tokens", number | null | undefined>>

export function toGuiUsage(u: unknown): GuiUsage | undefined {
  if (!u || typeof u !== "object") return undefined
  const r = u as RawUsage
  const out: GuiUsage = {
    promptTokens: r.promptTokens ?? r.prompt_tokens ?? 0,
    completionTokens: r.completionTokens ?? r.completion_tokens ?? 0,
    totalTokens: r.totalTokens ?? r.total_tokens ?? 0,
  }
  const cached = r.cachedTokens ?? r.cached_tokens
  if (cached != null) out.cachedTokens = cached
  return out
}
