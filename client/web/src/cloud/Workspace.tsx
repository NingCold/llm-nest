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
import type { DraftRecord } from "../social/drafts";
import { fileSize } from "../social/files";
import { download, request } from "./api";
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

interface Props {
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
  locate(message: Message): Promise<unknown>;
  act(fn: () => Promise<unknown>): Promise<void>;
  refresh(): Promise<unknown>;
  history(older?: boolean): Promise<unknown>;
  openDirect(account: Account): Promise<unknown>;
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
const avatar = (a?: Account) =>
  a ? { avatar: a.name.slice(0, 1), color: "sage" as const } : undefined;
const unread = (c: Conversation) => c.unreadCount > 0 || c.manualUnread;
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
    [unreadOnly, setUnreadOnly] = useState(false),
    [scope, setScope] = useState("all"),
    [searchBusy, setSearchBusy] = useState(false),
    [searched, setSearched] = useState(false),
    [settings, setSettings] = useState("appearance"),
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
          c.peer.name
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
              <p>真人 · 账号正常</p>
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
                            <Avatar contact={avatar(c.peer)} />
                            <span className="conversation-list-text">
                              <span>
                                <strong>{c.peer.name}</strong>
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
                              <summary aria-label={`${c.peer.name} 的消息选项`}>
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
                                            readPosition:
                                              c.lastMessage?.position,
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
                                    )?.peer.name
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
                  <button className="contact-list-item selected">
                    <Avatar contact={avatar(p.me)} />
                    <span>
                      <strong>账号资料</strong>
                      <small>公开资料与登录会话</small>
                    </span>
                  </button>
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
              profile(p.me, true)
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
                  aria-label={`${conversation.peer.name}聊天`}
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
                      onClick={() => openContact(conversation.peer)}
                    >
                      <Avatar contact={avatar(conversation.peer)} />
                      <span>
                        <strong>{conversation.peer.name}</strong>
                        <small>长期私聊 · 聊天记录持续保存</small>
                      </span>
                    </button>
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
                              m.senderId === p.me.id ? p.me : conversation.peer,
                            )}
                            self={m.senderId === p.me.id}
                            size="small"
                          />
                          <div className="live-message-content">
                            <div className="live-message-label">
                              <span>
                                {m.senderId === p.me.id
                                  ? p.me.name
                                  : conversation.peer.name}
                              </span>
                              <time>{shortTime(m.createdAt)}</time>
                            </div>
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
                            {m.senderId === p.me.id &&
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
                  <div className="composer-area">
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
                      <textarea
                        aria-label="消息输入框"
                        rows={2}
                        maxLength={20000}
                        disabled={!p.draftReady || p.busy || !!p.record.pending}
                        placeholder={
                          dragging
                            ? "松开添加附件"
                            : `给 ${conversation.peer.name} 发消息…`
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
                            !p.draftReady || p.busy || !!p.record.pending
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
                            !p.draftReady || p.busy || !!p.record.pending
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
                              !p.draftReady || p.busy || !!p.record.pending
                            }
                            onClick={() => fileInput.current?.click()}
                          >
                            <Paperclip />
                          </IconButton>
                          <IconButton
                            label="添加图片"
                            disabled={
                              !p.draftReady || p.busy || !!p.record.pending
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
                            disabled={p.busy}
                            onClick={() => void p.send()}
                          >
                            重试上次发送
                          </button>
                        </div>
                      )}
                    </div>
                  </div>
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
                      <div className="details-hero">
                        <Avatar
                          contact={avatar(conversation.peer)}
                          size="large"
                        />
                        <h2>{conversation.peer.name}</h2>
                        <p>一个持续积累的长期私聊</p>
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
                        <button
                          className="social-button secondary"
                          onClick={() => openContact(conversation.peer)}
                        >
                          查看联系人资料
                        </button>
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
