import type { NewGameFlowController } from "../app/useNewGameFlowController";
import type {
  AppRoute,
  CityOption,
  CountryOption,
  CountryPackStatus,
  DeletedSaveMeta,
  DifficultyProfile,
  GameSaveMeta,
  ScenarioSaveMeta,
} from "../types";
import HomeScreen from "./HomeScreen";
import LoadGameScreen from "./LoadGameScreen";
import LoadScenarioScreen from "./LoadScenarioScreen";
import NewGameScreen from "./NewGameScreen";
import NewScenarioScreen from "./NewScenarioScreen";

export default function AppRouteScreens(props: {
  route: AppRoute;
  canContinue: boolean;
  latestGameSave: GameSaveMeta | null;
  gameSaves: GameSaveMeta[];
  scenarioSaves: ScenarioSaveMeta[];
  deletedSaves: DeletedSaveMeta[];
  countries: CountryOption[];
  countryPacks: CountryPackStatus[];
  selectedCountryIso2: string;
  selectedCountryName: string | null;
  selectedCityId: number | null;
  selectedCityName: string | null;
  citySearch: string;
  filteredCities: CityOption[];
  busy: boolean;
  error: string | null;
  scenarioName: string;
  setScenarioName: (value: string) => void;
  selectedDifficultyProfile: DifficultyProfile;
  newGame: NewGameFlowController;
  onNextNewGameStep: () => void;
  onRouteHome: () => void;
  onRouteLoadGame: () => void;
  onRouteNewGame: () => void;
  onRouteNewScenario: () => void;
  onRouteLoadScenario: () => void;
  onContinueLatestGame: () => Promise<void>;
  onLoadGameSave: (saveId: string) => Promise<void>;
  onLoadScenarioSave: (saveId: string) => Promise<void>;
  onDeleteSave: (saveId: string, name: string) => Promise<void>;
  onRestoreDeletedSave: (deletedId: string) => Promise<void>;
  onPurgeDeletedSave: (deletedId: string) => Promise<void>;
  onCreateGame: () => Promise<void>;
  onCreateScenario: () => Promise<void>;
  onImportScenarioFromPicker: () => Promise<void>;
  onCountryChanged: (iso2: string) => Promise<void>;
  onInstallCountryPack: (iso2: string) => Promise<void>;
  onUninstallCountryPack: (iso2: string) => Promise<void>;
  onCitySearchChange: (value: string) => void;
  onCitySelected: (id: number | null) => void;
}) {
  if (props.route === "home") {
    return (
      <HomeScreen
        onContinueGame={() => {
          void props.onContinueLatestGame();
        }}
        onOpenRecentGame={(saveId) => {
          void props.onLoadGameSave(saveId);
        }}
        onLoadGame={props.onRouteLoadGame}
        onNewGame={props.onRouteNewGame}
        onNewScenario={props.onRouteNewScenario}
        onLoadScenario={props.onRouteLoadScenario}
        canContinue={props.canContinue}
        latestGameName={props.latestGameSave?.name ?? null}
        latestGameOpenedAt={props.latestGameSave?.last_opened_at ?? null}
        recentGames={props.gameSaves}
      />
    );
  }

  if (props.route === "new_game") {
    return (
      <NewGameScreen
        step={props.newGame.step}
        gameName={props.newGame.name}
        modeIntent={props.newGame.intent}
        difficulty={props.newGame.difficulty}
        difficultyProfile={props.selectedDifficultyProfile}
        currency={props.newGame.currency}
        budget={props.newGame.budget}
        countries={props.countries}
        countryPacks={props.countryPacks}
        selectedCountryIso2={props.selectedCountryIso2}
        selectedCityId={props.selectedCityId}
        selectedCountryName={props.selectedCountryName}
        selectedCityName={props.selectedCityName}
        citySearch={props.citySearch}
        filteredCities={props.filteredCities}
        busy={props.busy}
        error={props.error}
        onBack={props.onRouteHome}
        onNext={props.onNextNewGameStep}
        onPrev={props.newGame.previousStep}
        onCreate={() => {
          void props.onCreateGame();
        }}
        onNameChange={props.newGame.setName}
        onModeIntentChange={props.newGame.setIntent}
        onDifficultyChange={props.newGame.onDifficultyChanged}
        onCurrencyChange={props.newGame.onCurrencyChanged}
        onBudgetChange={props.newGame.onBudgetChanged}
        onCountryChange={(iso2) => {
          void props.onCountryChanged(iso2);
        }}
        onInstallPack={(iso2) => {
          void props.onInstallCountryPack(iso2);
        }}
        onUninstallPack={(iso2) => {
          void props.onUninstallCountryPack(iso2);
        }}
        onCitySearchChange={props.onCitySearchChange}
        onCitySelect={(id) => {
          props.onCitySelected(id);
        }}
      />
    );
  }

  if (props.route === "load_game") {
    return (
      <LoadGameScreen
        saves={props.gameSaves}
        deleted={props.deletedSaves.filter((row) => row.session_kind === "game")}
        onBack={props.onRouteHome}
        onOpen={(saveId) => {
          void props.onLoadGameSave(saveId);
        }}
        onDelete={(saveId, name) => {
          void props.onDeleteSave(saveId, name);
        }}
        onRestore={(deletedId) => {
          void props.onRestoreDeletedSave(deletedId);
        }}
        onPurge={(deletedId) => {
          void props.onPurgeDeletedSave(deletedId);
        }}
      />
    );
  }

  if (props.route === "new_scenario") {
    return (
      <NewScenarioScreen
        scenarioName={props.scenarioName}
        busy={props.busy}
        onNameChange={props.setScenarioName}
        onCreate={() => {
          void props.onCreateScenario();
        }}
        onBack={props.onRouteHome}
      />
    );
  }

  if (props.route === "load_scenario") {
    return (
      <LoadScenarioScreen
        saves={props.scenarioSaves}
        deleted={props.deletedSaves.filter((row) => row.session_kind === "scenario")}
        busy={props.busy}
        onBack={props.onRouteHome}
        onOpen={(saveId) => {
          void props.onLoadScenarioSave(saveId);
        }}
        onImport={() => {
          void props.onImportScenarioFromPicker();
        }}
        onDelete={(saveId, name) => {
          void props.onDeleteSave(saveId, name);
        }}
        onRestore={(deletedId) => {
          void props.onRestoreDeletedSave(deletedId);
        }}
        onPurge={(deletedId) => {
          void props.onPurgeDeletedSave(deletedId);
        }}
      />
    );
  }

  return null;
}
