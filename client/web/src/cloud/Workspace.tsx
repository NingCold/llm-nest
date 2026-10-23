import { useEffect, useRef, useState } from "react";
import {
  ArrowLeft,
  ArrowRight,
  ArrowUp,
  CheckCheck,
  Copy,
  Download,
  ImagePlus,
  Info,
  LogOut,
  Mail,
  MessageCircle,
  MoreHorizontal,
  Paperclip,
  Pencil,
  Pin,
  Plus,
  Search,
  Settings,
  X,
} from "lucide-react";
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { MarkdownView } from "@/components/chat/MarkdownView";
import { SocialSidebar } from "../social/components/SocialSidebar";
import {
  Avatar,
  EmptyState,
  IconButton,
  shortTime,
} from "../social/components/common";
import { useUiStore } from "@/store/ui";
import type { CloudDraftRecord as DraftRecord, DraftReference } from "./types";
import { ConsciousnessPanel, RememberMessage } from "./Consciousness";
import { ParticipationPanel } from "./Participation";
import { ContinuePrivately, ReferenceCard } from "./References";
import { fileSize } from "../social/files";
import { download, request } from "./api";
import { CreateGroup, GroupDetails, ContactBlock } from "./Groups";
import { AiManagement } from "./AiManagement";
import type {
  Account,
  Conversation,
  Message,
  Page,
  Snapshot,
  Attachment,
} from "./types";
import "../social/social.css";
import "./cloud.css";
import type { LocalView } from "./local-api";

interface Props {
  localChat?: LocalView;
  localSelected?: boolean;
  prepareLocal?: () => Promise<void>;
  syncLocal?: () => Promise<void>;
  stopLocal?: () => Promise<void>;
  closeLocal?: () => Promise<void>;
  me: Account;
  snapshot: Snapshot | null;
  active: string;
  location: { conversationId: string; messageId: string } | null;
  tab: string;
  page: Page | null;
  query: string;
  results: Message[];
  searchMore: boolean;
  record: DraftRecord;
  draftReady: boolean;
  busy: boolean;
  error: string;
  notice: string;
  preview?: string;
  editing: { id: string; text: string } | null;
  navigate(tab: string): void;
  selectConversation(id: string): void;
  queryChange(value: string): void;
  search(older: boolean, scope: string): Promise<unknown>;
  locate(message: Pick<Message, "id">): Promise<unknown>;
  act(fn: () => Promise<unknown>): Promise<void>;
  refresh(): Promise<unknown>;
  history(older?: boolean): Promise<unknown>;
  openDirect(account: Account): Promise<unknown>;
  continuePrivately(ai: Account, reference: DraftReference): Promise<void>;
  list(conversation: Conversation, patch: object): Promise<unknown>;
  mutate(message: Message, withdraw: boolean, text?: string): Promise<unknown>;
  edit(value: { id: string; text: string } | null): void;
  text(value: string): void;
  attach(files: File[], images: boolean): void;
  save(record: DraftRecord): Promise<void>;
  send(): Promise<void>;
  recoverDraft(): Promise<void>;
  onBottom(bottom: boolean): void;
  onProfile(account: Account): void;
  logout(): Promise<void>;
  endSession(): void;
  onPreview(url?: string): void;
  dismissError(): void;
}
const avatar = (a?: Pick<Account, "name"> | null) =>
  a ? { avatar: a.name.slice(0, 1), color: "sage" as const } : undefined;
const unread = (c: Conversation) => c.unreadCount > 0 || c.manualUnread;
function responseStatus(response: Message["aiResponses"][number]) {
  const errors: Record<string, string> = {
    membership_changed: "群成员已变化，这次回复已停止",
    account_disabled: "AI 账号已停用",
    direct_blocked: "双方私聊已屏蔽",
    image_input_unsupported: "这个 AI 使用的模型暂不支持图片",
    node_budget_exhausted: "节点今天的调用额度已用完",
    preparation_failed: "准备回复失败，请管理员检查本机的模型配置",
    source_changed: "相关消息已修改或撤回，这次回复已停止",
    node_changed: "运行节点已更换，这次回复已停止",
    node_revoked: "节点连接已撤销，这次回复已停止",
    empty_response: "模型没有返回正文，请发送新消息再试",
    generation_timeout: "模型回复超时",
    generation_interrupted: "回复已中断",
    node_restart_uncertain: "节点已重启，上次回复结果尚未确认",
    node_restart_before_start: "节点已重启，上次未开始的回复已停止",
    provider_authentication_failed: "模型服务认证失败，请管理员检查本机密钥",
    provider_rate_limited: "模型服务暂时繁忙，请稍后再试",
    provider_connection_failed: "暂时无法连接模型服务",
    provider_timeout: "连接模型服务超时",
    provider_protocol_failed: "模型服务返回的内容无法读取",
    provider_configuration_failed: "模型配置需要管理员检查",
    provider_api_failed: "模型服务未能完成这次回复",
    node_connection_failed: "节点连接中断，回复已停止",
    node_storage_failed: "节点保存失败，请管理员检查本机存储",
  };
  const states: Record<string, string> = {
    completed: "已回复", queued: "等待 AI 回复", running: "AI 正在回复", cancelled: "已停止回复",
    expired: "等待已超时，请发送新消息", uncertain: "回复被中断，尚未确认结果",
    failed: "这次未能回复，请管理员检查节点", interrupted: "回复已中断",
  };
  return (response.error && errors[response.error]) || states[response.status] || `AI 回复状态：${response.status}`;
}
const attachmentPath = (m: Message, a: Attachment) =>
  `/chat/messages/${m.id}/attachments/${a.index}?revision=${m.revision}`;
