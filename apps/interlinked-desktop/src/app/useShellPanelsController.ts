import { useCallback, useEffect, useMemo, useState, type Dispatch, type SetStateAction } from "react";
import type { AppRoute, SessionKind, SimulationClock, SimulationSpeed } from "../types";
import type { BuildAction } from "../build/types";

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

type WorkspacePanelSurface =
  | "none"
  | "filters"
  | "network"
  | "missions"
  | "operations"
  | "fares"
  | "alerts"
  | "settings";

type WorkspaceOverlaySurface = "none" | "menu" | "command_palette" | "financial_dashboard";

type OverlayStateKind = "none" | "modal" | "blocking_overlay";

export type WorkspaceModeState = "view" | "build";
export type WorkspaceActiveTool = "line_builder" | "vehicle_manager" | "region_tool" | "none";

type WorkspaceSurfaceState = {
  activePanel: WorkspacePanelSurface;
  activeOverlay: WorkspaceOverlaySurface;
  commandPaletteQuery: string;
};

type WorkspaceSnapshot = {
  workspaceMode: WorkspaceModeState;
  activeTool: WorkspaceActiveTool;
  activePanel: WorkspacePanelSurface;
  activeOverlay: WorkspaceOverlaySurface;
  overlayState: OverlayStateKind;
  interactionSuppressed: boolean;
};

function resolveBooleanUpdate(update: SetStateAction<boolean>, previous: boolean): boolean {
  if (typeof update === "function") return update(previous);
  return update;
}

