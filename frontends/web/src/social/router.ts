export interface SocialRoute {
  page: "messages" | "contacts" | "settings" | "account" | "ai-management" | "account-view"
  id: string | null
  details: boolean
  memories: boolean
}

export function parseSocialRoute(hash: string): SocialRoute {
  const parts = hash.replace(/^#\/?/, "").split("/")
  const page: SocialRoute["page"] = ["contacts", "settings", "account", "ai-management", "account-view"].includes(parts[0]) ? parts[0] as SocialRoute["page"] : "messages"
  return {
    page, id: page === "settings" || !parts[1] ? null : parts[1],
    details: page === "messages" && parts[2] === "details",
    memories: page === "contacts" ? parts[2] === "memories" : parts[3] === "memories",
  }
}

export function navigate(page: Exclude<SocialRoute["page"], "settings">, id?: string, details = false, memories = false) {
  location.hash = `/${page}${id ? `/${id}` : ""}${details ? "/details" : ""}${memories ? "/memories" : ""}`
}
