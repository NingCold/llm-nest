import type { GenerationLimits } from "./live-types"

export function generationError(temperature: string, output: string, limits: GenerationLimits): string | null {
  if (temperature.trim()) {
    if (limits.temperatureMax === null) return "当前模型或思考模式不支持调整温度，请清空温度。"
    const t = Number(temperature)
    if (!Number.isFinite(t) || t < 0 || t > limits.temperatureMax) return `温度必须在 0 到 ${limits.temperatureMax} 之间。`
  }
  if (limits.maxOutputTokens !== null && limits.minOutputTokens > limits.maxOutputTokens) return "当前思考预算超过模型输出能力，请选择其他思考强度或模型。"
  if (output.trim()) {
    const max = Number(output)
    if (!Number.isSafeInteger(max) || max < limits.minOutputTokens || max > (limits.maxOutputTokens ?? 4294967295)) return `输出上限必须是至少 ${limits.minOutputTokens}${limits.maxOutputTokens === null ? "" : `、最多 ${limits.maxOutputTokens}`} 的整数。`
  }
  return null
}

export function GenerationFields({ temperature, output, limits, setTemperature, setOutput }: {
  temperature: string; output: string; limits: GenerationLimits
  setTemperature(value: string): void; setOutput(value: string): void
}) {
  const error = generationError(temperature, output, limits)
  return <>
    <label>温度<input aria-label="AI 温度" type="number" min="0" max={limits.temperatureMax ?? undefined} step="any" disabled={limits.temperatureMax === null} value={temperature} onChange={e => setTemperature(e.target.value)} placeholder={limits.temperatureMax === null ? "当前模式不支持" : "留空使用模型服务默认值"} /></label>
    {limits.temperatureMax === null && temperature && <button type="button" className="text-button" onClick={() => setTemperature("")}>清空温度</button>}
    <label>最大输出 token 数<input aria-label="AI 最大输出 token 数" type="number" min={limits.minOutputTokens} max={limits.maxOutputTokens ?? 4294967295} step="1" value={output} onChange={e => setOutput(e.target.value)} placeholder="留空由意识系统分配" /></label>
    <p className="form-note">输出上限包括模型计入输出的思考 token。留空时使用这个 AI 的默认策略，不继承全局生成设置。{limits.minOutputTokens > 1 && `当前思考强度要求输出至少 ${limits.minOutputTokens} token。`}</p>
    {error && <p role="alert" className="form-error">{error}</p>}
  </>
}
