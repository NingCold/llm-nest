import { useCallback, useEffect, useState } from "react"
import { useConfigStore } from "@/store/config"

let startup: Promise<void> | null = null
function initialize() {
  if (!startup) startup = useConfigStore.getState().init().finally(() => { startup = null })
  return startup
}
export function useInit() {
  const [ready, setReady] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [attempt, setAttempt] = useState(0)
  const retry = useCallback(() => setAttempt(n => n + 1), [])
  useEffect(() => {
    let active = true
    setReady(false); setError(null)
    initialize().then(() => { if (active) setReady(true) })
      .catch(err => { if (active) setError(String(err)) })
    return () => { active = false }
  }, [attempt])
  return { ready, error, retry }
}
