import { useState } from "react";
import { ArrowLeft, Moon, Sun } from "lucide-react";
import { BrandMark } from "@/components/BrandMark";
import { CUSTOM_WINDOW_CHROME, IS_DESKTOP } from "@/lib/desktop";
import { useUiStore } from "@/store/ui";
import "../social/social.css";
import "./auth.css";

interface AuthPageProps {
  mode: string;
  onMode(mode: string): void;
  busy: boolean;
  connected: boolean;
  connecting: boolean;
  serverUrl: string;
  onServerUrl(value: string): void;
  onConnect(): void;
  onSubmit(form: HTMLFormElement): void;
  error: string;
  notice: string;
  canResume: boolean;
  onCorrectEmail(email: string): void;
  onResend(): void;
}

const copy: Record<string, { title: string; description: string; action: string }> = {
  login: { title: "欢迎回来", description: "登录你的账号，继续聊天。", action: "登录" },
  register: { title: "加入 LLM Nest", description: "目前需要邀请才能注册。", action: "注册并验证邮箱" },
  verify: { title: "验证你的邮箱", description: "打开验证邮件中的链接，或在下方填写验证码。", action: "验证并登录" },
  forgot: { title: "找回密码", description: "我们会向你的邮箱发送重置密码的邮件。", action: "发送重置邮件" },
  reset: { title: "设置新密码", description: "填写重置邮件中的验证码和新密码。", action: "保存新密码" },
};

export function AuthPage(props: AuthPageProps) {
  const { mode, onMode, busy, connected, connecting } = props;
  const theme = useUiStore(s => s.theme);
  const toggleTheme = useUiStore(s => s.toggleTheme);
  const [correctingEmail, setCorrectingEmail] = useState(false);
  const content = copy[mode];
  const params = new URLSearchParams(location.hash.split("?")[1] || location.search);
  const changeMode = (next: string) => { setCorrectingEmail(false); onMode(next); };

  return <main className={`social-shell auth-page${CUSTOM_WINDOW_CHROME ? " desktop-chrome" : ""}`}>
    <header className="auth-header" data-tauri-drag-region={CUSTOM_WINDOW_CHROME ? "deep" : undefined}>
      <span className="auth-brand"><BrandMark /><span>LLM Nest</span></span>
      <button type="button" className="social-icon-button" aria-label={theme === "light" ? "切换深色主题" : "切换浅色主题"} onClick={toggleTheme}>
        {theme === "light" ? <Moon /> : <Sun />}
      </button>
    </header>
    <div className="auth-body">
      <section className="auth-card" aria-labelledby="auth-heading">
        {mode !== "login" && <button className="auth-back" type="button" onClick={() => changeMode("login")}><ArrowLeft size={15} />返回登录</button>}
        <h1 id="auth-heading">{content.title}</h1>
        <p className="auth-description">{content.description}</p>
        {IS_DESKTOP && <form className="social-form auth-server" onSubmit={e => { e.preventDefault(); props.onConnect(); }}>
          <label>服务器地址<input type="url" required placeholder="https://您的服务器地址" value={props.serverUrl} disabled={busy || connecting} onChange={e => props.onServerUrl(e.target.value)} /></label>
          <button className="social-button secondary" disabled={busy || connecting}>{connecting ? "正在连接…" : connected ? "重新连接" : "连接服务器"}</button>
          {!connected && !connecting && <small>连接服务器后即可登录或注册。</small>}
        </form>}
        {props.error && <p className="auth-feedback auth-error" role="alert">{props.error}</p>}
        {props.notice && <p className="auth-feedback" role="status">{props.notice}</p>}
        <form className="social-form auth-form" key={mode} onSubmit={e => { e.preventDefault(); props.onSubmit(e.currentTarget); }}>
          {mode === "register" && <>
            <label>邀请码<input name="invite" required defaultValue={params.get("invite") || ""} autoComplete="off" /></label>
            <label>名字<input name="name" required maxLength={80} autoComplete="nickname" /></label>
          </>}
          {["login", "register", "forgot"].includes(mode) && <label>邮箱<input name="email" type="email" autoComplete="email" required placeholder="name@example.com" /></label>}
          {["login", "register", "reset"].includes(mode) && <div className="auth-password">
            <span className="auth-label-row"><label htmlFor="auth-password">密码</label>{mode === "login" && <button className="auth-link" type="button" onClick={() => changeMode("forgot")}>忘记密码？</button>}</span>
            <input id="auth-password" name="password" type="password" autoComplete={mode === "login" ? "current-password" : "new-password"} minLength={10} required aria-describedby={mode !== "login" ? "password-hint" : undefined} />
            {mode !== "login" && <small id="password-hint">至少 10 个字符</small>}
          </div>}
          {["verify", "reset"].includes(mode) && <label>邮件验证码<input name="token" required defaultValue={params.get("token") || ""} autoComplete="one-time-code" /></label>}
          <button className="social-button primary auth-submit" disabled={busy || !connected}>{busy ? "请稍候…" : content.action}</button>
        </form>
        {mode === "verify" && props.canResume && <div className="auth-verification">
          <div className="auth-link-row"><button className="auth-link" type="button" disabled={busy} onClick={props.onResend}>重新发送邮件</button><button className="auth-link" type="button" onClick={() => setCorrectingEmail(!correctingEmail)}>更正邮箱</button></div>
          {correctingEmail && <form className="social-form" onSubmit={e => { e.preventDefault(); props.onCorrectEmail(String(new FormData(e.currentTarget).get("email"))); }}><label>新的邮箱<input name="email" type="email" autoComplete="email" required /></label><button className="social-button secondary" disabled={busy}>更正并重发</button></form>}
        </div>}
        <nav className="auth-footer" aria-label="账号帮助">
          {mode === "login" && <><span>还没有账号？ <button className="auth-link" type="button" onClick={() => changeMode("register")}>使用邀请注册</button></span><button className="auth-link auth-secondary-link" type="button" onClick={() => changeMode("verify")}>继续验证邮箱</button></>}
          {mode === "register" && <span>已有账号？ <button className="auth-link" type="button" onClick={() => changeMode("login")}>登录</button></span>}
          {mode === "forgot" && <button className="auth-link" type="button" onClick={() => changeMode("reset")}>我已收到重置邮件</button>}
        </nav>
      </section>
    </div>
    <footer className="auth-page-footer">让联系自然发生</footer>
  </main>;
}
