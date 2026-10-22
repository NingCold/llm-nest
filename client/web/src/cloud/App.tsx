import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CUSTOM_WINDOW_CHROME, IS_DESKTOP } from "@/lib/desktop";
import { useUiStore } from "@/store/ui";
import { useDesktopReady } from "@/hooks/useDesktopReady";
import { DraftDatabase } from "../social/draft-storage";
import { emptyDraft, type DraftRecord } from "../social/drafts";
import { readImages } from "../social/images";
import { readFiles } from "../social/files";
import {
  request,
  DownloadCancelled,
  connectServer,
  server,
  invalidate,
  subscribe,
  download,
} from "./api";
import type { Account, Conversation, Snapshot, Page, Message } from "./types";
import "./cloud.css";
const db = new DraftDatabase();
const blank = (): DraftRecord => ({ draft: emptyDraft(), pending: null });
export default function CloudApp() {
  const theme = useUiStore((s) => s.theme);
  const toggleTheme = useUiStore((s) => s.toggleTheme);
  const [connected, setConnected] = useState(!IS_DESKTOP);
  const [connecting, setConnecting] = useState(false);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(
    null,
  );
  const [searchScope, setSearchScope] = useState("current");
  const located = useRef<{
    conversationId: string;
    data: Page;
    messageId: string;
  } | null>(null);
  const [me, setMe] = useState<Account | null>(null),
    [ready, setReady] = useState(false),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [active, setActive] = useState(""),
    [tab, setTab] = useState("messages"),
    [page, setPage] = useState<Page | null>(null),
    [query, setQuery] = useState(""),
    [results, setResults] = useState<Message[]>([]),
    [people, setPeople] = useState<Account[] | null>(null),
    [searchMore, setSearchMore] = useState(false);
  const [record, setRecord] = useState<DraftRecord>(blank),
    [draftReady, setDraftReady] = useState(false),
    [busy, setBusy] = useState(false),
    [sessions, setSessions] = useState<
      { id: string; createdAt: string; current: boolean }[]
    >([]);
  const [mode, setMode] = useState(
      location.hash.startsWith("#/verify")
        ? "verify"
        : location.hash.startsWith("#/reset")
          ? "reset"
          : location.hash.startsWith("#/register")
            ? "register"
            : "login",
    ),
    [resume, setResume] = useState<{
      registrationId: string;
      resumeToken: string;
    } | null>(null),
    [url, setUrl] = useState(server),
    [preview, setPreview] = useState<string>();
  const epoch = useRef(0),
    refreshSeq = useRef(0),
    searchSeq = useRef(0),
    view = useRef(0),
    writes = useRef<Promise<void>>(Promise.resolve()),
    pendingReads = useRef<Promise<unknown>[]>([]),
    recordRef = useRef(record),
    pageRef = useRef(page),
    snapshotRef = useRef(snapshot),
    tabRef = useRef(tab),
    activeRef = useRef(active),
    ownerRef = useRef(""),
    closing = useRef(false),
    dirty = useRef(false),
    acknowledging = useRef(false),
    bottom = useRef(true);
  recordRef.current = record;
  pageRef.current = page;
  snapshotRef.current = snapshot;
  tabRef.current = tab;
  activeRef.current = active;
  ownerRef.current = me ? `${server}/${me.id}` : "";
  const report = (e: unknown) =>
    setError(e instanceof Error ? e.message : String(e));
  const run = async (fn: () => Promise<unknown>) => {
    const accountEpoch = epoch.current;
    setError("");
    try {
      await fn();
    } catch (e) {
      if (accountEpoch === epoch.current) {
        if (e instanceof DownloadCancelled) setNotice(e.message);
        else report(e);
      }
    }
  };
  const clear = () => {
    epoch.current++;
    view.current++;
    invalidate();
    setMe(null);
    setSnapshot(null);
    setPage(null);
    setActive("");
    setRecord(blank());
    setDraftReady(false);
    setResults([]);
    setPeople(null);
    setSessions([]);
    setPreview(undefined);
    setEditing(null);
    located.current = null;
  };
  const refresh = async () => {
    const e = epoch.current,
      sequence = ++refreshSeq.current;
    const data = await request<Snapshot>("/chat/init");
    if (e === epoch.current && sequence === refreshSeq.current) {
      setSnapshot(data);
      setMe(data.self);
    }
    return data.cursor;
  };
  const history = async (id: string, older = false) => {
    const e = epoch.current,
      v = view.current;
    const data = await request<Page>(
      `/chat/conversations/${id}/messages${older && page?.messages.length ? `?before=${page.messages[0].position}` : ""}`,
    );
    if (e === epoch.current && v === view.current && id === activeRef.current)
      setPage((old) =>
        older && old
          ? { ...data, messages: [...data.messages, ...old.messages] }
          : data,
      );
  };
  const syncHistory = async () => {
    const id = activeRef.current,
      old = pageRef.current,
      e = epoch.current,
      v = view.current;
    if (!id || !old) return;
    if (!old.messages.length) {
      await history(id);
      return;
    }
    const first = BigInt(old.messages[0].position),
      last = BigInt(old.messages[old.messages.length - 1].position);
    let before = bottom.current ? "" : String(last + 1n),
      messages: Message[] = [],
      latestPosition = old.latestPosition,
      hasMore = old.hasMore;
    do {
      const data = await request<Page>(
        `/chat/conversations/${id}/messages?limit=100${before ? `&before=${before}` : ""}`,
      );
      latestPosition = data.latestPosition;
      messages = [...data.messages, ...messages];
      hasMore = data.hasMore;
      if (
        !data.hasMore ||
        !data.messages.length ||
        BigInt(data.messages[0].position) <= first
      )
        break;
      before = data.messages[0].position;
    } while (true);
    if (e === epoch.current && v === view.current && id === activeRef.current)
      setPage({
        messages: messages.filter((m) => BigInt(m.position) >= first),
        latestPosition,
        hasMore,
      });
  };
  useDesktopReady(ready);
  useEffect(() => {
    const init = async () => {
      try {
        if (IS_DESKTOP && !server) return;
        if (IS_DESKTOP) {
          setConnecting(true);
          const account = await connectServer<Account>(server);
          setUrl(server);
          setConnected(true);
          setMe(account);
          if (!account) setNotice("服务器已连接，请登录");
        } else setMe(await request<Account>("/me"));
      } catch (e) {
        if (!(e instanceof Error && "status" in e && e.status === 401))
          report(e);
      } finally {
        setConnecting(false);
        setReady(true);
      }
    };
    void init();
    const hash = () => {
      if (location.hash.startsWith("#/register")) setMode("register");
      if (location.hash.startsWith("#/verify")) setMode("verify");
      if (location.hash.startsWith("#/reset")) setMode("reset");
    };
    window.addEventListener("hashchange", hash);
    const unauthorized = () => clear();
    window.addEventListener("cloud-unauthorized", unauthorized);
    return () => {
      window.removeEventListener("cloud-unauthorized", unauthorized);
      window.removeEventListener("hashchange", hash);
    };
  }, []);
  useEffect(() => {
    if (!me) return;
    void run(refresh);
    const stop = subscribe(
      snapshot?.cursor || "0",
      async () => {
        const cursor = await refresh();
        await syncHistory();
        return cursor;
      },
      report,
    );
    return stop;
  }, [me?.id]);
  useEffect(() => {
    view.current++;
    setPage(null);
    setEditing(null);
    setDraftReady(false);
    setRecord(blank());
    setResults([]);
    if (!active || !me) return;
    const v = view.current;
    void run(async () => {
      const loaded = await db.load(`${server}/${me.id}`, [active]);
      if (v !== view.current) return;
      setRecord(loaded[active] || blank());
      setDraftReady(true);
      const target = located.current;
      if (target?.conversationId === active) {
        located.current = null;
        setPage(target.data);
        bottom.current = false;
        setTimeout(
          () =>
            document
              .getElementById(target.messageId)
              ?.scrollIntoView({ block: "center" }),
          0,
        );
      } else await history(active);
    });
  }, [active, me?.id]);
  useEffect(
    () => () => {
      if (preview) URL.revokeObjectURL(preview);
    },
    [preview],
  );
  useEffect(() => {
    let disposed = false;
    const pageId = crypto.randomUUID();
    const before = (e: BeforeUnloadEvent) => {
      if (pendingReads.current.length || dirty.current || closing.current) {
        e.preventDefault();
        e.returnValue = "";
      }
    };
    window.addEventListener("beforeunload", before);
    const setup = async () => {
      if (!IS_DESKTOP) return;
      const unlisten = await listen<string>(
        "drafts-prepare-close",
        async (event) => {
          closing.current = true;
          let err: string | null = null;
          try {
            await Promise.all(pendingReads.current);
            await writes.current;
          } catch (e) {
            err = String(e);
          }
          try {
            await invoke("desktop_finish_draft_close", {
              page: pageId,
              request: event.payload,
              error: err,
            });
          } catch (e) {
            closing.current = false;
            report(e);
          }
          if (err) closing.current = false;
        },
      );
      if (disposed) {
        unlisten();
        return;
      }
      const failed = await listen("backend-shutdown-failed", () => {
        closing.current = false;
      });
      await invoke("desktop_register_draft_guard", { page: pageId });
      return () => {
        unlisten();
        failed();
      };
    };
    const p = setup().catch(report);
    return () => {
      disposed = true;
      window.removeEventListener("beforeunload", before);
      void p.then((u) => {
        u?.();
        if (IS_DESKTOP)
          void invoke("desktop_unregister_draft_guard", { page: pageId });
      });
    };
  }, []);
  const save = (next: DraftRecord) => {
    if (closing.current) throw new Error("正在保存并关闭");
    setRecord(next);
    const owner = ownerRef.current,
      id = activeRef.current;
    dirty.current = true;
    const p = writes.current
      .catch(() => {})
      .then(() => db.save(owner, id, next));
    writes.current = p;
    void p.then(() => {
      if (writes.current === p) dirty.current = false;
    }, report);
    return p;
  };
  const changeText = (text: string) => {
    void save({
      ...recordRef.current,
      draft: { ...recordRef.current.draft, text, requestId: null },
    });
  };
  const attach = (files: File[], images: boolean) => {
    if (!draftReady || busy || recordRef.current.pending || closing.current)
      return;
    const id = activeRef.current,
      v = view.current;
    const task = (async () => {
      setBusy(true);
      try {
        const draft = recordRef.current.draft;
        const items = images
          ? await readImages(files, draft.images || [])
          : await readFiles(files, draft.files || []);
        if (v !== view.current || id !== activeRef.current) return;
        const current = recordRef.current;
        await save({
          ...current,
          draft: {
            ...current.draft,
            [images ? "images" : "files"]: [
              ...(images
                ? current.draft.images || []
                : current.draft.files || []),
              ...items,
            ],
            requestId: null,
          },
        });
      } catch (e) {
        report(e);
      } finally {
        setBusy(false);
      }
    })();
    pendingReads.current.push(task);
    void task.finally(() => {
      pendingReads.current = pendingReads.current.filter((p) => p !== task);
    });
  };
  const send = () =>
    run(async () => {
      if (!draftReady || busy) return;
      setBusy(true);
      try {
        const current = recordRef.current;
        const pending = current.pending || {
          ...current.draft,
          requestId: crypto.randomUUID(),
        };
        if (
          !pending.text.trim() &&
          !pending.images?.length &&
          !pending.files?.length
        )
          return;
        const id = active,
          owner = ownerRef.current,
          e = epoch.current,
          v = view.current;
        await save({ ...current, pending });
        await request(`/chat/conversations/${id}/messages`, "POST", {
          requestId: pending.requestId,
          text: pending.text,
          images: pending.images?.map(({ mime, data }) => ({ mime, data })),
          files: pending.files?.map(({ name, mime, data }) => ({
            name,
            mime,
            data,
          })),
        });
        if (e !== epoch.current) return;
        if (v === view.current && activeRef.current === id)
          await save({ draft: emptyDraft(), pending: null });
        else await db.save(owner, id, { draft: emptyDraft(), pending: null });
        await history(id);
        await refresh();
      } finally {
        setBusy(false);
      }
    });
  const list = async (c: Conversation, patch: object) => {
    await request(`/chat/conversations/${c.id}/list`, "PUT", {
      requestId: crypto.randomUUID(),
      expectedRevision: c.listRevision,
      ...patch,
    });
    await refresh();
  };
  const openDirect = async (a: Account) => {
    const sequence = ++searchSeq.current,
      e = epoch.current;
    const c = await request<Conversation>("/chat/direct", "POST", {
      accountId: a.id,
    });
    await refresh();
    if (e !== epoch.current || sequence !== searchSeq.current) return;
    setTab("messages");
    setActive(c.id);
  };
  const conversation = snapshot?.conversations.find((c) => c.id === active);
  const acknowledge = () => {
    const current = snapshotRef.current?.conversations.find(
        (c) => c.id === activeRef.current,
      ),
      data = pageRef.current;
    if (
      tabRef.current !== "messages" ||
      !current ||
      !data?.latestPosition ||
      data.messages[data.messages.length - 1]?.position !==
        data.latestPosition ||
      !bottom.current ||
      !document.hasFocus() ||
      document.visibilityState !== "visible"
    )
      return;
    if (current.unreadCount > 0 && !acknowledging.current) {
      acknowledging.current = true;
      void run(() =>
        list(current, {
          readPosition: data.latestPosition,
          manualUnread: false,
        }),
      ).finally(() => {
        acknowledging.current = false;
      });
    }
  };
  useEffect(() => {
    if (bottom.current) {
      const el = document.querySelector(".cloud-messages");
      if (el) el.scrollTop = el.scrollHeight;
    }
    acknowledge();
  }, [page?.latestPosition, active, conversation?.unreadCount]);
  useEffect(() => {
    window.addEventListener("focus", acknowledge);
    document.addEventListener("visibilitychange", acknowledge);
    return () => {
      window.removeEventListener("focus", acknowledge);
      document.removeEventListener("visibilitychange", acknowledge);
    };
  }, []);

  const mutation = async (m: Message, withdraw: boolean, text?: string) => {
    await request(
      `/chat/messages/${m.id}${withdraw ? "/withdraw" : ""}`,
      withdraw ? "POST" : "PATCH",
      {
        requestId: crypto.randomUUID(),
        expectedRevision: m.revision,
        ...(!withdraw ? { text } : {}),
      },
    );
    setEditing(null);
    await syncHistory();
    await refresh();
  };
  const searchMessages = async (older = false, scope = searchScope) => {
    const sequence = ++searchSeq.current,
      id = active,
      v = view.current;
    const before =
      older && results.length
        ? `&beforePosition=${results[results.length - 1].position}`
        : "";
    const found = await request<Page>(
      `/chat/search?query=${encodeURIComponent(query)}${scope === "current" ? `&conversationId=${id}` : ""}${before}`,
    );
    if (
      sequence !== searchSeq.current ||
      v !== view.current ||
      id !== activeRef.current
    )
      return;
    setResults((old) => (older ? [...old, ...found.messages] : found.messages));
    setSearchMore(found.hasMore);
    if (!older && !found.messages.length) setNotice("没有找到匹配消息");
  };
  const locate = async (m: Message) => {
    const sequence = ++searchSeq.current,
      v = view.current;
    const data = await request<Page & { conversationId: string }>(
      `/chat/messages/${m.id}/context`,
    );
    if (v !== view.current || sequence !== searchSeq.current) return;
    const target = { ...data, hasMore: true };
    setResults([]);
    bottom.current = false;
    if (data.conversationId !== activeRef.current) {
      located.current = {
        conversationId: data.conversationId,
        data: target,
        messageId: m.id,
      };
      setActive(data.conversationId);
      setTab("messages");
    } else {
      setPage(target);
      setTimeout(
        () =>
          document.getElementById(m.id)?.scrollIntoView({ block: "center" }),
        0,
      );
    }
  };
  const formData = (form: HTMLFormElement) =>
    Object.fromEntries(new FormData(form).entries()) as Record<string, string>;
  const auth = async (form: HTMLFormElement) => {
    if (busy || !connected) return;
    setBusy(true);
    try {
      const data = formData(form);
      if (mode === "register") {
        setResume(await request("/auth/register", "POST", data));
        setMode("verify");
        setNotice("验证邮件已发送，请填写邮件中的令牌。");
      } else if (mode === "verify" || mode === "login") {
        const r = await request<{ account: Account }>(
          `/auth/${mode}`,
          "POST",
          data,
        );
        setMe(r.account);
        setNotice("");
      } else {
        await request(`/auth/${mode}`, "POST", data);
        setNotice(
          mode === "forgot"
            ? "如邮箱存在，重置邮件已发送。"
            : "密码已重置，请登录。",
        );
        if (mode === "reset") setMode("login");
      }
    } finally {
      setBusy(false);
    }
  };
  if (!ready) return <div className="cloud-loading">正在连接…</div>;
  const feedback = (
    <>
      {error && (
        <p className="cloud-error" role="alert">
          {error}
        </p>
      )}
      {notice && <p role="status">{notice}</p>}
    </>
  );
  if (!me)
    return (
      <main
        className="cloud-auth"
        data-tauri-drag-region={CUSTOM_WINDOW_CHROME || undefined}
      >
        <section>
          <div className="cloud-brand">LLM Nest</div>
          <p className="cloud-muted">让联系自然发生</p>
          {IS_DESKTOP && (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                if (connecting || busy) return;
                setConnecting(true);
                setConnected(false);
                setError("");
                setNotice("");
                void (async () => {
                  try {
                    const account = await connectServer<Account>(url);
                    setUrl(server);
                    setConnected(true);
                    setMe(account);
                    setNotice(
                      account ? "服务器已连接" : "服务器已连接，请登录",
                    );
                  } catch (error) {
                    report(error);
                  } finally {
                    setConnecting(false);
                  }
                })();
              }}
            >
              <label>
                服务器 HTTPS 地址
                <input
                  value={url}
                  onChange={(e) => {
                    setUrl(e.target.value);
                    setResume(null);
                    setConnected(false);
                    setError("");
                    setNotice("地址已更改，请先连接服务器");
                    invalidate();
                  }}
                  disabled={connecting || busy}
                  type="url"
                  placeholder="https://您的服务器地址"
                  required
                />
              </label>
              <button disabled={connecting || busy}>
                {connecting ? "正在连接…" : "连接服务器"}
              </button>
              {!connected && !connecting && (
                <p className="cloud-muted">请先连接服务器，再注册或登录。</p>
              )}
            </form>
          )}
          <h1>
            {
              {
                login: "欢迎回来",
                register: "邀请注册",
                verify: "验证邮箱",
                forgot: "找回密码",
                reset: "重置密码",
              }[mode]
            }
          </h1>
          {feedback}
          <form
            key={mode}
            onSubmit={(e) => {
              e.preventDefault();
              void run(() => auth(e.currentTarget));
            }}
          >
            {mode === "register" && (
              <>
                <label>
                  邀请码
                  <input
                    name="invite"
                    required
                    defaultValue={
                      new URLSearchParams(location.hash.split("?")[1]).get(
                        "invite",
                      ) || ""
                    }
                  />
                </label>
                <label>
                  名字
                  <input name="name" required maxLength={80} />
                </label>
              </>
            )}
            {["login", "register", "forgot"].includes(mode) && (
              <label>
                邮箱
                <input
                  name="email"
                  type="email"
                  autoComplete="email"
                  required
                />
              </label>
            )}
            {["login", "register", "reset"].includes(mode) && (
              <label>
                密码
                <input
                  name="password"
                  type="password"
                  autoComplete={
                    mode === "login" ? "current-password" : "new-password"
                  }
                  minLength={10}
                  required
                />
              </label>
            )}
            {["verify", "reset"].includes(mode) && (
              <label>
                邮件令牌
                <input
                  name="token"
                  required
                  defaultValue={
                    new URLSearchParams(
                      location.hash.split("?")[1] || location.search,
                    ).get("token") || ""
                  }
                />
              </label>
            )}
            <button className="primary" disabled={busy || !connected}>
              继续
            </button>
          </form>
          {mode === "verify" && resume && (
            <form
              onSubmit={(e) => {
                e.preventDefault();
                const email = formData(e.currentTarget).email;
                void run(async () => {
                  await request("/auth/registration", "PATCH", {
                    ...resume,
                    email,
                  });
                  setNotice("已向新邮箱发送验证邮件");
                });
              }}
            >
              <label>
                更正邮箱
                <input type="email" name="email" required />
              </label>
              <button>更正并重发</button>
              <button
                type="button"
                onClick={() =>
                  void run(async () => {
                    await request("/auth/resend", "POST", resume);
                    setNotice("已重发验证邮件");
                  })
                }
              >
                重发邮件
              </button>
            </form>
          )}
          <nav>
            {[
              ["login", "登录"],
              ["register", "注册"],
              ["verify", "验证邮箱"],
              ["forgot", "忘记密码"],
              ["reset", "已有重置令牌"],
            ].map(([key, label]) => (
              <button
                key={key}
                onClick={() => {
                  setMode(key);
                  setError("");
                }}
              >
                {label}
              </button>
            ))}
          </nav>
        </section>
      </main>
    );
  return (
    <main
      className={`cloud-shell ${CUSTOM_WINDOW_CHROME ? "cloud-desktop" : ""}`}
    >
      <aside
        className="cloud-rail"
        data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}
      >
        <div className="cloud-brand">Nest</div>
        {[
          ["messages", "消息"],
          ["contacts", "通讯录"],
          ["account", "我的账号"],
        ].map(([key, label]) => (
          <button
            className={tab === key ? "selected" : ""}
            onClick={() => {
              searchSeq.current++;
              setTab(key);
              setQuery("");
              setResults([]);
            }}
            key={key}
          >
            {label}
          </button>
        ))}
        <span className="cloud-self">{me.name}</span>
      </aside>
      <aside
        className="cloud-list"
        data-tauri-drag-region={CUSTOM_WINDOW_CHROME || undefined}
      >
        <h1>
          {tab === "messages"
            ? "消息"
            : tab === "contacts"
              ? "通讯录"
              : "我的账号"}
        </h1>
        {tab === "messages" && (
          <form
            data-tauri-drag-region="false"
            onSubmit={(e) => {
              e.preventDefault();
              setSearchScope("all");
              void run(() => searchMessages(false, "all"));
            }}
          >
            <input
              aria-label="搜索全部聊天"
              placeholder="搜索全部聊天正文"
              value={query}
              onChange={(e) => {
                searchSeq.current++;
                setQuery(e.target.value);
              }}
            />
            <button>搜索全部聊天</button>
          </form>
        )}
        {tab === "messages" && searchScope === "all" && results.length > 0 && (
          <div className="cloud-results">
            {results.map((m) => (
              <button key={m.id} onClick={() => void run(() => locate(m))}>
                {m.text}
              </button>
            ))}
            {searchMore && (
              <button
                onClick={() => void run(() => searchMessages(true, "all"))}
              >
                更多结果
              </button>
            )}
          </div>
        )}
        {tab === "messages" &&
          [...(snapshot?.conversations || [])]
            .sort((a, b) => Number(b.pinned) - Number(a.pinned))
            .map((c) => (
              <button
                className={`cloud-conversation ${active === c.id ? "selected" : ""}`}
                key={c.id}
                onClick={() => {
                  searchSeq.current++;
                  setActive(c.id);
                  bottom.current = true;
                }}
              >
                <strong>
                  {c.pinned ? "· " : ""}
                  {c.peer.name}
                </strong>
                <span>
                  {c.lastMessage?.status === "withdrawn"
                    ? "消息已撤回"
                    : c.lastMessage?.text || "开始聊天"}
                </span>
                {(c.unreadCount > 0 || c.manualUnread) && (
                  <small>{c.unreadCount || "未读"}</small>
                )}
              </button>
            ))}
        {tab === "contacts" && (
          <>
            <form
              onSubmit={(e) => {
                e.preventDefault();
                void run(async () => {
                  const sequence = ++searchSeq.current;
                  const found = await request<Account[]>(
                    `/chat/accounts?query=${encodeURIComponent(query)}`,
                  );
                  if (sequence === searchSeq.current) setPeople(found);
                });
              }}
            >
              <input
                placeholder="查找名字或完整账号 ID"
                value={query}
                onChange={(e) => (
                  searchSeq.current++,
                  setQuery(e.target.value)
                )}
              />
              <button>查找</button>
            </form>
            {(people ?? snapshot?.contacts ?? [])
              .filter((a) => a.id !== me.id)
              .map((a) => (
                <div className="cloud-person" key={a.id}>
                  <strong>{a.name}</strong>
                  <p>{a.bio}</p>
                  <button onClick={() => void run(() => openDirect(a))}>
                    私聊
                  </button>
                  <button
                    onClick={() =>
                      void run(async () => {
                        await request("/chat/contacts", "POST", {
                          accountId: a.id,
                          listed: !snapshot?.contacts.some(
                            (c) => c.id === a.id,
                          ),
                        });
                        await refresh();
                        setPeople(null);
                      })
                    }
                  >
                    {snapshot?.contacts.some((c) => c.id === a.id)
                      ? "移除联系人"
                      : "添加联系人"}
                  </button>
                </div>
              ))}
          </>
        )}
        {tab === "account" && (
          <p className="cloud-muted">
            {me.name}
            <br />
            {me.bio}
          </p>
        )}
      </aside>
      <section className="cloud-main">
        {feedback}
        {tab === "account" ? (
          <>
            <header
              data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}
            >
              <h2>个人资料与登录会话</h2>
              <button onClick={toggleTheme}>
                {theme === "dark" ? "浅色外观" : "深色外观"}
              </button>
            </header>
            <div className="cloud-account-id">
              <label>
                账号 ID
                <input value={me.id} readOnly aria-label="账号 ID" />
              </label>
              <button
                onClick={() =>
                  void run(async () => {
                    await navigator.clipboard.writeText(me.id);
                    setNotice("账号 ID 已复制");
                  })
                }
              >
                复制账号 ID
              </button>
            </div>
            <form
              className="cloud-profile"
              onSubmit={(e) => {
                e.preventDefault();
                const data = formData(e.currentTarget);
                void run(async () => {
                  setMe(await request<Account>("/me", "PATCH", data));
                  setNotice("资料已保存");
                });
              }}
            >
              <label>
                名字
                <input name="name" defaultValue={me.name} required />
              </label>
              <label>
                简介
                <textarea name="bio" defaultValue={me.bio} />
              </label>
              <button>保存资料</button>
            </form>
            <button
              onClick={() =>
                void run(async () =>
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
              <div key={s.id}>
                {new Date(s.createdAt).toLocaleString()}{" "}
                {s.current ? "当前会话" : ""}
                <button
                  onClick={() =>
                    void run(async () => {
                      await request(`/auth/sessions/${s.id}`, "DELETE");
                      if (s.current) clear();
                      else setSessions(sessions.filter((x) => x.id !== s.id));
                    })
                  }
                >
                  撤销
                </button>
              </div>
            ))}
            <button
              onClick={() =>
                void run(async () => {
                  await Promise.all(pendingReads.current);
                  await writes.current;
                  await request("/auth/logout", "POST");
                  clear();
                })
              }
            >
              退出登录
            </button>
          </>
        ) : tab === "messages" && conversation ? (
          <>
            <header
              data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}
            >
              <h2>{conversation.peer.name}</h2>
              <button
                onClick={() =>
                  void run(() =>
                    list(conversation, { pinned: !conversation.pinned }),
                  )
                }
              >
                {conversation.pinned ? "取消置顶" : "置顶"}
              </button>
              <button
                onClick={() => {
                  bottom.current = false;
                  void run(() => list(conversation, { manualUnread: true }));
                }}
              >
                标为未读
              </button>
              <button
                onClick={() =>
                  void run(() =>
                    list(conversation, {
                      readPosition: page?.latestPosition,
                      manualUnread: false,
                    }),
                  )
                }
              >
                标为已读
              </button>
              <details>
                <summary>导出</summary>
                <form
                  onSubmit={(e) => {
                    e.preventDefault();
                    const d = new FormData(e.currentTarget);
                    void run(async () => {
                      await download(
                        `/chat/conversations/${active}/export?format=${d.get("format")}&includeImages=${d.has("images")}&includeFiles=${d.has("files")}`,
                        d.has("images") || d.has("files")
                          ? "chat.zip"
                          : `chat.${d.get("format") === "json" ? "json" : "md"}`,
                      );
                    });
                  }}
                >
                  <select name="format">
                    <option value="markdown">Markdown</option>
                    <option value="json">JSON</option>
                  </select>
                  <label>
                    <input type="checkbox" name="images" />
                    含图片
                  </label>
                  <label>
                    <input type="checkbox" name="files" />
                    含文件
                  </label>
                  <button>下载导出</button>
                </form>
              </details>
            </header>
            <form
              data-tauri-drag-region="false"
              className="cloud-search"
              onSubmit={(e) => {
                e.preventDefault();
                setSearchScope("current");
                void run(() => searchMessages(false, "current"));
              }}
            >
              <input
                placeholder="搜索当前聊天"
                value={query}
                onChange={(e) => (
                  searchSeq.current++,
                  setQuery(e.target.value)
                )}
              />
              <button>搜索</button>
              <button
                type="button"
                onClick={() => {
                  searchSeq.current++;
                  setResults([]);
                  bottom.current = true;
                  void run(() => history(active));
                }}
              >
                最新消息
              </button>
            </form>
            {searchScope === "current" && results.length > 0 && (
              <div className="cloud-results">
                {searchMore && (
                  <button onClick={() => void run(() => searchMessages(true))}>
                    更多搜索结果
                  </button>
                )}
                {results.map((m) => (
                  <button key={m.id} onClick={() => void run(() => locate(m))}>
                    {m.text}
                  </button>
                ))}
              </div>
            )}
            <div
              className="cloud-messages"
              onScroll={(e) => {
                const el = e.currentTarget;
                bottom.current =
                  el.scrollHeight - el.scrollTop - el.clientHeight < 40;
                if (bottom.current) acknowledge();
              }}
            >
              {page?.hasMore && (
                <button onClick={() => void run(() => history(active, true))}>
                  加载更早消息
                </button>
              )}
              {page?.messages.map((m) => (
                <article
                  id={m.id}
                  key={m.id}
                  className={m.senderId === me.id ? "mine" : ""}
                >
                  <small>
                    {m.senderId === me.id ? me.name : conversation.peer.name} ·{" "}
                    {new Date(m.createdAt).toLocaleString()}
                  </small>
                  {editing?.id === m.id ? (
                    <form
                      className="cloud-inline-edit"
                      onSubmit={(e) => {
                        e.preventDefault();
                        void run(() => mutation(m, false, editing.text));
                      }}
                    >
                      <textarea
                        aria-label="编辑消息"
                        value={editing.text}
                        onChange={(e) =>
                          setEditing({ ...editing, text: e.target.value })
                        }
                      />
                      <button>保存</button>
                      <button type="button" onClick={() => setEditing(null)}>
                        取消
                      </button>
                    </form>
                  ) : (
                    <p>{m.status === "withdrawn" ? "消息已撤回" : m.text}</p>
                  )}
                  {m.attachments.map((a) => (
                    <div key={a.index}>
                      <button
                        onClick={() =>
                          void run(async () => {
                            const path = `/chat/messages/${m.id}/attachments/${a.index}?revision=${m.revision}`;
                            if (a.kind === "image")
                              setPreview(
                                await download(
                                  path + "&inline=true",
                                  a.name || "图片",
                                  true,
                                ),
                              );
                            else await download(path, a.name || "附件");
                          })
                        }
                      >
                        {a.kind === "image" ? "查看图片" : "下载文件"} ·{" "}
                        {a.name || a.mime} ({a.byteLength} B)
                      </button>
                      {a.kind === "image" && (
                        <button
                          onClick={() =>
                            void run(async () => {
                              await download(
                                `/chat/messages/${m.id}/attachments/${a.index}?revision=${m.revision}`,
                                `image-${m.id}-${a.index}`,
                              );
                            })
                          }
                        >
                          保存原图
                        </button>
                      )}
                    </div>
                  ))}
                  {m.senderId === me.id && m.status !== "withdrawn" && (
                    <div>
                      <button
                        onClick={() => setEditing({ id: m.id, text: m.text })}
                      >
                        编辑
                      </button>
                      <button onClick={() => void run(() => mutation(m, true))}>
                        撤回
                      </button>
                    </div>
                  )}
                </article>
              ))}
            </div>
            <div
              className="cloud-composer"
              onDragOver={(e) => {
                if (Array.from(e.dataTransfer.types).includes("Files"))
                  e.preventDefault();
              }}
              onDrop={(e) => {
                if (e.dataTransfer.files.length) {
                  e.preventDefault();
                  const files = Array.from(e.dataTransfer.files);
                  attach(
                    files,
                    files.every((file) => file.type.startsWith("image/")),
                  );
                }
              }}
            >
              {!draftReady ? (
                <p>
                  正在恢复草稿。
                  <button
                    onClick={() =>
                      void run(async () => {
                        const id = active,
                          v = view.current;
                        const loaded = await db.load(ownerRef.current, [id]);
                        if (v === view.current) {
                          setRecord(loaded[id] || blank());
                          setDraftReady(true);
                        }
                      })
                    }
                  >
                    重试恢复
                  </button>
                </p>
              ) : (
                <>
                  {record.pending && (
                    <p>上次发送尚未确认。请手动重试同一条消息。</p>
                  )}
                  <textarea
                    aria-label="消息"
                    placeholder="写一条消息…（Enter 发送，Shift+Enter 换行）"
                    value={record.draft.text}
                    disabled={busy || !!record.pending}
                    onChange={(e) => changeText(e.target.value)}
                    onPaste={(e) => {
                      const files = Array.from(e.clipboardData.items)
                        .filter((i) => i.kind === "file")
                        .map((i) => i.getAsFile())
                        .filter((f): f is File => f !== null);
                      if (files.length) {
                        e.preventDefault();
                        attach(
                          files,
                          files.every((file) => file.type.startsWith("image/")),
                        );
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
                        void send();
                      }
                    }}
                  />
                  <div className="cloud-drafts">
                    {record.draft.images?.map((image) => (
                      <button
                        key={`preview-${image.id}`}
                        onClick={() =>
                          setPreview(`data:${image.mime};base64,${image.data}`)
                        }
                      >
                        <img
                          className="cloud-draft-image"
                          src={`data:${image.mime};base64,${image.data}`}
                          alt={image.name}
                        />
                      </button>
                    ))}
                    {[
                      ...(record.draft.images || []),
                      ...(record.draft.files || []),
                    ].map((a) => (
                      <button
                        disabled={!!record.pending}
                        key={a.id}
                        onClick={() =>
                          void save({
                            ...record,
                            draft: {
                              ...record.draft,
                              images: record.draft.images?.filter(
                                (i) => i.id !== a.id,
                              ),
                              files: record.draft.files?.filter(
                                (i) => i.id !== a.id,
                              ),
                            },
                          })
                        }
                      >
                        {a.name} ×
                      </button>
                    ))}
                  </div>
                  <label className="cloud-upload">
                    图片
                    <input
                      type="file"
                      accept="image/png,image/jpeg,image/webp"
                      multiple
                      disabled={busy || !!record.pending}
                      onChange={(e) => {
                        attach(Array.from(e.target.files || []), true);
                        e.target.value = "";
                      }}
                    />
                  </label>
                  <label className="cloud-upload">
                    文件
                    <input
                      type="file"
                      multiple
                      disabled={busy || !!record.pending}
                      onChange={(e) => {
                        attach(Array.from(e.target.files || []), false);
                        e.target.value = "";
                      }}
                    />
                  </label>
                  <button
                    className="primary"
                    disabled={busy}
                    onClick={() => void send()}
                  >
                    {busy ? "发送中…" : record.pending ? "重试发送" : "发送"}
                  </button>
                </>
              )}
            </div>
          </>
        ) : (
          <div
            className="cloud-empty"
            data-tauri-drag-region={CUSTOM_WINDOW_CHROME || undefined}
          >
            <h2>让联系自然发生</h2>
            <p>从通讯录选择联系人，开启一段对话。</p>
          </div>
        )}
      </section>
      {preview && (
        <div className="cloud-preview" role="dialog" aria-label="图片预览">
          <button onClick={() => setPreview(undefined)}>关闭</button>
          <img src={preview} alt="消息原图" />
        </div>
      )}
    </main>
  );
}
