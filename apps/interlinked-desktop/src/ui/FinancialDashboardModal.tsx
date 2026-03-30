import type {
  FinancialDashboardRequest,
  FinancialDashboardResponse,
  RegionStatus,
} from "../types";

function formatMoney(value: number | null | undefined, currency: string, compact = false): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "-";
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    notation: compact ? "compact" : "standard",
    maximumFractionDigits: compact ? 1 : 0,
  }).format(value);
}

function formatNumber(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "-";
  return new Intl.NumberFormat(undefined, { maximumFractionDigits: 0 }).format(value);
}

function summarizeRange(values: number[]): { min: number; max: number } {
  if (values.length === 0) return { min: 0, max: 0 };
  return {
    min: Math.min(...values),
    max: Math.max(...values),
  };
}

function renderNetTrendSvg(points: FinancialDashboardResponse["points"]) {
  const width = 780;
  const height = 210;
  const paddingX = 34;
  const paddingY = 24;
  if (!points.length) {
    return (
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="No financial trend data">
        <rect x={0} y={0} width={width} height={height} fill="rgba(255,255,255,0.03)" rx={14} />
      </svg>
    );
  }
  const values = points.map((point) => point.net_base);
  const { min, max } = summarizeRange(values);
  const safeSpan = Math.max(max - min, 1);
  const scaleX = (index: number): number => {
    if (points.length === 1) return width * 0.5;
    const ratio = index / Math.max(points.length - 1, 1);
    return paddingX + ratio * (width - paddingX * 2);
  };
  const scaleY = (value: number): number => {
    const ratio = (value - min) / safeSpan;
    return height - paddingY - ratio * (height - paddingY * 2);
  };
  const path = points
    .map((point, index) => `${index === 0 ? "M" : "L"} ${scaleX(index).toFixed(2)} ${scaleY(point.net_base).toFixed(2)}`)
    .join(" ");
  const zeroY = scaleY(0);
  return (
    <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Net financial trend">
      <rect x={0} y={0} width={width} height={height} fill="rgba(255,255,255,0.03)" rx={14} />
      <line x1={paddingX} x2={width - paddingX} y1={zeroY} y2={zeroY} stroke="rgba(184,199,220,0.5)" strokeWidth={1} />
      <path d={path} fill="none" stroke="#5ea4ff" strokeWidth={3} />
      {points.map((point, index) => (
        <circle key={`net-${point.period_index}-${index}`} cx={scaleX(index)} cy={scaleY(point.net_base)} r={3.2} fill="#cfe4ff" />
      ))}
    </svg>
  );
}

function renderRevenueOpexSvg(points: FinancialDashboardResponse["points"]) {
  const width = 780;
  const height = 210;
  const paddingX = 34;
  const paddingY = 20;
  if (!points.length) {
    return (
      <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="No revenue and opex data">
        <rect x={0} y={0} width={width} height={height} fill="rgba(255,255,255,0.03)" rx={14} />
      </svg>
    );
  }
  const peak = Math.max(
    ...points.map((point) => Math.max(point.revenue_base, point.opex_base, 1))
  );
  const plotWidth = width - paddingX * 2;
  const slot = plotWidth / Math.max(points.length, 1);
  const barWidth = Math.max(slot * 0.32, 4);
  const scaleY = (value: number): number => {
    const ratio = Math.max(value, 0) / Math.max(peak, 1);
    return height - paddingY - ratio * (height - paddingY * 2);
  };
  return (
    <svg viewBox={`0 0 ${width} ${height}`} role="img" aria-label="Revenue vs opex trend">
      <rect x={0} y={0} width={width} height={height} fill="rgba(255,255,255,0.03)" rx={14} />
      {points.map((point, index) => {
        const centerX = paddingX + slot * index + slot * 0.5;
        const revenueY = scaleY(point.revenue_base);
        const opexY = scaleY(point.opex_base);
        const baseline = height - paddingY;
        return (
          <g key={`rev-opex-${point.period_index}-${index}`}>
            <rect
              x={centerX - barWidth - 1}
              y={revenueY}
              width={barWidth}
              height={Math.max(baseline - revenueY, 1)}
              fill="rgba(111,207,151,0.82)"
              rx={2}
            />
            <rect
              x={centerX + 1}
              y={opexY}
              width={barWidth}
              height={Math.max(baseline - opexY, 1)}
              fill="rgba(255,146,146,0.8)"
              rx={2}
            />
          </g>
        );
      })}
    </svg>
  );
}

