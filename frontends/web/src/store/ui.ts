import { create } from "zustand"
import { IS_DESKTOP } from "@/lib/desktop"

export type Theme = "dark" | "light"
export type ApiMode = "tauri" | "http" | null
export type SettingsSection = "models" | "appearance" | "config" | "about"

function routeFromHash() {
  const [, page, section] = location.hash.replace(/^#/, "").split("/")
  return {
    page: page === "settings" ? "settings" as const : "messages" as const,
    settingsSection: (["models", "appearance", "config", "about"].includes(section)
      ? section : "models") as SettingsSection,
  }
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
  page: "messages" | "settings"
  settingsSection: SettingsSection
  showSettings: (section?: SettingsSection) => void
  showMessages: () => void
  syncRoute: () => void
  theme: Theme
  desktopReady: boolean
  setDesktopReady: () => void
  apiMode: ApiMode

  toggleTheme: () => void
  setTheme: (t: Theme) => void
  setApiMode: (m: ApiMode) => void
}

export const useUiStore = create<UiStore>((set, get) => ({
  ...routeFromHash(),
  showSettings: (section = get().settingsSection) => {
    set({ page: "settings", settingsSection: section })
    location.hash = `/settings/${section}`
  },
  showMessages: () => {
    set({ page: "messages" })
    location.hash = "/messages"
  },
  syncRoute: () => {
    const route = routeFromHash()
    // Going back to chat should remember the last settings category.
    set(route.page === "settings" ? route : { page: "messages" })
  },
  theme: initialTheme(),
  desktopReady: false,
  setDesktopReady: () => set({ desktopReady: true }),
  apiMode: IS_DESKTOP ? "tauri" : "http",

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

  setApiMode: (m) => set({ apiMode: m }),
}))
