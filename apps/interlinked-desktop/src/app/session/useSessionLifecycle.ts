import { useCallback, useEffect } from "react";

import {
  continueLatestGame as continueLatestGameRequest,
  createGame as createGameRequest,
  createScenario as createScenarioRequest,
  deleteSave as deleteSaveRequest,
  expediteFleetDelivery as expediteFleetDeliveryRequest,
  getRuntimeSnapshot,
  installCountryPack as installCountryPackRequest,
  listCities,
  listCountries,
  listCountryPackStatus,
  listDemandCoverage,
  listDeletedSaves,
  listGameSaves,
  listRegions,
  listScenarioSaves,
  loadGameSave as loadGameSaveRequest,
  loadMapRuntimeConfig,
  loadScenarioSave as loadScenarioSaveRequest,
  openProject,
  purgeDeletedSave as purgeDeletedSaveRequest,
  restoreDeletedSave as restoreDeletedSaveRequest,
  saveAndQuit as saveAndQuitRequest,
  saveSession as saveSessionRequest,
  uninstallCountryPack as uninstallCountryPackRequest,
} from "../../api/desktopApi";
import {
  sameEconomy,
  sameRuntimeTrains,
  sameServiceLoads,
} from "../runtimeFreshness";
import type {
  CountryOption,
  CountryPackStatus,
  DemandCoverageMeta,
  DeleteSaveResult,
  DeletedSaveMeta,
  GameCreatePayload,
  GameSaveMeta,
  InstallResult,
  OpenSessionResult,
  PurgeSaveResult,
  RegionStatus,
  RestoreSaveResult,
  ScenarioSaveMeta,
  UninstallResult,
} from "../../types";
import type {
  MapBootProgressPayload,
  UseSessionControllerParams,
  WithBusy,
} from "./contracts";

function finiteNumber(value: number | null | undefined, fallback = 0): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function parsePositiveBudget(value: string): number | null {
  const normalized = value.replace(/[^\d.-]/g, "").trim();
  if (!normalized) return null;
  const parsed = Number(normalized);
  if (!Number.isFinite(parsed) || parsed <= 0) return null;
  return parsed;
}

type UseSessionLifecycleHookParams = {
  params: UseSessionControllerParams;
  withBusy: WithBusy;
};

type UseSessionLifecycleResult = {
  applyOpenedSession: (opened: OpenSessionResult) => void;
  refreshLibraries: () => Promise<void>;
  onCountryChanged: (iso2: string) => Promise<void>;
  installCountryPack: (iso2: string) => Promise<void>;
  uninstallCountryPack: (iso2: string) => Promise<void>;
  continueLatestGame: () => Promise<void>;
  loadGameSave: (saveId: string) => Promise<void>;
  loadScenarioSave: (saveId: string) => Promise<void>;
  deleteSave: (saveId: string, name: string) => Promise<void>;
  restoreDeletedSave: (deletedId: string) => Promise<void>;
  purgeDeletedSave: (deletedId: string) => Promise<void>;
  createGame: () => Promise<void>;
  createScenario: () => Promise<void>;
  saveSession: () => Promise<void>;
  saveQuit: () => Promise<void>;
  handleMapBootProgress: (payload: MapBootProgressPayload) => void;
  retryMapLoad: () => void;
  expediteFleetDelivery: (delivery: {
    id: string;
    orderId: string;
    label: string;
    lineId: string;
    lineName: string;
  }) => Promise<void>;
};

