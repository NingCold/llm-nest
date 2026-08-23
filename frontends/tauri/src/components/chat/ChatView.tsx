import { useChat } from "@/hooks/useChat"
import { MessageList } from "./MessageList"
import { InputBox } from "./InputBox"

export function ChatView() {
  const { send, cancel } = useChat()

  return (
    <div className="flex-1 flex flex-col h-full">
      <MessageList />
      <InputBox onSend={send} onCancel={cancel} />
    </div>
  )
}