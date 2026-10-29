import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ArrowLeft } from "lucide-react";
import { CUSTOM_WINDOW_CHROME, IS_DESKTOP } from "@/lib/desktop";
import { useDesktopReady } from "@/hooks/useDesktopReady";
import { StartupSurface } from "@/components/StartupSurface";
import { AuthPage } from "./AuthPage";
import { LockScreen } from "./LockScreen";
import ChatApp, { type ChatLocation } from "./ChatApp";
import { CryptoDevices } from "./crypto/Devices";
import { cryptoDevice, hasCryptoDevice, lockCrypto } from "./crypto/engine";
import { forgetAutoUnlock, pauseAutoUnlock, tryAutoUnlock, unlockManually } from "./crypto/auto-unlock";
import { request, HttpError, connectServer, server, invalidate } from "./api";
import type { Account } from "./types";
import "./cloud.css";

const message = (error: unknown) => error instanceof Error ? error.message : String(error);

// Locking unmounts ChatApp and its subscriptions. Only account metadata and
// navigation survive; no decrypted chat remains behind the glass placeholders.
export function AccountSession({ account, onLogout, onEndSession }: {
  account: Account; onLogout(): Promise<void>; onEndSession(): void;
}) {
  const [checking, setChecking] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [recovery, setRecovery] = useState(!hasCryptoDevice(account.id));
  const [, changed] = useState(0);
  const automatic = useRef<Promise<boolean> | null>(null);
  const position = useRef<ChatLocation>({ active: "", tab: "messages", accountSection: "profile" });
  useEffect(() => {
    let alive = true;
    const update = () => changed(n => n + 1);
    window.addEventListener("llmn-crypto-change", update);
    // StrictMode subscriptions share one attempt to open the SDK store.
    automatic.current ??= cryptoDevice(account.id) ? Promise.resolve(true) : tryAutoUnlock(account.id);
    void automatic.current.catch(e => { if (alive) setError(message(e)); })
      .finally(() => { if (alive) setChecking(false); });
    return () => { alive = false; window.removeEventListener("llmn-crypto-change", update); };
  }, [account.id]);
  const act = async (work: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await work(); } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  const lock = () => { pauseAutoUnlock(account.id); lockCrypto(); setRecovery(false); setError(""); };
  if (checking) return <LockScreen accountName={account.name} busy autoUnlocking error="" onUnlock={() => {}} onRecovery={() => {}} onLogout={() => void act(onLogout)} />;
  if (recovery) return <main className={`social-shell session-recovery${CUSTOM_WINDOW_CHROME ? " desktop-chrome" : ""}`}>
    <div className="session-recovery-actions">
      <button className="social-button secondary" onClick={() => setRecovery(false)}><ArrowLeft size={16} />{cryptoDevice(account.id) ? "进入聊天" : "返回解锁"}</button>
      <button className="text-button" disabled={busy} onClick={() => void act(onLogout)}>退出登录</button>
    </div>
    {error && <p className="social-error" role="alert">{error}</p>}
    <CryptoDevices accountId={account.id} onLock={lock} />
  </main>;
  if (!cryptoDevice(account.id) || busy) return <LockScreen accountName={account.name} busy={busy} error={error}
    onUnlock={passphrase => void act(() => unlockManually(account.id, passphrase))}
    onRecovery={() => { setError(""); setRecovery(true); }} onLogout={() => void act(onLogout)} />;
  return <ChatApp account={account} location={position.current} rememberLocation={value => { position.current = { ...position.current, ...value }; }}
    onLock={lock} onLogout={onLogout} onEndSession={onEndSession} />;
}

