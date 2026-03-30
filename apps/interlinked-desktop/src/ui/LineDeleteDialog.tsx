import { useEffect, useState } from "react";
import type { CurrencyCode } from "../types";

function formatMoney(value: number, currency: CurrencyCode): string {
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency,
    maximumFractionDigits: 0,
  }).format(value);
}

export default function LineDeleteDialog(props: {
  open: boolean;
  lineName: string;
  unitLabel: string;
  unitsOwned: number;
  unitsPending: number;
  transferTargets: Array<{ lineId: string; lineName: string }>;
  budgetCurrency: CurrencyCode;
  estimatedScrapValueBase: number;
  onCancel: () => void;
  onConfirmScrap: () => void;
  onConfirmTransfer: (targetLineId: string) => void;
}) {
  const [targetLineId, setTargetLineId] = useState("");

  useEffect(() => {
    if (!props.open) return;
    setTargetLineId(props.transferTargets[0]?.lineId ?? "");
  }, [props.open, props.transferTargets]);

  if (!props.open) return null;

  const owned = Math.max(Math.round(props.unitsOwned), 0);
  const pending = Math.max(Math.round(props.unitsPending), 0);
  const totalStock = owned + pending;
  const hasStock = totalStock > 0;
  const hasTransferTarget = props.transferTargets.length > 0;

  return (
    <div className="line-delete-overlay">
      <aside className="line-delete-dialog">
        <div className="inspector-section-head">
          <h5>Delete Line</h5>
          <button onClick={props.onCancel}>Close</button>
        </div>
        <p className="hint-line">
          {props.lineName}: choose what happens to existing rolling stock before deleting the line.
        </p>
        <div className="inspector-stat-row">
          <div className="inspector-stat">
            <small>{props.unitLabel}s Owned</small>
            <strong>{owned.toLocaleString()}</strong>
          </div>
          <div className="inspector-stat">
            <small>{props.unitLabel}s On Order</small>
            <strong>{pending.toLocaleString()}</strong>
          </div>
          <div className="inspector-stat">
            <small>Scrap Return</small>
            <strong>{formatMoney(props.estimatedScrapValueBase, props.budgetCurrency)}</strong>
          </div>
        </div>

        {hasStock ? (
          <div className="line-delete-actions">
            <button className="danger-button" onClick={props.onConfirmScrap}>
              Scrap All + Delete
            </button>
            {hasTransferTarget ? (
              <div className="line-delete-transfer">
                <label>
                  Transfer To Line
                  <select value={targetLineId} onChange={(event) => setTargetLineId(event.target.value)}>
                    {props.transferTargets.map((target) => (
                      <option key={target.lineId} value={target.lineId}>
                        {target.lineName}
                      </option>
                    ))}
                  </select>
                </label>
                <button
                  className="primary"
                  disabled={!targetLineId}
                  onClick={() => props.onConfirmTransfer(targetLineId)}
                >
                  Transfer + Delete
                </button>
              </div>
            ) : (
              <p className="hint-line">No compatible target line available for transfer.</p>
            )}
          </div>
        ) : (
          <div className="line-delete-actions">
            <p className="hint-line">This line has no active stock. You can delete it directly.</p>
            <button className="danger-button" onClick={props.onConfirmScrap}>
              Delete Line
            </button>
          </div>
        )}

        <div className="editor-drawer-footer">
          <button onClick={props.onCancel}>Cancel</button>
        </div>
      </aside>
    </div>
  );
}
