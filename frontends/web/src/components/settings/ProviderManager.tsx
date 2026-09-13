import { useEffect, useMemo, useState } from "react"
import { Pencil, Plus, Trash2, X } from "lucide-react"
import { useConfigStore } from "@/store/config"
import { cn } from "@/lib/utils"
import type { ProviderDraft, ProviderInfo, ProviderTemplate } from "@/api/types"

async function getApi() {
  const mod = await import("@/api")
  return mod.getApi()
}

const PROTOCOLS = [
  "openai",
  "openai_responses",
  "anthropic",
  "gemini",
  "ollama",
] as const

/** 与后端一致的路由 id 规则（TOML 键安全 + 小写 kebab） */
const ID_RE = /^[a-z][a-z0-9-]*$/

interface ModelRow {
  id: string
  displayName: string
}

const emptyModels: ModelRow[] = []

/**
 * 模型供应商管理（DSH ui-settings-models 风格）：
 * - 内置目录模板：选中自动填充协议/端点/模型，补个 key 即可
 * - 自定义供应商：手填 id/协议/端点/模型
 * - 编辑已有供应商：只改模型与 key，协议/端点留空 = 保持不变（最小 path 变更）
 * - 删除：confirm 后移除
 */
export function ProviderManager() {
  const providers = useConfigStore((s) => s.providers)
  const setProviders = useConfigStore((s) => s.setProviders)

  const [templates, setTemplates] = useState<ProviderTemplate[]>([])
  const [adding, setAdding] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | undefined>(undefined)

  // 表单字段
  const [mode, setMode] = useState<"template" | "custom">("template")
  const [templateId, setTemplateId] = useState("")
  const [id, setId] = useState("")
  const [protocol, setProtocol] = useState("openai")
  const [baseUrl, setBaseUrl] = useState("")
  const [apiKey, setApiKey] = useState("")
  const [models, setModels] = useState<ModelRow[]>(emptyModels)

  const configured = useMemo(
    () => new Set(providers.map((p) => p.id)),
    [providers],
  )
  const availableTemplates = useMemo(
    () => templates.filter((t) => !configured.has(t.id)),
    [templates, configured],
  )

  useEffect(() => {
    if (!adding) return
    getApi()
      .then((a) => a.listProviderTemplates())
      .then(setTemplates)
      .catch(e => setError(`供应商模板加载失败：${String(e)}；可切换自定义添加或重新打开重试。`))
  }, [adding])

  const resetForm = () => {
    setTemplateId("")
    setId("")
    setProtocol("openai")
    setBaseUrl("")
    setApiKey("")
    setModels(emptyModels)
    setError(undefined)
  }

  const openAdd = () => {
    setEditing(null)
    setMode("template")
    resetForm()
    setAdding(true)
  }

  const openEdit = (p: ProviderInfo) => {
    setEditing(p.id)
    setMode("custom")
    setTemplateId("")
    setId(p.id)
    // 协议/端点留空 = 后端保持原值（最小变更）
    setProtocol("")
    setBaseUrl("")
    setApiKey("")
    setModels(p.models.map((m) => ({ id: m.id, displayName: m.displayName })))
    setError(undefined)
    setAdding(true)
  }

  const closeForm = () => {
    setAdding(false)
    setEditing(null)
  }

  const selectTemplate = (tid: string) => {
    setTemplateId(tid)
    const t = templates.find((x) => x.id === tid)
    if (!t) return
    setId(t.id)
    setProtocol(t.protocol)
    setBaseUrl(t.baseUrl)
    setModels(t.models.map((m) => ({ id: m.id, displayName: m.displayName })))
  }

  const switchMode = (next: "template" | "custom") => {
    setMode(next)
    setError(undefined)
    setTemplateId("")
    setId("")
    // 创建模式下默认 OpenAI 兼容协议；编辑模式（openEdit）单独置空 = 保持不变
    setProtocol("openai")
    setBaseUrl("")
    setModels(emptyModels)
  }

  const idInvalid = id.length > 0 && !ID_RE.test(id)
  const idTaken = editing === null && id.length > 0 && configured.has(id)
  const missingProtocol =
    editing === null && mode === "custom" && protocol.trim() === ""
  const missingBase = editing === null && mode === "custom" && baseUrl.trim() === ""
  const missingModels = models.length === 0
  const anyEmptyModel = models.some((m) => m.id.trim() === "")
  const missingKey = editing === null && apiKey.trim() === ""
  const ready =
    !busy &&
    id.length > 0 &&
    !idInvalid &&
    !idTaken &&
    (mode === "template" ? templateId !== "" : !missingProtocol && !missingBase) &&
    !missingModels &&
    !missingKey &&
    !anyEmptyModel

  const submit = async () => {
    if (!ready) return
    setBusy(true)
    setError(undefined)
    try {
      const draft: ProviderDraft = {
        id,
        ...(protocol.trim() ? { protocol: protocol.trim() } : {}),
        ...(baseUrl.trim() ? { baseUrl: baseUrl.trim() } : {}),
        ...(apiKey.trim() ? { apiKey: apiKey.trim() } : {}),
        models: models.map((m) => ({
          id: m.id.trim(),
          ...(m.displayName.trim() ? { displayName: m.displayName.trim() } : {}),
        })),
      }
      const next = await (await getApi()).addProvider(draft)
      setProviders(next)
      closeForm()
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    } finally {
      setBusy(false)
    }
  }

  const remove = async (p: ProviderInfo) => {
    if (!window.confirm(`删除供应商 ${p.id}？其配置将从 config 中移除。`)) return
    try {
      const next = await (await getApi()).deleteProvider(p.id)
      setProviders(next)
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e))
    }
  }

  const inputCls =
    "w-full rounded-lg border border-border bg-background px-2.5 py-1.5 text-sm outline-none transition-colors focus:border-zinc-400 dark:focus:border-zinc-500 disabled:opacity-50"
  const labelCls = "text-xs font-medium text-muted-foreground"

  return (
    <div className="space-y-2.5">
      <p className="text-sm font-medium">模型供应商</p>

      {error && !adding && <p role="alert" className="text-sm text-red-500">{error}</p>}
      {/* 已配置供应商列表 */}
      {providers.length > 0 && (
        <ul className="space-y-1.5">
          {providers.map((p) => (
            <li
              key={p.id}
              className="flex items-center gap-2 rounded-lg border border-border bg-muted/40 px-2.5 py-1.5"
            >
              <div className="min-w-0 flex-1">
                <p className="truncate text-sm font-medium">{p.id}</p>
                <p className="truncate text-xs text-muted-foreground">
                  {p.models.length} 个模型
                </p>
              </div>
              <button
                type="button"
                onClick={() => openEdit(p)}
                className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
                title={`编辑 ${p.id}`}
              >
                <Pencil className="h-3.5 w-3.5" />
              </button>
              <button
                type="button"
                onClick={() => void remove(p)}
                className="flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-red-500/10 hover:text-red-500"
                title={`删除 ${p.id}`}
              >
                <Trash2 className="h-3.5 w-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}

      {!adding ? (
        <button
          type="button"
          onClick={openAdd}
          className="flex w-full items-center justify-center gap-1.5 rounded-lg border border-dashed border-border py-2 text-sm text-muted-foreground transition-colors hover:border-zinc-400 hover:text-foreground"
        >
          <Plus className="h-4 w-4" />
          添加供应商
        </button>
      ) : (
        /* —— 添加/编辑表单（DSH ProviderEditor 风格） —— */
        <div className="space-y-3 rounded-lg border border-border p-3">
          <div className="flex items-center justify-between">
            <p className="text-sm font-medium">
              {editing ? `编辑 ${editing}` : "添加供应商"}
            </p>
            <button
              type="button"
              onClick={closeForm}
              className="flex h-6 w-6 items-center justify-center rounded-md text-muted-foreground hover:bg-accent hover:text-foreground"
              title="关闭"
            >
              <X className="h-4 w-4" />
            </button>
          </div>

          {editing === null && (
            <div className="grid grid-cols-2 gap-1 rounded-lg bg-muted/50 p-1 text-sm">
              {(
                [
                  { key: "template", label: "内置模板" },
                  { key: "custom", label: "自定义" },
                ] as const
              ).map(({ key, label }) => (
                <button
                  key={key}
                  type="button"
                  onClick={() => switchMode(key)}
                  className={cn(
                    "rounded-md py-1 transition-colors",
                    mode === key
                      ? "bg-background font-medium shadow-sm"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  {label}
                </button>
              ))}
            </div>
          )}

          {mode === "template" && editing === null && (
            <div className="space-y-1">
              <span className={labelCls}>供应商模板</span>
              <select
                className={inputCls}
                value={templateId}
                onChange={(e) => selectTemplate(e.target.value)}
              >
                <option value="">选择一个内置供应商…</option>
                {availableTemplates.map((t) => (
                  <option key={t.id} value={t.id}>
                    {t.displayName}（{t.id}）
                  </option>
                ))}
              </select>
              <p className="text-xs text-muted-foreground/70">
                协议/端点/模型已自动填充，只需填入 API Key
              </p>
            </div>
          )}

          {editing === null && mode === "custom" && (
            <div className="space-y-1">
              <span className={labelCls}>供应商 ID</span>
              <input
                className={inputCls}
                value={id}
                placeholder="acme-gateway"
                onChange={(e) => setId(e.target.value)}
              />
              {idInvalid ? (
                <p className="text-xs text-red-500">
                  只能用小写字母/数字/连字符（如 acme-gateway）
                </p>
              ) : idTaken ? (
                <p className="text-xs text-red-500">该供应商已存在</p>
              ) : null}
            </div>
          )}

          {editing !== null && (
            <p className="text-xs text-muted-foreground/70">
              协议与端点保持不变；下方字段留空 = 不修改，模型列表将整体替换
            </p>
          )}

          {mode === "custom" && (
            <div className="space-y-1">
              <span className={labelCls}>协议</span>
              <select
                className={inputCls}
                value={protocol}
                disabled={editing !== null}
                onChange={(e) => setProtocol(e.target.value)}
              >
                <option value="">保持不变</option>
                {PROTOCOLS.map((p) => (
                  <option key={p} value={p}>
                    {p}
                  </option>
                ))}
              </select>
            </div>
          )}

          <div className="space-y-1">
            <span className={labelCls}>
              {editing ? "端点（留空保持不变）" : "Base URL"}
            </span>
            <input
              className={inputCls}
              value={baseUrl}
              placeholder="https://gateway.example/v1"
              onChange={(e) => setBaseUrl(e.target.value)}
            />
          </div>

          <div className="space-y-1">
            <label htmlFor="provider-api-key" className={labelCls}>
              {editing ? "API Key（留空保持现有密钥）" : "API Key（新建时必填）"}
            </label>
            <input
              id="provider-api-key"
              aria-describedby="provider-api-key-hint"
              required={editing === null}
              className={inputCls}
              type="password"
              autoComplete="off"
              value={apiKey}
              placeholder={editing ? "留空 = 保持现有 key" : "sk-…"}
              onChange={(e) => setApiKey(e.target.value)}
            />
            <p id="provider-api-key-hint" className="text-xs text-muted-foreground">
              {editing
                ? "现有密钥或环境变量引用不会显示；仅填写时才替换。"
                : "填写此供应商的密钥后即可创建；无认证的本地服务可填写占位值。"}
            </p>
          </div>

          {/* 模型列表 */}
          <div className="space-y-1">
            <span className={labelCls}>模型（至少 1 个）</span>
            <ul className="space-y-1.5">
              {models.map((m, i) => (
                <li key={i} className="flex items-center gap-1.5">
                  <input
                    className={cn(inputCls, "flex-1")}
                    value={m.id}
                    placeholder="模型 id（wire 名）"
                    onChange={(e) =>
                      setModels((prev) =>
                        prev.map((x, j) => (j === i ? { ...x, id: e.target.value } : x)),
                      )
                    }
                  />
                  <input
                    className={cn(inputCls, "w-32")}
                    value={m.displayName}
                    placeholder="显示名（可选）"
                    onChange={(e) =>
                      setModels((prev) =>
                        prev.map((x, j) =>
                          j === i ? { ...x, displayName: e.target.value } : x,
                        ),
                      )
                    }
                  />
                  <button
                    type="button"
                    onClick={() =>
                      setModels((prev) => prev.filter((_, j) => j !== i))
                    }
                    className="flex h-7 w-7 shrink-0 items-center justify-center rounded-md text-muted-foreground hover:bg-red-500/10 hover:text-red-500"
                    title="移除该模型"
                  >
                    <Trash2 className="h-3.5 w-3.5" />
                  </button>
                </li>
              ))}
            </ul>
            <button
              type="button"
              onClick={() =>
                setModels((prev) => [...prev, { id: "", displayName: "" }])
              }
              className="flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
            >
              <Plus className="h-3 w-3" />
              添加模型
            </button>
          </div>

          {error ? (
            <p className="rounded-md border border-red-500/30 bg-red-500/5 px-2 py-1.5 text-xs text-red-500">
              {error}
            </p>
          ) : null}

          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={closeForm}
              className="rounded-lg px-3 py-1.5 text-sm text-muted-foreground transition-colors hover:bg-accent hover:text-foreground"
            >
              取消
            </button>
            <button
              type="button"
              disabled={!ready}
              onClick={() => void submit()}
              className="rounded-lg bg-zinc-900 px-4 py-1.5 text-sm font-medium text-white transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-40 dark:bg-zinc-100 dark:text-zinc-900"
            >
              {busy ? "保存中…" : editing ? "保存" : "创建"}
            </button>
          </div>
        </div>
      )}
    </div>
  )
}
