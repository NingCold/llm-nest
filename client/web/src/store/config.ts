import { create } from "zustand"
import type { ProviderInfo } from "@/api/types"
import { getApi } from "@/api"

interface ConfigStore {
  providers: ProviderInfo[]
  version: string
  init(): Promise<void>
  setProviders(providers: ProviderInfo[]): void
}
export const useConfigStore = create<ConfigStore>(set => ({
  providers: [], version: "",
  init: async () => {
    const snapshot = await (await getApi()).init()
    set(snapshot)
  },
  setProviders: providers => set({ providers }),
}))
