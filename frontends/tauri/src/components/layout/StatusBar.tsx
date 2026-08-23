import { useConfigStore } from "@/store/config"

export function StatusBar() {
  const version = useConfigStore((s) => s.version)

  return (
    <div className="h-6 border-t px-4 flex items-center text-xs text-muted-foreground shrink-0">
      <span className="ml-auto">v{version}</span>
    </div>
  )
}