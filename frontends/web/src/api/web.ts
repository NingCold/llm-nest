import type {
  AppInit,
  ChatApi,
  ChatParams,
  ChatEventHandler,
  GuiConfig,
  GuiAttachment,
  GuiTimings,
  GuiUsage,
  ProviderDraft,
  ProviderInfo,
  ProviderTemplate,
  SessionSummary,
} from "./types"
import { useUiStore } from "@/store/ui"

/* ------------------------------------------------------------------ *
 *  Web demo adapter — simulates the LLM Nest backend entirely in the
 *  browser (localStorage persistence + streamed reasoning/content).
 *  The Tauri shell keeps using tauri.ts; this adapter makes `npm run
 *  dev` / the built page fully interactive for design & demo purposes.
 * ------------------------------------------------------------------ */

const DB_KEY = "llm-nest-demo-v1"
const CFG_KEY = "llm-nest-demo-config-v1"

export interface DemoMessage {
  id: string
  role: "user" | "assistant"
  content: string
  reasoning?: string
  status: "done" | "streaming" | "pending" | "error" | "cancelled"
  error?: string
  thinkingMs?: number
  feedback?: "up" | "down" | null
  attachments?: GuiAttachment[]
  usage?: GuiUsage
  timings?: GuiTimings
  createdAt: number
}

interface DemoDB {
  sessions: SessionSummary[]
  messages: Record<string, DemoMessage[]>
  /** 演示供应商列表（可从设置里添加/删除，持久化） */
  providers: ProviderInfo[]
}

/* ---------------- canned demo content ---------------- */

const PROVIDERS: ProviderInfo[] = [
  {
    id: "deepseek",
    displayName: "DeepSeek",
    models: [
      { id: "deepseek-reasoner", displayName: "DeepSeek-R1" },
      { id: "deepseek-chat", displayName: "DeepSeek-V3" },
    ],
  },
  {
    id: "anthropic",
    displayName: "Anthropic",
    models: [
      { id: "claude-5-sonnet", displayName: "Claude 5 Sonnet" },
      { id: "claude-4-5-opus", displayName: "Claude Opus 4.5" },
    ],
  },
  {
    id: "openai",
    displayName: "OpenAI",
    models: [
      { id: "gpt-5.6", displayName: "GPT-5.6" },
      { id: "gpt-4.1-mini", displayName: "GPT-4.1 mini" },
    ],
  },
  {
    id: "gemini",
    displayName: "Google",
    models: [{ id: "gemini-3.7-pro", displayName: "Gemini 3.7 Pro" }],
  },
  {
    id: "ecnu",
    displayName: "ECNU",
    models: [{ id: "ecnu-max", displayName: "ECNU Max" }],
  },
]

const DEFAULT_MODEL = { provider: "deepseek", model: "deepseek-reasoner" }

function buildReasoning(input: string): string[] {
  return [
    `用户的问题：${input.trim().slice(0, 60) || "（未提供具体内容）"}`,
    "我需要先理解问题的边界：是想要完整可运行的示例，还是侧重讲解原理？",
    "从提问方式看，用户希望兼顾「可运行」与「关键点解释」，所以我按\"代码 + 要点清单\"的结构组织回答。",
    "回答中会包含一段带语法高亮的代码块，并配一个对照表帮助理解。",
  ]
}

