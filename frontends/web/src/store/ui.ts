import { create } from "zustand"

export type Theme = "dark" | "light"
export type ApiMode = "tauri" | "http" | "demo" | null
export type ReasoningEffort = "off" | "low" | "medium" | "high" | "max"
export type SettingsSection = "models" | "generation" | "appearance" | "about"

function routeFromHash() {
  const [, page, section] = location.hash.replace(/^#/, "").split("/")
  return {
    page: page === "settings" ? "settings" as const : "chat" as const,
    settingsSection: (["models", "generation", "appearance", "about"].includes(section)
      ? section : "models") as SettingsSection,
  }
}

/** 把选中的思考强度钳制到模型实际支持的级别内（模型切换后 effort 可能失效） */
export function clampEffort(effort: string, levels?: string[]): string {
  if (!levels || levels.length === 0) return "off"
  if (levels.includes(effort)) return effort
  return levels.find((l) => l !== "off") ?? "off"
}

const THEME_KEY = "llm-nest-theme"

function initialTheme(): Theme {
  try {
    // 调试/截图用：?theme=light
    const q = new URLSearchParams(location.search).get("theme")
    if (q === "light" || q === "dark") return q
    const saved = localStorage.getItem(THEME_KEY)
    if (saved === "light" || saved === "dark") return saved
  } catch {
    /* ignore */
  }
  return "dark"
}

interface UiStore {
  page: "chat" | "settings"
  settingsSection: SettingsSection
  showSettings: (section?: SettingsSection) => void
  showChat: () => void
  syncRoute: () => void
  theme: Theme
  sidebarCollapsed: boolean
  /** 当前选中的思考强度（off/low/medium/high/max） */
  reasoningEffort: string
  webSearchEnabled: boolean
  searchQuery: string
  /** 当前适配器模式：桌面壳 / 真实 HTTP 后端 / 演示（localStorage） */
  apiMode: ApiMode

  toggleTheme: () => void
  setTheme: (t: Theme) => void
  toggleSidebar: () => void
  setReasoningEffort: (e: string) => void
  toggleWebSearch: () => void
  setSearchQuery: (q: string) => void
  setApiMode: (m: ApiMode) => void
}

export const useUiStore = create<UiStore>((set, get) => ({
  ...routeFromHash(),
  showSettings: (section = get().settingsSection) => {
    set({ page: "settings", settingsSection: section })
    location.hash = `/settings/${section}`
  },
  showChat: () => {
    set({ page: "chat" })
    location.hash = "/chat"
  },
  syncRoute: () => {
    const route = routeFromHash()
    // Going back to chat should remember the last settings category.
    set(route.page === "settings" ? route : { page: "chat" })
  },
  theme: initialTheme(),
  sidebarCollapsed:
    new URLSearchParams(location.search).get("sidebar") === "collapsed",
  reasoningEffort: "high",
  webSearchEnabled: false,
  searchQuery: "",
  apiMode: null,

  toggleTheme: () => {
    const next: Theme = get().theme === "dark" ? "light" : "dark"
    set({ theme: next })
    try {
      localStorage.setItem(THEME_KEY, next)
    } catch {
      /* ignore */
    }
  },

  setTheme: (t) => {
    set({ theme: t })
    try {
      localStorage.setItem(THEME_KEY, t)
    } catch {
      /* ignore */
    }
  },

  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  setReasoningEffort: (e) => set({ reasoningEffort: e }),
  toggleWebSearch: () => set((s) => ({ webSearchEnabled: !s.webSearchEnabled })),
  setSearchQuery: (q) => set({ searchQuery: q }),
  setApiMode: (m) => set({ apiMode: m }),
}))
