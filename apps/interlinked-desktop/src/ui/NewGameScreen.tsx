import type {
  CityOption,
  CountryOption,
  CountryPackStatus,
  CurrencyCode,
  Difficulty,
  DifficultyProfile,
} from "../types";

function multiplierLabel(value: number): string {
  if (!Number.isFinite(value)) return "-";
  return `x${value.toFixed(2)}`;
}

type DifficultyCard = {
  id: Difficulty;
  title: string;
  summary: string;
};

const DIFFICULTY_CARDS: DifficultyCard[] = [
  {
    id: "easy",
    title: "Easy",
    summary: "Friendly cashflow and lower penalty pressure. Best for learning systems.",
  },
  {
    id: "standard",
    title: "Standard",
    summary: "Balanced economics and demand behavior tuned for core gameplay.",
  },
  {
    id: "hard",
    title: "Hard",
    summary: "Higher demand and stronger penalties. Requires tight operational discipline.",
  },
];

export default function NewGameScreen(props: {
  step: 1 | 2 | 3 | 4;
  gameName: string;
  modeIntent: string;
  difficulty: Difficulty;
  difficultyProfile: DifficultyProfile;
  currency: CurrencyCode;
  budget: string;
  countries: CountryOption[];
  countryPacks: CountryPackStatus[];
  selectedCountryIso2: string;
  selectedCityId: number | null;
  selectedCountryName: string | null;
  selectedCityName: string | null;
  citySearch: string;
  filteredCities: CityOption[];
  busy: boolean;
  error: string | null;
  onBack: () => void;
  onNext: () => void;
  onPrev: () => void;
  onCreate: () => void;
  onNameChange: (v: string) => void;
  onModeIntentChange: (v: string) => void;
  onDifficultyChange: (v: Difficulty) => void;
  onCurrencyChange: (v: CurrencyCode) => void;
  onBudgetChange: (v: string) => void;
  onCountryChange: (v: string) => void;
  onInstallPack: (iso2: string) => void;
  onUninstallPack: (iso2: string) => void;
  onCitySearchChange: (v: string) => void;
  onCitySelect: (id: number) => void;
}) {
  const selectedPack =
    props.countryPacks.find((p) => p.country_iso2 === props.selectedCountryIso2) ?? null;
  const countryStatusLabel = (iso2: string): string => {
    const status = props.countryPacks.find((p) => p.country_iso2 === iso2);
    if (!status) return "Unknown";
    if (status.build_state === "installed" && status.eligible) return "Installed";
    return status.reason ?? "Unavailable";
  };
  const budgetValue = Number(props.budget);
  const validBudget = Number.isFinite(budgetValue) && budgetValue > 0 ? budgetValue : null;

  return (
    <div className="form-screen">
      <header>
        <h2>New Game</h2>
        <p>Step {props.step} of 4</p>
      </header>

      {props.step === 1 && (
        <div className="form-card">
          <h3>Player Profile</h3>
          <label>Save Name</label>
          <input
            value={props.gameName}
            onChange={(e) => props.onNameChange(e.target.value)}
            placeholder="Interlinked World"
          />
          <label>Mode Intent</label>
          <select value={props.modeIntent} onChange={(e) => props.onModeIntentChange(e.target.value)}>
            <option value="balanced">Balanced sandbox</option>
            <option value="builder">Creative builder</option>
            <option value="operator">Operational challenge</option>
          </select>
          <p className="form-hint">
            You can change economics and operations after game start, but this sets your first-hour guidance.
          </p>
        </div>
      )}

      {props.step === 2 && (
        <div className="form-card">
          <h3>Start Location</h3>
          {props.countries.length === 0 && (
            <p className="form-error">
              No location catalog found. Build one with `interlinked-osm build-location-catalog`.
            </p>
          )}
          <label>Country</label>
          <select
            value={props.selectedCountryIso2}
            onChange={(e) => props.onCountryChange(e.target.value)}
          >
            {props.countries.map((c) => (
              <option
                key={c.iso2}
                value={c.iso2}
                disabled={!(props.countryPacks.find((p) => p.country_iso2 === c.iso2)?.eligible ?? false)}
              >
                {c.name} ({countryStatusLabel(c.iso2)})
              </option>
            ))}
          </select>
          {selectedPack && (
            <p className={selectedPack.eligible ? "form-hint" : "form-error"}>
              {selectedPack.eligible
                ? `Pack installed: ${selectedPack.country_iso2} (${selectedPack.cells_count.toLocaleString()} cells).`
                : `${selectedPack.reason ?? "Country unavailable"} (${selectedPack.country_iso2}).`}
            </p>
          )}
          {selectedPack && (
            <div className="form-actions">
              {!selectedPack.eligible && selectedPack.reason !== "Coming Soon" && (
                <button
                  disabled={props.busy}
                  onClick={() => props.onInstallPack(selectedPack.country_iso2)}
                >
                  Install Pack
                </button>
              )}
              {selectedPack.build_state === "installed" && (
                <button
                  disabled={props.busy}
                  onClick={() => props.onUninstallPack(selectedPack.country_iso2)}
                >
                  Uninstall Pack
                </button>
              )}
            </div>
          )}
          <label>City Search</label>
          <input
            value={props.citySearch}
            onChange={(e) => props.onCitySearchChange(e.target.value)}
            placeholder="Search city"
          />
          <div className="city-list">
            {props.filteredCities.map((city) => (
              <button
                key={city.geonameid}
                className={props.selectedCityId === city.geonameid ? "active" : ""}
                onClick={() => props.onCitySelect(city.geonameid)}
              >
                {city.name}
              </button>
            ))}
          </div>
        </div>
      )}

      {props.step === 3 && (
        <div className="form-card">
          <h3>Difficulty And Economy</h3>
          <div className="difficulty-card-grid">
            {DIFFICULTY_CARDS.map((difficulty) => (
              <button
                key={difficulty.id}
                className={`difficulty-card ${props.difficulty === difficulty.id ? "active" : ""}`}
                onClick={() => props.onDifficultyChange(difficulty.id)}
              >
                <strong>{difficulty.title}</strong>
                <span>{difficulty.summary}</span>
              </button>
            ))}
          </div>
          <div className="difficulty-impact-grid">
            <article>
              <small>Demand Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.demand_mult)}</strong>
            </article>
            <article>
              <small>Build Cost Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.capex_mult)}</strong>
            </article>
            <article>
              <small>Operating Cost Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.opex_mult)}</strong>
            </article>
            <article>
              <small>Maintenance Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.maintenance_mult)}</strong>
            </article>
            <article>
              <small>Penalty Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.penalty_mult)}</strong>
            </article>
            <article>
              <small>Unlock Cost Pressure</small>
              <strong>{multiplierLabel(props.difficultyProfile.unlock_cost_mult)}</strong>
            </article>
            <article>
              <small>Ancillary Revenue</small>
              <strong>{multiplierLabel(props.difficultyProfile.ancillary_revenue_mult)}</strong>
            </article>
          </div>
          <div className="form-grid">
            <div>
              <label>Currency</label>
              <select
                value={props.currency}
                onChange={(e) => props.onCurrencyChange(e.target.value as CurrencyCode)}
              >
                <option value="GBP">GBP</option>
                <option value="USD">USD</option>
                <option value="EUR">EUR</option>
              </select>
            </div>
            <div>
              <label>Starting Budget</label>
              <input value={props.budget} onChange={(e) => props.onBudgetChange(e.target.value)} />
            </div>
          </div>
        </div>
      )}

      {props.step === 4 && (
        <div className="form-card">
          <h3>Review And Forecast</h3>
          <div className="review-grid">
            <article>
              <small>Save</small>
              <strong>{props.gameName.trim() || "Interlinked World"}</strong>
            </article>
            <article>
              <small>Intent</small>
              <strong>{props.modeIntent}</strong>
            </article>
            <article>
              <small>Location</small>
              <strong>
                {props.selectedCityName ?? "No city selected"}
                {props.selectedCountryName ? `, ${props.selectedCountryName}` : ""}
              </strong>
            </article>
            <article>
              <small>Difficulty</small>
              <strong>{props.difficulty}</strong>
            </article>
            <article>
              <small>Starting Budget</small>
              <strong>
                {validBudget === null
                  ? "Invalid budget"
                  : new Intl.NumberFormat(undefined, {
                      style: "currency",
                      currency: props.currency,
                      maximumFractionDigits: 0,
                    }).format(validBudget)}
              </strong>
            </article>
          </div>
          <div className="forecast-panel">
            <p>First-hour guidance</p>
            <ul>
              <li>Build one short corridor first and keep draft costs controlled.</li>
              <li>Order at least one vehicle before starting time to avoid idle routes.</li>
              <li>Watch penalties and staffing during the first peak period.</li>
            </ul>
          </div>
        </div>
      )}

      {props.error && <p className="form-error">{props.error}</p>}

      <div className="form-actions">
        <button onClick={props.onBack}>Back to Menu</button>
        {props.step > 1 && <button onClick={props.onPrev}>Previous</button>}
        {props.step < 4 && <button onClick={props.onNext}>Next</button>}
        {props.step === 4 && (
          <button onClick={props.onCreate} disabled={props.busy}>
            Create and Enter
          </button>
        )}
      </div>
    </div>
  );
}

