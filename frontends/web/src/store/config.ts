import { create } from "zustand"
import type { AppInit, GuiConfig, ModelSelection, ProviderInfo } from "@/api/types"
async function getApi() { return (await import("@/api")).getApi() }

interface ConfigStore {
  config: GuiConfig | null
  savedConfig: GuiConfig | null
  providers: ProviderInfo[]
  version: string
  loading: boolean
  saving: boolean
  saveError: string | null
  init: () => Promise<AppInit>
  setModel: (model: ModelSelection) => void
  restoreModel: (model?: ModelSelection) => void
  updateConfig: (partial: Partial<GuiConfig>) => Promise<boolean>
  retrySave: () => void
  setProviders: (providers: ProviderInfo[]) => void
}
let queue: Promise<unknown> = Promise.resolve()
let pending = 0
let modelEpoch = 0
let failed: Partial<GuiConfig> | null = null

export const useConfigStore = create<ConfigStore>((set, get) => ({
  config: null, savedConfig: null, providers: [], version: "", loading: false,
  saving: false, saveError: null,
  init: async () => {
    set({ loading: true })
    try {
      const init = await (await getApi()).init()
      set({ config: init.config, savedConfig: init.config, providers: init.providers, version: init.version })
      return init
    } finally { set({ loading: false }) }
  },
  setModel: (model) => { modelEpoch++; void get().updateConfig({ currentModel: model }) },
  // Session recall must never overwrite the saved default model.
  restoreModel: (model) => {
    modelEpoch++
    const config = get().config
    let selection = model ?? get().savedConfig?.currentModel
    const providers = get().providers
    if (selection && !providers.some(p => p.id === selection!.provider && p.models.some(m => m.id === selection!.model))) {
      const p = providers.find(p => p.models.length)
      selection = p ? {provider:p.id,model:p.models[0].id} : {provider:"",model:""}
    }
    if (config && selection) set({ config: { ...config, currentModel: selection } })
  },
  updateConfig: (partial) => {
    const epoch = modelEpoch
    pending++
    set({ saving: true, saveError: null })
    const task = queue.then(async () => {
      const current = get().config
      if (!current) return false
      const next = { ...current, ...partial }
      try {
        await (await getApi()).setConfig(next)
        const active = get().config!
        set({ savedConfig: next, config: { ...next, currentModel: epoch === modelEpoch ? next.currentModel : active.currentModel }, saveError: null })
        failed = null
        return true
      } catch (error) {
        failed = partial
        set({ saveError: `设置未保存：${String(error)}` })
        return false
      }
    }).finally(() => { pending--; set({ saving: pending > 0 }) })
    queue = task
    return task
  },
  retrySave: () => { if (failed) void get().updateConfig(failed) },
  setProviders: (providers) => {
    set({ providers })
    const config = get().config
    if (config && !providers.some(p => p.id === config.currentModel.provider && p.models.some(m => m.id === config.currentModel.model))) {
      const p = providers.find(p => p.models.length)
      get().restoreModel(p ? { provider: p.id, model: p.models[0].id } : { provider: "", model: "" })
    }
  },
}))
