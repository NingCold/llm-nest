import { useState } from "react";
import { LockKeyhole, Moon, Sun } from "lucide-react";
import { BrandMark } from "@/components/BrandMark";
import { CUSTOM_WINDOW_CHROME } from "@/lib/desktop";
import { useUiStore } from "@/store/ui";
import "../chat/social.css";
import "./lock-screen.css";

interface LockScreenProps {
  accountName: string;
  busy: boolean;
  error: string;
  onUnlock(passphrase: string): void;
  onRecovery(): void;
  onLogout(): void;
  autoUnlocking?: boolean;
}

export function LockScreen({ accountName, busy, error, onUnlock, onRecovery, onLogout, autoUnlocking = false }: LockScreenProps) {
  const [passphrase, setPassphrase] = useState("");
  const theme = useUiStore(s => s.theme);
  const toggleTheme = useUiStore(s => s.toggleTheme);
  return <main className={`social-shell lock-screen${CUSTOM_WINDOW_CHROME ? " desktop-chrome" : ""}`}>
    <div className="lock-screen-backdrop" aria-hidden="true"><div className="lock-backdrop-rail" /><div className="lock-backdrop-list" /><div className="lock-backdrop-main" /></div>
    <header className="lock-screen-header" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}>
      <span className="lock-screen-brand"><BrandMark /><span>LLM Nest</span></span>
      <button type="button" className="social-icon-button" aria-label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>{theme === "light" ? <Moon /> : <Sun />}</button>
    </header>
    <div className="lock-screen-body"><section className="lock-screen-card" aria-labelledby="lock-screen-heading" aria-busy={busy || autoUnlocking}>
      <span className="lock-screen-icon" aria-hidden="true"><LockKeyhole /></span>
      <h1 id="lock-screen-heading">解锁 LLM Nest</h1>
      <p className="lock-screen-account">{accountName}</p>
      <p className="lock-screen-description">本机加密资料已锁定，解锁后继续聊天。</p>
      {error && <p className="lock-screen-error" role="alert">{error}</p>}
      {autoUnlocking ? <p className="lock-screen-progress" role="status">正在解锁…</p> : <form className="social-form lock-screen-form" onSubmit={event => { event.preventDefault(); onUnlock(passphrase); setPassphrase(""); }}>
        <label htmlFor="lock-screen-passphrase">本机解锁口令</label>
        <input id="lock-screen-passphrase" name="passphrase" type="password" autoComplete="off" autoFocus required disabled={busy} value={passphrase} onChange={event => setPassphrase(event.target.value)} />
        <button type="submit" className="social-button primary" disabled={busy || !passphrase}>{busy ? "正在解锁…" : "解锁"}</button>
      </form>}
      <div className="lock-screen-actions"><button type="button" disabled={busy || autoUnlocking} onClick={onRecovery}>使用恢复文件</button><span aria-hidden="true">·</span><button type="button" disabled={busy || autoUnlocking} onClick={onLogout}>退出登录</button></div>
    </section></div>
    <footer className="lock-screen-footer">口令只用于解锁此设备的加密资料</footer>
  </main>;
}
