export type ChatEventHandler = (event: GuiEvent) => void

/** 归一化的 token 用量（各协议已在后端换算） */
export interface GuiUsage {
  promptTokens: number
  completionTokens: number
  totalTokens: number
  /** 缓存命中 token 数；命中率 = cached / (prompt + cached) */
  cachedTokens?: number
}

/** 单条 assistant 消息的计时统计（毫秒） */
export interface GuiTimings {
  /** 请求开始 → 首个 token（思考或正文），毫秒 */
  ttftMs?: number
  /** 请求开始 → 首个正文 delta（思考阶段），毫秒 */
  reasoningMs?: number
  /** 请求开始 → 流结束，毫秒 */
  totalMs?: number
}

/** 一条工具调用或结果（渲染工具卡片用） */
export interface GuiToolBlock {
  kind: "call" | "result"
  id: string
  name: string
  arguments?: string
  content?: string
  isError?: boolean
  /** 工具执行耗时（毫秒），仅 result 块有 */
  durationMs?: number
}

export type MessageFeedback = "up" | "down" | null

export interface ChatApi {
  init(): Promise<AppInit>
  chat(params: ChatParams, onEvent: ChatEventHandler): Promise<void>
  cancelChat(sessionId: string): Promise<void>
  getMessages(sessionId: string): Promise<StoredMessage[]>
  listSessions(): Promise<SessionSummary[]>
  newSession(): Promise<SessionSummary>
  deleteSession(id: string): Promise<void>
  renameSession(id: string, title: string): Promise<void>
  setConfig(config: GuiConfig): Promise<void>
  /** 设置/清除某条消息的反馈（persisted by backend）；messageId 为 StoredMessage.id */
  setMessageFeedback(
    sessionId: string,
    messageId: string,
    feedback: MessageFeedback,
  ): Promise<void>
  /** 内置目录的供应商模板（DSH 风格"已知路由"） */
  listProviderTemplates(): Promise<ProviderTemplate[]>
  /** 创建/更新供应商（upsert），返回刷新后的供应商列表 */
  addProvider(draft: ProviderDraft): Promise<ProviderInfo[]>
  /** 删除供应商，返回刷新后的供应商列表 */
  deleteProvider(id: string): Promise<ProviderInfo[]>
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

export interface StoredMessage {
  id: string
  role: "user" | "assistant" | "tool"
  content: string
  reasoning?: string
  thinkingMs?: number
  status: "pending" | "streaming" | "done" | "error" | "cancelled"
  error?: string
  feedback?: MessageFeedback
  createdAt: number
  /** 用户消息的多模态附件（图片/文件），随消息持久化 */
  attachments?: GuiAttachment[]
  usage?: GuiUsage
  timings?: GuiTimings
  /** 工具调用（assistant）/ 工具结果（tool 角色消息） */
  tools?: GuiToolBlock[]
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
  /** 模型支持的思考强度（off/low/medium/high/max）；缺省 = 不支持思考 */
  reasoningLevels?: string[]
}

export interface ModelSelection {
  provider: string
  model: string
  /** 思考强度（off/low/medium/high/max）；由"深度思考"开关映射，缺省让后端用模型默认 */
  reasoningEffort?: string
}

/** 附件（多模态）：图片为 Base64 dataUrl，其他文件同样可传 dataUrl 或留空走文件路径方案 */
export interface GuiAttachment {
  id: string
  name: string
  mime: string
  size: number
  dataUrl: string
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
  | { type: "reasoning_delta"; messageId: string; content: string }
  | {
      type: "finished"
      messageId: string
      usage?: GuiUsage
      timings?: GuiTimings
    }
  | { type: "error"; messageId: string; error: string }
  | { type: "cancelled"; messageId: string }
  | { type: "message_start"; messageId: string }
  | {
      type: "tool_call"
      messageId: string
      toolId: string
      toolName: string
      toolArguments: string
    }
  | {
      type: "tool_result"
      messageId: string
      toolId: string
      toolName: string
      toolContent: string
      isError: boolean
      /** 工具执行耗时（毫秒） */
      durationMs?: number
    }

export interface ChatParams {
  sessionId: string
  /** 前端预创建的 assistant 消息 id；后端事件统一用它回推 */
  messageId?: string
  input: string
  model: ModelSelection
  temperature: number
  maxTokens?: number
  attachments?: GuiAttachment[]
}
