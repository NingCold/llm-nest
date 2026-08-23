import type { ChatApi, GuiEvent } from "./types"

export const webApi: ChatApi = {
  async init() {
    throw new Error("Web API not implemented yet")
  },
  async chat() {
    throw new Error("Web API not implemented yet")
  },
  async cancelChat() {
    throw new Error("Web API not implemented yet")
  },
  async listSessions() {
    throw new Error("Web API not implemented yet")
  },
  async newSession() {
    throw new Error("Web API not implemented yet")
  },
  async deleteSession() {
    throw new Error("Web API not implemented yet")
  },
  async renameSession() {
    throw new Error("Web API not implemented yet")
  },
  async setConfig() {
    throw new Error("Web API not implemented yet")
  },
}