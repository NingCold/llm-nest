import { Cpu } from "lucide-react"
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select"
import { useConfigStore } from "@/store/config"
import { cn } from "@/lib/utils"

function findModel(providerId: string, modelId: string) {
  const providers = useConfigStore.getState().providers
  const p = providers.find((x) => x.id === providerId)
  const m = p?.models.find((x) => x.id === modelId)
  return { provider: p, model: m }
}

export function ModelPicker({ className }: { className?: string }) {
  const config = useConfigStore((s) => s.config)
  const providers = useConfigStore((s) => s.providers)
  const setModel = useConfigStore((s) => s.setModel)

  const current = config?.currentModel
  const { provider: currentProvider, model: currentModel } = findModel(
    current?.provider ?? "",
    current?.model ?? "",
  )

  const value = current ? `${current.provider}::${current.model}` : ""

  return (
    <Select
      value={value}
      onValueChange={(v) => {
        const [provider, model] = v.split("::")
        if (provider && model) setModel({ provider, model })
      }}
    >
      <SelectTrigger
        className={cn(
          "h-8 w-auto gap-2 border border-transparent bg-transparent px-2.5 text-sm shadow-none hover:bg-accent hover:text-accent-foreground focus:ring-0 focus:ring-offset-0 data-[placeholder]:text-muted-foreground",
          className,
        )}
      >
        <Cpu className="h-4 w-4 shrink-0 text-muted-foreground" />
        {currentProvider && currentModel ? (
          <span className="flex items-center gap-1.5">
            <span className="text-muted-foreground">{currentProvider.displayName}</span>
            <span className="text-muted-foreground/50">/</span>
            <span className="font-medium">{currentModel.displayName}</span>
          </span>
        ) : (
          <SelectValue placeholder="选择模型" />
        )}
      </SelectTrigger>
      <SelectContent className="max-h-[420px] w-72" position="popper">
        {providers.map((p) => (
          <SelectGroup key={p.id}>
            <SelectLabel className="text-xs font-semibold uppercase tracking-wide text-muted-foreground">
              {p.displayName}
            </SelectLabel>
            {p.models.map((m) => (
              <SelectItem
                key={`${p.id}::${m.id}`}
                value={`${p.id}::${m.id}`}
                className="py-2 pr-6"
              >
                <span className="flex flex-col">
                  <span className="font-medium leading-tight">{m.displayName}</span>
                  <span className="font-mono text-[11px] leading-tight text-muted-foreground">
                    {m.id}
                  </span>
                </span>
              </SelectItem>
            ))}
          </SelectGroup>
        ))}
      </SelectContent>
    </Select>
  )
}
