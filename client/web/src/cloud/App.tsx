import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IS_DESKTOP } from "@/lib/desktop";
import { clearConsciousnessCommands } from "./Consciousness";
import { clearAiConfigurationCommands } from "./AiConfiguration";
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
import { localView, prepareLocal, sendLocal, syncLocal, stopLocal, localPage, type LocalView } from "./local-api";
import { encryptMessage, projectPage, projectSnapshot, enableEncryption } from "./crypto/content";
import { cryptoDevice, lockCrypto, localKeyMessages, receiveLocalKeys } from "./crypto/engine";
import { synchronizeResources } from "./crypto/resources";
import { mutateEncrypted } from "./crypto/mutations";
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
  const [localChats, setLocalChats] = useState<Record<string, LocalView>>({});
  const localRef = useRef(localChats);
  localRef.current = localChats;
  const localRouteKey = (id: string) => `llmn-local-chat-route:${JSON.stringify([ownerRef.current, id])}`;
  const savedLocal = (id: string) => IS_DESKTOP && window.localStorage?.getItem(localRouteKey(id)) === "local";
  const acceptLocal = (data: LocalView, expectedEpoch = epoch.current) => {
    if (expectedEpoch !== epoch.current || ownerRef.current !== `${server}/${data.human.id}`) return;
    localRef.current = { ...localRef.current, [data.conversation.id]: data };
    setLocalChats(localRef.current);
    if (activeRef.current === data.conversation.id) {
      const page = localPage(data);
      if (page.messages.some(m => m.sealed) || snapshotRef.current?.conversations.some(c => c.id === data.conversation.id && c.encryption)) void receiveLocalKeys(data.ai.id, data.keyMessages ?? []).then(() => projectPage(page, snapshotRef.current?.conversations.find(c => c.id === data.conversation.id))).then(value => { if (expectedEpoch === epoch.current && activeRef.current === data.conversation.id) setPage(value); }).catch(report);
      else setPage(page);
    }
  };
  const loadLocal = async (id: string) => {
    const e = epoch.current;
    acceptLocal(await localView(id), e);
  };
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
    lockCrypto();
    localRef.current = {};
    setLocalChats({});
    clearConsciousnessCommands();
    clearAiConfigurationCommands();
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
    const raw = await request<Snapshot>("/chat/init");
    const data = await projectSnapshot(raw);
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
  const requestPage = async (path: string) => {
    const data = await request<Page>(path);
    const id = path.match(/\/conversations\/([^/]+)\/messages/)?.[1];
    const conversation = snapshotRef.current?.conversations.find(c => c.id === id);
    return data.messages?.some(m => m.sealed) || conversation?.encryption ? projectPage(data, conversation) : data;
  };
  const history = async (id: string, older = false) => {
    if (localRef.current[id] || savedLocal(id)) { await loadLocal(id); return; }
    const e = epoch.current,
      v = view.current;
    const data = await requestPage(
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
    if (localRef.current[id] || savedLocal(id)) { await loadLocal(id); return; }
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
      const data = await requestPage(
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
    if (!me) return;
    const update = () => {
      if (!cryptoDevice(me.id)) {
        const hide = (message: Message) => message.sealed ? { ...message, text: "", cryptoError: "本机加密资料已锁定" } : message;
        setPage(previous => previous ? { ...previous, messages: previous.messages.map(hide) } : previous);
        setSnapshot(previous => previous ? { ...previous, conversations: previous.conversations.map(c => ({ ...c, lastMessage: c.lastMessage ? hide(c.lastMessage) : null })) } : previous);
        setResults(previous => previous.map(hide)); setEditing(null); setPreview(undefined);
        return;
      }
      void run(async () => { await refresh(); await syncHistory(); });
    };
    const timer = setInterval(() => { if (cryptoDevice(me.id)) void synchronizeResources(me.id).catch(report); }, 3000);
    window.addEventListener("llmn-crypto-change", update);
    return () => { clearInterval(timer); window.removeEventListener("llmn-crypto-change", update); };
  }, [me?.id]);
  useEffect(() => {
    if (!IS_DESKTOP || !me || !active || !(localChats[active] || savedLocal(active))) return;
    const e = epoch.current;
    let cancelled = false, polling = false;
    const timer = setInterval(() => {
      if (polling) return;
      polling = true;
      void localView(active).then(data => { if (!cancelled) acceptLocal(data, e); }).catch(error => { if (!cancelled && e === epoch.current) report(error); }).finally(() => { polling = false; });
    }, 500);
    return () => { cancelled = true; clearInterval(timer); };
  }, [me?.id, active, !!localChats[active], savedLocal(active)]);
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
    window.addEventListener("cloud-crypto-lock", lockCrypto);
    return () => {
      window.removeEventListener("cloud-unauthorized", unauthorized);
      window.removeEventListener("cloud-crypto-lock", lockCrypto);
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
      if (loaded[active]?.pending?.route === "local") {
        window.localStorage?.setItem(localRouteKey(active), "local");
        await loadLocal(active);
        return;
      }
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
      const retained = await listen("desktop-close-retained", () => {
        closing.current = false;
      });
      await invoke("desktop_register_draft_guard", { page: pageId });
      return () => {
        unlisten();
        failed();
        retained();
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
    if (localRef.current[activeRef.current] || savedLocal(activeRef.current)) { report(new Error("同机私聊仅支持普通文字，请使用云端发送附件")); return; }
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
      const target = snapshotRef.current?.conversations.find((c) => c.id === active);
      if (!draftReady || busy || target?.directBlocked || target?.readOnly) return;
      setBusy(true);
      try {
        const current = recordRef.current;
        const pending = current.pending || {
          ...current.draft,
          requestId: crypto.randomUUID(),
          ...(localRef.current[active] || savedLocal(active) ? { route: "local" as const } : {}),
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
        if (target?.encryption && !current.pending) {
          if (pending.images?.length || pending.files?.length || pending.reference) throw new Error("本批加密私聊仅支持文字，附件和群话题引用请等待后续支持");
          pending.messageId = crypto.randomUUID();
          pending.sealed = await encryptMessage(target, me!.id, pending.messageId, pending.text, "1", pending.replyTo ?? null, pending.mentions ?? [], pending.route === "local");
          if (pending.route === "local") pending.keyMessages = localKeyMessages(target.members.map(m => m.account.id));
        }
        if (target?.encryption && !pending.sealed) throw new Error("当前窗口已经启用加密，原明文待确认发送不能降级或重新加密重试，请先核对旧发送结果");
        await save({ ...current, pending });
        try {
          if (pending.route === "local") {
            if (pending.images?.length || pending.files?.length || pending.replyTo || pending.reference || pending.mentions?.length)
              throw new HttpError(422, "同机私聊本批仅支持普通文字，请清除附件、回复、提及和群引用后再发送；这些内容可使用云端发送。");
            const data = await sendLocal(id, pending.requestId, pending.sealed ? "" : pending.text, pending.sealed, pending.messageId, pending.keyMessages);
            if (e !== epoch.current) return;
            acceptLocal(data, e);
          } else await request(`/chat/conversations/${id}/messages`, "POST", {
            requestId: pending.requestId,
            ...(pending.sealed ? { messageId: pending.messageId, sealed: pending.sealed, text: "" } : { text: pending.text }),
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
        if (!localRef.current[id]) await refresh();
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
    if (localRef.current[m.conversationId] || savedLocal(m.conversationId)) throw new Error("同机私聊暂不支持编辑与撤回");
    const target = snapshotRef.current?.conversations.find(c => c.id === m.conversationId);
    if (target?.encryption) await mutateEncrypted(target, m, me!.id, withdraw, text);
    else await request(
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
    if (snapshotRef.current?.conversations.some(c => c.encryption)) {
      const messages = pageRef.current?.messages ?? [];
      setResults(messages.filter(m => !m.cryptoError && m.status !== "withdrawn" && m.text.toLocaleLowerCase().includes(query.toLocaleLowerCase())));
      setSearchMore(false);
      setNotice("加密搜索仅检查当前已解密页面；更早历史和其他窗口尚未建立本机索引。搜索词没有上传云端。");
      return;
    }
    const before =
      older && results.length
        ? `&beforePosition=${results[results.length - 1].position}`
        : "";
    const found = await requestPage(
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
    const raw = await request<Page & { conversationId: string }>(
      `/chat/messages/${m.id}/context`,
    );
    const conversation = snapshotRef.current?.conversations.find(c => c.id === raw.conversationId);
    const decoded = raw.messages.some(m => m.sealed) || conversation?.encryption ? await projectPage(raw, conversation) : raw;
    const data = { ...raw, ...decoded };
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
      enableEncryption={async () => { const target = snapshotRef.current?.conversations.find(c => c.id === active); if (target) { await enableEncryption(target, me!.id); await refresh(); } }}
      localChat={localChats[active]}
      cryptoUnlocked={!!cryptoDevice(me.id)}
      localSelected={!!localChats[active] || savedLocal(active)}
      prepareLocal={IS_DESKTOP ? async () => {
        if (recordRef.current.pending) throw new Error("请先确认原发送结果，再开启同机私聊");
        const target = snapshotRef.current?.conversations.find(c => c.id === active);
        if (!target?.peer) return;
        const e = epoch.current, key = localRouteKey(active);
        const data = await prepareLocal(active, target.peer.id);
        if (e !== epoch.current) return;
        window.localStorage.setItem(key, "local");
        acceptLocal(data, e);
      } : undefined}
      syncLocal={async () => { const e = epoch.current; acceptLocal(await syncLocal(active), e); }}
      stopLocal={async () => { const e = epoch.current; acceptLocal(await stopLocal(active), e); }}
      closeLocal={async () => {
        if (recordRef.current.pending || localRef.current[active]?.running || localRef.current[active]?.syncState !== "synced")
          throw new Error("请先完成本机回复并同步云端，再返回云端发送");
        const e = epoch.current, id = active, key = localRouteKey(active);
        const data = await requestPage(`/chat/conversations/${id}/messages`);
        if (e !== epoch.current) return;
        window.localStorage.removeItem(key);
        const next = { ...localRef.current }; delete next[id];
        localRef.current = next;
        setLocalChats(next);
        if (activeRef.current === id) setPage(data);
      }}
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
