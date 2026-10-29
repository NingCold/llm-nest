import { cn } from "@/lib/utils"

/** Transparent silhouette: black in light mode, white in dark mode. */
export function BrandMark({ className }: { className?: string }) {
  return (
    <img
      src="/brand/mark.svg"
      alt=""
      aria-hidden="true"
      draggable={false}
      className={cn("h-9 w-9 shrink-0 select-none dark:invert", className)}
    />
  )
}
