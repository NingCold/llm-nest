import { useMemo, useState } from "react"
import hljs from "highlight.js"
import { Check, Copy } from "lucide-react"
import { copyText } from "@/lib/format"

/** 常见别名 → highlight.js 语言名 */
const ALIASES: Record<string, string> = {
  js: "javascript",
  ts: "typescript",
  tsx: "typescript",
  jsx: "javascript",
  py: "python",
  rb: "ruby",
  rs: "rust",
  sh: "bash",
  shell: "bash",
  zsh: "bash",
  yml: "yaml",
  md: "markdown",
  kt: "kotlin",
  go: "go",
  cpp: "cpp",
  cs: "csharp",
  html: "xml",
  toml: "ini",
}

function escapeHtml(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
}

export function CodeBlock({ code, language }: { code: string; language?: string }) {
  const [copied, setCopied] = useState(false)

  const highlighted = useMemo(() => {
    const lang = ALIASES[language ?? ""] ?? language
    try {
      if (lang && hljs.getLanguage(lang)) {
        return hljs.highlight(code, { language: lang }).value
      }
      return hljs.highlightAuto(code).value
    } catch {
      return escapeHtml(code)
    }
  }, [code, language])

  const handleCopy = async () => {
    const ok = await copyText(code)
    if (ok) {
      setCopied(true)
      setTimeout(() => setCopied(false), 1800)
    }
  }

  return (
    <div className="my-3 overflow-hidden rounded-xl border border-[#21262d] bg-[#0d1117]">
      {/* Header bar */}
      <div className="flex items-center justify-between border-b border-white/10 bg-white/[0.04] px-4 py-2">
        <span className="font-mono text-xs font-medium text-zinc-400">
          {language || "text"}
        </span>
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
              复制代码
            </>
          )}
        </button>
      </div>
      {/* Code body */}
      <pre
        className="overflow-x-auto"
        style={{ margin: 0, background: "transparent" }}
      >
        <code
          dangerouslySetInnerHTML={{ __html: highlighted }}
          className="hljs"
          style={{ display: "block", padding: "0.95em 1.1em" }}
        />
      </pre>
    </div>
  )
}
