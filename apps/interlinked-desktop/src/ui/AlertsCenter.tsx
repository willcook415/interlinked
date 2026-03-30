import type { AlertItem } from "../types";

type Severity = AlertItem["severity"];

const SEVERITY_ORDER: Severity[] = ["critical", "warn", "info"];

const SEVERITY_LABEL: Record<Severity, string> = {
  critical: "Critical",
  warn: "Warnings",
  info: "Information",
};

function severityCountLabel(alerts: AlertItem[], severity: Severity): string {
  const count = alerts.filter((alert) => alert.severity === severity).length;
  return `${count}`;
}

export default function AlertsCenter(props: {
  open: boolean;
  alerts: AlertItem[];
  onClose: () => void;
  onNavigate: (alert: AlertItem) => void;
  onDismiss: (alertId: string) => void;
}) {
  if (!props.open) return null;

  return (
    <div className="alerts-overlay" onClick={props.onClose}>
      <aside className="alerts-sheet" onClick={(event) => event.stopPropagation()}>
        <div className="alerts-head">
          <div>
            <p>Alerts Center</p>
            <h4>{props.alerts.length.toLocaleString()} active alerts</h4>
          </div>
          <button onClick={props.onClose}>Close</button>
        </div>

        {props.alerts.length === 0 ? (
          <section className="alerts-empty">
            <strong>All clear</strong>
            <span>No active alerts right now.</span>
          </section>
        ) : (
          SEVERITY_ORDER.map((severity) => {
            const rows = props.alerts.filter((alert) => alert.severity === severity);
            if (!rows.length) return null;
            return (
              <section key={severity} className={`alerts-group severity-${severity}`}>
                <div className="alerts-group-head">
                  <h5>{SEVERITY_LABEL[severity]}</h5>
                  <span>{severityCountLabel(props.alerts, severity)}</span>
                </div>
                <div className="alerts-list">
                  {rows.map((alert) => (
                    <article key={alert.id} className={`alert-card severity-${alert.severity}`}>
                      <div>
                        <strong>{alert.title}</strong>
                        {alert.detail?.trim() ? <p>{alert.detail}</p> : null}
                      </div>
                      <div className="alert-card-actions">
                        {alert.target?.id ? (
                          <button className="primary" onClick={() => props.onNavigate(alert)}>
                            {alert.action_label?.trim() || "View"}
                          </button>
                        ) : null}
                        <button onClick={() => props.onDismiss(alert.id)}>Dismiss</button>
                      </div>
                    </article>
                  ))}
                </div>
              </section>
            );
          })
        )}
      </aside>
    </div>
  );
}
