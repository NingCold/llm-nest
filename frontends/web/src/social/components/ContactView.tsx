import { ArrowLeft, Brain, MessageCircle, Pencil, Users } from "lucide-react"
import { useSocialStore } from "../store"
import { navigate } from "../router"
import type { AiContact } from "../types"
import type { Editor } from "./EditorDialogs"
import { Avatar, IconButton } from "./common"
import { MemoryPanel } from "./MemoryPanel"

export function ContactView({ contact, memories, edit }: { contact: AiContact; memories: boolean; edit(editor: Editor): void }) {
  const snapshot = useSocialStore(s => s.snapshot!)
  const action = useSocialStore(s => s.action)
  const direct = snapshot.conversations.find(c => c.kind === "direct" && c.memberIds.includes(contact.id))
  const groups = snapshot.conversations.filter(c => c.kind === "group" && c.memberIds.includes(contact.id))
  return <section className="contact-view">
    <header className="contact-header"><IconButton label="返回通讯录" className="mobile-back" onClick={() => navigate("contacts")}><ArrowLeft /></IconButton><span>联系人资料</span><button className="text-button" onClick={() => edit({ kind: "contact", contact })}><Pencil />编辑资料</button></header>
    <div className="contact-scroll"><div className="profile-cover"><span className={`profile-orbit orbit-${contact.color}`} /><span className="profile-orbit orbit-small" /></div>
      <div className="profile-content"><div className="profile-intro"><Avatar contact={contact} size="large" /><div><h1>{contact.name}<span className="ai-badge">AI 联系人</span></h1><p>{contact.tagline || "等待你们的故事，从一次聊天开始。"}</p></div><button className="social-button primary" onClick={() => void action(api => api.getDirectConversation(contact.id)).then(conversation => { if (conversation) navigate("messages", conversation.id) })}><MessageCircle />{direct?.status === "archived" ? "查看归档私聊" : "发消息"}</button></div>
        <div className="profile-tabs"><button className={!memories ? "selected" : ""} onClick={() => navigate("contacts", contact.id)}>资料</button><button className={memories ? "selected" : ""} onClick={() => navigate("contacts", contact.id, false, true)}><Brain />意识与记忆</button></div>
        {memories && direct ? <MemoryPanel conversation={direct} edit={edit} /> : <div className="profile-sections">
          <section><h2>关于 {contact.name}</h2><p className="persona-text">{contact.persona}</p><div className="interest-tags">{contact.interests.map(tag => <span key={tag}>{tag}</span>)}</div></section>
          <section><h2>交流方式</h2><dl className="profile-properties"><div><dt>交流风格</dt><dd>{{ natural: "自然交流", concise: "简洁清晰", thoughtful: "细腻展开" }[contact.responseStyle]}</dd></div><div><dt>回复配置</dt><dd><span className={`status-tag ${contact.execution === "unconfigured" ? "warning" : ""}`}>{contact.execution === "preview" ? "模拟回复" : "尚未配置"}</span></dd></div><div><dt>主动联系</dt><dd>首版预览暂未启用</dd></div></dl><p className="section-description">身份与执行配置相互独立，更换模型后仍是同一位联系人。</p></section>
          <section><h2>共同群聊 <span>{groups.length}</span></h2>{groups.length ? <div className="shared-groups">{groups.map(group => <button key={group.id} onClick={() => navigate("messages", group.id)}><Avatar group size="small" /><span><strong>{group.title}</strong><small>{group.memberIds.length} 位成员{group.status === "archived" && " · 已归档"}</small></span><Users /></button>)}</div> : <p className="section-description">你们还没有共同群聊，可以从消息页创建。</p>}</section>
        </div>}
      </div>
    </div>
  </section>
}
