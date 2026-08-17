import { useEffect, useState } from "react";

import { fmtCost, fmtRate, fmtTokens } from "../lib/format";
import { useT } from "../lib/i18n";
import { selActiveMetrics, selActiveSession, useStore } from "../state/store";

/** Bottom bar with every indicator of the active session. */
export function MetricsBar() {
  const m = useStore(selActiveMetrics);
  const session = useStore(selActiveSession);
  const t = useT();

  // Rolling tok/s history for the sparkline.
  const [hist, setHist] = useState<number[]>([]);
  const tps = m?.tokens_per_second ?? 0;
  useEffect(() => {
    setHist((h) => [...h.slice(-39), tps]);
  }, [tps]);

  if (!session) {
    return (
      <div className="metrics">
        <span className="metric">
          <span className="k">{t("m.noSession")}</span>
        </span>
      </div>
    );
  }

  const used = m?.context_used ?? 0;
  const window = m?.context_window ?? null;
  const pct = window && window > 0 ? Math.min(100, (used / window) * 100) : null;
  const level = pct === null ? "" : pct > 90 ? "err" : pct > 70 ? "warn" : "";
  const working = (m?.status ?? session.status) === "working";

  return (
    <div className="metrics">
      <span className="metric">
        {working && <span className="live-dot" title={t("st.working")} />}
        <span className="k">tok/s</span>
        <span className={`v ${tps > 0 ? "acc" : ""}`}>{fmtRate(tps)}</span>
        <Spark data={hist} active={tps > 0} />
      </span>
      <Metric k={t("m.peak")} v={fmtRate(m?.peak_tokens_per_second ?? 0)} />

      <div className="sep" />

      <div
        className="gauge"
        title={window ? t("m.contextTip", { u: used, w: window }) : t("m.contextUnknown")}
      >
        <span className="k">{t("m.context")}</span>
        <div className="gauge-bar">
          <div className={`gauge-fill ${level}`} style={{ width: `${pct ?? 0}%` }} />
        </div>
        <span className="v">
          {fmtTokens(used)}
          {window ? ` / ${fmtTokens(window)}` : ""}
          {pct !== null ? ` · ${pct.toFixed(0)}%` : ""}
        </span>
      </div>

      <div className="sep" />

      <Metric k={t("m.input")} v={fmtTokens(m?.total_input_tokens ?? 0)} />
      <Metric k={t("m.output")} v={fmtTokens(m?.total_output_tokens ?? 0)} />
      <Metric k={t("m.cache")} v={fmtTokens((m?.cache_read_tokens ?? 0) + (m?.cache_write_tokens ?? 0))} />
      {(m?.reasoning_tokens ?? 0) > 0 && <Metric k={t("m.reasoning")} v={fmtTokens(m!.reasoning_tokens)} />}
      <Metric k={t("m.turns")} v={String(m?.turns ?? 0)} />
      <Metric k={t("m.cost")} v={fmtCost(m?.cost_usd ?? 0)} cls={(m?.cost_usd ?? 0) > 0 ? "ok" : ""} />

      {session.exit_code !== null && session.status === "exited" && (
        <Metric k={t("m.code")} v={String(session.exit_code)} cls={session.exit_code === 0 ? "ok" : "err"} />
      )}
    </div>
  );
}

function Metric({ k, v, cls = "" }: { k: string; v: string; cls?: string }) {
  // key={v} remounts the value on change so the tick animation plays.
  return (
    <span className="metric">
      <span className="k">{k}</span>
      <span key={v} className={`v tick ${cls}`}>
        {v}
      </span>
    </span>
  );
}

/** Tiny animated sparkline of recent tok/s samples. */
function Spark({ data, active }: { data: number[]; active: boolean }) {
  const W = 64;
  const H = 16;
  const max = Math.max(1, ...data);
  const n = Math.max(1, data.length - 1);
  const pts = data
    .map((v, i) => {
      const x = (i / n) * W;
      const y = H - 1 - (v / max) * (H - 3);
      return `${x.toFixed(1)},${y.toFixed(1)}`;
    })
    .join(" ");
  return (
    <svg className={`spark ${active ? "on" : ""}`} width={W} height={H} viewBox={`0 0 ${W} ${H}`}>
      <polyline points={pts} fill="none" strokeWidth="1.5" strokeLinejoin="round" strokeLinecap="round" />
    </svg>
  );
}
