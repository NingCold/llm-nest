export interface Account {
  id: string;
  name: string;
  bio: string;
  kind: string;
}
export interface Attachment {
  index: number;
  kind: string;
  name?: string;
  mime: string;
  byteLength: number;
}
export interface Message {
  id: string;
  conversationId: string;
  senderId: string;
  text: string;
  revision: string;
  position: string;
  status: string;
  createdAt: string;
  attachments: Attachment[];
  aiResponse?: { taskId: string; status: string; error?: string | null } | null;
}
export interface Conversation {
  id: string;
  peer: Account;
  lastMessage: Message | null;
  unreadCount: number;
  manualUnread: boolean;
  pinned: boolean;
  listRevision: string;
  latestPosition: string;
}
export interface Snapshot {
  self: Account;
  contacts: Account[];
  conversations: Conversation[];
  cursor: string;
}
export interface Page {
  messages: Message[];
  hasMore: boolean;
  latestPosition: string;
}
