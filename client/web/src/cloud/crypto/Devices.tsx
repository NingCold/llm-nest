import { useEffect, useState } from "react";
import { cryptoDevice, hasCryptoDevice, unlockCrypto, lockCrypto, deviceStatus, initializeIdentity, devices, trustDevice, authorizeDevice, acceptAuthorization, syncCrypto, createRecoveryFile, commitRecoveryFile, recoveryConfirmed, restoreRecoveryFile, type RecoveryFile } from "./engine";
import { request } from "../api";
import "./devices.css";

export function CryptoDevices({ accountId }: { accountId: string }) {
  const [status, setStatus] = useState<Awaited<ReturnType<typeof deviceStatus>> | null>(null);
  const [newDevice, setNewDevice] = useState(false);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [target, setTarget] = useState(accountId);
  const [listed, setListed] = useState<Awaited<ReturnType<typeof devices>>>([]);
  const [candidate, setCandidate] = useState<Awaited<ReturnType<typeof devices>>[number] | null>(null);
  const [trustChecked, setTrustChecked] = useState(false);
  const [recoveryChecked, setRecoveryChecked] = useState(false);
  const [expectedEd, setExpectedEd] = useState("");
  const [expectedCurve, setExpectedCurve] = useState("");
  const [expectedMaster, setExpectedMaster] = useState("");
  const [recovery, setRecovery] = useState<RecoveryFile | null>(null);
  const [downloaded, setDownloaded] = useState(false);
  const [, setRevision] = useState(0);
  const act = async (work: () => Promise<unknown>) => {
    setBusy(true); setError("");
    try { await work(); if (cryptoDevice(accountId)) setStatus(await deviceStatus()); }
    catch (e) { setError(e instanceof Error ? e.message : String(e)); }
    finally { setBusy(false); }
  };
  useEffect(() => {
    let alive = true;
    const update = () => { setRevision(v => v + 1); if (cryptoDevice(accountId)) void deviceStatus().then(value => { if (alive) setStatus(value); }); else setStatus(null); };
    update(); window.addEventListener("llmn-crypto-change", update);
    return () => { alive = false; window.removeEventListener("llmn-crypto-change", update); };
  }, [accountId]);
  const known = hasCryptoDevice(accountId);
  const saved = recoveryConfirmed(accountId);
  const ready = !!status?.trusted && saved;
  const locked = known && !status && !newDevice;
  const next = !status ? known && !newDevice ? "解锁本机资料后，即可读取加密聊天。" : "先创建本机设备，再建立加密身份并保存恢复文件。" : !status.master ? "下一步：建立账号的首次加密身份。" : !status.trusted ? "下一步：使用恢复文件或已有可信设备授权本机。" : !saved ? "下一步：保存恢复文件，并确认已妥善保管。" : "已准备好使用加密聊天。你可以在聊天详情中开启端到端加密。";
  const localDeviceCard = status && (<section className="crypto-card"><div className="crypto-card-heading"><h3>本机设备</h3><span className="crypto-status">{status.trusted ? "已授权" : "待授权"}</span></div><p className="crypto-device-id">设备 ID：{status.deviceId}</p>
      <details className="crypto-disclosure"><summary>查看完整设备指纹</summary><div className="crypto-fingerprints"><p>Ed25519</p><code>{status.fingerprint}</code><p>Curve25519</p><code>{status.curve}</code><p>账号主指纹</p><code>{status.master ?? "尚未建立"}</code></div></details>
      <div className="crypto-actions">
      <button className="social-button secondary" disabled={busy} onClick={() => { lockCrypto(); setStatus(null); setListed([]); setRecovery(null); setCandidate(null); }}>锁定本机资料</button>
      <button className="social-button secondary" disabled={busy} onClick={() => void act(syncCrypto)}>检查授权与密钥更新</button></div></section>);
  return <section className="contact-view crypto-devices"><header className="contact-header"><h2>可信设备与恢复</h2></header><div className="contact-scroll"><div className="profile-content crypto-devices-content">
    <div className="crypto-overview"><span className="crypto-eyebrow">{ready ? "加密已就绪" : status ? "继续设置" : known && !newDevice ? "本机资料已锁定" : "首次设置"}</span><h3>{ready ? "本机设备已获可信授权" : locked ? "解锁本机资料" : "准备可信设备"}</h3><p role="status">{next}</p>{!ready && !locked && <ol className="crypto-steps" aria-label="首次设置进度"><li data-complete={!!status}>创建本机设备</li><li data-complete={!!status?.master}>建立加密身份</li><li data-complete={saved}>保存恢复文件</li></ol>}</div>
    {error && <p role="alert" className="social-error">{error}</p>}
    {!status ? <form className="cloud-form crypto-card crypto-current-step" onSubmit={e => { e.preventDefault(); const form = e.currentTarget; const passphrase = String(new FormData(form).get("passphrase")); void act(async () => { await unlockCrypto(accountId, passphrase, newDevice ? "new-device" : !known); form.reset(); setNewDevice(false); }); }}>
      <h3>{newDevice ? "建立另一独立设备" : known ? "解锁本机加密资料" : "创建本机设备"}</h3><label>独立本机解锁口令<input name="passphrase" type="password" autoComplete="off" required minLength={8} /></label>
      <p>口令只保护本机加密资料，不上传服务器。请使用至少 8 位的独立口令；登录密码不能恢复聊天密钥。</p>
      <button className="social-button primary" disabled={busy}>{known && !newDevice ? "解锁本机资料" : "确认创建独立设备"}</button>
      {known && !newDevice && <button type="button" className="text-button" disabled={busy} onClick={() => { if (window.confirm("原本机加密库会保留。将建立新的设备身份，必须使用恢复文件或现有可信设备重新授权；不会重置账号加密身份。")) setNewDevice(true); }}>无法解锁？明确建立新设备后恢复</button>}
    </form> : <>
      {ready && localDeviceCard}
      {status.authorizationAvailable && <div className="crypto-card crypto-current-step" role="status"><p>已收到通过指纹核对的设备提供的加密恢复授权。</p><button className="social-button primary" disabled={busy} onClick={() => void act(acceptAuthorization)}>确认授权并恢复此设备</button></div>}
      {!status.master && <div className="crypto-card crypto-current-step"><h3>建立加密身份</h3><p>此账号尚未建立加密身份。首次创建后须保存恢复文件；已有身份不会被自动替换。</p><button className="social-button primary" disabled={busy} onClick={() => void act(initializeIdentity)}>确认建立首次加密身份</button></div>}
      <section className={`crypto-card ${status.master && !saved ? "crypto-current-step" : ""}`}><div className="crypto-card-heading"><h3>恢复文件</h3><span className="crypto-status">{saved ? "已确认保存" : "待保存"}</span></div><p>{saved ? "此设备已确认保存或使用过恢复文件。" : "尚未确认恢复材料，请先保存，再开启加密私聊。"}</p>
      <button className={`social-button ${saved ? "secondary" : "primary"}`} disabled={busy || !status.hasRecoverySecrets} onClick={() => void act(async () => { setCandidate(null); setRecovery(await createRecoveryFile()); setDownloaded(false); setRecoveryChecked(false); })}>生成并保存恢复文件</button>
      {recovery && <div className="crypto-confirmation" role="status"><p>文件包含恢复秘密，持有它的人可恢复此账号的聊天。不要发给平台、聊天对象或普通文件导出。</p><button className="social-button primary" disabled={busy} onClick={() => { const url = URL.createObjectURL(new Blob([JSON.stringify(recovery, null, 2)], { type: "application/json" })); const a = document.createElement("a"); a.href = url; a.download = `llmn-recovery-${accountId}.json`; a.click(); URL.revokeObjectURL(url); setDownloaded(true); }}>下载恢复文件</button><label className="cloud-check"><input type="checkbox" checked={recoveryChecked} onChange={e => setRecoveryChecked(e.target.checked)} />我已在安全位置保存文件，理解平台无法代为恢复</label><button className="social-button secondary" disabled={busy || !downloaded || !recoveryChecked} onClick={() => void act(async () => { await commitRecoveryFile(recovery); setRecovery(null); setRevision(v => v + 1); })}>确认恢复文件已保存</button></div>}
      <details className="crypto-disclosure" open={!status.trusted || undefined}><summary>从恢复文件授权此设备</summary><label className="cloud-form">选择已保存的恢复文件<input type="file" accept="application/json,.json" disabled={busy} onChange={e => { const file = e.target.files?.[0]; e.target.value = ""; if (file) void act(async () => { setRecovery(null); setCandidate(null); await restoreRecoveryFile(JSON.parse(await file.text())); }); }} /></label>
      </details></section>
      {!ready && localDeviceCard}
      <details className="crypto-card crypto-other-devices"><summary>核对与管理其他设备</summary><h3>查询可信设备</h3><p>通过独立渠道核对完整指纹，再授权；服务器提供的名字不能证明设备身份。</p>
      <form className="cloud-form" onSubmit={e => { e.preventDefault(); void act(async () => setListed(await devices(target))); }}><label>账号 ID<input value={target} onChange={e => { setTarget(e.target.value); setListed([]); setCandidate(null); }} required /></label><button className="social-button secondary" disabled={busy}>查询设备指纹</button></form>
      {listed.map(d => <article className="crypto-listed-device" key={d.deviceId}><strong>{d.name} · {d.trusted ? "已信任" : "待核对"}</strong><small>{d.deviceId}</small><details className="crypto-disclosure"><summary>查看设备指纹</summary><div className="crypto-fingerprints"><p>Ed25519</p><code>{d.fingerprint}</code><p>Curve25519</p><code>{d.curve}</code><p>账号主指纹</p><code>{d.master}</code></div></details><button className="text-button" disabled={busy || d.trusted} onClick={() => { setRecovery(null); setCandidate(d); setTrustChecked(false); setExpectedEd(""); setExpectedCurve(""); setExpectedMaster(""); }}>核对并授权</button>{d.own && d.deviceId !== status.deviceId && d.trusted && status.hasRecoverySecrets && <button className="text-button" disabled={busy} onClick={() => void act(() => authorizeDevice(d.deviceId))}>向此设备发送加密恢复授权</button>}{d.own && <button className="text-button" disabled={busy} onClick={() => void act(async () => { await request(`/crypto/devices/${d.deviceId}/revoke`, "POST", {}); if (d.deviceId === status.deviceId) { lockCrypto(); setStatus(null); } setListed([]); })}>撤销设备</button>}</article>)}
      {candidate && <div className="crypto-confirmation cloud-form"><h4>确认设备指纹</h4><code className="account-id">{candidate.fingerprint}</code><code className="account-id">{candidate.curve}</code><code className="account-id">{candidate.master}</code><label>独立渠道取得的账号主指纹<input value={expectedMaster} onChange={e => setExpectedMaster(e.target.value.trim())} autoComplete="off" /></label><label>独立渠道取得的 Ed25519 指纹<input value={expectedEd} onChange={e => setExpectedEd(e.target.value.trim())} autoComplete="off" /></label><label>独立渠道取得的 Curve25519 指纹<input value={expectedCurve} onChange={e => setExpectedCurve(e.target.value.trim())} autoComplete="off" /></label><label className="cloud-check"><input type="checkbox" checked={trustChecked} onChange={e => setTrustChecked(e.target.checked)} />已通过独立可信渠道核对全部指纹</label><button className="social-button primary" disabled={busy || !trustChecked || expectedEd !== candidate.fingerprint || expectedCurve !== candidate.curve || expectedMaster !== candidate.master} onClick={() => void act(async () => { await trustDevice(target, candidate.deviceId, expectedEd, expectedCurve, expectedMaster); setCandidate(null); setListed(await devices(target)); })}>确认信任此设备</button><button type="button" className="text-button" disabled={busy} onClick={() => setCandidate(null)}>取消核对</button></div>}
    </details></>}
  </div></div></section>;
}