function InlineImage({
  message,
  attachment,
  act,
  onPreview,
}: {
  message: Message;
  attachment: Attachment;
  act: Props["act"];
  onPreview(url: string | undefined, trigger: HTMLElement): void;
}) {
  const [url, setUrl] = useState<string>(),
    [error, setError] = useState("");
  useEffect(() => {
    let active = true,
      objectUrl: string | undefined;
    setUrl(undefined);
    setError("");
    download(
      attachmentPath(message, attachment) + "&inline=true",
      "图片",
      true,
    ).then(
      (value) => {
        objectUrl = value;
        if (active) setUrl(value);
        else if (value) URL.revokeObjectURL(value);
      },
      (error) => {
        if (active) setError(String(error));
      },
    );
    return () => {
      active = false;
      if (objectUrl) URL.revokeObjectURL(objectUrl);
    };
  }, [message.id, message.revision, attachment.index]);
  return (
    <div className="cloud-inline-image">
      {url ? (
        <button
          aria-label="查看原图"
          onClick={(event) => {
            const trigger = event.currentTarget;
            void act(async () =>
              onPreview(
                await download(
                  attachmentPath(message, attachment) + "&inline=true",
                  "图片",
                  true,
                ),
                trigger,
              ),
            );
          }}
        >
          <img src={url} alt={attachment.name || "聊天图片"} />
        </button>
      ) : (
        <span>{error || "图片读取中…"}</span>
      )}
      <button
        className="text-button"
        onClick={() =>
          void act(async () => {
            await download(
              attachmentPath(message, attachment),
              `image-${message.id}-${attachment.index}`,
            );
          })
        }
      >
        <Download />
        保存原图
      </button>
    </div>
  );
}
export function Workspace(p: Props) {
  const [contact, setContact] = useState<Account | null>(null),
    [filter, setFilter] = useState("listed"),
    [directory, setDirectory] = useState<Account[]>([]),
    [directoryBusy, setDirectoryBusy] = useState(false),
    [details, setDetails] = useState(false),
    [creatingGroup, setCreatingGroup] = useState(false),
    [managedAiIds, setManagedAiIds] = useState<string[] | null>(null),
    [managementError, setManagementError] = useState(""),
    [unreadOnly, setUnreadOnly] = useState(false),
    [scope, setScope] = useState("all"),
    [searchBusy, setSearchBusy] = useState(false),
    [searched, setSearched] = useState(false),
    [settings, setSettings] = useState("appearance"),
    [accountSection, setAccountSection] = useState("profile"),
    [profileEditing, setProfileEditing] = useState(false),
    [sessions, setSessions] = useState<
      { id: string; createdAt: string; expiresAt: string; current: boolean }[]
    >([]),
    [dragging, setDragging] = useState(false);
  const imageInput = useRef<HTMLInputElement>(null),
    fileInput = useRef<HTMLInputElement>(null),
    timeline = useRef<HTMLDivElement>(null),
    stick = useRef(true),
    previewTrigger = useRef<HTMLElement | null>(null);
  const showPreview = (url: string | undefined, trigger: HTMLElement) => {
    previewTrigger.current = trigger;
    p.onPreview(url);
  };
  const replyLabel = (id: string) => {
    const source = p.page?.messages.find(message => message.id === id);
    return source ? source.status === "withdrawn" ? "已撤回的消息" : `${source.senderName}：${source.text.slice(0, 80) || "附件消息"}` : "查看回复的消息";
  };
  useEffect(() => {
    let current = true;
    setManagedAiIds(null); setManagementError("");
    request<{ account: Account }[]>("/ai").then(items => { if (current) setManagedAiIds(items.map(item => item.account.id)); }, error => { if (current) setManagementError(error instanceof Error ? error.message : String(error)); });
    return () => { current = false; };
  }, [p.me.id]);
  const located = p.location?.conversationId === p.active;
  const theme = useUiStore((s) => s.theme),
    setTheme = useUiStore((s) => s.setTheme);
  const conversation = p.snapshot?.conversations.find((c) => c.id === p.active),
    listed = p.snapshot?.contacts || [];
  const selectedContact =
    contact &&
    (listed.find((a) => a.id === contact.id) ||
      directory.find((a) => a.id === contact.id) ||
      contact);
  const contacts = (filter === "listed" ? listed : directory).filter(
    (a) =>
      a.id !== p.me.id &&
      (filter !== "listed" ||
        `${a.name} ${a.bio} ${a.id}`
          .toLocaleLowerCase()
          .includes(p.query.toLocaleLowerCase())),
  );
  const conversations = [...(p.snapshot?.conversations || [])]
    .sort(
      (a, b) =>
        Number(b.pinned) - Number(a.pinned) ||
        (BigInt(b.lastMessage?.position || 0) >
        BigInt(a.lastMessage?.position || 0)
          ? 1
          : -1),
    )
    .filter(
      (c) =>
        (!unreadOnly || unread(c)) &&
        (!p.query ||
          c.name
            .toLocaleLowerCase()
            .includes(p.query.toLocaleLowerCase())),
    );
  useEffect(() => {
    setDetails(false);
    stick.current = !located;
  }, [p.active, p.location?.messageId]);
  useEffect(() => {
    if (stick.current && timeline.current)
      timeline.current.scrollTop = timeline.current.scrollHeight;
  }, [p.page?.messages, p.tab]);
  useEffect(() => {
    if (p.tab !== "contacts" || filter !== "directory") return;
    let active = true;
    setDirectoryBusy(true);
    const timer = setTimeout(
      () =>
        void p.act(async () => {
          try {
            const found = await request<Account[]>(
              `/chat/accounts?query=${encodeURIComponent(p.query)}`,
            );
            if (active) setDirectory(found);
          } finally {
            if (active) setDirectoryBusy(false);
          }
        }),
      250,
    );
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [p.tab, filter, p.query]);
  useEffect(() => {
    if (p.tab !== "messages" || !p.query.trim()) {
      setSearched(false);
      setSearchBusy(false);
      return;
    }
    let active = true;
    setSearched(false);
    setSearchBusy(true);
    const timer = setTimeout(
      () =>
        void p.act(async () => {
          try {
            await p.search(false, scope);
            if (active) setSearched(true);
          } finally {
            if (active) setSearchBusy(false);
          }
        }),
      250,
    );
    return () => {
      active = false;
      clearTimeout(timer);
    };
  }, [p.tab, p.query, scope, p.active]);
  const navigate = (tab: string) => {
    p.navigate(tab);
    setProfileEditing(false);
  };
  const changeList = (c: Conversation, patch: object) => {
    if ("manualUnread" in patch) {
      stick.current = false;
      p.onBottom(false);
    }
    void p.act(() => p.list(c, patch));
  };
  const openContact = (a: Account) => {
    setContact(a);
    navigate("contacts");
  };
  const locate = (m: Message) => {
    stick.current = false;
    p.onBottom(false);
    void p.act(() => p.locate(m));
  };
  const latest = () => {
    stick.current = true;
    p.onBottom(true);
    void p.act(() => p.history());
  };
  const updateContact = (a: Account) =>
    void p.act(async () => {
      await request("/chat/contacts", "POST", {
        accountId: a.id,
        listed: !listed.some((c) => c.id === a.id),
      });
      await p.refresh();
    });
  const add = (files: File[], generic = false) => {
    if (!files.length) return;
    p.attach(
      files,
      !generic && files.every((f) => f.type.startsWith("image/")),
    );
  };
  const profile = (account: Account, own: boolean) => (
    <section className="contact-view account-profile">
      <header
        className="contact-header"
        data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}
      >
        <IconButton
          label="返回"
          className="mobile-back"
          onClick={() => {
            setContact(null);
            navigate("contacts");
          }}
        >
          <ArrowLeft />
        </IconButton>
        <span>{own ? "我的账号" : "账号资料"}</span>
        {own && (
          <button
            className="text-button"
            onClick={() => setProfileEditing(!profileEditing)}
          >
            <Pencil />
            编辑资料
          </button>
        )}
      </header>
      <div className="contact-scroll">
        <div className="profile-cover" />
        <div className="profile-content">
          <div className="profile-intro">
            <Avatar contact={avatar(account)} size="large" />
            <div>
              <h1>{account.name}</h1>
              <p>{account.kind === "ai" ? "AI" : "真人"} · 账号正常</p>
            </div>
            {!own && (
              <button
                className="social-button primary"
                onClick={() => void p.act(() => p.openDirect(account))}
              >
                <MessageCircle />
                发消息
              </button>
            )}
          </div>
          <div className="profile-sections">
            {own && profileEditing ? (
              <section>
                <h2>编辑公开资料</h2>
                <form
                  className="cloud-form"
                  onSubmit={(e) => {
                    e.preventDefault();
                    const form = new FormData(e.currentTarget);
                    void p.act(async () => {
                      p.onProfile(
                        await request<Account>("/me", "PATCH", {
                          name: form.get("name"),
                          bio: form.get("bio"),
                        }),
                      );
                      setProfileEditing(false);
                      await p.refresh();
                    });
                  }}
                >
                  <label>
                    名字
                    <input
                      name="name"
                      defaultValue={account.name}
                      required
                      maxLength={80}
                    />
                  </label>
                  <label>
                    公开简介
                    <textarea
                      name="bio"
                      defaultValue={account.bio}
                      maxLength={2000}
                    />
                  </label>
                  <div className="cloud-actions">
                    <button className="social-button primary">保存</button>
                    <button
                      type="button"
                      className="social-button secondary"
                      onClick={() => setProfileEditing(false)}
                    >
                      取消
                    </button>
                  </div>
                </form>
              </section>
            ) : (
              <section>
                <h2>公开简介</h2>
                <p className="persona-text">
                  {account.bio || "还没有填写简介"}
                </p>
              </section>
            )}
            <section>
              <h2>账号 ID</h2>
              <p className="account-id">{account.id}</p>
              <button
                className="text-button"
                onClick={() =>
                  void p.act(async () => {
                    await navigator.clipboard.writeText(account.id);
                  })
                }
              >
                <Copy />
                复制账号 ID
              </button>
            </section>
            {!own ? (
              <section>
                {account.kind === "ai" && (() => {
                  const direct = p.snapshot?.conversations.find(c => c.kind === "direct" && c.peer?.id === account.id);
                  return direct ? <ConsciousnessPanel operatorId={p.me.id} key={`${p.me.id}/${direct.id}`} conversation={direct} onlyAi={account.id} cursor={p.snapshot!.cursor} summaryAiIds={managedAiIds} summaryError={managementError} /> : <p>先打开与此 AI 的唯一私聊，再管理你们之间的记忆与私有摘要。</p>;
                })()}
                <ContactBlock key={account.id} account={account} busy={p.busy} act={p.act} refresh={p.refresh} />
                <h2>联系人</h2>
                <p>通讯录只记录自己的联系人，移除不会删除聊天历史。</p>
                <button
                  className="social-button secondary"
                  onClick={() => updateContact(account)}
                >
                  {listed.some((a) => a.id === account.id)
                    ? "移除联系人"
                    : "添加联系人"}
                </button>
              </section>
            ) : (
              <>
                <section>
                  <h2>登录会话</h2>
                  <p>撤销会话后，对应设备需要重新登录。</p>
                  <button
                    className="social-button secondary"
                    onClick={() =>
                      void p.act(async () =>
                        setSessions(
                          (
                            await request<{ sessions: typeof sessions }>(
                              "/auth/sessions",
                            )
                          ).sessions,
                        ),
                      )
                    }
                  >
                    查看登录会话
                  </button>
                  {sessions.map((s) => (
                    <div className="cloud-session" key={s.id}>
                      <span>
                        {s.current ? "当前会话" : "其他会话"}
                        <small>
                          {new Date(s.createdAt).toLocaleString()} 登录
                        </small>
                      </span>
                      <button
                        className="text-button"
                        onClick={() =>
                          void p.act(async () => {
                            await request(`/auth/sessions/${s.id}`, "DELETE");
                            if (s.current) p.endSession();
                            else
                              setSessions(
                                sessions.filter((x) => x.id !== s.id),
                              );
                          })
                        }
                      >
                        撤销
                      </button>
                    </div>
                  ))}
                </section>
                <section>
                  <button
                    className="social-button secondary"
                    onClick={() => void p.act(p.logout)}
                  >
                    <LogOut />
                    退出登录
                  </button>
                </section>
              </>
            )}
          </div>
        </div>
      </div>
    </section>
  );
  return (
    <div
      className={`social-shell cloud-workspace${CUSTOM_WINDOW_CHROME ? " desktop-chrome" : ""}`}
    >
      <div
        className={`social-main ${(p.tab === "messages" ? !!p.active : p.tab === "contacts" ? !!selectedContact : ["account", "settings"].includes(p.tab)) ? "has-selection" : ""} ${p.tab === "messages" && conversation ? "has-chat" : ""}`}
      >
        <SocialSidebar
          page={p.tab}
          navigate={navigate}
          onAccount={() => navigate("account")}
          onSettings={() => navigate("settings")}
          self={avatar(p.me)}
          name={p.me.name}
          unread={p.snapshot?.conversations.some(unread)}
        >
          <aside className="social-list-pane">
            <header className="list-header">
              <div>
                <h1>
                  {
                    (
                      {
                        messages: "消息",
                        contacts: "通讯录",
                        account: "我的账号",
                        settings: "设置",
                      } as Record<string, string>
                    )[p.tab]
                  }
                </h1>
                <p>
                  {p.tab === "contacts"
                    ? `${listed.length} 位联系人`
                    : p.tab === "messages"
                      ? `${p.snapshot?.conversations.length || 0} 个聊天窗口`
                      : p.me.name}
                </p>
              </div>
              {p.tab === "messages" && <IconButton label="创建群聊" onClick={() => setCreatingGroup(true)}><Plus /></IconButton>}
              {p.tab === "contacts" && (
                <IconButton
                  label="添加联系人"
                  onClick={() => setFilter("directory")}
                >
                  <Plus />
                </IconButton>
              )}
            </header>
            {["messages", "contacts"].includes(p.tab) ? (
              <>
                <div className="list-search" data-tauri-drag-region="false">
                  <Search />
                  <input
                    aria-label={
                      p.tab === "contacts" ? "搜索联系人" : "搜索聊天和正文"
                    }
                    placeholder={
                      p.tab === "contacts"
                        ? filter === "directory"
                          ? "查找名字或完整账号 ID…"
                          : "搜索联系人…"
                        : "搜索聊天和正文…"
                    }
                    value={p.query}
                    maxLength={200}
                    onChange={(e) => p.queryChange(e.target.value)}
                  />
                  {p.query && (
                    <IconButton
                      label="清空搜索"
                      onClick={() => p.queryChange("")}
                    >
                      <X />
                    </IconButton>
                  )}
                </div>
                {p.tab === "contacts" ? (
                  <>
                    <div className="profile-tabs live-contact-tabs">
                      <button
                        className={filter === "listed" ? "selected" : ""}
                        onClick={() => setFilter("listed")}
                      >
                        我的联系人
                      </button>
                      <button
                        className={filter === "directory" ? "selected" : ""}
                        onClick={() => setFilter("directory")}
                      >
                        查找账号
                      </button>
                    </div>
                    <div className="social-list">
                      {contacts.map((a) => (
                        <button
                          key={a.id}
                          className={`contact-list-item ${selectedContact?.id === a.id ? "selected" : ""}`}
                          aria-current={
                            selectedContact?.id === a.id ? "page" : undefined
                          }
                          onClick={() => setContact(a)}
                        >
                          <Avatar contact={avatar(a)} />
                          <span>
                            <strong>{a.name}</strong>
                            <small>{a.bio || "还没有填写简介"}</small>
                          </span>
                          <ArrowRight />
                        </button>
                      ))}
                      {directoryBusy && filter === "directory" ? (
                        <p className="list-empty">正在查找…</p>
                      ) : (
                        !contacts.length && (
                          <p className="list-empty">
                            {p.query ? "没有找到联系人" : "从认识一位朋友开始"}
                          </p>
                        )
                      )}
                    </div>
                  </>
                ) : (
                  <>
                    {!p.query && (
                      <div className="message-list-filters">
                        <button
                          aria-pressed={!unreadOnly}
                          onClick={() => setUnreadOnly(false)}
                        >
                          全部
                        </button>
                        <button
                          aria-pressed={unreadOnly}
                          onClick={() => setUnreadOnly(true)}
                        >
                          未读
                        </button>
                      </div>
                    )}
                    <div className="social-list">
                      {p.query && <p className="search-section-label">聊天</p>}
                      {conversations.map((c) => (
                        <div
                          className={`conversation-row ${c.id === p.active ? "selected" : ""} ${c.pinned ? "pinned" : ""}`}
                          key={c.id}
                        >
                          <button
                            className="conversation-list-item"
                            aria-current={
                              c.id === p.active ? "page" : undefined
                            }
                            onClick={() => {
                              stick.current = true;
                              p.selectConversation(c.id);
                            }}
                          >
                            <Avatar contact={avatar({ name: c.name })} />
                            <span className="conversation-list-text">
                              <span>
                                <strong>{c.name}</strong>
                                <time>
                                  {c.lastMessage &&
                                    shortTime(c.lastMessage.createdAt)}
                                </time>
                              </span>
                              <span>
                                <small>
                                  {c.lastMessage?.status === "withdrawn"
                                    ? "消息已撤回"
                                    : c.lastMessage?.text ||
                                      (c.lastMessage?.attachments.length
                                        ? "[附件]"
                                        : "开启一段对话")}
                                </small>
                                {unread(c) && (
                                  <span
                                    className={`unread-badge ${c.unreadCount ? "" : "dot"}`}
                                  >
                                    {c.unreadCount
                                      ? c.unreadCount > 99
                                        ? "99+"
                                        : c.unreadCount
                                      : ""}
                                  </span>
                                )}
                              </span>
                            </span>
                          </button>
                          <div
                            className="conversation-row-tools"
                            data-tauri-drag-region="false"
                          >
                            {c.pinned && <Pin className="pin-indicator" />}
                            <details className="conversation-menu">
                              <summary aria-label={`${c.name} 的消息选项`}>
                                <MoreHorizontal />
                              </summary>
                              <div>
                                <button
                                  onClick={() =>
                                    changeList(c, { pinned: !c.pinned })
                                  }
                                >
                                  <Pin />
                                  {c.pinned ? "取消置顶" : "置顶聊天"}
                                </button>
                                <button
                                  onClick={() =>
                                    changeList(
                                      c,
                                      unread(c)
                                        ? {
                                            readPosition: c.latestPosition,
                                            manualUnread: false,
                                          }
                                        : { manualUnread: true },
                                    )
                                  }
                                >
                                  {unread(c) ? <CheckCheck /> : <Mail />}
                                  {unread(c) ? "标为已读" : "标为未读"}
                                </button>
                              </div>
                            </details>
                          </div>
                        </div>
                      ))}
                      {!conversations.length && (
                        <p className="list-empty">
                          {p.query
                            ? "没有同名聊天"
                            : unreadOnly
                              ? "消息都已读完"
                              : "从认识一位朋友开始"}
                        </p>
                      )}
                      {p.query && (
                        <section className="message-search-results">
                          <div className="search-section-label">
                            <span>聊天记录</span>
                            <select
                              aria-label="搜索范围"
                              value={scope}
                              onChange={(e) => setScope(e.target.value)}
                            >
                              <option value="all">全部聊天</option>
                              {conversation && (
                                <option value="current">当前聊天</option>
                              )}
                            </select>
                          </div>
                          {p.results.map((m) => (
                            <button
                              className="message-search-hit"
                              key={m.id}
                              onClick={() => locate(m)}
                            >
                              <span>
                                <strong>
                                  {
                                    p.snapshot?.conversations.find(
                                      (c) => c.id === m.conversationId,
                                    )?.name
                                  }
                                </strong>
                                <time>{shortTime(m.createdAt)}</time>
                              </span>
                              <p>{m.text}</p>
                            </button>
                          ))}
                          {searchBusy ? (
                            <p className="list-empty" role="status">
                              正在搜索…
                            </p>
                          ) : (
                            searched &&
                            !p.results.length && (
                              <p className="list-empty">
                                没有找到包含这段文字的消息
                              </p>
                            )
                          )}
                          {p.searchMore && (
                            <button
                              className="social-button secondary search-more"
                              onClick={() =>
                                void p.act(() => p.search(true, scope))
                              }
                            >
                              更多结果
                            </button>
                          )}
                        </section>
                      )}
                    </div>
                  </>
                )}
              </>
            ) : (
              <div className="social-list">
                {p.tab === "account" ? (
                  <><button className={`contact-list-item ${accountSection === "profile" ? "selected" : ""}`} onClick={() => setAccountSection("profile")}>
                    <Avatar contact={avatar(p.me)} />
                    <span>
                      <strong>账号资料</strong>
                      <small>公开资料与登录会话</small>
                    </span>
                  </button>
                  <button className={`contact-list-item ${accountSection === "ai" ? "selected" : ""}`} onClick={() => setAccountSection("ai")}>
                    <Settings /><span><strong>我管理的 AI</strong><small>账号与运行节点</small></span><ArrowRight />
                  </button></>
                ) : (
                  (
                    [
                      ["appearance", "外观"],
                      ["about", "关于"],
                    ] as const
                  ).map(([key, label]) => (
                    <button
                      className={`contact-list-item ${settings === key ? "selected" : ""}`}
                      key={key}
                      onClick={() => setSettings(key)}
                    >
                      <Settings />
                      <span>{label}</span>
                      <ArrowRight />
                    </button>
                  ))
                )}
              </div>
            )}
            <div className="connection-indicator" role="status">
              <span />
              <div>
                <strong>云端聊天</strong>
                <small>记录持续保存</small>
              </div>
            </div>
          </aside>
        </SocialSidebar>
        <main className="social-workspace">
          {p.error && (
            <div className="social-error" role="alert">
              <span>{p.error}</span>
              <IconButton label="关闭错误提示" onClick={p.dismissError}>
                <X />
              </IconButton>
            </div>
          )}
          {p.notice && (
            <div className="cloud-notice" role="status">
              {p.notice}
            </div>
          )}
          <div className="workspace-content">
            {p.tab === "settings" ? (
              <section className="contact-view">
                <header
                  className="contact-header"
                  data-tauri-drag-region={
                    CUSTOM_WINDOW_CHROME ? "deep" : undefined
                  }
                >
                  <span>{settings === "appearance" ? "外观" : "关于"}</span>
                </header>
                <div className="contact-scroll">
                  <div className="profile-content cloud-settings-content">
                    {settings === "appearance" ? (
                      <section>
                        <h1>外观</h1>
                        <p>选择适合你的界面主题。</p>
                        <div className="cloud-theme-options">
                          {(
                            [
                              ["light", "浅色"],
                              ["dark", "深色"],
                            ] as const
                          ).map(([value, label]) => (
                            <button
                              className={`cloud-theme-card ${theme === value ? "selected" : ""}`}
                              aria-pressed={theme === value}
                              key={value}
                              onClick={() => setTheme(value)}
                            >
                              <span className={`cloud-theme-preview ${value}`}>
                                <i />
                                <i />
                                <i />
                              </span>
                              {label}
                            </button>
                          ))}
                        </div>
                      </section>
                    ) : (
                      <section>
                        <h1>LLM Nest</h1>
                        <p>让联系自然发生。</p>
                        <p>网页与桌面共用云端账号和聊天记录。</p>
                      </section>
                    )}
                  </div>
                </div>
              </section>
            ) : p.tab === "account" ? (
              accountSection === "ai" ? <AiManagement operatorId={p.me.id} conversations={p.snapshot?.conversations ?? []} cursor={p.snapshot?.cursor ?? ""} back={() => setAccountSection("profile")} openDirect={p.openDirect} refresh={p.refresh} /> : profile(p.me, true)
            ) : p.tab === "contacts" ? (
              selectedContact ? (
                profile(selectedContact, false)
              ) : (
                <EmptyState
                  title="从认识开始，让对话有归属"
                  description="选择一位联系人，查看资料或继续你们的聊天。"
                />
              )
            ) : conversation ? (
              <>
                <section
                  className="conversation-view"
                  aria-label={`${conversation.name}聊天`}
                >
                  <header
                    className="conversation-header"
                    data-tauri-drag-region={
                      CUSTOM_WINDOW_CHROME ? "deep" : undefined
                    }
                  >
                    <IconButton
                      label="返回消息列表"
                      className="mobile-back"
                      onClick={() => p.selectConversation("")}
                    >
                      <ArrowLeft />
                    </IconButton>
                    <button
                      className="conversation-heading"
                      onClick={() => conversation.peer ? openContact(conversation.peer) : setDetails(true)}
                    >
                      <Avatar contact={avatar({ name: conversation.name })} />
                      <span>
                        <strong>{conversation.name}</strong>
                        <small>{p.localChat ? `同机私聊 · ${p.localChat.syncState === "synced" ? "已同步云端" : p.localChat.syncState === "conflict" ? "云端同步冲突" : "等待云端同步"}` : conversation.kind === "group" ? `群聊 · ${conversation.members.length} 位成员` : conversation.peer?.kind === "ai" ? "AI · 长期私聊" : "长期私聊 · 聊天记录持续保存"}</small>
                      </span>
                    </button>
                    {conversation.kind === "direct" && conversation.peer?.kind === "ai" && p.prepareLocal && !p.localSelected && <button className="text-button" onClick={() => void p.act(p.prepareLocal!)}>开启同机私聊</button>}
                    {p.localSelected && !p.localChat && <div role="status">同机私聊暂未连接；发送仍使用本机 Agent，不会自动改发云端。<button className="text-button" onClick={() => void p.act(() => p.history())}>重新连接本机</button></div>}
                    {p.localChat && <div role="status"><small>同机私聊仅支持普通文字；附件、回复与群引用请使用云端。{p.localChat.error}{p.localChat.hasMore && "仅显示节点已缓存的历史。"}</small><button className="text-button" onClick={() => void p.act(p.syncLocal!)}>重试云端同步</button><button className="text-button" disabled={p.localChat.running || p.localChat.syncState !== "synced" || !!p.record.pending} onClick={() => void p.act(p.closeLocal!)}>返回云端发送</button>{p.localChat.running && <button className="text-button" onClick={() => void p.act(p.stopLocal!)}>停止本机回复</button>}</div>}
                    <IconButton
                      label="聊天详情"
                      aria-pressed={details}
                      onClick={() => setDetails(!details)}
                    >
                      <Info />
                    </IconButton>
                  </header>
                  {located && (
                    <div className="message-location-bar">
                      <span>正在查看搜索到的消息及前后记录</span>
                      <button onClick={latest}>返回最新消息</button>
                    </div>
                  )}
                  <div
                    className="chat-timeline cloud-messages"
                    ref={timeline}
                    onLoadCapture={() => {
                      if (stick.current && timeline.current)
                        timeline.current.scrollTop =
                          timeline.current.scrollHeight;
                    }}
                    onScroll={(e) => {
                      const el = e.currentTarget;
                      stick.current =
                        !located &&
                        el.scrollHeight - el.scrollTop - el.clientHeight < 60;
                      p.onBottom(stick.current);
                    }}
                  >
                    {p.page?.hasMore && (
                      <button
                        className="social-button secondary live-load-history"
                        onClick={() => {
                          stick.current = false;
                          p.onBottom(false);
                          void p.act(() => p.history(true));
                        }}
                      >
                        加载更早的消息
                      </button>
                    )}
                    {!p.page ? (
                      <div className="list-empty">正在读取聊天记录…</div>
                    ) : !p.page.messages.length ? (
                      <div className="conversation-start">
                        <span>
                          <MessageCircle />
                          每次相遇，都在同一个窗口
                        </span>
                        <p>从第一条消息开始你们的日常。</p>
                      </div>
                    ) : (
                      p.page.messages.map((m) => (
                        <article
                          id={m.id}
                          data-message-id={m.id}
                          className={`live-message ${m.senderId === p.me.id ? "own" : ""} ${p.location?.messageId === m.id ? "located-message" : ""}`}
                          key={m.id}
                        >
                          <Avatar
                            contact={avatar(
                              { name: m.senderName },
                            )}
                            self={m.senderId === p.me.id}
                            size="small"
                          />
                          <div className="live-message-content">
                            <div className="live-message-label">
                              <span>
                                {m.senderName}
                              </span>
                              <time>{shortTime(m.createdAt)}</time>
                              {m.importOrigin && <small title="从旧版聊天记录导入">导入历史</small>}
                            </div>
                            {m.contextReference && m.status !== "withdrawn" && <ReferenceCard directId={conversation.id} selection={{ conversationId: m.contextReference.conversationId, sources: m.contextReference.sources }} stale={m.contextReference.stale} cursor={p.snapshot!.cursor} />}
                            {m.status === "committed" && !conversation.readOnly && !p.localSelected && <RememberMessage key={p.me.id} operatorId={p.me.id} conversation={conversation} message={m} cursor={p.snapshot!.cursor} />}
                            {m.status === "committed" && conversation.kind === "group" && <ContinuePrivately conversation={conversation} message={m} messages={p.page!.messages} continueWith={p.continuePrivately} />}
                            {m.replyTo && <button className="text-button" onClick={() => void p.act(() => p.locate({ id: m.replyTo! }))}>回复消息 · {replyLabel(m.replyTo)}</button>}
                            {!!m.mentions.length && <div className="cloud-mentions">{m.mentions.map(id => <span key={id}>@{conversation.members.find(member => member.account.id === id)?.account.name || id} </span>)}</div>}
                            {m.status === "committed" && !conversation.readOnly && !p.localSelected && <button className="text-button" disabled={!!p.localSelected || !p.draftReady || p.busy || !!p.record.pending || conversation.directBlocked} onClick={() => void p.act(() => p.save({ ...p.record, draft: { ...p.record.draft, replyTo: m.id } }))}>回复</button>}
                            {p.editing?.id === m.id ? (
                              <form
                                className="live-message-editor cloud-form"
                                onSubmit={(e) => {
                                  e.preventDefault();
                                  void p.act(() =>
                                    p.mutate(m, false, p.editing!.text),
                                  );
                                }}
                              >
                                <textarea
                                  aria-label="编辑消息"
                                  value={p.editing.text}
                                  onChange={(e) =>
                                    p.edit({ id: m.id, text: e.target.value })
                                  }
                                />
                                <div className="cloud-actions">
                                  <button>保存</button>
                                  <button
                                    type="button"
                                    onClick={() => p.edit(null)}
                                  >
                                    取消
                                  </button>
                                </div>
                              </form>
                            ) : (
                              <div className="live-bubble">
                                {m.status === "withdrawn" ? (
                                  <span className="withdrawn-message">
                                    消息已撤回
                                  </span>
                                ) : (
                                  <>
                                    <MarkdownView content={m.text} />
                                    {m.status === "generating" && <span className="cloud-generation-status" role="status">正在回复…</span>}
                                    {["interrupted", "failed"].includes(m.status) && <span className="cloud-generation-status">{m.status === "failed" ? "回复未完成" : "回复已中断"}</span>}
                                    {m.attachments.map((a) =>
                                      a.kind === "image" ? (
                                        <InlineImage
                                          key={a.index}
                                          message={m}
                                          attachment={a}
                                          act={p.act}
                                          onPreview={showPreview}
                                        />
                                      ) : (
                                        <button
                                          className="cloud-file-card"
                                          key={a.index}
                                          onClick={() =>
                                            void p.act(async () => {
                                              await download(
                                                attachmentPath(m, a),
                                                a.name || "附件",
                                              );
                                            })
                                          }
                                        >
                                          <Paperclip />
                                          <span>
                                            <strong>{a.name || "文件"}</strong>
                                            <small>
                                              {fileSize(a.byteLength)} ·
                                              点击下载
                                            </small>
                                          </span>
                                          <Download />
                                        </button>
                                      ),
                                    )}
                                  </>
                                )}
                              </div>
                            )}
                            {m.local && <small role="status">{m.local.synced ? "本机消息 · 已同步云端" : "本机消息 · 待同步云端"}</small>}
                            {m.aiResponses.map(response => (
                              <div className="cloud-task-status" role="status" key={response.taskId}>
                                <span>{conversation.members.find(member => member.account.id === response.aiId)?.account.name}：{responseStatus(response)}</span>
                                {["queued", "running"].includes(response.status) && <button className="text-button" onClick={() => void p.act(async () => {
                                  await request(`/chat/ai-tasks/${response.taskId}/cancel`, "POST", { requestId: crypto.randomUUID() });
                                  await p.history();
                                  await p.refresh();
                                })}>停止回复</button>}
                              </div>
                            ))}
                            {m.senderId === p.me.id &&
                              !conversation.readOnly &&
                              !p.localSelected &&
                              m.status !== "withdrawn" && (
                                <div className="live-message-actions">
                                  <button
                                    onClick={() =>
                                      p.edit({ id: m.id, text: m.text })
                                    }
                                  >
                                    <Pencil />
                                    编辑
                                  </button>
                                  <button
                                    onClick={() =>
                                      void p.act(() => p.mutate(m, true))
                                    }
                                  >
                                    撤回
                                  </button>
                                </div>
                              )}
                          </div>
                        </article>
                      ))
                    )}
                  </div>
                  {conversation.readOnly ? <div className="composer-area"><p role="status">这是从旧版导入的只读聊天，可以查看、搜索和导出记录。</p></div> : <div className="composer-area">
                    <div
                      className={`composer ${dragging ? "image-drop-active" : ""}`}
                      onDragOver={(e) => {
                        if (
                          Array.from(e.dataTransfer.types).includes("Files")
                        ) {
                          e.preventDefault();
                          setDragging(true);
                        }
                      }}
                      onDragLeave={() => setDragging(false)}
                      onDrop={(e) => {
                        e.preventDefault();
                        setDragging(false);
                        add(Array.from(e.dataTransfer.files));
                      }}
                    >
                      {!!p.record.draft.images?.length && (
                        <div className="draft-images">
                          {p.record.draft.images.map((image) => (
                            <div className="draft-image" key={image.id}>
                              <button
                                aria-label={`预览 ${image.name}`}
                                onClick={(event) =>
                                  showPreview(
                                    `data:${image.mime};base64,${image.data}`,
                                    event.currentTarget,
                                  )
                                }
                              >
                                <img
                                  src={`data:${image.mime};base64,${image.data}`}
                                  alt={image.name}
                                />
                              </button>
                              <IconButton
                                label={`移除图片 ${image.name}`}
                                disabled={p.busy || !!p.record.pending}
                                onClick={() =>
                                  void p.save({
                                    ...p.record,
                                    draft: {
                                      ...p.record.draft,
                                      images: p.record.draft.images?.filter(
                                        (i) => i.id !== image.id,
                                      ),
                                    },
                                  })
                                }
                              >
                                <X />
                              </IconButton>
                            </div>
                          ))}
                        </div>
                      )}
                      {!!p.record.draft.files?.length && (
                        <div className="draft-files">
                          {p.record.draft.files.map((file) => (
                            <div className="draft-file" key={file.id}>
                              <span>
                                {file.name} · {fileSize(file.byteLength)}
                              </span>
                              <IconButton
                                label={`移除文件 ${file.name}`}
                                disabled={p.busy || !!p.record.pending}
                                onClick={() =>
                                  void p.save({
                                    ...p.record,
                                    draft: {
                                      ...p.record.draft,
                                      files: p.record.draft.files?.filter(
                                        (f) => f.id !== file.id,
                                      ),
                                    },
                                  })
                                }
                              >
                                <X />
                              </IconButton>
                            </div>
                          ))}
                        </div>
                      )}
                      {p.record.draft.reference && <ReferenceCard directId={conversation.id} selection={p.record.draft.reference.selection} cursor={p.snapshot!.cursor} remove={p.draftReady && !p.busy && !p.record.pending ? () => void p.act(() => p.save({ ...p.record, draft: { ...p.record.draft, reference: null } })) : undefined} />}
                      {conversation.directBlocked && <p role="status">双方私聊已屏蔽，历史和草稿保留。解除屏蔽后可发送新消息。</p>}
                      {p.record.draft.replyTo && <div className="cloud-reply-draft">回复：{replyLabel(p.record.draft.replyTo)}<button className="text-button" disabled={p.busy || !!p.record.pending || !p.draftReady} onClick={() => void p.act(() => p.save({ ...p.record, draft: { ...p.record.draft, replyTo: null } }))}>取消回复</button></div>}
                      {conversation.kind === "group" && <fieldset className="cloud-mention-picker" disabled={!!p.localSelected || !p.draftReady || p.busy || !!p.record.pending || conversation.directBlocked}>
                        <legend>提及成员（可明确指定回复的 AI）</legend>
                        {conversation.members.filter(member => member.account.id !== p.me.id).map(({ account }) => <label className="cloud-check" key={account.id}><input type="checkbox" checked={p.record.draft.mentions?.includes(account.id) || false} onChange={e => { const mentions = p.record.draft.mentions || []; void p.act(() => p.save({ ...p.record, draft: { ...p.record.draft, mentions: e.target.checked ? [...mentions, account.id] : mentions.filter(id => id !== account.id) } })); }} />@{account.name}</label>)}
                        {p.record.draft.mentions?.filter(id => !conversation.members.some(member => member.account.id === id)).map(id => <button type="button" className="text-button" key={id} onClick={() => void p.act(() => p.save({ ...p.record, draft: { ...p.record.draft, mentions: p.record.draft.mentions!.filter(previous => previous !== id) } }))}>移除已离群提及：{id}</button>)}
                      </fieldset>}
                      <textarea
                        aria-label="消息输入框"
                        rows={2}
                        maxLength={20000}
                        disabled={!p.draftReady || p.busy || !!p.record.pending || conversation.directBlocked}
                        placeholder={
                          dragging
                            ? "松开添加附件"
                            : `给 ${conversation.name} 发消息…`
                        }
                        value={p.record.draft.text}
                        onChange={(e) => p.text(e.target.value)}
                        onPaste={(e) => {
                          const files = Array.from(e.clipboardData.items)
                            .filter((i) => i.kind === "file")
                            .map((i) => i.getAsFile())
                            .filter((f): f is File => f !== null);
                          if (files.length) {
                            e.preventDefault();
                            add(files);
                          }
                        }}
                        onKeyDown={(e) => {
                          if (
                            e.key === "Enter" &&
                            !e.shiftKey &&
                            !e.nativeEvent.isComposing &&
                            e.keyCode !== 229
                          ) {
                            e.preventDefault();
                            void p.send();
                          }
                        }}
                      />
                      <div className="composer-bottom">
                        <input
                          ref={imageInput}
                          className="sr-only"
                          type="file"
                          multiple
                          accept="image/png,image/jpeg,image/webp"
                          disabled={
                            !!p.localSelected || !p.draftReady || p.busy || !!p.record.pending
                          }
                          onChange={(e) => {
                            add(Array.from(e.target.files || []));
                            e.target.value = "";
                          }}
                        />
                        <input
                          ref={fileInput}
                          className="sr-only"
                          type="file"
                          multiple
                          disabled={
                            !!p.localSelected || !p.draftReady || p.busy || !!p.record.pending
                          }
                          onChange={(e) => {
                            add(Array.from(e.target.files || []), true);
                            e.target.value = "";
                          }}
                        />
                        <div className="composer-attachments">
                          <IconButton
                            label="添加文件"
                            disabled={
                              !!p.localSelected || !p.draftReady || p.busy || !!p.record.pending
                            }
                            onClick={() => fileInput.current?.click()}
                          >
                            <Paperclip />
                          </IconButton>
                          <IconButton
                            label="添加图片"
                            disabled={
                              !!p.localSelected || !p.draftReady || p.busy || !!p.record.pending
                            }
                            onClick={() => imageInput.current?.click()}
                          >
                            <ImagePlus />
                          </IconButton>
                        </div>
                        <span className="composer-hint">
                          {p.busy
                            ? "正在处理…"
                            : "Enter 发送，Shift+Enter 换行"}
                        </span>
                        <button
                          className="send-button"
                          aria-label="发送消息"
                          disabled={
                            !p.draftReady ||
                            conversation.directBlocked ||
                            p.busy ||
                            !!p.record.pending ||
                            (!p.record.draft.text.trim() &&
                              !p.record.draft.images?.length &&
                              !p.record.draft.files?.length)
                          }
                          onClick={() => void p.send()}
                        >
                          <ArrowUp />
                        </button>
                      </div>
                      {!p.draftReady && (
                        <div className="draft-storage-status">
                          正在恢复本地草稿…
                          <button
                            className="text-button"
                            onClick={() => void p.act(p.recoverDraft)}
                          >
                            重试恢复草稿
                          </button>
                        </div>
                      )}
                      {p.record.pending && !p.busy && (
                        <div className="draft-pending-send">
                          <p>上次发送尚未确认。重试会使用同一请求确认结果。</p>
                          <button
                            className="text-button"
                            disabled={p.busy || conversation.directBlocked}
                            onClick={() => void p.send()}
                          >
                            重试上次发送
                          </button>
                        </div>
                      )}
                    </div>
                  </div>}
                </section>
                {details && (
                  <aside className="details-panel">
                    <header
                      data-tauri-drag-region={
                        CUSTOM_WINDOW_CHROME ? "deep" : undefined
                      }
                    >
                      <strong>聊天详情</strong>
                      <IconButton
                        label="关闭详情"
                        onClick={() => setDetails(false)}
                      >
                        <X />
                      </IconButton>
                    </header>
                    <div className="details-scroll">
                      {!conversation.readOnly && <ParticipationPanel operatorId={p.me.id} key={`participation/${p.me.id}/${conversation.id}`} conversation={conversation} cursor={p.snapshot!.cursor} />}
                      {!conversation.readOnly && <ConsciousnessPanel operatorId={p.me.id} key={`${p.me.id}/${conversation.id}`} conversation={conversation} cursor={p.snapshot!.cursor} summaryAiIds={managedAiIds} summaryError={managementError} />}
                      {conversation.kind === "group" && !conversation.readOnly && <GroupDetails key={conversation.id + conversation.revision} conversation={conversation} me={p.me.id} busy={p.busy} act={p.act} refresh={p.refresh} leave={() => p.selectConversation("")} />}
                      <div className="details-hero">
                        <Avatar
                          contact={avatar({ name: conversation.name })}
                          size="large"
                        />
                        <h2>{conversation.name}</h2>
                        <p>{conversation.kind === "group" ? "所有当前成员共享完整群历史" : "一个持续积累的长期私聊"}</p>
                      </div>
                      <section className="cloud-detail-section">
                        <h3>聊天设置</h3>
                        <button
                          className="social-button secondary"
                          onClick={() =>
                            changeList(conversation, {
                              pinned: !conversation.pinned,
                            })
                          }
                        >
                          <Pin />
                          {conversation.pinned ? "取消置顶" : "置顶聊天"}
                        </button>
                        <button
                          className="social-button secondary"
                          onClick={() =>
                            changeList(conversation, { manualUnread: true })
                          }
                        >
                          <Mail />
                          标为未读
                        </button>
                        <button
                          className="social-button secondary"
                          onClick={() =>
                            changeList(conversation, {
                              readPosition: p.page?.latestPosition,
                              manualUnread: false,
                            })
                          }
                        >
                          <CheckCheck />
                          标为已读
                        </button>
                        {conversation.peer && <button className="social-button secondary" onClick={() => openContact(conversation.peer!)}>查看联系人资料</button>}
                      </section>
                      <section className="cloud-detail-section">
                        <h3>导出聊天记录</h3>
                        <p>导出完整窗口；附件可以分别选择。</p>
                        <form
                          className="cloud-form"
                          onSubmit={(e) => {
                            e.preventDefault();
                            const form = new FormData(e.currentTarget);
                            void p.act(async () => {
                              await download(
                                `/chat/conversations/${conversation.id}/export?format=${form.get("format")}&includeImages=${form.has("images")}&includeFiles=${form.has("files")}`,
                                form.has("images") || form.has("files")
                                  ? "chat.zip"
                                  : `chat.${form.get("format") === "json" ? "json" : "md"}`,
                              );
                            });
                          }}
                        >
                          <select name="format">
                            <option value="markdown">Markdown</option>
                            <option value="json">JSON</option>
                          </select>
                          <label className="cloud-check">
                            <input name="images" type="checkbox" />
                            包含图片
                          </label>
                          <label className="cloud-check">
                            <input name="files" type="checkbox" />
                            包含普通文件
                          </label>
                          <button className="social-button secondary">
                            <Download />
                            导出记录
                          </button>
                        </form>
                      </section>
                    </div>
                  </aside>
                )}
              </>
            ) : (
              <EmptyState
                title="从认识开始，让对话有归属"
                description="每个账号都有自己的身份，每段关系都有持续的聊天空间。"
              >
                <button
                  className="social-button primary"
                  onClick={() => {
                    navigate("contacts");
                    setFilter("directory");
                  }}
                >
                  <Plus />
                  添加联系人
                </button>
              </EmptyState>
            )}
          </div>
        </main>
      </div>
      {creatingGroup && <CreateGroup me={p.me.id} contacts={listed} busy={p.busy} error={p.error} act={p.act} refresh={p.refresh} select={id => { p.selectConversation(id); navigate("messages"); }} close={() => setCreatingGroup(false)} />}
      <Dialog
        open={!!p.preview}
        onOpenChange={(open) => {
          if (!open) p.onPreview(undefined);
        }}
      >
        <DialogContent
          className="social-dialog cloud-image-dialog"
          aria-describedby={undefined}
          onCloseAutoFocus={(event) => {
            event.preventDefault();
            previewTrigger.current?.focus();
          }}
        >
          <DialogTitle className="sr-only">图片预览</DialogTitle>
          {p.preview && <img src={p.preview} alt="聊天图片" />}
        </DialogContent>
      </Dialog>
    </div>
  );
}
