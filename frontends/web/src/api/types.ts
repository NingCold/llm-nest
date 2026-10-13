export interface SettingsApi {
  init(): Promise<SettingsSnapshot>
  listProviderTemplates(): Promise<ProviderTemplate[]>
  catalogStatus(): Promise<CatalogStatus>
  syncCatalog(): Promise<CatalogSyncResponse>
  addProvider(draft: ProviderDraft): Promise<ProviderInfo[]>
  deleteProvider(id: string): Promise<ProviderInfo[]>
}
export interface SettingsSnapshot { providers: ProviderInfo[]; version: string }

export interface CatalogStatus {
  source: string
  updatedAt: string | null
  providerCount: number
  modelCount: number
  cachePath: string | null
  warnings: string[]
}

export interface CatalogSyncResponse {
  report: { status: CatalogStatus; added: number; updated: number; skipped: number }
  providers: ProviderInfo[]
}

/** 内置目录模板：选中后自动填充协议/端点/模型，只需补 key */
export interface ProviderTemplate {
  id: string
  displayName: string
  protocol: string
  baseUrl: string
  apiKeyEnv: string
  defaultModel: string
  models: { id: string; displayName: string }[]
}

/** 供应商表单提交（camelCase wire；protocol/baseUrl/apiKey 缺省 = 保持不变） */
export interface ProviderDraft {
  id: string
  protocol?: string
  baseUrl?: string
  apiKey?: string
  models?: { id: string; model?: string; displayName?: string }[]
}

export interface ProviderInfo {
  id: string
  displayName: string
  models: ModelInfo[]
}

export interface ModelInfo {
  id: string
  displayName: string
  /** 模型支持的思考强度（off/low/medium/high/max）；缺省 = 不支持思考 */
  reasoningLevels?: string[]
}

export interface ModelSelection {
  provider: string
  model: string
  /** 思考强度由管理员的执行绑定指定；缺省使用模型默认 */
  reasoningEffort?: string
}
