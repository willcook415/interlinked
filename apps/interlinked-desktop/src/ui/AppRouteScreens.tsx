import type { NewGameFlowController } from "../app/useNewGameFlowController";
import type { SaveBrowserViewModel } from "../app/useSaveBrowserController";
import type {
  AppRoute,
  CityOption,
  CountryOption,
  CountryPackStatus,
  DifficultyProfile,
  SaveBrowserEntry,
  SaveBrowserSortKey,
  SaveBrowserViewGroup,
  SessionKind,
} from "../types";
import HomeScreen from "./HomeScreen";
import LoadGameScreen from "./LoadGameScreen";
import LoadScenarioScreen from "./LoadScenarioScreen";
import NewGameScreen from "./NewGameScreen";
import NewScenarioScreen from "./NewScenarioScreen";

export default function AppRouteScreens(props: {
  route: AppRoute;
  canContinue: boolean;
  latestGameSave: SaveBrowserEntry | null;
  gameBrowserView: SaveBrowserViewModel;
  scenarioBrowserView: SaveBrowserViewModel;
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
  onOpenSettings: () => void;
  onContinueLatestGame: () => Promise<void>;
  onLoadGameSave: (saveId: string) => Promise<void>;
  onLoadScenarioSave: (saveId: string) => Promise<void>;
  onSaveBrowserQueryChange: (kind: SessionKind, query: string) => void;
  onSaveBrowserSortChange: (kind: SessionKind, sortKey: SaveBrowserSortKey) => void;
  onSaveBrowserGroupChange: (kind: SessionKind, group: SaveBrowserViewGroup) => void;
  onSaveBrowserSelectProject: (kind: SessionKind, projectId: string | null) => void;
  onDeleteSave: (saveId: string, name: string) => Promise<void>;
  onRestoreDeletedSave: (deletedId: string) => Promise<void>;
  onPurgeDeletedSave: (deletedId: string) => Promise<void>;
  onCreateGame: () => Promise<void>;
  onCreateScenario: () => Promise<void>;
  onImportScenarioFromPicker: () => Promise<void>;
  onCountryChanged: (iso2: string) => Promise<void>;
  onInstallCountryPack: (iso2: string) => Promise<void>;
  onCitySearchChange: (value: string) => void;
  onCitySelected: (id: number | null) => void;
}) {
  if (props.route === "home") {
    return (
      <HomeScreen
        onContinueGame={() => {
          void props.onContinueLatestGame();
        }}
        onLoadGame={props.onRouteLoadGame}
        onNewGame={props.onRouteNewGame}
        onOpenSettings={props.onOpenSettings}
        canContinue={props.canContinue}
        latestGame={props.latestGameSave}
      />
    );
  }

  if (props.route === "new_game") {
    return (
      <NewGameScreen
        step={props.newGame.step}
        gameName={props.newGame.name}
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
        onDifficultyChange={props.newGame.onDifficultyChanged}
        onCurrencyChange={props.newGame.onCurrencyChanged}
        onCountryChange={(iso2) => {
          void props.onCountryChanged(iso2);
        }}
        onInstallPack={(iso2) => {
          void props.onInstallCountryPack(iso2);
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
        busy={props.busy}
        view={props.gameBrowserView}
        onBack={props.onRouteHome}
        onQueryChange={(query) => props.onSaveBrowserQueryChange("game", query)}
        onSortChange={(sortKey) => props.onSaveBrowserSortChange("game", sortKey)}
        onSelect={(projectId) => props.onSaveBrowserSelectProject("game", projectId)}
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
        view={props.scenarioBrowserView}
        busy={props.busy}
        onBack={props.onRouteHome}
        onQueryChange={(query) => props.onSaveBrowserQueryChange("scenario", query)}
        onSortChange={(sortKey) => props.onSaveBrowserSortChange("scenario", sortKey)}
        onGroupChange={(group) => props.onSaveBrowserGroupChange("scenario", group)}
        onSelect={(projectId) => props.onSaveBrowserSelectProject("scenario", projectId)}
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