export default function CloudApp() {
  const [me, setMe] = useState<Account | null>(null);
  const [ready, setReady] = useState(false);
  const [connected, setConnected] = useState(!IS_DESKTOP);
  const [connecting, setConnecting] = useState(false);
  const [rememberSupported, setRememberSupported] = useState(!IS_DESKTOP);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [url, setUrl] = useState(server);
  const [mode, setMode] = useState(location.hash.startsWith("#/verify") ? "verify" : location.hash.startsWith("#/reset") ? "reset" : location.hash.startsWith("#/register") ? "register" : "login");
  const [resume, setResume] = useState<{ registrationId: string; resumeToken: string } | null>(null);
  const account = useRef(me); account.current = me;
  const startup = useRef<Promise<{ account: Account | null; supported: boolean }> | null>(null);
  const endSession = () => {
    const id = account.current?.id;
    account.current = null;
    lockCrypto(); invalidate(); setMe(null);
    // Native rejection clears its protected files before returning the 401.
    if (id && !IS_DESKTOP) void forgetAutoUnlock(id).catch(e => setError(message(e)));
  };
  useDesktopReady(ready);
  useEffect(() => {
    let alive = true;
    const hash = () => { for (const value of ["register", "verify", "reset"]) if (location.hash.startsWith(`#/${value}`)) setMode(value); };
    const unauthorized = () => { if (account.current) endSession(); };
    window.addEventListener("hashchange", hash);
    window.addEventListener("cloud-unauthorized", unauthorized);
    window.addEventListener("cloud-crypto-lock", lockCrypto);
    const nativeAuth = IS_DESKTOP ? listen<{ server: string; accountId: string | null; message: string | null }>("cloud-auth-lost", event => {
      if (!alive || event.payload.server !== server || (event.payload.accountId && event.payload.accountId !== account.current?.id)) return;
      endSession();
      if (event.payload.message) setError(event.payload.message);
    }) : undefined;
    void nativeAuth?.catch(e => { if (alive) setError(message(e)); });
    setConnecting(IS_DESKTOP && !!server);
    startup.current ??= (async () => {
      if (IS_DESKTOP) return { supported: await invoke<boolean>("cloud_unlock_support"), account: server ? await connectServer<Account>(server) : null };
      try { return { supported: true, account: await request<Account>("/me") }; }
      catch (e) { if (e instanceof HttpError && e.status === 401) return { supported: true, account: null }; throw e; }
    })();
    void startup.current.then(value => {
      if (!alive) return;
      setRememberSupported(value.supported); setMe(value.account); setConnected(!IS_DESKTOP || !!server);
    }).catch(e => { if (alive) setError(message(e)); })
      .finally(() => { if (alive) { setConnecting(false); setReady(true); } });
    return () => {
      alive = false;
      window.removeEventListener("hashchange", hash);
      window.removeEventListener("cloud-unauthorized", unauthorized);
      window.removeEventListener("cloud-crypto-lock", lockCrypto);
      void nativeAuth?.then(stop => stop(), () => {});
    };
  }, []);
  const run = async (work: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await work(); } catch (e) { setError(message(e)); } finally { setBusy(false); }
  };
  const auth = async (form: HTMLFormElement) => {
    const fields = new FormData(form);
    const data = { ...Object.fromEntries(fields.entries()), rememberLogin: rememberSupported && fields.get("rememberLogin") === "on" };
    if (mode === "register") {
      setResume(await request("/auth/register", "POST", data));
      setMode("verify"); setNotice("验证邮件已发送，请填写邮件中的验证码。");
    } else if (mode === "login" || mode === "verify") {
      try {
        const result = await request<{ account: Account }>(`/auth/${mode}`, "POST", data);
        lockCrypto(); setMe(result.account); setNotice("");
      } catch (e) {
        if (mode === "login" && e instanceof HttpError && e.status === 401) throw new Error("邮箱或密码不正确，请检查后重试。");
        throw e;
      }
    } else {
      await request(`/auth/${mode}`, "POST", data);
      setNotice(mode === "forgot" ? "如邮箱存在，重置邮件已发送。" : "密码已重置，请登录。");
      if (mode === "reset") setMode("login");
    }
  };
  const logout = async () => {
    if (account.current) await forgetAutoUnlock(account.current.id);
    try { await request("/auth/logout", "POST"); }
    catch (e) { if (account.current) throw e; }
    if (account.current) endSession();
  };
  if (!ready) return <StartupSurface />;
  if (me) return <AccountSession key={`${server}/${me.id}`} account={me} onLogout={logout} onEndSession={() => void run(logout)} />;
  return <AuthPage mode={mode} onMode={value => { setMode(value); setError(""); }} busy={busy}
    connected={connected} connecting={connecting} rememberSupported={rememberSupported} serverUrl={url}
    onServerUrl={value => { setUrl(value); setResume(null); setConnected(false); setError(""); setNotice("地址已更改，请先连接服务器"); invalidate(); }}
    onConnect={() => { setConnecting(true); setConnected(false); void run(async () => {
      try { const value = await connectServer<Account>(url); setUrl(server); setConnected(true); setMe(value); setNotice(value ? "服务器已连接" : "服务器已连接，请登录"); }
      finally { setConnecting(false); }
    }); }}
    onSubmit={form => { if (!busy && connected) void run(() => auth(form)); }} error={error} notice={notice} canResume={!!resume}
    onCorrectEmail={email => void run(async () => { await request("/auth/registration", "PATCH", { ...resume, email }); setNotice("已向新邮箱发送验证邮件"); })}
    onResend={() => void run(async () => { await request("/auth/resend", "POST", resume); setNotice("已重发验证邮件"); })} />;
}
