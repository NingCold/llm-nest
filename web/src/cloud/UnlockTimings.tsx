import { useState } from "react";
import { selectUnlockTimings, type UnlockTiming } from "./unlock-timings";

export function UnlockTimings() {
  const [entries, setEntries] = useState<UnlockTiming[]>([]);
  const [notice, setNotice] = useState("");
  const read = () => {
    setEntries(selectUnlockTimings(typeof performance === "undefined" ? [] : performance.getEntriesByType("measure")));
    setNotice("");
  };
  const copy = async () => {
    try {
      await navigator.clipboard.writeText(JSON.stringify({ unit: "ms", startTime: "页面启动以来的偏移；阶段可能重叠，不能相加", entries }, null, 2));
      setNotice("已复制解锁耗时");
    } catch { setNotice("复制失败，请选择下方数据手动复制"); }
  };
  return <details className="cloud-unlock-timings" onToggle={event => { if (event.currentTarget.open) read(); }}>
    <summary>解锁耗时</summary>
    <div className="cloud-form">
    <p>仅包含本次运行内存中的固定阶段耗时，不含账号或聊天内容。关闭后重新展开可刷新。</p>
    {entries.length ? <>
      <p>时间单位为毫秒（ms）；startTime 是页面启动以来的偏移。阶段可能重叠，不能相加。</p>
      <button type="button" className="text-button" onClick={() => void copy()}>复制解锁耗时</button>
      <textarea aria-label="解锁耗时数据" readOnly rows={12} value={JSON.stringify(entries, null, 2)} />
    </> : <p>当前尚无有效解锁计时。完成一次解锁后，再展开查看。</p>}
    {notice && <p role="status">{notice}</p>}
    </div>
  </details>;
}
