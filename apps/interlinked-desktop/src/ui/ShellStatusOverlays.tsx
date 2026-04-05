import type { SessionBootState } from "../app/useSessionController";

export default function ShellStatusOverlays(props: {
  busy: boolean;
  showSessionBootOverlay: boolean;
  sessionBootState: SessionBootState;
  onRetryMapLoad: () => void;
  isOffline: boolean;
  showPausedBanner: boolean;
  saveStatus: string;
  onDismissSaveStatus: () => void;
  onboardingActive: boolean;
  onboardingStep: number;
  onboardingStepCount: number;
  onboardingTitle: string;
  onboardingDescription: string;
  onSkipOnboarding: () => void;
  onAdvanceOnboarding: () => void;
}) {
  return (
    <>
      {props.busy ? (
        <div className="app-status-overlay">
          <div className="app-status-card">
            <strong>Working...</strong>
            <span>Preparing data and applying your request.</span>
          </div>
        </div>
      ) : null}

      {props.showSessionBootOverlay ? (
        <div className="session-boot-overlay">
          <div className="session-boot-card">
            <strong>{props.sessionBootState.stage === "error" ? "Load issue" : "Loading session"}</strong>
            <span>{props.sessionBootState.message || "Preparing map and runtime state..."}</span>
            <div className="session-boot-progress">
              <div
                className="session-boot-progress-fill"
                style={{ width: `${Math.max(Math.min(props.sessionBootState.progress, 1), 0) * 100}%` }}
              />
            </div>
            {props.sessionBootState.error ? <p className="form-error">{props.sessionBootState.error}</p> : null}
            {props.sessionBootState.stage === "error" ? (
              <div className="session-boot-actions">
                <button onClick={props.onRetryMapLoad}>Retry Map Load</button>
              </div>
            ) : null}
          </div>
        </div>
      ) : null}

      {props.isOffline ? (
        <div className="offline-banner">Offline: cloud-dependent features are temporarily unavailable.</div>
      ) : null}

      {props.showPausedBanner ? <div className="paused-banner">Simulation Paused</div> : null}

      {props.saveStatus.trim() ? (
        <div className="status-toast">
          <span>{props.saveStatus}</span>
          <button onClick={props.onDismissSaveStatus}>Dismiss</button>
        </div>
      ) : null}

      {props.onboardingActive ? (
        <aside className="onboarding-card">
          <p>Quick Start Guide</p>
          <strong>
            Step {Math.min(props.onboardingStep + 1, props.onboardingStepCount)} / {props.onboardingStepCount}:{" "}
            {props.onboardingTitle}
          </strong>
          <span>{props.onboardingDescription}</span>
          <div className="onboarding-actions">
            <button onClick={props.onSkipOnboarding}>Skip Guide</button>
            <button className="primary" onClick={props.onAdvanceOnboarding}>
              Next Tip
            </button>
          </div>
        </aside>
      ) : null}
    </>
  );
}
