import type { SettingsApi, SettingsSnapshot, ProviderDraft, ProviderInfo, ProviderTemplate, CatalogStatus, CatalogSyncResponse } from "./types"
const BASE = "/api"

async function req<T>(path: string, init?: RequestInit): Promise<T> {
  const headers = new Headers(init?.headers)
  headers.set("X-LLMN-Client", "1")
  const res = await fetch(`${BASE}${path}`, { ...init, headers })
  if (!res.ok) {
    let detail = res.statusText
    try {
      const body = (await res.json()) as { error?: string }
      if (body.error) detail = body.error
    } catch {
      /* keep statusText */
    }
    throw new Error(`${res.status} ${detail}`)
  }
  if (res.status === 204) return undefined as T
  return (await res.json()) as T
}

function jsonInit(method: string, body: unknown): RequestInit {
  return {
    method,
    headers: { "Content-Type": "application/json", "X-LLMN-Client": "1" },
    body: JSON.stringify(body),
  }
}

export const httpApi: SettingsApi = {
  setModelImageInput: input => req<ProviderInfo[]>("/models/image-input", jsonInit("POST", input)),
  init: () => req<SettingsSnapshot>("/settings"),
  async listProviderTemplates(): Promise<ProviderTemplate[]> {
    return req<ProviderTemplate[]>("/providers/templates")
  },

  async catalogStatus(): Promise<CatalogStatus> {
    return req<CatalogStatus>("/catalog")
  },

  async syncCatalog(): Promise<CatalogSyncResponse> {
    return req<CatalogSyncResponse>("/catalog/sync", jsonInit("POST", {}))
  },

  async addProvider(draft: ProviderDraft): Promise<ProviderInfo[]> {
    return req<ProviderInfo[]>("/providers", jsonInit("POST", draft))
  },

  async deleteProvider(id: string): Promise<ProviderInfo[]> {
    return req<ProviderInfo[]>(
      `/providers/${encodeURIComponent(id)}`,
      { method: "DELETE" },
    )
  },
}
