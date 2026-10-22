import { IS_DESKTOP } from "@/lib/desktop"
import * as http from "./http"
import * as tauri from "./tauri"
import type { SocialClient, Subscribe } from "./transport"

export { SocialApiError } from "./transport"
export const socialClient: SocialClient = IS_DESKTOP ? tauri.socialClient : http.socialClient
export const subscribe: Subscribe = IS_DESKTOP ? tauri.subscribe : http.subscribe