export function useShellPanelsController(args: {
  route: AppRoute;
  workspaceMode: WorkspaceModeState;
  buildAction?: BuildAction;
  sessionKind: SessionKind | null;
}) {
  const { route, workspaceMode, buildAction, sessionKind } = args;
  const inSession = route === "session_game" || route === "session_scenario";
  const [surfaceState, setSurfaceState] = useState<WorkspaceSurfaceState>({
    activePanel: "none",
    activeOverlay: "none",
    commandPaletteQuery: "",
  });

  const panelAllowed = useCallback(
    (panel: WorkspacePanelSurface): boolean => {
      if (!inSession) return false;
      if (panel === "none") return true;
      if (panel === "missions" || panel === "operations" || panel === "fares") {
        return sessionKind === "game";
      }
      return true;
    },
    [inSession, sessionKind, workspaceMode]
  );

  const overlayAllowed = useCallback(
    (overlay: WorkspaceOverlaySurface): boolean => {
      if (!inSession) return false;
      if (overlay === "none") return true;
      if (workspaceMode === "build") {
        return overlay === "menu" || overlay === "command_palette";
      }
      return true;
    },
    [inSession, workspaceMode]
  );

  const setPanelVisibility = useCallback(
    (panel: Exclude<WorkspacePanelSurface, "none">, update: SetStateAction<boolean>) => {
      setSurfaceState((previous) => {
        const wasVisible = previous.activePanel === panel;
        const shouldShow = resolveBooleanUpdate(update, wasVisible);
        if (!shouldShow) {
          if (!wasVisible) return previous;
          return { ...previous, activePanel: "none" };
        }
        if (!panelAllowed(panel)) return previous;
        return {
          ...previous,
          activePanel: panel,
          activeOverlay: "none",
        };
      });
    },
    [panelAllowed]
  );

  const setOverlayVisibility = useCallback(
    (overlay: Exclude<WorkspaceOverlaySurface, "none">, update: SetStateAction<boolean>) => {
      setSurfaceState((previous) => {
        const wasVisible = previous.activeOverlay === overlay;
        const shouldShow = resolveBooleanUpdate(update, wasVisible);
        if (!shouldShow) {
          if (!wasVisible) return previous;
          return { ...previous, activeOverlay: "none" };
        }
        if (!overlayAllowed(overlay)) return previous;
        return {
          ...previous,
          activeOverlay: overlay,
          commandPaletteQuery: overlay === "command_palette" ? "" : previous.commandPaletteQuery,
        };
      });
    },
    [overlayAllowed]
  );

  const closeActiveOverlay = useCallback((): boolean => {
    if (surfaceState.activeOverlay === "none") return false;
    setSurfaceState((previous) => {
      if (previous.activeOverlay === "none") return previous;
      return { ...previous, activeOverlay: "none" };
    });
    return true;
  }, [surfaceState.activeOverlay]);

  const closeActivePanel = useCallback((): boolean => {
    if (surfaceState.activePanel === "none") return false;
    setSurfaceState((previous) => {
      if (previous.activePanel === "none") return previous;
      return { ...previous, activePanel: "none" };
    });
    return true;
  }, [surfaceState.activePanel]);

  const dismissSurfaceByPriority = useCallback((): boolean => {
    if (surfaceState.activeOverlay === "financial_dashboard") {
      return closeActiveOverlay();
    }
    if (surfaceState.activeOverlay === "command_palette") {
      return closeActiveOverlay();
    }
    if (surfaceState.activeOverlay === "menu") {
      return closeActiveOverlay();
    }
    return closeActivePanel();
  }, [closeActiveOverlay, closeActivePanel, surfaceState.activeOverlay]);

  const openSettingsPanel = useCallback(() => {
    setPanelVisibility("settings", true);
  }, [setPanelVisibility]);

  const toggleFiltersPanel = useCallback(() => {
    setPanelVisibility("filters", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleMissionsPanel = useCallback(() => {
    setPanelVisibility("missions", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleNetworkPanel = useCallback(() => {
    setPanelVisibility("network", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleCountryInfoPanel = useCallback(() => {
    setPanelVisibility("operations", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleFarePanel = useCallback(() => {
    setPanelVisibility("fares", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleAlertsPanel = useCallback(() => {
    setPanelVisibility("alerts", (previous) => !previous);
  }, [setPanelVisibility]);

  const toggleMenuPanel = useCallback(() => {
    setOverlayVisibility("menu", (previous) => !previous);
  }, [setOverlayVisibility]);

  const openCommandPaletteFromHud = useCallback(() => {
    if (workspaceMode === "build") return;
    setOverlayVisibility("command_palette", true);
  }, [setOverlayVisibility, workspaceMode]);

  const openCommandPaletteFromShortcut = useCallback(() => {
    setOverlayVisibility("command_palette", true);
  }, [setOverlayVisibility]);

  const closeCommandPalette = useCallback(() => {
    setOverlayVisibility("command_palette", false);
  }, [setOverlayVisibility]);

  const setCommandPaletteQuery: Dispatch<SetStateAction<string>> = useCallback((update) => {
    setSurfaceState((previous) => {
      const nextValue =
        typeof update === "function" ? update(previous.commandPaletteQuery) : update;
      if (nextValue === previous.commandPaletteQuery) return previous;
      return {
        ...previous,
        commandPaletteQuery: nextValue,
      };
    });
  }, []);

  const setShowCountryInfo: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("operations", update),
    [setPanelVisibility]
  );
  const setShowFares: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("fares", update),
    [setPanelVisibility]
  );
  const setShowFilters: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("filters", update),
    [setPanelVisibility]
  );
  const setShowMissions: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("missions", update),
    [setPanelVisibility]
  );
  const setShowNetwork: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("network", update),
    [setPanelVisibility]
  );
  const setShowAlerts: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("alerts", update),
    [setPanelVisibility]
  );
  const setShowSettings: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setPanelVisibility("settings", update),
    [setPanelVisibility]
  );
  const setShowMenu: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setOverlayVisibility("menu", update),
    [setOverlayVisibility]
  );
  const setShowFinancialDashboard: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setOverlayVisibility("financial_dashboard", update),
    [setOverlayVisibility]
  );
  const setCommandPaletteOpen: Dispatch<SetStateAction<boolean>> = useCallback(
    (update) => setOverlayVisibility("command_palette", update),
    [setOverlayVisibility]
  );

  const activeTool = useMemo<WorkspaceActiveTool>(() => {
    if (workspaceMode === "build") {
      if (buildAction && buildAction !== "select") return "line_builder";
      return "none";
    }
    if (surfaceState.activePanel === "operations") return "region_tool";
    return "none";
  }, [buildAction, surfaceState.activePanel, workspaceMode]);

  const overlayState = useMemo<OverlayStateKind>(() => {
    if (surfaceState.activeOverlay === "none") return "none";
    return "modal";
  }, [surfaceState.activeOverlay]);

  const workspace = useMemo<WorkspaceSnapshot>(
    () => ({
      workspaceMode,
      activeTool,
      activePanel: surfaceState.activePanel,
      activeOverlay: surfaceState.activeOverlay,
      overlayState,
      interactionSuppressed: overlayState !== "none",
    }),
    [activeTool, overlayState, surfaceState.activeOverlay, surfaceState.activePanel, workspaceMode]
  );

  useEffect(() => {
    if (workspaceMode !== "build") return;
    setSurfaceState((previous) => {
      if (previous.activePanel === "none" && previous.activeOverlay === "none") return previous;
      return {
        ...previous,
        activePanel: "none",
        activeOverlay: "none",
      };
    });
  }, [workspaceMode]);

  useEffect(() => {
    if (inSession) return;
    setSurfaceState((previous) => {
      if (previous.activePanel === "none" && previous.activeOverlay === "none") return previous;
      return {
        ...previous,
        activePanel: "none",
        activeOverlay: "none",
      };
    });
  }, [inSession]);

  useEffect(() => {
    if (sessionKind === "game") return;
    setSurfaceState((previous) => {
      if (
        previous.activePanel !== "missions" &&
        previous.activePanel !== "operations" &&
        previous.activePanel !== "fares"
      ) {
        return previous;
      }
      return {
        ...previous,
        activePanel: "none",
      };
    });
  }, [sessionKind]);

  const showCountryInfo = surfaceState.activePanel === "operations";
  const showFares = surfaceState.activePanel === "fares";
  const showFilters = surfaceState.activePanel === "filters";
  const showMissions = surfaceState.activePanel === "missions";
  const showNetwork = surfaceState.activePanel === "network";
  const showAlerts = surfaceState.activePanel === "alerts";
  const showSettings = surfaceState.activePanel === "settings";
  const showMenu = surfaceState.activeOverlay === "menu";
  const showFinancialDashboard = surfaceState.activeOverlay === "financial_dashboard";
  const commandPaletteOpen = surfaceState.activeOverlay === "command_palette";

  return {
    workspace,
    showCountryInfo,
    setShowCountryInfo,
    showFares,
    setShowFares,
    showFilters,
    setShowFilters,
    showMissions,
    setShowMissions,
    showNetwork,
    setShowNetwork,
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
    commandPaletteQuery: surfaceState.commandPaletteQuery,
    setCommandPaletteQuery,
    openSettingsPanel,
    toggleFiltersPanel,
    toggleMissionsPanel,
    toggleNetworkPanel,
    toggleCountryInfoPanel,
    toggleFarePanel,
    toggleAlertsPanel,
    toggleMenuPanel,
    openCommandPaletteFromHud,
    openCommandPaletteFromShortcut,
    closeCommandPalette,
    closeActivePanel,
    closeActiveOverlay,
    dismissSurfaceByPriority,
  };
}

export function useShellCommandOrchestration(args: {
  route: AppRoute;
  workspaceMode: WorkspaceModeState;
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
  blockingOverlayActive?: boolean;
  onDismissBlockingOverlay?: () => boolean | void;
  canCancelWorkspaceToolStep?: boolean;
  onCancelWorkspaceToolStep?: () => void;
  canClearWorkspaceSelection?: boolean;
  onClearWorkspaceSelection?: () => void;
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
    blockingOverlayActive = false,
    onDismissBlockingOverlay,
    canCancelWorkspaceToolStep = false,
    onCancelWorkspaceToolStep,
    canClearWorkspaceSelection = false,
    onClearWorkspaceSelection,
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
        label: "Toggle Operations",
        detail: "Open the regions and strategic scope surface",
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
        if (blockingOverlayActive) {
          const didDismissBlockingOverlay = onDismissBlockingOverlay?.();
          if (didDismissBlockingOverlay === true) return;
          return;
        }
        if (panels.dismissSurfaceByPriority()) return;
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
        if (canCancelWorkspaceToolStep) {
          onCancelWorkspaceToolStep?.();
          return;
        }
        if (canClearWorkspaceSelection) {
          onClearWorkspaceSelection?.();
          return;
        }
        return;
      }
      if (!inSession) return;
      if (panels.workspace.interactionSuppressed) return;
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
    blockingOverlayActive,
    canCancelWorkspaceToolStep,
    canClearWorkspaceSelection,
    onCancelDeleteSelectedLine,
    onCancelWorkspaceToolStep,
    onCloseRollingStockEditor,
    onCloseScheduleEditor,
    onClearWorkspaceSelection,
    onDismissBlockingOverlay,
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
