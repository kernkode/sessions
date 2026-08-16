import { fmtBytes, fmtCost, fmtRate, fmtTokens } from "../lib/format";
import { useT } from "../lib/i18n";
import { selActiveMetrics, selActiveSession, useStore } from "../state/store";

/** Bottom bar with every indicator of the active session. */
export function MetricsBar() {
  const m = useStore(selActiveMetrics);
  const session = useStore(selActiveSession);
  const t = useT();

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

  return (
    <div className="metrics">
      <Metric
        k="tok/s"
        v={fmtRate(m?.tokens_per_second ?? 0)}
        cls={(m?.tokens_per_second ?? 0) > 0 ? "acc" : ""}
      />
      <Metric k={t("m.peak")} v={fmtRate(m?.peak_tokens_per_second ?? 0)} />

      <div className="sep" />

      <div
        className="gauge"
        title={
          window ? t("m.contextTip", { u: used, w: window }) : t("m.contextUnknown")
        }
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

      <div className="sep" />

      <Metric k={t("m.ptyOut")} v={`${fmtBytes(m?.bytes_per_second ?? 0)}/s`} />
      <Metric k={t("m.total")} v={fmtBytes(m?.total_bytes ?? 0)} />
      {session.exit_code !== null && session.status === "exited" && (
        <Metric k={t("m.code")} v={String(session.exit_code)} cls={session.exit_code === 0 ? "ok" : "err"} />
      )}
    </div>
  );
}

function Metric({ k, v, cls = "" }: { k: string; v: string; cls?: string }) {
  return (
    <span className="metric">
      <span className="k">{k}</span>
      <span className={`v ${cls}`}>{v}</span>
    </span>
  );
}
