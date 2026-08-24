import type { LucideIcon } from "lucide-react"
import {
  Code2,
  FileText,
  Languages,
  Lightbulb,
  ListChecks,
  Sparkles,
} from "lucide-react"

export interface PromptTemplate {
  id: string
  title: string
  description: string
  icon: LucideIcon
  prompt: string
}

export const PROMPT_TEMPLATES: PromptTemplate[] = [
  {
    id: "translate",
    title: "翻译",
    description: "中英互译，保留语气",
    icon: Languages,
    prompt: "请帮我翻译下面这段文字为英文，保持自然语气：",
  },
  {
    id: "code",
    title: "写代码",
    description: "生成带注释的示例代码",
    icon: Code2,
    prompt: "请用 Rust 写一个示例程序，并解释关键点：",
  },
  {
    id: "summarize",
    title: "总结",
    description: "提炼要点与行动项",
    icon: ListChecks,
    prompt: "请总结以下内容的要点，并列出行动项：",
  },
  {
    id: "explain",
    title: "解释概念",
    description: "用通俗语言讲清楚",
    icon: Lightbulb,
    prompt: "请用通俗易懂的语言解释一下这个概念：",
  },
  {
    id: "polish",
    title: "润色",
    description: "改写得更通顺专业",
    icon: Sparkles,
    prompt: "请帮我润色以下文本，使其更通顺、更专业：",
  },
  {
    id: "brainstorm",
    title: "头脑风暴",
    description: "生成多个创意方向",
    icon: FileText,
    prompt: "针对以下主题，头脑风暴 5 个有创意的方向：",
  },
]
