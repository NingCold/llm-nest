export type ChatEventHandler = (event: GuiEvent) => void

export interface ChatApi {
  init(): Promise<AppInit>
  chat(params: ChatParams, onEvent: ChatEventHandler): Promise<void>
  cancelChat(sessionId: string): Promise<void>
  listSessions(): Promise<SessionSummary[]>
  newSession(): Promise<SessionSummary>
  deleteSession(id: string): Promise<void>
  renameSession(id: string, title: string): Promise<void>
  setConfig(config: GuiConfig): Promise<void>
}

export interface AppInit {
  config: GuiConfig
  providers: ProviderInfo[]
  sessions: SessionSummary[]
  version: string
}

export interface GuiConfig {
  currentModel: ModelSelection
  temperature: number
  maxTokens?: number
}

export interface ProviderInfo {
  id: string
  displayName: string
  models: ModelInfo[]
}

export interface ModelInfo {
  id: string
  displayName: string
}

export interface ModelSelection {
  provider: string
  model: string
}

export interface SessionSummary {
  id: string
  title: string
  createdAt: string
  updatedAt: string
  messageCount: number
}

export type GuiEvent =
  | { type: "delta"; messageId: string; content: string }
  | { type: "finished"; messageId: string }
  | { type: "error"; messageId: string; error: string }
  | { type: "cancelled"; messageId: string }
  | { type: "message_start"; messageId: string }

export interface ChatParams {
  sessionId: string
  input: string
  model: ModelSelection
  temperature: number
  maxTokens?: number
}