function buildAnswer(input: string): string {
  if (/rust|代码|服务器|http|编程/i.test(input)) {
    return `## 一个最小可用的异步 HTTP 服务器

下面用 **tokio + hyper** 实现一个返回 \`Hello, world\` 的服务器，整套代码只需要一个文件：

\`\`\`rust
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use std::convert::Infallible;

async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello, world!")))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let make_svc = make_service_fn(|_conn| async {
        Ok::<_, Infallible>(service_fn(handle))
    });

    let addr = "127.0.0.1:3000".parse()?;
    println!("listening on {}", addr);

    Server::bind(&addr).serve(make_svc).await?;
    Ok(())
}
\`\`\`

### 几个关键点

1. **异步运行时**：\`#[tokio::main]\` 宏负责启动 tokio runtime，\`serve().await\` 之后进程不会退出；
2. **Service 模型**：\`make_service_fn\` 为每个连接构造服务，\`service_fn\` 把普通函数包装成 hyper 的 \`Service\` trait；
3. **错误处理**：返回 \`Result<_, Infallible>\` 表示该服务永不失败，简化了示例。

> 提示：生产环境建议叠加超时与并发限制（\`tower\` 的 \`Timeout\` / \`ConcurrencyLimit\`）。

### 下一步

| 需求 | 推荐 |
| --- | --- |
| 路由匹配 | 引入 \`axum\`（基于 hyper 的路由 DSL） |
| TLS | \`rustls\` + \`axum-server\` |
| 优雅停机 | \`tokio::signal\` 监听 SIGTERM |

如果还想继续深入，我可以帮你把这段代码改造成带 \`/api\` 路由的 axum 版本。`
  }
  if (/翻译|translate/i.test(input)) {
    return `## 翻译结果

**原文**：${input.trim()}

**译文**：Your message has been translated into natural, idiomatic English with the tone preserved.

### 词句对照

| 原文 | 译文 | 说明 |
| --- | --- | --- |
| 译文示例 | translation sample | 术语保持直译 |
| 语气自然 | natural tone | 意译优先 |

> 说明：如果这是商务场景，我可以再调整一版更正式的语气。`
  }
  if (/公式|latex|math|数学/i.test(input)) {
    return `## 三个常见的数学公式

### 1. 一元二次方程的求根公式

对于 $ax^2 + bx + c = 0$（$a \\ne 0$），解为：

$$
x = \\frac{-b \\pm \\sqrt{b^2 - 4ac}}{2a}
$$

### 2. 欧拉公式

$$
e^{i\\theta} = \\cos\\theta + i\\sin\\theta
$$

当 $\\theta = \\pi$ 时得到 $e^{i\\pi} + 1 = 0$，常被称为数学中最优美的恒等式之一。

### 3. 正态分布的概率密度函数

$$
f(x) = \\frac{1}{\\sigma\\sqrt{2\\pi}} e^{-\\frac{(x-\\mu)^2}{2\\sigma^2}}
$$

> 需要我展开推导其中某一个，或补充微积分、线性代数、概率统计方向的常用公式吗？`
  }
  if (/推导|求导|梯度|反向传播|backprop/i.test(input)) {
    return `## 反向传播的梯度推导

以最简单的两层网络为例，损失 $L$ 对权重 $w$ 的梯度可以通过链式法则逐层回传：

$$
\\frac{\\partial L}{\\partial w} = \\frac{\\partial L}{\\partial y} \\cdot \\frac{\\partial y}{\\partial z} \\cdot \\frac{\\partial z}{\\partial w}
$$

其中 $z = w x + b$，激活函数为 $\\sigma$，则：

- $\\dfrac{\\partial z}{\\partial w} = x$
- $\\dfrac{\\partial y}{\\partial z} = \\sigma'(z)$

### 关键结论

1. **链式法则**：梯度是各层局部导数的乘积；
2. **参数更新**：$w \\leftarrow w - \\eta \\frac{\\partial L}{\\partial w}$，$\\eta$ 为学习率；
3. **数值稳定性**：Softmax 常与交叉熵配合，梯度可化简为 $y - t$。

> 练习：对 $L = \\frac{1}{2}(y - t)^2$ 手推一次，结果应与 $\\sigma$ 的导数形式一致。`
  }
  if (/架构|mermaid|流程图|拓扑|画图/i.test(input)) {
    return `## 系统架构流程

下面用 Mermaid 画一个典型的前后端 + LLM 网关架构：

\`\`\`mermaid
graph TD
    A["桌面客户端<br/>React + Tauri"] -->|invoke| B[Runtime]
    B --> C[SessionManager]
    B --> D[AiClient]
    D --> E{ModelRouter}
    E -->|protocol| F["OpenAI Provider"]
    E -->|protocol| G["Anthropic Provider"]
    E -->|protocol| H["Gemini Provider"]
    F --> I[("LLM API")]
    G --> I
    H --> I
    B --> J[("Session Store")]
\`\`\`

### 说明

- 客户端只调用 **Feature 业务方法**，消费 \`ChatEvent\` 流渲染；
- \`AiClient\` 负责模型路由 + 协议分发，\`ModelRouter\` 在首个 await 前冻结快照；
- 会话持久化写穿透：先落盘成功再改内存，失败不静默。

> 修改 \`graph TD\` 为 \`graph LR\` 可切换横向布局。`
  }
  return `## 关于这个问题

这是一个很好的问题，我从几个角度展开：

### 核心思路

- **先明确目标**：搞清楚真正要解决的问题，而不是直接动手；
- **拆解步骤**：把大问题切成 2–3 个小步骤，逐个击破；
- **验证反馈**：每完成一步都检查结果，及时纠偏。

### 推荐的做法

1. 先写一版最简实现，跑通主路径；
2. 再补充边界情况与错误处理；
3. 最后做优化（性能 / 可读性）。

| 阶段 | 关注点 | 产出 |
| --- | --- | --- |
| 明确目标 | 需求、约束 | 一句话描述 |
| 最简实现 | 主路径 | 可运行的 Demo |
| 打磨 | 边界、错误 | 稳定版本 |

### 一个小示例

\`\`\`ts
const result = items
  .filter((it) => it.active)
  .map((it) => it.value)
  .reduce((a, b) => a + b, 0)
\`\`\`

如果还有更具体的背景，欢迎补充细节，我可以给出更贴合你场景的方案。`
}

