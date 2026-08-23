import { useSessionStore } from "@/store/session"
import { useConfigStore } from "@/store/config"
import { Button } from "@/components/ui/button"

export function Toolbar() {
  const currentSessionId = useSessionStore((s) => s.currentSessionId)
  const sessions = useSessionStore((s) => s.sessions)
  const createSession = useSessionStore((s) => s.createSession)
  const deleteSession = useSessionStore((s) => s.deleteSession)
  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)
  const setModel = useConfigStore((s) => s.setModel)

  const currentSession = sessions.find((s) => s.id === currentSessionId)

  const handleDelete = async () => {
    if (!currentSessionId) return
    await deleteSession(currentSessionId)
  }

  const allModels = providers.flatMap((p) =>
    p.models.map((m) => ({
      key: `${p.id}/${m.id}`,
      label: `${p.displayName} / ${m.displayName}`,
      provider: p.id,
      model: m.id,
    })),
  )

  return (
    <div className="h-12 border-b flex items-center px-4 gap-3 shrink-0 bg-white">
      <div className="flex-1 truncate font-medium text-sm text-muted-foreground">
        {currentSession?.title || "Untitled"}
      </div>
      <div className="flex items-center gap-2">
        {config && allModels.length > 0 && (
          <select
            value={`${config?.currentModel?.provider ?? ""}/${config?.currentModel?.model ?? ""}`}
            onChange={(e) => {
              const [provider, model] = e.target.value.split("/")
              setModel({ provider, model })
            }}
            className="h-8 text-xs border border-input rounded-md px-2 bg-white"
          >
            {allModels.map((m) => (
              <option key={m.key} value={m.key}>
                {m.label}
              </option>
            ))}
          </select>
        )}
        {currentSession && (
          <>
            <Button onClick={() => createSession()}>New Session</Button>
            <Button variant="destructive" onClick={handleDelete}>Delete</Button>
          </>
        )}
      </div>
    </div>
  )
}