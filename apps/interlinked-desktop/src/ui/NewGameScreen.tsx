import { useEffect, useMemo } from "react";
import type {
  CityOption,
  CountryOption,
  CountryPackStatus,
  CurrencyCode,
  Difficulty,
  DifficultyProfile,
} from "../types";
import { InterlinkedButton, InterlinkedPageShell } from "./primitives";

function multiplierLabel(value: number): string {
  if (!Number.isFinite(value)) return "-";
  return `x${value.toFixed(2)}`;
}

function parsePositiveBudget(value: string): number | null {
  const normalized = value.replace(/[^\d.-]/g, "").trim();
  if (!normalized) return null;
  const parsed = Number(normalized);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
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
    summary: "Higher cash buffer and lighter penalties for steady onboarding.",
  },
  {
    id: "standard",
    title: "Standard",
    summary: "Balanced operating pressure tuned for core simulation play.",
  },
  {
    id: "hard",
    title: "Hard",
    summary: "Tighter budgets and heavier penalties for disciplined operations.",
  },
];

const STAGE_LABELS = ["Identity & Location", "Simulation Setup", "Review & Launch"] as const;

function countryStatusLabel(status: CountryPackStatus | null): string {
  if (!status) return "Unknown";
  if (status.eligible) return "Ready";
  if (status.build_state === "building") return "Installing";
  if (status.reason) return status.reason;
  if (status.build_state === "missing") return "Not Installed";
  return "Unavailable";
}

function canInstallCountryPack(status: CountryPackStatus | null): boolean {
  if (!status) return false;
  if (status.eligible) return false;
  if (status.reason === "Coming Soon") return false;
  if (status.build_state === "building") return false;
  return true;
}

function cityPopulationLabel(city: CityOption | null): string | null {
  if (!city) return null;
  if (!Number.isFinite(city.population) || city.population <= 0) return null;
  return city.population.toLocaleString();
}

