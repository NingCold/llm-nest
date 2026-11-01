import "./local-ai-setup.css";
import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { cryptoDevice, deviceStatus, devices, recoveryConfirmed, trustDevice } from "./crypto/engine";
import { EncryptedResourcePanel } from "./crypto/ResourcePanel";

interface CryptoStatus { deviceId: string | null; fingerprint: string | null; curve: string | null; master: string | null; identityReady: boolean; recoverySaved: boolean; peerTrusted: boolean; ready: boolean }
export function LocalAiEncryption({ operatorId, aiId, bindingVersion, running, configuration, start, openChat }: { bindingVersion: string; operatorId: string; aiId: string; running: boolean; configuration: { provider: string; model: string; persona?: string | null; temperature?: number | null; maxTokens?: number | null; reasoningEffort?: string | null }; start(): Promise<boolean>; openChat(): Promise<unknown> }) {
  const [status, setStatus] = useState<CryptoStatus | null>(null);
  const [trusted, setTrusted] = useState(false);
  const [saved, setSaved] = useState(false);
  const [checked, setChecked] = useState(false);
  const [profile, setProfile] = useState({ saved: false, applied: false });
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const [human, setHuman] = useState<Awaited<ReturnType<typeof deviceStatus>> | null>(null);
  const identity = useRef("");
  const updateStatus = (value: CryptoStatus) => {
    const signature = JSON.stringify([value.deviceId, value.fingerprint, value.curve, value.master]);
    if (identity.current !== signature) { identity.current = signature; setSaved(false); setChecked(false); setTrusted(false); setProfile({ saved: false, applied: false }); }
    if (!value.recoverySaved) setSaved(false);
    setStatus(value);
  };
  const epoch = useRef(0);
  const deviceId = cryptoDevice(operatorId);
  const current = (ticket: number) => ticket === epoch.current && !!deviceId && cryptoDevice(operatorId) === deviceId;
  const args = { aiId, deviceId };
  const read = async (ticket: number) => {
    const value = await invoke<CryptoStatus>("local_agent_crypto_status", args);
    if (!current(ticket)) return;
    updateStatus(value);
    const local = await deviceStatus();
    if (!current(ticket)) return;
    setHuman(local);
    const known = await devices(aiId);
    if (current(ticket)) setTrusted(known.some(d => d.trusted && d.deviceId === value.deviceId && d.fingerprint === value.fingerprint && d.curve === value.curve && d.master === value.master));
  };
  useEffect(() => {
    const ticket = ++epoch.current; setBusy(false);
    if (!running && deviceId) void read(ticket).catch(e => { if (current(ticket)) setError(String(e)); });
    return () => { epoch.current++; };
  }, [operatorId, aiId, running, deviceId]);
  const act = async (work: (ticket: number) => Promise<void>) => {
    const ticket = epoch.current; setBusy(true); setError(""); setNotice("");
    try { await work(ticket); } catch (e) { if (current(ticket)) setError(e instanceof Error ? e.message : String(e)); }
    finally { if (current(ticket)) setBusy(false); }
  };
  if (!deviceId || !recoveryConfirmed(operatorId)) return <p role="status">请先在“我的账号 → 可信设备与恢复”解锁可信设备，并确认真人账号恢复文件已妥善保存，再继续配置 AI。</p>;
  const mutual = !!status?.identityReady && !!status.recoverySaved && !!status.peerTrusted && trusted;
  return <section className="cloud-ai-section">
    <h3>完成加密 AI 设置</h3>
    <ol><li>准备 AI 独立身份，保存恢复文件。</li><li>核对本机真人与 AI 指纹，建立双方信任。</li><li>保存加密角色与模型，启动并等待节点应用。</li><li>进入私聊，在聊天详情开启加密后发送消息。</li></ol>
    {error && <p className="social-error" role="alert">{error}</p>}{notice && <p role="status">{notice}</p>}
    {running ? <p role="status">Agent 已启动。请等待下方资料显示“节点已应用”；启动成功不代表已经可以加密聊天。修改身份和信任前请先停止 Agent。</p> : <>
      <button className="text-button" disabled={busy} onClick={() => void act(read)}>检查加密设置</button>
      {status && !status.identityReady && <div className="cloud-actions"><button className="social-button secondary" disabled={busy} onClick={() => void act(async ticket => { const value = await invoke<CryptoStatus>("local_agent_crypto_initialize", args); if (current(ticket)) { updateStatus(value); await read(ticket); } })}>准备首次加密身份</button><button className="social-button secondary" disabled={busy} onClick={() => void act(async ticket => { const imported = await invoke<boolean>("local_agent_crypto_recover", args); if (!current(ticket)) return; if (imported) await read(ticket); else setNotice("已取消导入，尚未恢复身份。"); })}>使用恢复文件恢复身份</button></div>}
      {status?.identityReady && <>
        <p>AI 已有独立身份。已有身份不会重新建立；需要迁移时导入原有恢复文件。</p>
        <button className="social-button secondary" disabled={busy} onClick={() => void act(async ticket => { const exported = await invoke<boolean>("local_agent_crypto_export", args); if (!current(ticket)) return; if (exported) { await read(ticket); if (current(ticket)) setNotice("恢复文件已保存，请确认已妥善保管。"); } else setNotice("已取消保存恢复文件。"); })}>保存 AI 恢复文件</button>
        <label className="cloud-check"><input type="checkbox" checked={saved} disabled={!status.recoverySaved || busy} onChange={e => setSaved(e.target.checked)} />我确认 AI 恢复文件已妥善保存</label>
        <details open={!mutual}><summary>核对本机 AI 独立指纹</summary><p>这些指纹直接来自本机 Agent。请核对 AI 账号与设备；确认后，本机真人设备与 AI 将互相信任。</p><p className="account-id">设备：{status.deviceId}</p><p className="account-id">Ed25519：{status.fingerprint}</p><p className="account-id">Curve25519：{status.curve}</p><p className="account-id">账号主指纹：{status.master}</p></details>
        {human && <details open={!mutual}><summary>核对当前真人设备指纹</summary><p>这些指纹来自你当前解锁的设备。</p><p className="account-id">设备：{human.deviceId}</p><p className="account-id">Ed25519：{human.fingerprint}</p><p className="account-id">Curve25519：{human.curve}</p><p className="account-id">账号主指纹：{human.master}</p></details>}
        <label className="cloud-check"><input type="checkbox" checked={checked} disabled={busy} onChange={e => setChecked(e.target.checked)} />已核对上述本机 AI 身份及全部指纹，同意与当前真人设备互相信任</label>
        <button className="social-button primary" disabled={busy || !human?.trusted || !saved || !checked || !status.fingerprint || !status.curve || !status.master} onClick={() => void act(async ticket => {
          const local = await deviceStatus();
          if (!current(ticket)) return;
          if (!local.trusted || !local.master || !human || local.deviceId !== human.deviceId || local.fingerprint !== human.fingerprint || local.curve !== human.curve || local.master !== human.master) throw new Error("请先完成真人设备的可信授权");
          const value = await invoke<CryptoStatus>("local_agent_crypto_trust", { ...args, device: { deviceId: human.deviceId, fingerprint: human.fingerprint, curve: human.curve, master: human.master } });
          if (!current(ticket)) return;
          updateStatus(value);
          await trustDevice(aiId, status.deviceId!, status.fingerprint!, status.curve!, status.master!);
          if (current(ticket)) await read(ticket);
        })}>确认双方信任</button>
      </>}
    </>}
    {(mutual || running) && <><EncryptedResourcePanel operatorId={operatorId} aiId={aiId} kind="profile" bindingVersion={bindingVersion} initialProfile={{ persona: configuration.persona ?? "", selection: { provider: configuration.provider, model: configuration.model, reasoning_effort: configuration.reasoningEffort }, temperature: configuration.temperature, max_tokens: configuration.maxTokens }} onProfileStatus={setProfile} />
      {!running && <button className="social-button primary" disabled={busy || !saved || !profile.saved} onClick={() => void act(async ticket => { if (!current(ticket)) return; await start(); })}>启动 Agent 并等待资料应用</button>}
      {profile.applied && running && <><p role="status">加密角色与模型已由节点应用。进入唯一私聊后，在聊天详情确认双方可信并开启加密。</p><button className="social-button primary" disabled={busy} onClick={() => void act(async ticket => { if (current(ticket)) await openChat(); })}>进入私聊，开启加密聊天</button></>}
    </>}
  </section>;
}
