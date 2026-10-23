import { invoke } from "@tauri-apps/api/core";
import type { Account, Message, Page } from "./types";
export interface LocalView {
  ai: Account;
  human: Account;
  conversation: { id: string };
  messages: { message: Omit<Message, "conversationId" | "status" | "aiResponses" | "contextReference">; status: string; local: boolean; synced: boolean }[];
  hasMore: boolean;
  syncState: string;
  error: string | null;
  running: boolean;
}
export const localView = (conversationId: string) => invoke<LocalView>("local_chat_view", { conversationId });
export const prepareLocal = (conversationId: string, aiId: string) => invoke<LocalView>("local_chat_prepare", { conversationId, aiId });
export const sendLocal = (conversationId: string, requestId: string, text: string) => invoke<LocalView>("local_chat_send", { conversationId, requestId, text });
export const syncLocal = (conversationId: string) => invoke<LocalView>("local_chat_sync", { conversationId });
export const stopLocal = (conversationId: string) => invoke<LocalView>("local_chat_stop", { conversationId });
export function localPage(view: LocalView): Page {
  return { hasMore: false, latestPosition: "0", messages: view.messages.map(item => ({
    ...item.message, conversationId: view.conversation.id, status: item.status,
    aiResponses: [], contextReference: null,
    ...(item.local ? { local: { synced: item.synced } } : {}),
  })) };
}
