import { useEffect } from "react"
import { useConfigStore } from "@/store/config"
import { useSessionStore } from "@/store/session"

export function useInit() {
  const initConfig = useConfigStore((s) => s.init)
  const setSessions = useSessionStore((s) => s.setSessions)
  const setCurrentSession = useSessionStore((s) => s.setCurrentSession)
  const sessions = useSessionStore((s) => s.sessions)
  const config = useConfigStore((s) => s.config)
  const configLoading = useConfigStore((s) => s.loading)

  useEffect(() => {
    ;(async () => {
      try {
        await initConfig()
        const { getApi } = await import("@/api")
        const a = await getApi()
        const list = await a.listSessions()
        setSessions(list)
        if (list.length > 0) {
          setCurrentSession(list[0].id)
        }
      } catch (err) {
        console.error("[useInit] error:", err)
      }
    })()
  }, []) // eslint-disable-line react-hooks/exhaustive-deps

  return {
    ready: !configLoading || config !== null,
    hasSessions: sessions.length > 0,
  }
}