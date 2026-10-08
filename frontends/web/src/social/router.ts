export interface SocialRoute {
  page: "messages" | "contacts" | "settings"
  id: string | null
  details: boolean
  memories: boolean
}

export function parseSocialRoute(hash: string): SocialRoute {
  const parts = hash.replace(/^#\/?/, "").split("/")
  const page = parts[0] === "contacts" ? "contacts" : parts[0] === "settings" ? "settings" : "messages"
  return {
    page, id: page === "settings" || !parts[1] ? null : parts[1],
    details: page === "messages" && parts[2] === "details",
    memories: page === "contacts" ? parts[2] === "memories" : parts[3] === "memories",
  }
}

export function navigate(page: "messages" | "contacts", id?: string, details = false, memories = false) {
  location.hash = `/${page}${id ? `/${id}` : ""}${details ? "/details" : ""}${memories ? "/memories" : ""}`
}
