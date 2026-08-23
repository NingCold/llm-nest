import { create } from "zustand"
import type { GuiConfig, ModelSelection, ProviderInfo } from "@/api/types"
async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

interface ConfigStore {
  config: GuiConfig | null
  providers: ProviderInfo[]
  version: string
  loading: boolean
  init: () => Promise<void>
  setModel: (model: ModelSelection) => void
  updateConfig: (partial: Partial<GuiConfig>) => void
}

export const useConfigStore = create<ConfigStore>((set, get) => ({
  config: null,
  providers: [],
  version: "",
  loading: false,

  init: async () => {
    set({ loading: true })
    try {
      const api = await getApi()
      const init = await api.init()
      set({
        config: init.config,
        providers: init.providers,
        version: init.version,
        loading: false,
      })
    } catch {
      set({ loading: false })
    }
  },

  setModel: (model) => {
    const current = get().config
    if (!current) return
    const newConfig = { ...current, currentModel: model }
    set({ config: newConfig })
    getApi().then((api) => api.setConfig(newConfig))
  },

  updateConfig: (partial) => {
    const current = get().config
    if (!current) return
    const newConfig = { ...current, ...partial }
    set({ config: newConfig })
    getApi().then((api) => api.setConfig(newConfig))
  },
}))