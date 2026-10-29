import { create } from "zustand"

export type Theme = "dark" | "light"
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
  theme: Theme
  desktopReady: boolean
  setDesktopReady: () => void

  toggleTheme: () => void
  setTheme: (t: Theme) => void
}

export const useUiStore = create<UiStore>((set, get) => ({
  theme: initialTheme(),
  desktopReady: false,
  setDesktopReady: () => set({ desktopReady: true }),

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

}))
