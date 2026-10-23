import { invoke, Channel } from "@tauri-apps/api/core";
import { IS_DESKTOP } from "@/lib/desktop";
export class DownloadCancelled extends Error {}
export class HttpError extends Error {
  constructor(
    public status: number,
    message: string,
    public code?: string,
  ) {
    super(message);
  }
}
function responseError(status: number, text: string) {
  if (status === 401) window.dispatchEvent(new Event("cloud-unauthorized"));
  let message = text;
  let code: string | undefined;
  try {
    const body = JSON.parse(text);
    message = body.message || body.error || text;
    code = body.code;
  } catch {}
  return new HttpError(status, message, code);
}
export let server = IS_DESKTOP
  ? localStorage.getItem("llmn-cloud-server") || ""
  : location.origin;
let generation = 0;
export function invalidate() {
  generation++;
}
export async function configure(url: string) {
  await invoke("cloud_set_server", { url });
  server = new URL(url).origin;
  localStorage.setItem("llmn-cloud-server", server);
  invalidate();
}
// A valid server may be reachable without an authenticated session.
export async function connectServer<T>(url: string): Promise<T | null> {
  await configure(url);
  try {
    return await request<T>("/me");
  } catch (error) {
    if (error instanceof HttpError && error.status === 401) return null;
    throw error;
  }
}
export async function request<T = unknown>(
  path: string,
  method = "GET",
  body?: unknown,
): Promise<T> {
  const epoch = generation;
  let status: number, text: string;
  if (IS_DESKTOP) {
    const r = await invoke<{ status: number; body: string }>("cloud_request", {
      method,
      path: `/api${path}`,
      body,
    });
    status = r.status;
    text = r.body;
  } else {
    const r = await fetch(`/api${path}`, {
      method,
      credentials: "include",
      headers: {
        ...(body ? { "Content-Type": "application/json" } : {}),
        ...(method !== "GET" ? { "X-LLMN-Client": "1" } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    });
    status = r.status;
    text = await r.text();
  }
  if (epoch !== generation) throw new Error("账号已切换");
  if (status >= 400) throw responseError(status, text);
  return (text ? JSON.parse(text) : null) as T;
}
export function subscribe(
  cursor: string,
  change: () => Promise<string | void> | void,
  error: (e: unknown) => void,
) {
  let closed = false,
    id: string | undefined,
    source: EventSource | undefined,
    timer: ReturnType<typeof setTimeout> | undefined,
    buffer = "",
    refreshing = false,
    queued = false;
  const notify = async () => {
    if (closed) return;
    if (refreshing) {
      queued = true;
      return;
    }
    refreshing = true;
    try {
      const next = await change();
      if (next) cursor = next;
    } catch (e) {
      error(e);
      timer = setTimeout(() => void notify(), 3000);
    } finally {
      refreshing = false;
      if (queued) {
        queued = false;
        void notify();
      }
    }
  };
  const reconnect = () => {
    if (closed) return;
    source?.close();
    source = undefined;
    void notify();
    timer = setTimeout(() => void connect(), 3000);
  };
  const consume = (chunk: string) => {
    if (closed) return;
    if (chunk === "") {
      id = undefined;
      reconnect();
      return;
    }
    buffer += chunk.replace(/\r\n/g, "\n");
    let end: number;
    while ((end = buffer.indexOf("\n\n")) >= 0) {
      const event = buffer.slice(0, end);
      buffer = buffer.slice(end + 2);
      if (event.includes("data:")) void notify();
    }
  };
  const connect = async () => {
    if (closed) return;
    try {
      if (IS_DESKTOP) {
        buffer = "";
        const channel = new Channel<string>();
        channel.onmessage = consume;
        id = await invoke<string>("cloud_subscribe", {
          path: `/api/chat/events?after=${cursor}`,
          onChunk: channel,
        });
        if (closed) await invoke("cloud_unsubscribe", { id });
        else void notify();
      } else {
        source = new EventSource(`/api/chat/events?after=${cursor}`, {
          withCredentials: true,
        });
        source.addEventListener("change", () => void notify());
        source.onopen = () => void notify();
        source.onerror = reconnect;
      }
    } catch (e) {
      error(e);
      timer = setTimeout(() => void connect(), 3000);
    }
  };
  void connect();
  return () => {
    closed = true;
    clearTimeout(timer);
    source?.close();
    if (id) void invoke("cloud_unsubscribe", { id });
  };
}
export async function download(
  path: string,
  name: string,
  preview = false,
): Promise<string | undefined> {
  const epoch = generation;
  if (IS_DESKTOP && !preview) {
    const result = await invoke<{ saved: boolean }>("cloud_save_download", {
      path: `/api${path}`,
    });
    if (!result.saved) throw new DownloadCancelled("已取消保存");
    return;
  }
  let blob: Blob;
  if (IS_DESKTOP) {
    const r = await invoke<{
      status: number;
      body: string;
      contentType: string;
    }>("cloud_request", { method: "GET", path: `/api${path}` });
    if (epoch !== generation) throw new Error("账号已切换");
    if (r.status >= 400) throw responseError(r.status, r.body);
    blob = new Blob([Uint8Array.from(atob(r.body), (c) => c.charCodeAt(0))], {
      type: r.contentType,
    });
  } else {
    const r = await fetch(`/api${path}`, { credentials: "include" });
    if (!r.ok) {
      const text = await r.text();
      if (epoch !== generation) throw new Error("账号已切换");
      throw responseError(r.status, text);
    }
    blob = await r.blob();
  }
  if (epoch !== generation) throw new Error("账号已切换");
  const url = URL.createObjectURL(blob);
  if (preview) return url;
  const a = document.createElement("a");
  a.href = url;
  a.download = name;
  a.click();
  setTimeout(() => URL.revokeObjectURL(url), 1000);
}
