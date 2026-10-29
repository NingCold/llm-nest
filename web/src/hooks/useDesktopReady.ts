import { useEffect } from "react"
import { useUiStore } from "@/store/ui"

export function useDesktopReady(settled: boolean) {
  useEffect(() => {
    if (settled) useUiStore.getState().setDesktopReady()
  }, [settled])
}
