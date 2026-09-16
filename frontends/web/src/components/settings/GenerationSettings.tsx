import { useEffect, useState } from "react"
import { useConfigStore } from "@/store/config"

export function GenerationSettings() {
  const config = useConfigStore(s => s.config)
  const saving = useConfigStore(s => s.saving)
  const saveError = useConfigStore(s => s.saveError)
  const update = useConfigStore(s => s.updateConfig)
  const [temperature, setTemperature] = useState(String(Number((config?.temperature ?? 0.7).toFixed(6))))
  const [maxTokens, setMaxTokens] = useState(String(config?.maxTokens ?? ""))
  const [notice, setNotice] = useState("")
  const [dirty, setDirty] = useState(false)
  useEffect(() => {
    if (!dirty) {
      setTemperature(String(Number((config?.temperature ?? 0.7).toFixed(6))))
      setMaxTokens(String(config?.maxTokens ?? ""))
    }
  }, [config?.temperature, config?.maxTokens, dirty])
  return <form className="space-y-3" onSubmit={async e => {
    e.preventDefault()
    const t = Number(temperature), m = maxTokens.trim() ? Number(maxTokens) : undefined
    if (!temperature.trim() || !Number.isFinite(t) || t < 0 || t > 2 || (m !== undefined && (!Number.isSafeInteger(m) || m < 1 || m > 4294967295))) {
      setNotice("温度应为 0–2；最大输出应为正整数或留空。")
      return
    }
    setNotice("")
    if (await update({ temperature: t, maxTokens: m })) {
      setDirty(false)
      setNotice("已保存，下一次生成生效。")
    }
  }}>
    <p className="text-sm font-medium">生成设置</p>
    <p className="text-xs text-muted-foreground">模型选择与参数保存在配置文件中。已有会话恢复上次使用的模型，新会话使用保存的默认模型。</p>
    <label className="block text-sm">温度（0–2）<input aria-label="温度" disabled={saving} type="number" min="0" max="2" step="any" required value={temperature} onChange={e => {setTemperature(e.target.value);setDirty(true);setNotice("")}} className="mt-2 w-full rounded-lg border border-border bg-background p-2.5 disabled:opacity-50" /></label>
    <label className="block text-sm">最大输出 tokens<input aria-label="最大输出 tokens" disabled={saving} type="number" min="1" max="4294967295" step="1" placeholder="留空使用模型默认值" value={maxTokens} onChange={e => {setMaxTokens(e.target.value);setDirty(true);setNotice("")}} className="mt-2 w-full rounded-lg border border-border bg-background p-2.5 disabled:opacity-50" /></label>
    {dirty && <p className="text-xs text-muted-foreground">有未保存的修改</p>}
    <button disabled={saving} className="rounded border px-3 py-2 text-sm disabled:opacity-50">{saving ? "正在保存…" : "保存生成设置"}</button>
    {(saveError || notice) && <p role="status" className="break-words text-sm">{saveError || notice}</p>}
  </form>
}