export default function FinancialDashboardModal(props: {
  open: boolean;
  busy: boolean;
  error: string | null;
  currency: string;
  request: FinancialDashboardRequest;
  data: FinancialDashboardResponse | null;
  regions: RegionStatus[];
  lineOptions: Array<{ lineId: string; name: string }>;
  onRequestChange: (patch: Partial<FinancialDashboardRequest>) => void;
  onRefresh: () => void;
  onClose: () => void;
}) {
  if (!props.open) return null;
  const data = props.data;
  const currency = data?.currency ?? props.currency ?? "GBP";
  const regionNameById = new Map(
    props.regions.map((region) => [region.region_id, region.name] as const)
  );

  return (
    <div className="financial-dashboard-overlay">
      <section className="financial-dashboard-sheet">
        <div className="financial-dashboard-head">
          <div>
            <p>Financial Dashboard</p>
            <h4>Economy And Operations</h4>
          </div>
          <div className="financial-dashboard-head-actions">
            <button onClick={props.onRefresh} disabled={props.busy}>
              Refresh
            </button>
            <button onClick={props.onClose}>Close</button>
          </div>
        </div>

        <div className="financial-filter-row">
          <label>
            Period
            <select
              value={props.request.granularity ?? "month"}
              onChange={(event) =>
                props.onRequestChange({
                  granularity: event.target.value as "day" | "week" | "month" | "year",
                })
              }
            >
              <option value="day">Daily</option>
              <option value="week">Weekly</option>
              <option value="month">Monthly</option>
              <option value="year">Yearly</option>
            </select>
          </label>
          <label>
            Mode
            <select
              value={props.request.mode ?? "all"}
              onChange={(event) =>
                props.onRequestChange({
                  mode: event.target.value === "all" ? null : event.target.value,
                })
              }
            >
              <option value="all">All</option>
              <option value="bus">Bus</option>
              <option value="tram">Tram</option>
              <option value="metro">Metro</option>
              <option value="ferry">Ferry</option>
              <option value="rail">Rail</option>
            </select>
          </label>
          <label>
            Line
            <select
              value={props.request.line_id ?? "all"}
              onChange={(event) =>
                props.onRequestChange({
                  line_id: event.target.value === "all" ? null : event.target.value,
                })
              }
            >
              <option value="all">All lines</option>
              {props.lineOptions.map((line) => (
                <option key={line.lineId} value={line.lineId}>
                  {line.name}
                </option>
              ))}
            </select>
          </label>
          <label>
            Region
            <select
              value={props.request.region_id ?? "all"}
              onChange={(event) =>
                props.onRequestChange({
                  region_id: event.target.value === "all" ? null : event.target.value,
                })
              }
            >
              <option value="all">All regions</option>
              {props.regions.map((region) => (
                <option key={region.region_id} value={region.region_id}>
                  {region.name}
                </option>
              ))}
            </select>
          </label>
        </div>

        {props.error ? <p className="form-error">{props.error}</p> : null}

        <div className="financial-kpi-grid">
          <article>
            <small>Current Balance</small>
            <strong title={formatMoney(data?.current_balance_base, currency, false)}>
              {formatMoney(data?.current_balance_base, currency, true)}
            </strong>
          </article>
          <article>
            <small>Total Revenue</small>
            <strong title={formatMoney(data?.total_revenue_base, currency, false)}>
              {formatMoney(data?.total_revenue_base, currency, true)}
            </strong>
          </article>
          <article>
            <small>Total Opex</small>
            <strong title={formatMoney(data?.total_opex_base, currency, false)}>
              {formatMoney(data?.total_opex_base, currency, true)}
            </strong>
          </article>
          <article>
            <small>Total Capex</small>
            <strong title={formatMoney(data?.total_capex_base, currency, false)}>
              {formatMoney(data?.total_capex_base, currency, true)}
            </strong>
          </article>
          <article>
            <small>Total Penalties</small>
            <strong title={formatMoney(data?.total_penalties_base, currency, false)}>
              {formatMoney(data?.total_penalties_base, currency, true)}
            </strong>
          </article>
          <article>
            <small>Net</small>
            <strong title={formatMoney(data?.total_net_base, currency, false)}>
              {formatMoney(data?.total_net_base, currency, true)}
            </strong>
          </article>
        </div>

        <div className="financial-chart-grid">
          <article>
            <div className="financial-chart-head">
              <h5>Net Trend</h5>
              <span>{formatNumber(data?.points.length ?? 0)} points</span>
            </div>
            {renderNetTrendSvg(data?.points ?? [])}
          </article>
          <article>
            <div className="financial-chart-head">
              <h5>Revenue Vs Opex</h5>
              <span>{data?.granularity ?? "month"}</span>
            </div>
            {renderRevenueOpexSvg(data?.points ?? [])}
          </article>
        </div>

        <div className="financial-breakdown-grid">
          <section>
            <div className="financial-chart-head">
              <h5>By Mode</h5>
            </div>
            <div className="financial-table">
              {(data?.mode_breakdown ?? []).map((row) => (
                <div key={row.mode} className="financial-table-row">
                  <span>{row.mode}</span>
                  <span>{row.lines} lines</span>
                  <span title={formatMoney(row.net_base, currency, false)}>{formatMoney(row.net_base, currency, true)}</span>
                </div>
              ))}
            </div>
          </section>
          <section>
            <div className="financial-chart-head">
              <h5>By Region</h5>
            </div>
            <div className="financial-table">
              {(data?.region_breakdown ?? []).map((row) => (
                <div key={row.region_id} className="financial-table-row">
                  <span>{regionNameById.get(row.region_id) ?? row.region_id}</span>
                  <span title={formatMoney(row.net_base, currency, false)}>{formatMoney(row.net_base, currency, true)}</span>
                </div>
              ))}
            </div>
          </section>
          <section>
            <div className="financial-chart-head">
              <h5>By Line</h5>
            </div>
            <div className="financial-table">
              {(data?.line_breakdown ?? []).slice(0, 40).map((row) => (
                <div key={row.line_id} className="financial-table-row">
                  <span>{row.line_name}</span>
                  <span>{row.mode}</span>
                  <span title={formatMoney(-row.estimated_opex_per_hour_base, currency, false)}>
                    {formatMoney(-row.estimated_opex_per_hour_base, currency, true)}/hr
                  </span>
                </div>
              ))}
            </div>
          </section>
        </div>
      </section>
    </div>
  );
}
