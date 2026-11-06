/** The shared SDK sees the authenticated native connection, never WebView fetch. */
export interface NativeBridge {
  context(): string;
  request(id: string, method: string, path: string, body: string): void;
  ready(): void;
  result(id: string, result: string): void;
  recovery(action: string): void;
  done(): void;
}
declare global {
  interface Window {
    NativeCrypto: NativeBridge;
    cryptoReply(id: string, reply: { value?: unknown; error?: { status?: number; message: string; code?: string } }): void;
  }
}
export const context = JSON.parse(window.NativeCrypto.context()) as { server: string; account: string };
export const server = context.server;
export class ConnectionError extends Error {
  readonly status = 0;
  constructor(message: string, public code?: string) { super(message); }
}
export class HttpError extends Error {
  constructor(public status: number, message: string, public code?: string) { super(message); }
}
let sequence = 0;
const pending = new Map<string, { resolve(value: unknown): void; reject(error: Error): void }>();
window.cryptoReply = (id, reply) => {
  const task = pending.get(id);
  if (!task) return;
  pending.delete(id);
  if (reply.error) {
    const { status, message, code } = reply.error;
    const network = ["connection_error", "dns_error", "timeout", "tls_error"].includes(code ?? "");
    task.reject(network ? new ConnectionError(message, code) : typeof status === "number" ? new HttpError(status, message, code) : new Error(message));
  } else task.resolve(reply.value);
};
export function request<T = unknown>(path: string, method = "GET", body?: unknown): Promise<T> {
  const id = String(++sequence);
  return new Promise<T>((resolve, reject) => {
    pending.set(id, { resolve: value => resolve(value as T), reject });
    window.NativeCrypto.request(id, method, path, body === undefined ? "" : JSON.stringify(body));
  });
}
