import type { Contact } from "./live-types"

export function contactAvatar(contact?: Contact) {
  return contact ? { avatar: contact.name.slice(0, 1), color: (["sage", "clay", "blue", "lavender"] as const)[parseInt(contact.id[0], 16) % 4] } : undefined
}
