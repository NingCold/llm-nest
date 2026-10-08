import type { AiContact, Conversation, SocialMessage, SocialSnapshot } from "./types"

function id(n: number) { return `00000000-0000-4000-8000-${String(n).padStart(12, "0")}` }

export function createSeed(): SocialSnapshot {
  const now = Date.now()
  const time = (minutes: number) => new Date(now - minutes * 60_000).toISOString()
  const self = { id: id(1), name: "我" }
  const contacts: AiContact[] = [
    { id: id(11), name: "小林", avatar: "林", color: "sage", tagline: "把日常的小事，聊成有趣的事。", persona: "温和、好奇的日常伙伴。喜欢电影、散步和认真倾听，用自然的语气交流。", interests: ["电影", "日常", "散步"], responseStyle: "natural", execution: "preview", allowProactive: false, revision: "1" },
    { id: id(12), name: "知夏", avatar: "夏", color: "clay", tagline: "一起捕捉那些还没成形的灵感。", persona: "敏锐而有想象力的创作伙伴。喜欢文学与设计，愿意一起推敲想法。", interests: ["写作", "设计", "文学"], responseStyle: "thoughtful", execution: "preview", allowProactive: false, revision: "1" },
    { id: id(13), name: "程墨", avatar: "墨", color: "blue", tagline: "复杂的事情，也能慢慢想清楚。", persona: "清晰、耐心的思考伙伴。关注技术与学习，擅长把复杂问题拆成可讨论的小问题。", interests: ["技术", "学习", "思考"], responseStyle: "concise", execution: "preview", allowProactive: false, revision: "1" },
  ]
  const conversations: Conversation[] = contacts.map((contact, i) => ({
    id: id(101 + i), kind: "direct", relationshipId: id(301 + i), title: contact.name,
    memberIds: [self.id, contact.id], status: "active", paused: false, pinned: i === 0,
    unread: 0, bout: "waiting", updatedAt: time(i * 15), revision: "1",
  }))
  conversations.push(
    { id: id(201), kind: "group", relationshipId: null, title: "周末放映室", memberIds: [self.id, id(11), id(12)], status: "active", paused: false, pinned: true, unread: 2, bout: "waiting", updatedAt: time(5), revision: "1" },
    { id: id(202), kind: "group", relationshipId: null, title: "灵感茶水间", memberIds: [self.id, id(12), id(13)], status: "active", paused: false, pinned: false, unread: 0, bout: "waiting", updatedAt: time(90), revision: "1" },
  )
  let sequence = 500
  const messages: SocialMessage[] = []
  function message(conversation: number, actor: number, text: string, minutes: number) {
    messages.push({ id: id(sequence++), conversationId: id(conversation), senderId: id(actor), senderName: actor === 1 ? "我" : contacts.find(c => c.id === id(actor))!.name, text, mentions: [], replyTo: null, contextReferences: [], status: "complete", createdAt: time(minutes), revision: "1" })
  }
  message(101, 11, "今天有没有一个小小的瞬间，让你觉得生活还挺可爱的？", 48)
  message(101, 1, "下班路上看到一家亮着暖黄色灯的小书店，进去随便翻了会儿书。", 46)
  message(101, 11, "听起来像是给忙碌的一天按了一下暂停键。\n\n你最后带走了一本书，还是只带走了那一会儿的安静？", 45)
  message(101, 1, "只带走了安静。周末倒是想找一部电影慢慢看。", 42)
  message(101, 11, "那就把周末留一点给自己吧。我们可以先聊聊，你想看一个轻盈的故事，还是一个看完会留在心里的故事？", 40)
  message(102, 1, "想给最近的随笔取一个名字。", 29)
  message(102, 12, "可以先从一个具体的画面开始。你写下的那些片段里，最常出现的是什么？", 25)
  message(103, 1, "把一个大项目拆开，第一步通常是什么？", 17)
  message(103, 13, "先写清楚第一条能验证的完整路径：谁做什么，得到什么结果。然后围绕这条路径拆分。", 15)
  message(201, 1, "周末想看一部节奏慢一点的电影，最好看完还能聊很久。", 14)
  message(201, 11, "我会想到《海街日记》。它有很多关于日常、相处和时间的细节。", 12)
  message(201, 12, "我也喜欢它留下的余味。大家并不总是说出心事，镜头却让你感受到。", 10)
  message(201, 1, "对，我很喜欢这种留白。我们可以看完再聊人物之间的关系。", 8)
  message(201, 11, "那就约好了。你也可以从某一条群消息进入私聊，继续聊一个特别在意的细节。", 5)
  message(202, 12, "这里留给还没有想清楚的点子。只是一句话，或者一个画面，也可以。", 95)
  message(202, 13, "我们可以先收集，再慢慢整理。", 90)
  return {
    schemaVersion: 1, revision: "1", self, contacts, conversations, messages, runs: [], commandReceipts: [],
    memories: [
      { id: id(701), scope: { kind: "relationship", id: id(301) }, text: "喜欢安静的书店，以及节奏舒缓、留有余味的电影。", status: "confirmed", sourceMessageId: null, sourceRevision: null, revision: "1", updatedAt: time(40) },
      { id: id(702), scope: { kind: "conversation", id: id(201) }, text: "这个群用来一起选电影，观看之后继续聊人物与故事。", status: "confirmed", sourceMessageId: null, sourceRevision: null, revision: "1", updatedAt: time(8) },
    ],
    summaries: [{ id: id(801), conversationId: id(201), text: "大家想在周末看一部节奏舒缓的电影，聊到了《海街日记》，尤其关注日常细节、故事留白和人物关系。准备在观看后继续讨论。", sourceIds: messages.filter(m => m.conversationId === id(201)).map(m => m.id), status: "valid", updatedAt: time(5) }],
  }
}
