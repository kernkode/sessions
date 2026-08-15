import { fmtBytes, fmtCost, fmtRate, fmtTokens } from "../lib/format";
import { selActiveMetrics, selActiveSession, useStore } from "../state/store";

/** Bottom bar with every indicator of the active session. */
export function MetricsBar() {
  const m = useStore(selActiveMetrics);
  const session = useStore(selActiveSession);

  if (!session) {
    return (
      <div className="metrics">
        <span className="metric">
          <span className="k">sin sesión</span>
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
      <Metric k="pico" v={fmtRate(m?.peak_tokens_per_second ?? 0)} />

      <div className="sep" />

      <div
        className="gauge"
        title={
          window ? `${used} de ${window} tokens de contexto` : "Ventana de contexto desconocida"
        }
      >
        <span className="k" style={{ color: "var(--txt-3)", fontSize: 10.5, textTransform: "uppercase" }}>
          contexto
        </span>
        <div className="gauge-bar">
          <div className={`gauge-fill ${level}`} style={{ width: `${pct ?? 0}%` }} />
        </div>
        <span className="v" style={{ fontFamily: "var(--font-mono)", fontSize: 12 }}>
          {fmtTokens(used)}
          {window ? ` / ${fmtTokens(window)}` : ""}
          {pct !== null ? ` · ${pct.toFixed(0)}%` : ""}
        </span>
      </div>

      <div className="sep" />

      <Metric k="entrada" v={fmtTokens(m?.total_input_tokens ?? 0)} />
      <Metric k="salida" v={fmtTokens(m?.total_output_tokens ?? 0)} />
      <Metric k="caché" v={fmtTokens((m?.cache_read_tokens ?? 0) + (m?.cache_write_tokens ?? 0))} />
      {(m?.reasoning_tokens ?? 0) > 0 && <Metric k="razonam." v={fmtTokens(m!.reasoning_tokens)} />}
      <Metric k="turnos" v={String(m?.turns ?? 0)} />
      <Metric k="coste" v={fmtCost(m?.cost_usd ?? 0)} cls={(m?.cost_usd ?? 0) > 0 ? "ok" : ""} />

      <div className="sep" />

      <Metric k="salida pty" v={`${fmtBytes(m?.bytes_per_second ?? 0)}/s`} />
      <Metric k="total" v={fmtBytes(m?.total_bytes ?? 0)} />
      {session.exit_code !== null && session.status === "exited" && (
        <Metric k="código" v={String(session.exit_code)} cls={session.exit_code === 0 ? "ok" : "err"} />
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
