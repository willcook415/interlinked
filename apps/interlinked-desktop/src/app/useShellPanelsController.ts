import { useCallback, useEffect, useMemo, useState } from "react";
import type { AppRoute, SessionKind, SimulationClock, SimulationSpeed } from "../types";

export type ShellCommandAction = {
  id: string;
  label: string;
  detail?: string;
  shortcut?: string;
  disabled?: boolean;
  run: () => void;
};

function isTextInputLike(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || tag === "select" || target.isContentEditable;
}

export function useShellPanelsController(args: {
  route: AppRoute;
  workspaceMode: "view" | "build";
  sessionKind: SessionKind | null;
}) {
  const { route, workspaceMode, sessionKind } = args;
  const [showCountryInfo, setShowCountryInfo] = useState(false);
  const [showFares, setShowFares] = useState(false);
  const [showFilters, setShowFilters] = useState(false);
  const [showMissions, setShowMissions] = useState(false);
  const [showAlerts, setShowAlerts] = useState(false);
  const [showMenu, setShowMenu] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [showFinancialDashboard, setShowFinancialDashboard] = useState(false);
  const [commandPaletteOpen, setCommandPaletteOpen] = useState(false);
  const [commandPaletteQuery, setCommandPaletteQuery] = useState("");

  const openSettingsPanel = useCallback(() => {
    if (workspaceMode === "build") return;
    setShowSettings(true);
    setShowAlerts(false);
    setShowMenu(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setCommandPaletteOpen(false);
  }, [workspaceMode]);

  const toggleFiltersPanel = useCallback(() => {
    if (workspaceMode === "build") return;
    setShowFilters((previous) => !previous);
    setShowAlerts(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [workspaceMode]);

  const toggleMissionsPanel = useCallback(() => {
    if (workspaceMode === "build" || sessionKind !== "game") return;
    setShowMissions((previous) => !previous);
    setShowAlerts(false);
    setShowFilters(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [sessionKind, workspaceMode]);

  const toggleCountryInfoPanel = useCallback(() => {
    if (workspaceMode === "build" || sessionKind !== "game") return;
    setShowCountryInfo((previous) => !previous);
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [sessionKind, workspaceMode]);

  const toggleFarePanel = useCallback(() => {
    if (workspaceMode === "build" || sessionKind !== "game") return;
    setShowFares((previous) => !previous);
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [sessionKind, workspaceMode]);

  const toggleAlertsPanel = useCallback(() => {
    if (workspaceMode === "build") return;
    setShowAlerts((previous) => !previous);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [workspaceMode]);

  const toggleMenuPanel = useCallback(() => {
    setShowMenu((previous) => !previous);
    setShowSettings(false);
    setShowAlerts(false);
    setCommandPaletteOpen(false);
  }, []);

  const openCommandPaletteFromHud = useCallback(() => {
    if (workspaceMode === "build") return;
    setShowMenu(false);
    setShowSettings(false);
    setShowAlerts(false);
    setCommandPaletteQuery("");
    setCommandPaletteOpen(true);
  }, [workspaceMode]);

  const openCommandPaletteFromShortcut = useCallback(() => {
    setShowAlerts(false);
    setCommandPaletteOpen(true);
    setCommandPaletteQuery("");
  }, []);

  const closeCommandPalette = useCallback(() => {
    setCommandPaletteOpen(false);
  }, []);

  useEffect(() => {
    if (workspaceMode !== "build") return;
    setShowAlerts(false);
    setShowFilters(false);
    setShowMissions(false);
    setShowCountryInfo(false);
    setShowFares(false);
    setShowMenu(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
  }, [workspaceMode]);

  useEffect(() => {
    const inSession = route === "session_game" || route === "session_scenario";
    if (inSession) return;
    setShowAlerts(false);
    setShowSettings(false);
    setCommandPaletteOpen(false);
    setShowMenu(false);
  }, [route]);

  return {
    showCountryInfo,
    setShowCountryInfo,
    showFares,
    setShowFares,
    showFilters,
    setShowFilters,
    showMissions,
    setShowMissions,
    showAlerts,
    setShowAlerts,
    showMenu,
    setShowMenu,
    showSettings,
    setShowSettings,
    showFinancialDashboard,
    setShowFinancialDashboard,
    commandPaletteOpen,
    setCommandPaletteOpen,
    commandPaletteQuery,
    setCommandPaletteQuery,
    openSettingsPanel,
    toggleFiltersPanel,
    toggleMissionsPanel,
    toggleCountryInfoPanel,
    toggleFarePanel,
    toggleAlertsPanel,
    toggleMenuPanel,
    openCommandPaletteFromHud,
    openCommandPaletteFromShortcut,
    closeCommandPalette,
  };
}

export function useShellCommandOrchestration(args: {
  route: AppRoute;
  workspaceMode: "view" | "build";
  clock: SimulationClock | null;
  hasActiveSessionBundle: boolean;
  panels: ReturnType<typeof useShellPanelsController>;
  onSaveSession: () => Promise<void>;
  onSaveQuit: () => Promise<void>;
  onSetRunning: (running: boolean) => Promise<void>;
  onSetSpeed: (speed: SimulationSpeed) => Promise<void>;
  onEnterBuildMode: () => void;
  onLeaveBuildMode: () => void;
  rollingStockEditorOpen: boolean;
  onCloseRollingStockEditor: () => void;
  scheduleEditorOpen: boolean;
  onCloseScheduleEditor: () => void;
  lineDeleteDialogOpen: boolean;
  onCancelDeleteSelectedLine: () => void;
}) {
  const {
    route,
    workspaceMode,
    clock,
    hasActiveSessionBundle,
    panels,
    onSaveSession,
    onSaveQuit,
    onSetRunning,
    onSetSpeed,
    onEnterBuildMode,
    onLeaveBuildMode,
    rollingStockEditorOpen,
    onCloseRollingStockEditor,
    scheduleEditorOpen,
    onCloseScheduleEditor,
    lineDeleteDialogOpen,
    onCancelDeleteSelectedLine,
  } = args;

  const inSession = route === "session_game" || route === "session_scenario";
  const inGame = route === "session_game";
  const inViewMode = workspaceMode === "view";
  const canRun = hasActiveSessionBundle && inSession && Boolean(clock);

  const commandActions = useMemo<ShellCommandAction[]>(
    () => [
      {
        id: "save",
        label: "Quick Save",
        detail: "Save the active session",
        shortcut: "Ctrl/Cmd+S",
        disabled: !canRun,
        run: () => {
          void onSaveSession();
        },
      },
      {
        id: "save_quit",
        label: "Save and Quit",
        detail: "Return to main menu",
        shortcut: "Ctrl/Cmd+Shift+S",
        disabled: !canRun,
        run: () => {
          void onSaveQuit();
        },
      },
      {
        id: "toggle_running",
        label: clock?.running ? "Pause Simulation" : "Start Simulation",
        detail: "Toggle simulation runtime",
        shortcut: "Space",
        disabled: !canRun || workspaceMode === "build",
        run: () => {
          if (!clock) return;
          void onSetRunning(!clock.running);
        },
      },
      {
        id: "speed_1x",
        label: "Set Speed 1x",
        shortcut: "1",
        disabled: !canRun || workspaceMode === "build",
        run: () => {
          void onSetSpeed(1);
        },
      },
      {
        id: "speed_2x",
        label: "Set Speed 2x",
        shortcut: "2",
        disabled: !canRun || workspaceMode === "build",
        run: () => {
          void onSetSpeed(2);
        },
      },
      {
        id: "speed_4x",
        label: "Set Speed 4x",
        shortcut: "3",
        disabled: !canRun || workspaceMode === "build",
        run: () => {
          void onSetSpeed(4);
        },
      },
      {
        id: "enter_build",
        label: "Enter Build Mode",
        shortcut: "B",
        disabled: !inSession || workspaceMode === "build",
        run: () => {
          onEnterBuildMode();
        },
      },
      {
        id: "exit_build",
        label: "Exit Build Mode",
        shortcut: "V",
        disabled: !inSession || workspaceMode !== "build",
        run: () => {
          onLeaveBuildMode();
        },
      },
      {
        id: "open_filters",
        label: "Toggle Map Filters",
        detail: "Show or hide map layer controls",
        disabled: !inSession || !inViewMode,
        run: () => {
          panels.toggleFiltersPanel();
        },
      },
      {
        id: "open_counties",
        label: "Toggle County Info",
        detail: "Open county progression panel",
        disabled: !inGame || !inViewMode,
        run: () => {
          panels.toggleCountryInfoPanel();
        },
      },
      {
        id: "open_alerts",
        label: "Open Alerts Center",
        detail: "Review grouped alerts and jump to affected lines/stations",
        disabled: !inSession || workspaceMode === "build",
        run: () => {
          panels.toggleAlertsPanel();
        },
      },
      {
        id: "open_settings",
        label: "Open Settings",
        detail: "Display, accessibility, and diagnostics",
        shortcut: "Ctrl/Cmd+,",
        disabled: !inSession || workspaceMode === "build",
        run: () => {
          panels.openSettingsPanel();
        },
      },
    ],
    [
      canRun,
      clock,
      inGame,
      inSession,
      inViewMode,
      onEnterBuildMode,
      onLeaveBuildMode,
      onSaveQuit,
      onSaveSession,
      onSetRunning,
      onSetSpeed,
      panels,
      workspaceMode,
    ]
  );

  const runPaletteCommand = useCallback(
    (commandId: string) => {
      const command = commandActions.find((item) => item.id === commandId);
      if (!command || command.disabled) return;
      command.run();
      panels.closeCommandPalette();
      panels.setCommandPaletteQuery("");
    },
    [commandActions, panels]
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (isTextInputLike(event.target)) return;
      const isMeta = event.metaKey || event.ctrlKey;
      if (isMeta && event.key.toLowerCase() === "k") {
        event.preventDefault();
        if (!inSession) return;
        panels.openCommandPaletteFromShortcut();
        return;
      }
      if (isMeta && event.key === ",") {
        event.preventDefault();
        if (!inSession || workspaceMode === "build") return;
        panels.openSettingsPanel();
        return;
      }
      if (isMeta && event.shiftKey && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (!inSession) return;
        void onSaveQuit();
        return;
      }
      if (isMeta && event.key.toLowerCase() === "s") {
        event.preventDefault();
        if (!inSession) return;
        void onSaveSession();
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        if (panels.commandPaletteOpen) {
          panels.closeCommandPalette();
          return;
        }
        if (panels.showSettings) {
          panels.setShowSettings(false);
          return;
        }
        if (panels.showAlerts) {
          panels.setShowAlerts(false);
          return;
        }
        if (panels.showFinancialDashboard) {
          panels.setShowFinancialDashboard(false);
          return;
        }
        if (rollingStockEditorOpen) {
          onCloseRollingStockEditor();
          return;
        }
        if (scheduleEditorOpen) {
          onCloseScheduleEditor();
          return;
        }
        if (lineDeleteDialogOpen) {
          onCancelDeleteSelectedLine();
          return;
        }
        if (panels.showMenu) {
          panels.setShowMenu(false);
          return;
        }
        if (panels.showFilters) {
          panels.setShowFilters(false);
          return;
        }
        if (panels.showMissions) {
          panels.setShowMissions(false);
          return;
        }
        if (panels.showCountryInfo) {
          panels.setShowCountryInfo(false);
          return;
        }
        if (panels.showFares) {
          panels.setShowFares(false);
        }
        return;
      }
      if (!inSession) return;
      if (event.key === " " && workspaceMode !== "build" && clock) {
        event.preventDefault();
        void onSetRunning(!clock.running);
        return;
      }
      if (event.key === "1" && workspaceMode !== "build") {
        event.preventDefault();
        void onSetSpeed(1);
        return;
      }
      if (event.key === "2" && workspaceMode !== "build") {
        event.preventDefault();
        void onSetSpeed(2);
        return;
      }
      if (event.key === "3" && workspaceMode !== "build") {
        event.preventDefault();
        void onSetSpeed(4);
        return;
      }
      if (event.key.toLowerCase() === "b" && workspaceMode !== "build") {
        event.preventDefault();
        onEnterBuildMode();
        return;
      }
      if (event.key.toLowerCase() === "v" && workspaceMode === "build") {
        event.preventDefault();
        onLeaveBuildMode();
      }
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    clock,
    inSession,
    lineDeleteDialogOpen,
    onCancelDeleteSelectedLine,
    onCloseRollingStockEditor,
    onCloseScheduleEditor,
    onEnterBuildMode,
    onLeaveBuildMode,
    onSaveQuit,
    onSaveSession,
    onSetRunning,
    onSetSpeed,
    panels,
    rollingStockEditorOpen,
    scheduleEditorOpen,
    workspaceMode,
  ]);

  return {
    commandActions,
    runPaletteCommand,
  };
}