/* ---------------- persistence helpers ---------------- */

let didReset = false

function loadDB(): DemoDB {
  try {
    // 调试/截图用：?reset=1 重置演示数据（仅每次页面加载生效一次）
    if (!didReset && new URLSearchParams(location.search).get("reset")) {
      didReset = true
      localStorage.removeItem(DB_KEY)
      localStorage.removeItem(CFG_KEY)
    }
    const raw = localStorage.getItem(DB_KEY)
    if (raw) {
      const db = JSON.parse(raw) as DemoDB
      // 旧版本数据无 providers 字段：回退内置演示供应商
      db.providers ??= PROVIDERS
      return db
    }
  } catch {
    /* fall through to seed */
  }
  const db = seedDB()
  saveDB(db)
  return db
}

function saveDB(db: DemoDB) {
  try {
    localStorage.setItem(DB_KEY, JSON.stringify(db))
  } catch {
    /* storage full / private mode — demo keeps working in memory */
  }
}

function seedDB(): DemoDB {
  const now = Date.now()
  const iso = (offsetMs: number) => new Date(now - offsetMs).toISOString()
  const H = 3600_000
  const D = 24 * H

  const s1: SessionSummary = {
    id: "s-demo-1",
    title: "用 Rust 写一个异步 HTTP 服务器",
    createdAt: iso(2 * H),
    updatedAt: iso(20 * 60_000),
    messageCount: 2,
  }
  const mk = (
    id: string,
    title: string,
    age: number,
  ): SessionSummary => ({
    id,
    title,
    createdAt: iso(age),
    updatedAt: iso(age - 25 * 60_000),
    messageCount: 0,
  })

  const demoMessages: DemoMessage[] = [
    {
      id: "m-demo-u1",
      role: "user",
      content: "用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点",
      status: "done",
      createdAt: now - 21 * 60_000,
    },
    {
      id: "m-demo-a1",
      role: "assistant",
      content: buildAnswer("rust"),
      reasoning: buildReasoning(
        "用 Rust 写一个简单的异步 HTTP 服务器，并解释关键点",
      ).join("\n"),
      status: "done",
      thinkingMs: 2840,
      createdAt: now - 20 * 60_000,
    },
  ]

  return {
    sessions: [
      s1,
      mk("s-demo-2", "React 组件性能优化技巧", 5 * H),
      mk("s-demo-3", "Tailwind CSS v4 新特性总结", 1 * D + 3 * H),
      mk("s-demo-4", "神经网络反向传播推导", 3 * D),
      mk("s-demo-5", "深夜食堂推荐", 9 * D),
      mk("s-demo-6", "项目周报模板", 26 * D),
    ],
    messages: { "s-demo-1": demoMessages },
    providers: PROVIDERS,
  }
}

function loadConfig(): GuiConfig {
  try {
    const raw = localStorage.getItem(CFG_KEY)
    if (raw) return JSON.parse(raw) as GuiConfig
  } catch {
    /* ignore */
  }
  return { currentModel: DEFAULT_MODEL, temperature: 0.7, maxTokens: 4096 }
}

function saveConfig(cfg: GuiConfig) {
  try {
    localStorage.setItem(CFG_KEY, JSON.stringify(cfg))
  } catch {
    /* ignore */
  }
}

