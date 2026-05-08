import { memo } from "react";
import MapView from "../MapView";
import type { LinkModeFilter } from "./MapFiltersPanel";
import type { MapLineAction, MapStopAction, MapWorldPoint } from "../MapView";
import type { VehicleInspection } from "../app/vehicleInspection";
import type {
  DemandOverlayPayload,
  DemandOverlayType,
  MapRuntimeConfig,
  RegionStatus,
  ScenarioLite,
  SessionKind,
  SimulationClock,
  TrainRuntimeView,
} from "../types";

function MapCanvas(props: {
  scenario: ScenarioLite | null;
  projectPath?: string | null;
  mapRuntimeConfig: MapRuntimeConfig | null;
  clock: SimulationClock | null;
  showShapeStops: boolean;
  showZoneCentroids: boolean;
  showStations: boolean;
  showLinks: boolean;
  linkMode: LinkModeFilter;
  showDemandOverlay: boolean;
  demandOverlayType: DemandOverlayType;
  demandOverlayPayload: DemandOverlayPayload | null;
  startCenter: [number, number] | null;
  serviceLoadByServiceId?: Record<string, number>;
  runtimeTrains?: TrainRuntimeView[];
  trainsAuthoritative?: boolean;
  sessionKind?: SessionKind | null;
  visibleCountryIso2: string[] | null;
  regions: RegionStatus[];
  focusRegionId: string | null;
  activeRegionIds: string[];
  selectedRegionId: string | null;
  interactionMode?: "view" | "build";
  buildAction?: "select" | "place_station" | "start_line" | "add_station_to_line" | "delete";
  buildConstraintMode?: string | null;
  selectedStopId?: string | null;
  selectedLineId?: string | null;
  selectedVehicleId?: string | null;
  activeLineId?: string | null;
  focusStopId?: string | null;
  focusStopToken?: number;
  focusVehicleId?: string | null;
  focusVehicleToken?: number;
  previewAnchorPoint?: { x: number; y: number } | null;
  previewColor?: string | null;
  instanceToken?: number;
  onBootProgress?: (payload: {
    stage: "map_style" | "map_context" | "ready" | "error";
    progress: number;
    message: string;
    error?: string | null;
  }) => void;
  onSelectCounty: (regionId: string) => void;
  onStopAction?: (payload: MapStopAction) => void;
  onLineAction?: (payload: MapLineAction) => void;
  onVehicleAction?: (payload: VehicleInspection) => void;
  onClearVehicleSelection?: () => void;
  onMapPointAction?: (payload: MapWorldPoint) => void;
  onClearSelection?: () => void;
}) {
  return (
    <div className="map-canvas">
      <MapView
        key={`${props.mapRuntimeConfig?.style_url ?? "legacy-map"}:${props.instanceToken ?? 0}`}
        scenario={props.scenario}
        projectPath={props.projectPath}
        mapRuntimeConfig={props.mapRuntimeConfig}
        clock={props.clock}
        showShapeStops={props.showShapeStops}
        showZoneCentroids={props.showZoneCentroids}
        showStations={props.showStations}
        showLinks={props.showLinks}
        linkMode={props.linkMode}
        demandOverlayEnabled={props.showDemandOverlay}
        demandOverlayType={props.demandOverlayType}
        demandOverlayPayload={props.demandOverlayPayload}
        startCenter={props.startCenter}
        serviceLoadByServiceId={props.serviceLoadByServiceId}
        runtimeTrains={props.runtimeTrains}
        trainsAuthoritative={props.trainsAuthoritative}
        sessionKind={props.sessionKind}
        visibleCountryIso2={props.visibleCountryIso2}
        regions={props.regions}
        focusRegionId={props.focusRegionId}
        activeRegionIds={props.activeRegionIds}
        selectedRegionId={props.selectedRegionId}
        interactionMode={props.interactionMode}
        buildAction={props.buildAction}
        buildConstraintMode={props.buildConstraintMode}
        selectedStopId={props.selectedStopId}
        selectedLineId={props.selectedLineId}
        selectedVehicleId={props.selectedVehicleId}
        activeLineId={props.activeLineId}
        focusStopId={props.focusStopId}
        focusStopToken={props.focusStopToken}
        focusVehicleId={props.focusVehicleId}
        focusVehicleToken={props.focusVehicleToken}
        previewAnchorPoint={props.previewAnchorPoint}
        previewColor={props.previewColor}
        onBootProgress={props.onBootProgress}
        onSelectCounty={props.onSelectCounty}
        onStopAction={props.onStopAction}
        onLineAction={props.onLineAction}
        onVehicleAction={props.onVehicleAction}
        onClearVehicleSelection={props.onClearVehicleSelection}
        onMapPointAction={props.onMapPointAction}
        onClearSelection={props.onClearSelection}
      />
    </div>
  );
}

export default memo(MapCanvas);
