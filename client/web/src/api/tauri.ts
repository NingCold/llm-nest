import { invoke } from "@tauri-apps/api/core"
import type { SettingsApi, SettingsSnapshot, ProviderDraft, ProviderInfo, ProviderTemplate, CatalogStatus, CatalogSyncResponse } from "./types"
export const tauriApi: SettingsApi = {
  setModelImageInput: input => invoke<ProviderInfo[]>("set_model_image_input", { input }),
  init: () => invoke<SettingsSnapshot>("settings_snapshot"),
  async listProviderTemplates(): Promise<ProviderTemplate[]> {
    return invoke<ProviderTemplate[]>("provider_templates")
  },

  async catalogStatus(): Promise<CatalogStatus> {
    return invoke<CatalogStatus>("catalog_status")
  },

  async syncCatalog(): Promise<CatalogSyncResponse> {
    return invoke<CatalogSyncResponse>("sync_catalog")
  },

  async addProvider(draft: ProviderDraft): Promise<ProviderInfo[]> {
    return invoke<ProviderInfo[]>("upsert_provider", { payload: draft })
  },

  async deleteProvider(id: string): Promise<ProviderInfo[]> {
    return invoke<ProviderInfo[]>("delete_provider", { id })
  },
}
