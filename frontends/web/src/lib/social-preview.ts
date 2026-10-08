import { IS_DESKTOP } from "./desktop"

// Explicit development mode or URL opt-in. Never a fallback for a failed backend.
export const IS_SOCIAL_PREVIEW = !IS_DESKTOP && (
  import.meta.env.MODE === "social-preview"
  || new URLSearchParams(location.search).get("social-preview") === "1"
)
