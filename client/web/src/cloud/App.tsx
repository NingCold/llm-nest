import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IS_DESKTOP } from "@/lib/desktop";
import { clearConsciousnessCommands } from "./Consciousness";
import { clearParticipationCommands } from "./Participation";
import { Workspace } from "./Workspace";
import { AuthPage } from "./AuthPage";
import { StartupSurface } from "@/components/StartupSurface";
import { useDesktopReady } from "@/hooks/useDesktopReady";
import { DraftDatabase } from "../social/draft-storage";
import { emptyDraft } from "../social/drafts";
import { readImages } from "../social/images";
import { readFiles } from "../social/files";
import {
  request,
  DownloadCancelled,
  HttpError,
  connectServer,
  server,
  invalidate,
  subscribe,
} from "./api";
import type { Account, Conversation, Snapshot, Page, Message, CloudDraftRecord as DraftRecord, DraftReference, ReferencePreview } from "./types";
import "./cloud.css";
const db = new DraftDatabase<DraftReference>();
const blank = (): DraftRecord => ({ draft: emptyDraft<DraftReference>(), pending: null });
export default function CloudApp() {
  const [connected, setConnected] = useState(!IS_DESKTOP);
  const [connecting, setConnecting] = useState(false);
  const [editing, setEditing] = useState<{ id: string; text: string } | null>(
    null,
  );
  const searchScope = "all";
  const [messageLocation, setMessageLocation] = useState<{
    conversationId: string;
    messageId: string;
  } | null>(null);
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
    [searchMore, setSearchMore] = useState(false);
  const [record, setRecord] = useState<DraftRecord>(blank),
    [draftReady, setDraftReady] = useState(false),
    [busy, setBusy] = useState(false);
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
    clearConsciousnessCommands();
    clearParticipationCommands();
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
    setPreview(undefined);
    setEditing(null);
    located.current = null;
    setMessageLocation(null);
  };
  const refresh = async () => {
    const e = epoch.current,
      sequence = ++refreshSeq.current;
    const data = await request<Snapshot>("/chat/init");
    if (e === epoch.current && sequence === refreshSeq.current) {
      setSnapshot(data);
      snapshotRef.current = data;
      setMe(data.self);
      const selected = activeRef.current;
      if (selected && !data.conversations.some(c => c.id === selected)) {
        activeRef.current = "";
        view.current++;
        setActive("");
        setPage(null);
        setResults([]);
        setMessageLocation(null);
      }
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
    const unauthorized = () => {
      if (ownerRef.current) clear();
    };
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
      if (!draftReady || busy || snapshotRef.current?.conversations.find((c) => c.id === active)?.directBlocked) return;
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
        try {
          await request(`/chat/conversations/${id}/messages`, "POST", {
            requestId: pending.requestId,
            text: pending.text,
            mentions: pending.mentions ?? [],
            replyTo: pending.replyTo ?? null,
            ...(pending.reference ? { reference: pending.reference.selection } : {}),
            images: pending.images?.map(({ mime, data }) => ({ mime, data })),
            files: pending.files?.map(({ name, mime, data }) => ({
              name,
              mime,
              data,
            })),
          });
        } catch (error) {
          // An old pending command may already have succeeded before access changed.
          // Only an explicit rejection of the first dispatch unlocks its editable draft.
          if (!current.pending && error instanceof HttpError &&
              [400, 403, 404, 409, 413, 415, 422].includes(error.status) && e === epoch.current) {
            if (v === view.current && activeRef.current === id) await save(current);
            else await db.save(owner, id, current);
          }
          throw error;
        }
        if (e !== epoch.current) return;
        if (v === view.current && activeRef.current === id)
          await save({ draft: emptyDraft<DraftReference>(), pending: null });
        else await db.save(owner, id, { draft: emptyDraft<DraftReference>(), pending: null });
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
    setMessageLocation(null);
    setActive(c.id);
  };
  const continuePrivately = async (ai: Account, reference: DraftReference) => {
    const accountEpoch = epoch.current, owner = ownerRef.current, originView = view.current;
    const direct = await request<Conversation>("/chat/direct", "POST", { accountId: ai.id });
    await request<ReferencePreview>(`/chat/conversations/${direct.id}/reference-preview`, "POST", { reference: reference.selection });
    if (accountEpoch !== epoch.current || originView !== view.current) throw new Error("窗口已变化，请重新选择话题来源");
    await Promise.all(pendingReads.current);
    await writes.current;
    const loaded = await db.load(owner, [direct.id]);
    if (accountEpoch !== epoch.current || originView !== view.current) throw new Error("窗口已变化，请重新选择话题来源");
    const current = direct.id === activeRef.current ? recordRef.current : loaded[direct.id] || blank();
    if (current.pending) throw new Error("这个私聊有尚未确认的发送，请先确认原发送结果，再添加群话题引用。");
    await db.save(owner, direct.id, { ...current, draft: { ...current.draft, reference, requestId: null } });
    await refresh();
    if (accountEpoch !== epoch.current || originView !== view.current) throw new Error("引用已保存至私聊草稿，但当前窗口已变化，请重新打开该私聊。");
    setActive(direct.id); setTab("messages"); setMessageLocation(null);
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
  const locate = async (m: Pick<Message, "id">) => {
    const sequence = ++searchSeq.current,
      v = view.current;
    const data = await request<Page & { conversationId: string }>(
      `/chat/messages/${m.id}/context`,
    );
    if (v !== view.current || sequence !== searchSeq.current) return;
    const target = { ...data, hasMore: true };
    setMessageLocation({
      conversationId: data.conversationId,
      messageId: m.id,
    });
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
    } catch (error) {
      if (
        mode === "login" &&
        error instanceof HttpError &&
        error.status === 401
      )
        throw new Error("邮箱或密码不正确，请检查后重试。");
      throw error;
    } finally {
      setBusy(false);
    }
  };
  const connect = () => {
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
        setNotice(account ? "服务器已连接" : "服务器已连接，请登录");
      } catch (error) {
        report(error);
      } finally {
        setConnecting(false);
      }
    })();
  };
  const navigate = (next: string) => {
    searchSeq.current++;
    setTab(next);
    setQuery("");
    setResults([]);
    setNotice("");
  };
  const queryChange = (value: string) => {
    searchSeq.current++;
    setQuery(value);
    setResults([]);
    setSearchMore(false);
  };
  const recoverDraft = async () => {
    const id = active,
      v = view.current;
    const loaded = await db.load(ownerRef.current, [id]);
    if (v === view.current) {
      setRecord(loaded[id] || blank());
      setDraftReady(true);
    }
  };
  const logout = async () => {
    await Promise.all(pendingReads.current);
    await writes.current;
    await request("/auth/logout", "POST");
    clear();
  };
  if (!ready) return <StartupSurface />;
  if (!me)
    return (
      <AuthPage
        mode={mode}
        onMode={(mode) => {
          setMode(mode);
          setError("");
        }}
        busy={busy}
        connected={connected}
        connecting={connecting}
        serverUrl={url}
        onServerUrl={(value) => {
          setUrl(value);
          setResume(null);
          setConnected(false);
          setError("");
          setNotice("地址已更改，请先连接服务器");
          invalidate();
        }}
        onConnect={connect}
        onSubmit={(form) => void run(() => auth(form))}
        error={error}
        notice={notice}
        canResume={!!resume}
        onCorrectEmail={(email) =>
          void run(async () => {
            await request("/auth/registration", "PATCH", { ...resume, email });
            setNotice("已向新邮箱发送验证邮件");
          })
        }
        onResend={() =>
          void run(async () => {
            await request("/auth/resend", "POST", resume);
            setNotice("已重发验证邮件");
          })
        }
      />
    );
  return (
    <Workspace
      me={me}
      snapshot={snapshot}
      active={active}
      location={messageLocation}
      tab={tab}
      page={page}
      query={query}
      results={results}
      searchMore={searchMore}
      record={record}
      draftReady={draftReady}
      busy={busy}
      error={error}
      notice={notice}
      preview={preview}
      editing={editing}
      navigate={navigate}
      selectConversation={(id) => {
        searchSeq.current++;
        setMessageLocation(null);
        setActive(id);
        if (id && id === active) void run(() => history(id));
        bottom.current = true;
        setTab("messages");
      }}
      queryChange={queryChange}
      search={searchMessages}
      locate={locate}
      act={run}
      refresh={refresh}
      history={(older) => {
        if (!older) setMessageLocation(null);
        return history(active, older);
      }}
      openDirect={openDirect}
      continuePrivately={continuePrivately}
      list={list}
      mutate={mutation}
      edit={setEditing}
      text={changeText}
      attach={attach}
      save={save}
      send={send}
      recoverDraft={recoverDraft}
      onBottom={(atBottom) => {
        bottom.current = atBottom;
        if (atBottom) acknowledge();
      }}
      onProfile={setMe}
      logout={logout}
      endSession={clear}
      onPreview={setPreview}
      dismissError={() => setError("")}
    />
  );
}