export default function NewGameScreen(props: {
  step: 1 | 2 | 3;
  gameName: string;
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
  onDifficultyChange: (v: Difficulty) => void;
  onCurrencyChange: (v: CurrencyCode) => void;
  onCountryChange: (v: string) => void;
  onInstallPack: (iso2: string) => void;
  onCitySearchChange: (v: string) => void;
  onCitySelect: (id: number) => void;
}) {
  const selectedPack =
    props.countryPacks.find((pack) => pack.country_iso2 === props.selectedCountryIso2) ?? null;
  const selectedCity =
    props.filteredCities.find((city) => city.geonameid === props.selectedCityId) ?? null;
  const selectedCityPopulation = cityPopulationLabel(selectedCity);
  const budgetValue = parsePositiveBudget(props.budget);
  const formattedBudget =
    budgetValue === null
      ? "Invalid"
      : new Intl.NumberFormat(undefined, {
          style: "currency",
          currency: props.currency,
          maximumFractionDigits: 0,
        }).format(budgetValue);
  const difficultyTitle = useMemo(
    () => DIFFICULTY_CARDS.find((card) => card.id === props.difficulty)?.title ?? "Standard",
    [props.difficulty]
  );

  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    const previousRootOverflow = root.style.overflow;
    const previousRootOverscroll = root.style.overscrollBehavior;
    const previousBodyOverflow = body.style.overflow;
    const previousBodyOverscroll = body.style.overscrollBehavior;
    root.classList.add("il-new-game-route");
    body.classList.add("il-new-game-route");
    root.style.overflow = "hidden";
    root.style.overscrollBehavior = "none";
    body.style.overflow = "hidden";
    body.style.overscrollBehavior = "none";
    return () => {
      root.classList.remove("il-new-game-route");
      body.classList.remove("il-new-game-route");
      root.style.overflow = previousRootOverflow;
      root.style.overscrollBehavior = previousRootOverscroll;
      body.style.overflow = previousBodyOverflow;
      body.style.overscrollBehavior = previousBodyOverscroll;
    };
  }, []);

  return (
    <InterlinkedPageShell className="il-new-game" centered>
      <div className="il-new-game-atmosphere" aria-hidden="true">
        <span className="il-new-trace is-a" />
        <span className="il-new-trace is-b" />
        <span className="il-new-trace is-c" />
        <span className="il-new-node is-a" />
        <span className="il-new-node is-b" />
        <span className="il-new-node is-c" />
        <span className="il-new-pulse" />
      </div>

      <section className="il-new-game-stage" aria-label="New Game Setup">
        <header className="il-new-topbar">
          <InterlinkedButton size="sm" tone="ghost" className="il-new-back" onClick={props.onBack}>
            Back to Menu
          </InterlinkedButton>
          <div className="il-new-title-wrap">
            <h1 className="il-new-title">NEW GAME</h1>
            <p className="il-new-subtitle">Configure a new transport simulation</p>
          </div>
          <p className="il-new-stage-counter">Stage {props.step} of 3</p>
        </header>

        <ol className="il-new-progress" aria-label="Setup Stages">
          {STAGE_LABELS.map((label, index) => {
            const stageIndex = index + 1;
            const isActive = props.step === stageIndex;
            const isComplete = props.step > stageIndex;
            return (
              <li
                key={label}
                className={`il-new-progress-item${isActive ? " is-active" : ""}${
                  isComplete ? " is-complete" : ""
                }`}
              >
                <span className="il-new-progress-index">{stageIndex}</span>
                <span className="il-new-progress-label">{label}</span>
              </li>
            );
          })}
        </ol>

        <section className="il-new-stage-panel">
          {props.step === 1 ? (
            <div className="il-new-stage-scroll" aria-label="Identity and Location">
              <section className="il-new-block">
                <h2>Identity</h2>
                <label className="il-new-field">
                  <span>Save Name</span>
                  <input
                    value={props.gameName}
                    onChange={(event) => props.onNameChange(event.target.value)}
                    placeholder="Interlinked World"
                  />
                </label>
              </section>

              <section className="il-new-block">
                <h2>Location</h2>
                {props.countries.length === 0 ? (
                  <p className="il-new-error">No country catalog available. Build location catalog data first.</p>
                ) : null}

                <label className="il-new-field">
                  <span>Start Country</span>
                  <select
                    value={props.selectedCountryIso2}
                    onChange={(event) => props.onCountryChange(event.target.value)}
                  >
                    {props.countries.map((country) => {
                      const status =
                        props.countryPacks.find((pack) => pack.country_iso2 === country.iso2) ?? null;
                      return (
                        <option key={country.iso2} value={country.iso2}>
                          {country.name} ({countryStatusLabel(status)})
                        </option>
                      );
                    })}
                  </select>
                </label>

                <label className="il-new-field">
                  <span>City Search</span>
                  <input
                    value={props.citySearch}
                    onChange={(event) => props.onCitySearchChange(event.target.value)}
                    placeholder="Search city"
                    type="search"
                  />
                </label>

                <div className="il-new-city-browser">
                  <header>
                    <span>Starting City</span>
                  </header>
                  <div className="il-new-city-list" role="listbox" aria-label="Starting City Options">
                    {props.filteredCities.map((city) => {
                      const selected = props.selectedCityId === city.geonameid;
                      return (
                        <button
                          key={city.geonameid}
                          className={`il-new-city-option${selected ? " is-selected" : ""}`}
                          onClick={() => props.onCitySelect(city.geonameid)}
                          type="button"
                        >
                          <span>{city.name}</span>
                          <small>{city.population.toLocaleString()}</small>
                        </button>
                      );
                    })}
                  </div>
                </div>

                {selectedPack && canInstallCountryPack(selectedPack) ? (
                  <div className="il-new-pack-install">
                    <p>{selectedPack.reason ?? "Country data is not installed for this start location."}</p>
                    <InterlinkedButton
                      size="sm"
                      tone="secondary"
                      disabled={props.busy}
                      onClick={() => props.onInstallPack(selectedPack.country_iso2)}
                    >
                      Install Pack
                    </InterlinkedButton>
                  </div>
                ) : null}

                <p className="il-new-selection-line">
                  Starting city selected: <strong>{props.selectedCityName ?? "No city selected"}</strong>
                  {props.selectedCountryName ? `, ${props.selectedCountryName}` : ""}
                  {selectedCityPopulation ? ` · Population ${selectedCityPopulation}` : ""}
                </p>
              </section>
            </div>
          ) : null}

          {props.step === 2 ? (
            <div className="il-new-stage-scroll" aria-label="Simulation Setup">
              <section className="il-new-block">
                <h2>Difficulty</h2>
                <div className="il-new-difficulty-grid" role="radiogroup" aria-label="Difficulty">
                  {DIFFICULTY_CARDS.map((card) => (
                    <button
                      key={card.id}
                      className={`il-new-difficulty-card${props.difficulty === card.id ? " is-active" : ""}`}
                      onClick={() => props.onDifficultyChange(card.id)}
                      type="button"
                    >
                      <strong>{card.title}</strong>
                      <span>{card.summary}</span>
                    </button>
                  ))}
                </div>
              </section>

              <section className="il-new-block">
                <h2>World Economy</h2>
                <div className="il-new-setup-grid">
                  <label className="il-new-field il-new-currency-field">
                    <span>Currency</span>
                    <select
                      value={props.currency}
                      onChange={(event) => props.onCurrencyChange(event.target.value as CurrencyCode)}
                    >
                      <option value="GBP">GBP</option>
                      <option value="USD">USD</option>
                      <option value="EUR">EUR</option>
                    </select>
                  </label>
                  <article className="il-new-budget-card" aria-live="polite">
                    <small>Starting Budget</small>
                    <strong>{formattedBudget}</strong>
                  </article>
                </div>

                <div className="il-new-modifier-grid">
                  <article>
                    <small>Demand</small>
                    <strong>{multiplierLabel(props.difficultyProfile.demand_mult)}</strong>
                  </article>
                  <article>
                    <small>Build Cost</small>
                    <strong>{multiplierLabel(props.difficultyProfile.capex_mult)}</strong>
                  </article>
                  <article>
                    <small>Operating Cost</small>
                    <strong>{multiplierLabel(props.difficultyProfile.opex_mult)}</strong>
                  </article>
                  <article>
                    <small>Maintenance</small>
                    <strong>{multiplierLabel(props.difficultyProfile.maintenance_mult)}</strong>
                  </article>
                  <article>
                    <small>Penalty</small>
                    <strong>{multiplierLabel(props.difficultyProfile.penalty_mult)}</strong>
                  </article>
                  <article>
                    <small>Unlock Cost</small>
                    <strong>{multiplierLabel(props.difficultyProfile.unlock_cost_mult)}</strong>
                  </article>
                  <article>
                    <small>Ancillary Revenue</small>
                    <strong>{multiplierLabel(props.difficultyProfile.ancillary_revenue_mult)}</strong>
                  </article>
                </div>
              </section>
            </div>
          ) : null}

          {props.step === 3 ? (
            <div className="il-new-stage-scroll" aria-label="Review and Launch">
              <section className="il-new-block">
                <h2>Launch Summary</h2>
                <div className="il-new-review-grid">
                  <article>
                    <small>Save</small>
                    <strong>{props.gameName.trim() || "Interlinked World"}</strong>
                  </article>
                  <article>
                    <small>Start Location</small>
                    <strong>
                      {props.selectedCityName ?? "No city selected"}
                      {props.selectedCountryName ? `, ${props.selectedCountryName}` : ""}
                    </strong>
                  </article>
                  <article>
                    <small>Difficulty</small>
                    <strong>{difficultyTitle}</strong>
                  </article>
                  <article>
                    <small>Starting Budget</small>
                    <strong>{formattedBudget}</strong>
                  </article>
                </div>
              </section>

              <section className="il-new-block il-new-briefing">
                <h2>WARNING</h2>
                <p>
                  Difficulty, starting budget, currency, and start location cannot be altered after save
                  creation.
                </p>
              </section>
            </div>
          ) : null}
        </section>

        {props.error ? <p className="il-new-error">{props.error}</p> : null}

        <footer className="il-new-actions">
          {props.step > 1 ? (
            <InterlinkedButton className="il-new-prev" tone="ghost" onClick={props.onPrev}>
              Previous
            </InterlinkedButton>
          ) : (
            <span aria-hidden="true" />
          )}
          {props.step < 3 ? (
            <InterlinkedButton className="il-new-next" tone="primary" onClick={props.onNext}>
              Next
            </InterlinkedButton>
          ) : (
            <InterlinkedButton className="il-new-create" tone="primary" onClick={props.onCreate} disabled={props.busy}>
              Create and Enter
            </InterlinkedButton>
          )}
        </footer>
      </section>
    </InterlinkedPageShell>
  );
}