export function useSessionLifecycle({
  params,
  withBusy,
}: UseSessionLifecycleHookParams): UseSessionLifecycleResult {
  const markMapRuntimeConfigStarted = params.lifecycle.markMapRuntimeConfigStarted;
  const markMapRuntimeConfigReady = params.lifecycle.markMapRuntimeConfigReady;
  const markMapRuntimeConfigFailed = params.lifecycle.markMapRuntimeConfigFailed;

  const refreshDemandCoverage = useCallback(
    async (projectPath?: string) => {
      const target = projectPath ?? params.bundle?.project_path;
      if (!target) {
        params.setDemandCoverage([]);
        return;
      }
      const rows = await listDemandCoverage(target).catch(
        () => [] as DemandCoverageMeta[]
      );
      params.setDemandCoverage(rows);
      const missing = rows.filter((row) => !row.installed).map((row) => row.country_iso2);
      if (missing.length > 0) {
        params.setDemandWarning(
          `Demand data not installed for ${missing.join(", ")}. Install demand surface packs to enable country coverage.`
        );
      } else {
        params.setDemandWarning(null);
      }
    },
    [params]
  );

  const refreshRegions = useCallback(
    async (projectPath?: string, preferredFocusId?: string | null) => {
      const target = projectPath ?? params.bundle?.project_path;
      if (!target) {
        params.setRegions([]);
        params.setFocusRegionId(null);
        params.setSelectedRegionId(null);
        return;
      }
      const rows = await listRegions(target).catch(() => [] as RegionStatus[]);
      params.setRegions(rows);
      if (rows.length === 0) {
        params.setFocusRegionId(null);
        params.setSelectedRegionId(null);
        return;
      }
      const focusFromManifest =
        preferredFocusId ?? params.bundle?.manifest.region_state?.primary_focus_region_id ?? null;
      const resolvedFocus =
        rows.find((row) => row.region_id === focusFromManifest)?.region_id ??
        rows.find((row) => row.active)?.region_id ??
        rows.find((row) => row.unlocked)?.region_id ??
        rows[0].region_id;
      params.setFocusRegionId(resolvedFocus);
      params.setSelectedRegionId((previous) => {
        if (previous && rows.some((row) => row.region_id === previous)) return previous;
        return resolvedFocus;
      });
    },
    [params]
  );

  const primeRuntimeSnapshot = useCallback(
    async (projectPath: string) => {
      const snapshot = await getRuntimeSnapshot(projectPath).catch(() => null);
      if (!snapshot) return;
      params.setRuntimeTelemetry(snapshot.telemetry ?? null);
      const capturedAt = finiteNumber(snapshot.captured_at_epoch_ms, 0);
      params.setSnapshotLatencyMs(capturedAt > 0 ? Math.max(Date.now() - capturedAt, 0) : null);
      params.setTrainsAuthoritative(Boolean(snapshot.trains_authoritative));
      const nextRuntimeTrains = Array.isArray(snapshot.trains) ? snapshot.trains : [];
      params.setRuntimeTrains((previous) =>
        sameRuntimeTrains(previous, nextRuntimeTrains) ? previous : nextRuntimeTrains
      );
      params.setRuntimeStations(Array.isArray(snapshot.stations) ? snapshot.stations : []);
      params.setRuntimeLineOps(Array.isArray(snapshot.line_ops) ? snapshot.line_ops : []);
      if (snapshot.economy) {
        params.setLiveEconomy((previous) =>
          sameEconomy(previous, snapshot.economy ?? null) ? previous : snapshot.economy ?? null
        );
      }
      const nextServiceLoads: Record<string, number> = {};
      for (const row of snapshot.frame?.service_loads ?? []) {
        const serviceId = row.service_id?.trim();
        if (!serviceId) continue;
        const ratio = Number.isFinite(row.load_to_capacity) ? Math.max(row.load_to_capacity, 0) : 0;
        nextServiceLoads[serviceId] = Math.max(nextServiceLoads[serviceId] ?? 0, ratio);
      }
      params.setServiceLoadByServiceId((previous) =>
        sameServiceLoads(previous, nextServiceLoads) ? previous : nextServiceLoads
      );
    },
    [params]
  );

  const applyOpenedSession = useCallback(
    (opened: OpenSessionResult) => {
      params.setBundle(opened);
      params.setClock(opened.clock);
      params.setSaveStatus("");
      params.lifecycle.acceptOpenedSession(opened.manifest.session_kind);
      params.setRoute(opened.manifest.session_kind === "game" ? "session_game" : "session_scenario");
      params.setMapInstanceToken((value) => value + 1);
      void refreshDemandCoverage(opened.project_path);
      if (opened.manifest.session_kind === "game") {
        params.setShowCountryInfo(false);
        void refreshRegions(
          opened.project_path,
          opened.manifest.region_state?.primary_focus_region_id ?? null
        );
        void primeRuntimeSnapshot(opened.project_path);
      } else {
        params.setRegions([]);
        params.setFocusRegionId(null);
        params.setSelectedRegionId(null);
        params.setShowCountryInfo(false);
      }
    },
    [params, primeRuntimeSnapshot, refreshDemandCoverage, refreshRegions]
  );

  const onCountryChanged = useCallback(
    async (iso2: string) => {
      params.setSelectedCountryIso2(iso2);
      const cityList = await withBusy(async () => listCities(iso2));
      if (!cityList) return;
      params.setCities(cityList);
      params.setSelectedCityId(cityList[0]?.geonameid ?? null);
      params.setCitySearch("");
    },
    [params, withBusy]
  );

  const refreshLibraries = useCallback(async () => {
    const [games, scenarios, deleted, countryList, packList] = await Promise.all([
      listGameSaves().catch(() => [] as GameSaveMeta[]),
      listScenarioSaves().catch(() => [] as ScenarioSaveMeta[]),
      listDeletedSaves().catch(() => [] as DeletedSaveMeta[]),
      listCountries().catch(() => [] as CountryOption[]),
      listCountryPackStatus().catch(() => [] as CountryPackStatus[]),
    ]);
    params.setSaveLibrary({
      games,
      scenarios,
      deleted,
    });
    params.setCountries(countryList);
    params.setCountryPacks(packList);
    if (countryList.length > 0 && !params.selectedCountryIso2) {
      const eligibleIso = packList.find((pack) => pack.eligible)?.country_iso2 ?? countryList[0].iso2;
      void onCountryChanged(eligibleIso);
    }
  }, [onCountryChanged, params]);

  const continueLatestGame = useCallback(async () => {
    params.lifecycle.beginProjectSelection("Opening latest game save...");
    const opened = await withBusy(async () => continueLatestGameRequest());
    if (opened) {
      applyOpenedSession(opened);
      return;
    }
    params.lifecycle.resetToAppHome();
  }, [applyOpenedSession, params.lifecycle, withBusy]);

  const loadGameSave = useCallback(
    async (saveId: string) => {
      params.lifecycle.beginProjectSelection("Opening game save...");
      const opened = await withBusy(async () => loadGameSaveRequest(saveId));
      if (opened) {
        applyOpenedSession(opened);
        return;
      }
      params.lifecycle.resetToAppHome();
    },
    [applyOpenedSession, params.lifecycle, withBusy]
  );

  const loadScenarioSave = useCallback(
    async (saveId: string) => {
      params.lifecycle.beginProjectSelection("Opening scenario save...");
      const opened = await withBusy(async () => loadScenarioSaveRequest(saveId));
      if (opened) {
        applyOpenedSession(opened);
        return;
      }
      params.lifecycle.resetToAppHome();
    },
    [applyOpenedSession, params.lifecycle, withBusy]
  );

  const deleteSave = useCallback(
    async (saveId: string, name: string) => {
      const ok = window.confirm(`Move "${name}" to Recently Deleted?`);
      if (!ok) return;
      await withBusy(async () => deleteSaveRequest(saveId) as Promise<DeleteSaveResult>);
      await refreshLibraries();
    },
    [refreshLibraries, withBusy]
  );

  const restoreDeletedSave = useCallback(
    async (deletedId: string) => {
      await withBusy(
        async () => restoreDeletedSaveRequest(deletedId) as Promise<RestoreSaveResult>
      );
      await refreshLibraries();
    },
    [refreshLibraries, withBusy]
  );

  const purgeDeletedSave = useCallback(
    async (deletedId: string) => {
      const ok = window.confirm("Permanently delete this save?");
      if (!ok) return;
      await withBusy(
        async () => purgeDeletedSaveRequest(deletedId) as Promise<PurgeSaveResult>
      );
      await refreshLibraries();
    },
    [refreshLibraries, withBusy]
  );

  const createGame = useCallback(async () => {
    if (!params.selectedCountry || !params.selectedCity) {
      params.setError("Select a country and city before creating a game.");
      return;
    }
    if (!params.selectedCountryPack?.eligible) {
      params.setError(
        params.selectedCountryPack?.reason ??
          `Country ${params.selectedCountry.iso2} is not available yet.`
      );
      return;
    }
    const payload: GameCreatePayload = {
      name: params.newGameName.trim() || "Interlinked Game",
      country_iso2: params.selectedCountry.iso2,
      country_name: params.selectedCountry.name,
      city_id: params.selectedCity.geonameid,
      city_name: params.selectedCity.name,
      city_lon: params.selectedCity.lon,
      city_lat: params.selectedCity.lat,
      city_population: params.selectedCity.population,
      difficulty: params.newGameDifficulty,
      currency: params.newGameCurrency,
      starting_budget:
        parsePositiveBudget(params.newGameBudget) ||
        params.defaultBudgetFor(params.newGameDifficulty, params.newGameCurrency),
    };
    params.lifecycle.beginProjectSelection("Creating game...");
    const opened = await withBusy(async () => createGameRequest(payload));
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
      return;
    }
    params.lifecycle.resetToAppHome();
  }, [applyOpenedSession, params, refreshLibraries, withBusy]);

  const createScenario = useCallback(async () => {
    params.lifecycle.beginProjectSelection("Creating scenario...");
    const opened = await withBusy(async () =>
      createScenarioRequest(params.scenarioName.trim() || "Interlinked Scenario")
    );
    if (opened) {
      applyOpenedSession(opened);
      await refreshLibraries();
      return;
    }
    params.lifecycle.resetToAppHome();
  }, [applyOpenedSession, params.lifecycle, params.scenarioName, refreshLibraries, withBusy]);

  const saveSession = useCallback(async () => {
    if (!params.bundle) return;
    if (params.build.workspaceMode === "build" && params.build.isDirty) {
      params.build.setBuilderError("Apply or cancel the current build draft before saving the session.");
      return;
    }
    const result = await withBusy(async () =>
      saveSessionRequest(params.bundle!.project_path, {
        scenario_document: params.bundle!.scenario,
      })
    );
    if (result) {
      params.setSaveStatus(`Saved ${result.updated_at}`);
      params.playUiCue("confirm");
    }
  }, [params, withBusy]);

  const saveQuit = useCallback(async () => {
    if (!params.bundle) return;
    if (params.build.workspaceMode === "build" && params.build.isDirty) {
      params.build.setBuilderError("Apply or cancel the current build draft before saving and quitting.");
      return;
    }
    const result = await withBusy(async () =>
      saveAndQuitRequest(params.bundle!.project_path, {
        scenario_document: params.bundle!.scenario,
      })
    );
    if (!result) return;
    params.setBundle(null);
    params.setClock(null);
    params.setDemandCoverage([]);
    params.setDemandWarning(null);
    params.setRegions([]);
    params.setFocusRegionId(null);
    params.setSelectedRegionId(null);
    params.setShowCountryInfo(false);
    params.setRoute("home");
    params.setShowMenu(false);
    params.lifecycle.resetToAppHome();
    params.setSaveStatus(`Saved ${result.updated_at}`);
    params.playUiCue("confirm");
    await refreshLibraries();
  }, [params, refreshLibraries, withBusy]);

  const handleMapBootProgress = useCallback(
    (payload: MapBootProgressPayload) => {
      params.lifecycle.publishMapBootProgress(payload);
    },
    [params]
  );

  const retryMapLoad = useCallback(() => {
    params.lifecycle.beginRecovery("map", "Retrying map initialization...");
    params.setMapInstanceToken((value) => value + 1);
  }, [params]);

  const installCountryPack = useCallback(
    async (iso2: string) => {
      const result = await withBusy(async () =>
        installCountryPackRequest(iso2) as Promise<InstallResult>
      );
      if (!result) return;
      params.setSaveStatus(result.message);
      await refreshLibraries();
      await onCountryChanged(iso2);
    },
    [onCountryChanged, params.setSaveStatus, refreshLibraries, withBusy]
  );

  const uninstallCountryPack = useCallback(
    async (iso2: string) => {
      const result = await withBusy(async () =>
        uninstallCountryPackRequest(iso2) as Promise<UninstallResult>
      );
      if (!result) return;
      params.setSaveStatus(result.message);
      await refreshLibraries();
      await onCountryChanged(iso2);
    },
    [onCountryChanged, params.setSaveStatus, refreshLibraries, withBusy]
  );

  const expediteFleetDelivery = useCallback(
    async (delivery: {
      id: string;
      orderId: string;
      label: string;
      lineId: string;
      lineName: string;
    }) => {
      if (!params.bundle || !delivery.orderId.trim()) return;
      const confirmed = window.confirm(
        `Expedite ${delivery.label} on ${delivery.lineName}? This delivers immediately for a premium cost.`
      );
      if (!confirmed) return;
      const response = await withBusy(async () => {
        const result = await expediteFleetDeliveryRequest(
          params.bundle!.project_path,
          delivery.lineId,
          delivery.orderId
        );
        const reopened = await openProject(params.bundle!.project_path);
        return { result, reopened };
      });
      if (!response) return;
      applyOpenedSession(response.reopened);
      const costBaseLabel = Math.round(Math.max(response.result.expedite_cost_base, 0)).toLocaleString();
      params.setSaveStatus(
        `Expedited ${delivery.label} on ${delivery.lineName} for ${costBaseLabel} base cost.`
      );
    },
    [applyOpenedSession, params, withBusy]
  );

  useEffect(() => {
    void refreshLibraries();
    // Intentional one-shot bootstrap, matching prior shell behavior.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    let cancelled = false;
    if (!params.bundle || params.sessionKind !== "game") {
      params.setMapRuntimeConfig(null);
      return;
    }
    markMapRuntimeConfigStarted();
    void loadMapRuntimeConfig(params.bundle.project_path)
      .then((config) => {
        if (!cancelled) {
          params.setMapRuntimeConfig(config);
          markMapRuntimeConfigReady();
        }
      })
      .catch(() => {
        if (!cancelled) {
          params.setMapRuntimeConfig(null);
          markMapRuntimeConfigFailed(
            "Unable to load map runtime config. You can retry map loading."
          );
        }
      });
    return () => {
      cancelled = true;
    };
  }, [
    params.bundle?.project_path,
    params.sessionKind,
    params.setMapRuntimeConfig,
    markMapRuntimeConfigFailed,
    markMapRuntimeConfigReady,
    markMapRuntimeConfigStarted,
  ]);

  return {
    applyOpenedSession,
    refreshLibraries,
    onCountryChanged,
    installCountryPack,
    uninstallCountryPack,
    continueLatestGame,
    loadGameSave,
    loadScenarioSave,
    deleteSave,
    restoreDeletedSave,
    purgeDeletedSave,
    createGame,
    createScenario,
    saveSession,
    saveQuit,
    handleMapBootProgress,
    retryMapLoad,
    expediteFleetDelivery,
  };
}
