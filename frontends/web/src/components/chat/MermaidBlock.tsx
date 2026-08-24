import { useEffect, useState } from "react"
import mermaid from "mermaid"
import { Check, Copy } from "lucide-react"
import { useUiStore } from "@/store/ui"
import { copyText } from "@/lib/format"

let uid = 0

/* mermaid 是全局单例：把渲染串行化，避免 React StrictMode 双挂载并发 render
   互相污染（症状：偶发 "Syntax error in text"）。initialize 也在链内执行，
   保证每次渲染前主题生效、且不与在飞渲染冲突。 */
let renderChain: Promise<unknown> = Promise.resolve()

function renderMermaid(code: string, theme: string, loose = false): Promise<string> {
  const id = `mermaid-${Date.now()}-${uid++}`
  const run = renderChain.then(() => {
    mermaid.initialize({
      startOnLoad: false,
      theme: theme === "dark" ? "dark" : "default",
      // 流式输入过程中代码不完整时 mermaid 可能 resolve 一个错误 SVG；
      // 严格模式对 <br/> 等 HTML 也较敏感，宽松模式作为重试兜底
      securityLevel: loose ? "loose" : "strict",
      fontFamily:
        'ui-sans-serif, system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
    })
    return mermaid
      .render(id, code)
      .then(({ svg }) => svg)
      .finally(() => {
        // mermaid 渲染失败时会把错误 SVG 挂在 body 下的临时容器（id = "d" + id）
        // 里且不清理（成功时才自行移除）；这里兜底删除，防止流式期间
        // 部分代码的失败渲染在页面底部堆积孤儿错误图。
        document.getElementById(`d${id}`)?.remove()
      })
  })
  // 链上出错也不能毒化后续渲染
  renderChain = run.catch(() => {})
  return run
}

/** mermaid 失败时 resolve 的是带错误文案的 SVG，而不是 reject。
 *  注意：正常 SVG 的 <style> 里也定义了 .error-icon/.error-text 样式类，
 *  只能匹配实际错误文案 "Syntax error"，不能匹配类名。 */
function isErrorSvg(svg: string): boolean {
  return /Syntax error/i.test(svg)
}

/**
 * Mermaid 图表块：与代码块同款头部（语言标签 + 复制按钮），
 * 按当前主题（深/浅）渲染，失败时回退为错误提示 + 原始源码。
 */
export function MermaidBlock({ code }: { code: string }) {
  const theme = useUiStore((s) => s.theme)
  const [svg, setSvg] = useState<string | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    let cancelled = false

    const commit = (rendered: string | null, err: string | null) => {
      if (cancelled) return
      setSvg(rendered)
      setError(err)
    }

    renderMermaid(code, theme)
      .then((rendered) => {
        if (isErrorSvg(rendered)) {
          // 严格模式渲染失败 → 宽松模式重试一次（放行 <br/> 等 HTML）
          return renderMermaid(code, theme, true).then((retried) => {
            if (isErrorSvg(retried)) {
              commit(null, "Syntax error in text")
            } else {
              commit(retried, null)
            }
          })
        }
        commit(rendered, null)
      })
      .catch((err) => {
        commit(null, err?.message ? String(err.message) : String(err))
      })

    return () => {
      cancelled = true
    }
  }, [code, theme])

  const handleCopy = async () => {
    const ok = await copyText(code)
    if (ok) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    }
  }

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-[#21262d] bg-[#0d1117]">
      <div className="flex items-center justify-between border-b border-white/10 bg-white/[0.04] px-4 py-2">
        <span className="font-mono text-xs font-medium text-zinc-400">mermaid</span>
        <button
          type="button"
          onClick={handleCopy}
          className="flex items-center gap-1.5 rounded-md px-2 py-1 font-sans text-xs text-zinc-400 transition-colors hover:bg-white/10 hover:text-zinc-100"
        >
          {copied ? (
            <>
              <Check className="h-3.5 w-3.5 text-emerald-400" />
              <span className="text-emerald-400">已复制</span>
            </>
          ) : (
            <>
              <Copy className="h-3.5 w-3.5" />
              复制
            </>
          )}
        </button>
      </div>
      <div className="mermaid-svg p-4">
        {error ? (
          <div>
            <p className="text-sm text-red-400">图表渲染失败：{error}</p>
            <pre className="mt-2 overflow-x-auto whitespace-pre-wrap text-xs leading-relaxed text-zinc-400">
              {code}
            </pre>
          </div>
        ) : svg ? (
          <div dangerouslySetInnerHTML={{ __html: svg }} />
        ) : (
          <div className="flex h-20 items-center justify-center">
            <span className="h-1.5 w-1.5 rounded-full bg-zinc-500 animate-pulse-dot" />
            <span className="ml-2 text-xs text-zinc-400">渲染图表中…</span>
          </div>
        )}
      </div>
    </div>
  )
}
