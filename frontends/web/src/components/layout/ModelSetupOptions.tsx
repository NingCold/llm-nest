import { Plus } from "lucide-react"
import { SelectGroup, SelectItem, SelectLabel } from "@/components/ui/select"

// A SelectItem keeps the setup action usable with arrows/Enter as well as the mouse.
export const MODEL_SETTINGS_ACTION = "__open_model_settings__"

export function ModelSetupOptions({ message, action = "添加模型" }: { message: string; action?: string }) {
  return <SelectGroup>
    <SelectLabel className="whitespace-normal px-3 py-3 text-sm font-normal leading-relaxed text-muted-foreground">{message}</SelectLabel>
    <SelectItem value={MODEL_SETTINGS_ACTION} className="py-2">
      <span className="flex items-center gap-2 font-medium"><Plus className="h-4 w-4" />{action}</span>
    </SelectItem>
  </SelectGroup>
}
