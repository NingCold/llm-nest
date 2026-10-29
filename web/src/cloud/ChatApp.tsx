import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { IS_DESKTOP } from "@/lib/desktop";
import { clearConsciousnessCommands } from "./Consciousness";
import { clearAiConfigurationCommands } from "./AiConfiguration";
import { clearParticipationCommands } from "./Participation";
import { Workspace } from "./Workspace";
import { DraftDatabase } from "../chat/draft-storage";
import { emptyDraft } from "../chat/drafts";
import { readImages } from "../chat/images";
import { readFiles } from "../chat/files";
import {
  request,
  DownloadCancelled,
  HttpError,
  server,
  subscribe,
} from "./api";
import type { Account, Conversation, Snapshot, Page, Message, CloudDraftRecord as DraftRecord, DraftReference, ReferencePreview } from "./types";
import "./cloud.css";
import { localView, prepareLocal, prepareLocalReference, sendLocal, syncLocal, stopLocal, localPage, type LocalView } from "./local-api";
import { prepareAttachments } from "./crypto/attachments";
import { completeHistory } from "./crypto/history";
import { encryptMessage, projectPage, projectSnapshot, enableEncryption } from "./crypto/content";
import { cryptoDevice, localKeyMessages, receiveLocalKeys } from "./crypto/engine";
import { synchronizeResources } from "./crypto/resources";
import { mutateEncrypted } from "./crypto/mutations";
const db = new DraftDatabase<DraftReference>();
const blank = (): DraftRecord => ({ draft: emptyDraft<DraftReference>(), pending: null });
export interface ChatLocation { active: string; tab: string; accountSection: string; scrollTop?: number }
interface ChatAppProps {
  account: Account;
  location: ChatLocation;
  rememberLocation(location: Partial<ChatLocation>): void;
  onLock(): void;
  onLogout(): Promise<void>;
  onEndSession(): void;
}
export default function ChatApp(props: ChatAppProps) {
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
  const [me, setMe] = useState<Account>(props.account),
    [error, setError] = useState(""),
    [notice, setNotice] = useState("");
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null),
    [active, setActive] = useState(props.location.active),
    [tab, setTab] = useState(props.location.tab),
    [page, setPage] = useState<Page | null>(null),
    [query, setQuery] = useState(""),
    [results, setResults] = useState<Message[]>([]),
    [searchMore, setSearchMore] = useState(false);
  const [record, setRecord] = useState<DraftRecord>(blank),
    [draftReady, setDraftReady] = useState(false),
    [busy, setBusy] = useState(false);
  const [preview, setPreview] = useState<string>();
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
    suspending = useRef(false),
    dirty = useRef(false),
    acknowledging = useRef(false),
    bottom = useRef(props.location.scrollTop === undefined);
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
  useEffect(() => () => {
    epoch.current++; view.current++; searchSeq.current++; refreshSeq.current++;
    clearConsciousnessCommands(); clearAiConfigurationCommands(); clearParticipationCommands();
  }, []);
  useEffect(() => { props.rememberLocation({ active, tab }); }, [active, tab]);
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
  useEffect(() => {
    if (!me) return;
    const update = () => {
      if (!cryptoDevice(me.id)) {
        return;
      }
      void run(async () => { await refresh(); await syncHistory(); });
    };
    const timer = setInterval(() => { if (cryptoDevice(me.id)) void synchronizeResources(me.id).catch(report); }, 3000);
    const warning = (event: Event) => report(new Error((event as CustomEvent<string>).detail));
    window.addEventListener("llmn-crypto-change", update);
    window.addEventListener("llmn-crypto-warning", warning);
    return () => { clearInterval(timer); window.removeEventListener("llmn-crypto-change", update); window.removeEventListener("llmn-crypto-warning", warning); };
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
    if (suspending.current) return;
    void save({
      ...recordRef.current,
      draft: { ...recordRef.current.draft, text, requestId: null },
    });
  };
  const attach = (files: File[], images: boolean) => {
    if ((localRef.current[activeRef.current] || savedLocal(activeRef.current)) && !snapshotRef.current?.conversations.find(c => c.id === activeRef.current)?.encryption) { report(new Error("同机附件需要先启用端到端加密")); return; }
    if (!draftReady || busy || recordRef.current.pending || closing.current || suspending.current)
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
      if (!draftReady || busy || suspending.current || target?.directBlocked || target?.readOnly) return;
      setBusy(true);
      try {
        if (snapshotRef.current?.requireEncryption && !target?.encryption) throw new Error("服务器已要求加密，请先准备可信设备并开启此窗口加密");
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
        if (pending.route === "local" && pending.mentions?.length) throw new HttpError(422, "同机私聊不支持群提及，请移除提及后发送");
        if (target?.encryption && !current.pending) {
          if (pending.route === "local" && pending.reference) await prepareLocalReference(target.id, pending.reference.selection);
          const attachments = prepareAttachments(pending.images, pending.files);
          pending.encryptedAttachments = attachments.encryptedAttachments;
          pending.expectedConversationRevision = target.revision;
          pending.messageId = crypto.randomUUID();
          pending.sealed = await encryptMessage(target, me!.id, pending.messageId, pending.text, "1", pending.replyTo ?? null, pending.mentions ?? [], pending.route === "local", attachments.descriptors, pending.reference?.selection ?? null);
          if (pending.route === "local") pending.keyMessages = localKeyMessages(target.members.map(m => m.account.id));
        }
        if (target?.encryption && !pending.sealed) throw new Error("当前窗口已经启用加密，原明文待确认发送不能降级或重新加密重试，请先核对旧发送结果");
        await save({ ...current, pending });
        try {
          if (pending.route === "local") {
            if (!pending.sealed && (pending.images?.length || pending.files?.length)) throw new HttpError(422, "同机附件需要先启用端到端加密");
            const data = await sendLocal(id, pending.requestId, pending.sealed ? "" : pending.text, pending.sealed, pending.messageId, pending.keyMessages, { encryptedAttachments: pending.encryptedAttachments, replyTo: pending.replyTo, reference: pending.reference?.selection, expectedConversationRevision: pending.expectedConversationRevision });
            if (e !== epoch.current) return;
            acceptLocal(data, e);
          } else await request(`/chat/conversations/${id}/messages`, "POST", {
            requestId: pending.requestId,
            ...(pending.sealed ? { messageId: pending.messageId, sealed: pending.sealed, text: "", encryptedAttachments: pending.encryptedAttachments, expectedConversationRevision: pending.expectedConversationRevision } : { text: pending.text }),
            mentions: pending.mentions ?? [],
            replyTo: pending.replyTo ?? null,
            ...(pending.reference ? { reference: pending.reference.selection } : {}),
            images: pending.sealed ? [] : pending.images?.map(({ mime, data }) => ({ mime, data })),
            files: pending.sealed ? [] : pending.files?.map(({ name, mime, data }) => ({
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
    if (localRef.current[direct.id] || savedLocal(direct.id)) await prepareLocalReference(direct.id, reference.selection);
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
      const candidates = snapshotRef.current.conversations.filter(c => scope !== "current" || c.id === id);
      const current = () => sequence === searchSeq.current && v === view.current && id === activeRef.current;
      const messages = (await Promise.all(candidates.map(c => completeHistory(c, current)))).flat();
      if (!current()) return;
      setResults(messages.filter(m => !m.cryptoError && m.status !== "withdrawn" && m.text.toLocaleLowerCase().includes(query.toLocaleLowerCase())).sort((a, b) => b.createdAt.localeCompare(a.createdAt)));
      setSearchMore(false);
      const failed = messages.filter(m => m.cryptoError).length;
      setNotice(failed ? `完整历史已读取；${failed} 条消息尚不可解密，未纳入搜索。搜索词没有上传云端。` : "已搜索所有授权历史，搜索词没有上传云端。");
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
  const suspend = async (action: () => void | Promise<void>) => {
    if (suspending.current) return;
    suspending.current = true; setBusy(true);
    try {
      await Promise.all(pendingReads.current);
      await writes.current;
      await action();
    } finally { suspending.current = false; setBusy(false); }
  };
  const logout = () => suspend(props.onLogout);
  const lock = () => suspend(() => {
    const timeline = document.querySelector(".cloud-messages");
    props.rememberLocation({ active, tab, scrollTop: timeline?.scrollTop });
    props.onLock();
  });
  return (
    <Workspace
      me={me}
      lock={() => void run(lock)}
      initialAccountSection={props.location.accountSection}
      onAccountSectionChange={(accountSection) => props.rememberLocation({ accountSection })}
      restoreScrollTop={props.location.scrollTop}
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
      busy={busy || suspending.current}
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
      save={next => {
        if (suspending.current) return Promise.reject(new Error("正在锁定或退出，请稍候"));
        return save(next);
      }}
      send={send}
      recoverDraft={recoverDraft}
      onBottom={(atBottom) => {
        bottom.current = atBottom;
        if (atBottom) acknowledge();
      }}
      onProfile={setMe}
      logout={logout}
      endSession={props.onEndSession}
      onPreview={setPreview}
      dismissError={() => setError("")}
    />
  );
}
