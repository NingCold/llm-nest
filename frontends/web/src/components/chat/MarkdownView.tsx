import ReactMarkdown, { type Components } from "react-markdown"
import remarkGfm from "remark-gfm"
import remarkMath from "remark-math"
import rehypeKatex from "rehype-katex"
import { CodeBlock } from "@/components/chat/CodeBlock"
import { lazy, Suspense } from "react"
const MermaidBlock = lazy(() => import("@/components/chat/MermaidBlock").then((m) => ({ default: m.MermaidBlock })))

const components: Components = {
  a: ({ children, ...props }) => (
    <a {...props} target="_blank" rel="noreferrer noopener">
      {children}
    </a>
  ),
  // 让 code 组件接管代码块渲染；pre 只透传
  pre: ({ children }) => <>{children}</>,
  code: ({ className, children }) => {
    const match = /language-(\w+)/.exec(className ?? "")
    const text = String(children)
    if (match) {
      if (match[1].toLowerCase() === "mermaid") {
        return <Suspense fallback={<pre>{text}</pre>}><MermaidBlock code={text} /></Suspense>
      }
      return <CodeBlock code={text} language={match[1]} />
    }
    return <code className={className}>{children}</code>
  },
}

/**
 * Markdown 渲染：GFM（表格/任务列表）+ KaTeX（$…$ / $$…$$）+ 代码高亮 + Mermaid 图表
 */
export function MarkdownView({ content }: { content: string }) {
  return (
    <div className="md">
      <ReactMarkdown
        remarkPlugins={[remarkGfm, remarkMath]}
        rehypePlugins={[rehypeKatex]}
        components={components}
      >
        {content}
      </ReactMarkdown>
    </div>
  )
}
