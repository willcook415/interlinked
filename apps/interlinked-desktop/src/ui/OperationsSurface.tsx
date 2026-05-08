import type { RegionStatus } from "../types";
import { CountryInfoContent } from "./CountryInfoDrawer";

export default function OperationsSurface(props: {
  open: boolean;
  busy: boolean;
  regions: RegionStatus[];
  selectedRegionId: string | null;
  currentBalanceBase: number | null;
  onClose: () => void;
  onSelectRegion: (regionId: string) => void;
  onUnlockRegion: () => void;
}) {
  if (!props.open) return null;

  return (
    <div className="operations-surface-overlay" role="dialog" aria-modal="true" aria-label="Operations">
      <section className="operations-surface">
        <header className="operations-surface-head">
          <div>
            <p>Operations</p>
            <h3>Regions And Strategic Scope</h3>
          </div>
          <button onClick={props.onClose}>Close</button>
        </header>
        <div className="operations-surface-body">
          <CountryInfoContent
            busy={props.busy}
            regions={props.regions}
            selectedRegionId={props.selectedRegionId}
            currentBalanceBase={props.currentBalanceBase}
            onSelectRegion={props.onSelectRegion}
            onUnlockRegion={props.onUnlockRegion}
          />
        </div>
      </section>
    </div>
  );
}
