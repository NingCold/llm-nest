import { useEffect, useRef, useState } from "react";
import { HttpError } from "../api";
import { cryptoDevice, devices, type RecoveryFile } from "./engine";
import { legacyControl, reconcileLegacyControl, type RecoverySelection, beginMigration, confirmMigrationResult, convertMigrationPage, digest, migrationSession, previewMigration, refreshMigration, retryMigration, verifyAndFinishMigration, type MigrationPreview, type MigrationSession } from "./migration";
import type { Conversation } from "../types";

export function HistoryMigrationPanel({ account, conversation, blocked, onComplete }: { account: string; conversation: Conversation; blocked: boolean; onComplete(): Promise<unknown> }) {
  const [waiting, setWaiting] = useState<string[]>([]);
  const [preview, setPreview] = useState<MigrationPreview>();
  const [session, setSession] = useState<MigrationSession>({});
  const [backup, setBackup] = useState("");
  const [file, setFile] = useState<RecoveryFile>();
  const [recoveryDigest, setRecoveryDigest] = useState("");
  const [confirmed, setConfirmed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const [supplement, setSupplement] = useState<RecoverySelection>();
  const generation = useRef(0);
  const originalInput = useRef<HTMLInputElement>(null);
  const supplementInput = useRef<HTMLInputElement>(null);
  const clearSupplement = () => {
    setSupplement(undefined);
    if (supplementInput.current) supplementInput.current.value = "";
  };
  const clearConfirmation = () => {
    setPreview(undefined);
    setBackup("");
    setConfirmed(false);
    setFile(undefined);
    setRecoveryDigest("");
    if (originalInput.current) originalInput.current.value = "";
    clearSupplement();
  };
  const selectFile = async (input: HTMLInputElement, extra: boolean) => {
    const picked = input.files?.[0];
    const ticket = generation.current;
    if (extra) setSupplement(undefined);
    else {
      setFile(undefined);
      setRecoveryDigest("");
      clearSupplement();
    }
    if (!picked) return;
    setBusy(true);
    setError("");
    try {
      const bytes = new Uint8Array(await picked.arrayBuffer());
      const parsed: RecoveryFile = JSON.parse(new TextDecoder().decode(bytes));
      if (!parsed || !["llmn-recovery-v1", "llmn-recovery-v2"].includes(parsed.format) || !parsed.version)
        throw new Error("请选择恢复文件");
      if (extra && parsed.format !== "llmn-recovery-v2") throw new Error("请选择当前 v2 恢复文件");
      const fingerprint = await digest(bytes);
      if (ticket !== generation.current) return;
      if (extra) setSupplement({ file: parsed, digest: fingerprint });
      else {
        setFile(parsed);
        setRecoveryDigest(fingerprint);
      }
    } catch (error) {
      if (ticket !== generation.current) return;
      input.value = "";
      setError(error instanceof Error ? error.message : String(error));
    } finally {
      if (ticket === generation.current) setBusy(false);
    }
  };
  const permitted = conversation.kind !== "group" || conversation.members.some(m => m.account.id === account && m.role !== "member");
  useEffect(() => {
    const ticket = ++generation.current;
    clearConfirmation();
    setBusy(false);
    setError("");
    setSession({});
    setWaiting([]);
    if (permitted) void (async () => {
      const saved = await migrationSession(account, conversation.id);
      if (ticket !== generation.current) return;
      setSession(saved);
      const current = await refreshMigration(account, conversation.id);
      if (ticket === generation.current) setSession(current);
    })().catch(e => { if (ticket === generation.current) setError(e instanceof Error ? e.message : String(e)); });
    return () => { generation.current++; };
  }, [account, conversation.id, permitted]);
  const act = async (work: () => Promise<unknown>) => { const ticket = generation.current; setBusy(true); setError(""); try { await work(); if (ticket === generation.current) { setSession(await migrationSession(account, conversation.id)); await onComplete(); } } catch (e) { if (ticket === generation.current) { setSession(await migrationSession(account, conversation.id)); setError((e instanceof HttpError && e.status === 409 ? "转换发生冲突，原请求仍保留；请核对当前成员和来源，不能覆盖转换。" : "") + (e instanceof Error ? e.message : String(e))); } } finally { if (ticket === generation.current) setBusy(false); } };
  if (!permitted) return <section className="cloud-detail-section"><h3>转换旧历史</h3><p role="status">群历史转换须由当前群主或管理员操作。</p></section>;
  const noOpen = !session.status || session.status.state === "conflicted";
  const disabled = busy || blocked || !cryptoDevice(account) || !permitted;
  return <section className="cloud-detail-section"><h3>转换旧历史</h3>
    <p>保留原作者、顺序和只读归档状态。由实际转换者确认旧记录，不代表原作者重新发送。开始后窗口冻结，直至用恢复文件重新读取核对完成。仅向当前已核验成员设备投递密钥；尚无可信设备的成员须先授权后再分享历史。</p>
    {blocked && <p role="status">请先确认原发送结果、完成本机执行并同步全部本机记录，不能用历史转换重发旧请求。</p>}
    {!!waiting.length && <p role="status">等待可信设备授权及历史分享：{waiting.join("、")}</p>}
    {!!session.rejected?.length && <p role="status">冲突或明确拒绝的原转换请求已保留；请重新预览并核对一致备份，不能用新内容覆盖旧请求。</p>}
    <button className="text-button" disabled={busy} onClick={() => void act(() => refreshMigration(account, conversation.id))}>核对当前转换状态</button>
    {error && <p className="social-error" role="alert">{error}</p>}
    {!!session.archivedControls?.length && <p role="status">原旧版请求已保存在本机。新操作须重新确认。</p>}
    {legacyControl(session.pending) ? <div role="status"><p>旧版请求须先核对回执。已提交的会恢复进度；未提交的将归档，不会重新发送。</p><button className="text-button" disabled={busy} onClick={() => void act(async () => { await reconcileLegacyControl(account, conversation.id); clearConfirmation(); })}>核对并归档旧版请求</button></div> : session.pending && <div role="status"><p>原转换请求结果尚未确认；重试使用已保存的相同请求与密文。</p><button className="text-button" disabled={disabled} onClick={() => void act(() => retryMigration(account, conversation.id))}>手动重试原转换请求</button><button className="text-button" disabled={busy} onClick={() => void act(() => confirmMigrationResult(account, conversation.id))}>核对原转换结果</button></div>}
    {conversation.encryption?.historyComplete || session.status?.state === "complete" ? <p role="status">旧历史已转换，并已用恢复文件重新读取核对。</p> : <>
      {noOpen && !session.pending && <button className="text-button" disabled={disabled} onClick={() => void act(async () => { setPreview(await previewMigration(conversation.id)); const recipients = await Promise.all(conversation.members.map(async m => (await devices(m.account.id)).some(d => d.trusted) ? null : m.account.name)); setWaiting(recipients.filter((name): name is string => name !== null)); })}>预览转换范围</button>}
      {preview && noOpen && <>{preview.blockers && <p role="status">待处理：AI 任务 {preview.blockers.nodeTasks} · 摘要任务 {preview.blockers.summaryCalls} · 生成中消息 {preview.blockers.generatingMessages} · 未结束约定 {preview.blockers.contactIntents}</p>}{preview.canBegin === false && <p role="status">请先处理未完成事项；已有转换时，请核对状态并继续原转换。</p>}<p>消息 {preview.messageCount} 条 · 约定 {preview.contactIntentCount} 条 · 附件 {preview.attachmentCount} 个</p><small>来源摘要：{preview.sourceDigest}</small><label className="cloud-form">一致备份摘要<input aria-label="一致备份摘要" value={backup} onChange={e => setBackup(e.target.value.trim().toLowerCase())} placeholder="备份脚本提供的 SHA-256" disabled={busy || !!session.pending} /></label><p>须先备份服务数据库、附件和节点工作库。普通聊天导出不能替代一致备份；旧明文备份不会自动删除。</p><label className="cloud-check"><input type="checkbox" checked={confirmed} onChange={e => setConfirmed(e.target.checked)} disabled={busy || !!session.pending} />我已核对一致备份与预览，理解原明文备份仍需另行保管和清理</label></>}
      <label className="cloud-form">{session.status?.state === "active" ? "选择开始转换时的恢复文件" : "选择当前账号恢复文件"}
        <input ref={originalInput} type="file" accept="application/json,.json" disabled={busy}
          onChange={e => void selectFile(e.currentTarget, false)} />
      </label>
      {session.status?.state === "active" && session.status.remaining === 0 && !session.pending && file && <details className="crypto-disclosure">
        <summary>原备份已封存？补选当前 v2 恢复文件</summary>
        <label className="cloud-form">当前 v2 恢复文件
          <input ref={supplementInput} type="file" accept="application/json,.json" disabled={busy}
            onChange={e => void selectFile(e.currentTarget, true)} />
        </label>
        {supplement && <button className="text-button" disabled={busy} onClick={clearSupplement}>移除补充文件</button>}
      </details>}
      {preview && noOpen && !session.pending && <button className="social-button secondary" disabled={disabled || preview.canBegin === false || !confirmed || !/^[a-f0-9]{64}$/.test(backup) || !file} onClick={() => void act(() => beginMigration(account, conversation, preview, backup, recoveryDigest, file!))}>确认备份并开始转换</button>}

      {session.status && <><p role="status">待转换 {session.status.remaining} 项</p><button className="text-button" disabled={disabled} onClick={() => void act(() => refreshMigration(account, conversation.id))}>核对转换状态</button>{!session.pending && session.status.state === "active" && session.status.remaining > 0 && <button className="social-button secondary" disabled={disabled} onClick={() => void act(() => convertMigrationPage(account, conversation))}>转换下一页</button>}{!session.pending && session.status.state === "active" && session.status.remaining === 0 && <button className="social-button primary" disabled={disabled || !file} onClick={() => void act(() => verifyAndFinishMigration(account, conversation, file!, recoveryDigest, supplement))}>从恢复文件重新读取核对全部清单并完成</button>}</>}
    </>}
  </section>;
}