/* ---------------- streaming simulation ---------------- */

const genTokens = new Map<string, number>()

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms))

function chunkText(text: string): string[] {
  const out: string[] = []
  let i = 0
  while (i < text.length) {
    const size = 8 + Math.floor(Math.random() * 14)
    let end = Math.min(i + size, text.length)
    // prefer breaking at whitespace when possible
    if (end < text.length) {
      const nl = text.indexOf("\n", i)
      if (nl !== -1 && nl <= end) end = nl + 1
      else {
        const sp = text.lastIndexOf(" ", end)
        if (sp > i + 4) end = sp + 1
      }
    }
    out.push(text.slice(i, end))
    i = end
  }
  return out
}

export const webApi: ChatApi = {
  async init(): Promise<AppInit> {
    const db = loadDB()
    const providers = db.providers
    const config = loadConfig()
    const sessions = [...db.sessions].sort(
      (a, b) => +new Date(b.updatedAt) - +new Date(a.updatedAt),
    )
    return {
      config,
      providers,
      sessions,
      version: "0.1.0 (web demo)",
    }
  },

  async chat(params: ChatParams, onEvent: ChatEventHandler): Promise<void> {
    const sessionId = params.sessionId
    const token = (genTokens.get(sessionId) ?? 0) + 1
    genTokens.set(sessionId, token)
    const alive = () => genTokens.get(sessionId) === token

    const db = loadDB()
    const messages = db.messages[sessionId] ?? []
    // 前端预创建了 assistant 消息（store 里已有该 id），复用它的 id 保证
    // 事件能落到对应消息上；未传时自己生成（向后兼容）。
    const msgId =
      params.messageId ?? `ai-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`

    onEvent({ type: "message_start", messageId: msgId })

    const userMsg: DemoMessage = {
      id: `u-${Date.now()}`,
      role: "user",
      content: params.input,
      status: "done",
      ...(params.attachments && params.attachments.length > 0
        ? { attachments: params.attachments }
        : {}),
      createdAt: Date.now(),
    }
    messages.push(userMsg)

    const reasoningEnabled =
      useUiStore.getState().reasoningEffort !== "off"
    const streamStart = Date.now()
    const thinkingStart = Date.now()
    let reasoning = ""
    let content = ""

    if (reasoningEnabled) {
      const lines = buildReasoning(params.input)
      for (const line of lines) {
        for (const piece of chunkText(line)) {
          if (!alive()) {
            onEvent({ type: "cancelled", messageId: msgId })
            return
          }
          reasoning += piece
          onEvent({ type: "reasoning_delta", messageId: msgId, content: piece })
          await sleep(18 + Math.random() * 42)
        }
        await sleep(90 + Math.random() * 130)
      }
    }

    const thinkingMs = Date.now() - thinkingStart
    const attachNote = params.attachments?.length
      ? `> 已收到 ${params.attachments.length} 个附件：${params.attachments
          .map((a) => a.name)
          .join("、")}（${params.attachments
          .filter((a) => a.mime.startsWith("image/"))
          .length} 张图片）。\n\n`
      : ""
    const answer = attachNote + buildAnswer(params.input)

    for (const piece of chunkText(answer)) {
      if (!alive()) {
        onEvent({ type: "cancelled", messageId: msgId })
        return
      }
      content += piece
      onEvent({ type: "delta", messageId: msgId, content: piece })
      const pause = piece.endsWith("\n\n") ? 90 : 0
      await sleep(10 + Math.random() * 30 + pause)
    }

    if (!alive()) {
      onEvent({ type: "cancelled", messageId: msgId })
      return
    }

    // 演示用量/计时（与服务端同构，供状态栏与消息统计展示）
    const promptTokens = Math.max(32, Math.round(params.input.length / 2) + 56)
    const completionTokens = Math.max(1, Math.round(content.length / 2))
    const cachedTokens = Math.floor(promptTokens * 0.4)
    const usage: GuiUsage = {
      promptTokens,
      completionTokens,
      totalTokens: promptTokens + completionTokens,
      cachedTokens,
    }
    const timings: GuiTimings = {
      ttftMs: reasoningEnabled ? thinkingMs : Math.round(400 + Math.random() * 600),
      ...(reasoningEnabled ? { reasoningMs: thinkingMs } : {}),
      totalMs: Date.now() - streamStart,
    }

    const assistantMsg: DemoMessage = {
      id: msgId,
      role: "assistant",
      content,
      reasoning: reasoningEnabled ? reasoning : undefined,
      status: "done",
      thinkingMs: reasoningEnabled ? thinkingMs : undefined,
      usage,
      timings,
      createdAt: Date.now(),
    }
    messages.push(assistantMsg)

    const session = db.sessions.find((s) => s.id === sessionId)
    if (session) {
      // 与后端 ChatFeature 的自动标题规则一致：首个问题折叠空白后
      // 截断到 30 字符（超出加省略号）。
      if (session.title === "新对话" || !session.title) {
        const collapsed = params.input.trim().replace(/\s+/g, " ")
        session.title =
          collapsed.slice(0, 30) + (collapsed.length > 30 ? "…" : "")
      }
      session.updatedAt = new Date().toISOString()
      session.messageCount = messages.length
    }
    db.messages[sessionId] = messages
    saveDB(db)

    onEvent({ type: "finished", messageId: msgId, usage, timings })
  },

  async cancelChat(sessionId: string): Promise<void> {
    genTokens.set(sessionId, (genTokens.get(sessionId) ?? 0) + 1)
  },

  async getMessages(sessionId: string) {
    const db = loadDB()
    return db.messages[sessionId] ?? []
  },

  async listSessions(): Promise<SessionSummary[]> {
    const db = loadDB()
    return [...db.sessions].sort(
      (a, b) => +new Date(b.updatedAt) - +new Date(a.updatedAt),
    )
  },

  async newSession(): Promise<SessionSummary> {
    const db = loadDB()
    const session: SessionSummary = {
      id: `s-${Date.now()}-${Math.random().toString(36).slice(2, 7)}`,
      title: "新对话",
      createdAt: new Date().toISOString(),
      updatedAt: new Date().toISOString(),
      messageCount: 0,
    }
    db.sessions.push(session)
    db.messages[session.id] = []
    saveDB(db)
    return session
  },

  async deleteSession(id: string): Promise<void> {
    const db = loadDB()
    db.sessions = db.sessions.filter((s) => s.id !== id)
    delete db.messages[id]
    saveDB(db)
  },

  async renameSession(id: string, title: string): Promise<void> {
    const db = loadDB()
    const session = db.sessions.find((s) => s.id === id)
    if (session) {
      session.title = title
      saveDB(db)
    }
  },

  async setConfig(config: GuiConfig): Promise<void> {
    saveConfig(config)
  },

  async setMessageFeedback(
    sessionId: string,
    messageId: string,
    feedback: "up" | "down" | null,
  ): Promise<void> {
    const db = loadDB()
    const messages = db.messages[sessionId] ?? []
    const msg = messages.find((m) => m.id === messageId)
    if (msg) {
      msg.feedback = feedback
      saveDB(db)
    }
  },

  async listProviderTemplates(): Promise<ProviderTemplate[]> {
    // 演示：内置 PROVIDERS 本身就是模板（协议 openai、模型即清单）
    return PROVIDERS.map((p) => ({
      id: p.id,
      displayName: p.displayName,
      protocol: "openai",
      baseUrl: "",
      apiKeyEnv: `${p.id.toUpperCase().replace(/[^A-Z0-9]+/g, "_")}_API_KEY`,
      defaultModel: p.models[0]?.id ?? "",
      models: p.models.map((m) => ({ id: m.id, displayName: m.displayName })),
    }))
  },

  async addProvider(draft: ProviderDraft): Promise<ProviderInfo[]> {
    const db = loadDB()
    const existing = db.providers.find((p) => p.id === draft.id)
    const info: ProviderInfo = {
      id: draft.id,
      displayName: draft.id,
      models: (draft.models ?? []).map((m) => ({
        id: m.model ?? m.id,
        displayName: m.displayName ?? m.model ?? m.id,
      })),
    }
    if (existing) {
      existing.displayName = info.displayName
      existing.models = info.models
    } else {
      db.providers.push(info)
    }
    saveDB(db)
    return db.providers
  },

  async deleteProvider(id: string): Promise<ProviderInfo[]> {
    const db = loadDB()
    db.providers = db.providers.filter((p) => p.id !== id)
    saveDB(db)
    return db.providers
  },
}
