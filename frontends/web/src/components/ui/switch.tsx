import { cn } from "@/lib/utils"

interface SwitchProps {
  checked: boolean
  onCheckedChange: (v: boolean) => void
  disabled?: boolean
  size?: "sm" | "md"
  className?: string
  "aria-label"?: string
}

export function Switch({
  checked,
  onCheckedChange,
  disabled,
  size = "md",
  className,
  ...rest
}: SwitchProps) {
  const dims =
    size === "md" ? "h-6 w-10" : "h-5 w-8"
  const knob =
    size === "md" ? "h-4.5 w-4.5 translate-x-[18px]" : "h-3.5 w-3.5 translate-x-[14px]"
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      onClick={(e) => {
        e.stopPropagation()
        onCheckedChange(!checked)
      }}
      className={cn(
        "relative inline-flex shrink-0 cursor-pointer items-center rounded-full border border-transparent transition-colors duration-200 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2 focus-visible:ring-offset-background disabled:cursor-not-allowed disabled:opacity-40",
        checked ? "bg-zinc-900 dark:bg-zinc-100" : "bg-zinc-300 dark:bg-zinc-600",
        dims,
        className,
      )}
      {...rest}
    >
      <span
        className={cn(
          "pointer-events-none inline-block rounded-full bg-white shadow-sm ring-0 transition-transform duration-200 dark:bg-zinc-900",
          checked ? knob : "translate-x-1",
          size === "md" ? "h-4.5 w-4.5" : "h-3.5 w-3.5",
        )}
      />
    </button>
  )
}
