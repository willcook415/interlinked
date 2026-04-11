import { useEffect, useState } from "react";
import { InterlinkedButton, InterlinkedPageShell } from "./primitives";
import type { SaveBrowserEntry } from "../types";

export default function HomeScreen(props: {
  onContinueGame: () => void;
  onLoadGame: () => void;
  onNewGame: () => void;
  onNewScenario: () => void;
  onOpenSettings: () => void;
  canContinue: boolean;
  latestGame: SaveBrowserEntry | null;
}) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const latestGameName = props.latestGame?.name?.trim() || "Latest Save";
  const locationParts = [props.latestGame?.start_city?.trim(), props.latestGame?.start_country?.trim()]
    .filter((value): value is string => Boolean(value));
  const latestGameLocation = locationParts.length > 0 ? locationParts.join(", ") : "Location unavailable";
  const latestPlayedAt = props.latestGame?.last_played_at
    ? new Date(props.latestGame.last_played_at)
    : null;
  const latestPlayedLabel =
    latestPlayedAt && !Number.isNaN(latestPlayedAt.getTime())
      ? latestPlayedAt.toLocaleString(undefined, {
          dateStyle: "medium",
          timeStyle: "short",
        })
      : null;
  const buildLabel =
    ((import.meta.env.VITE_APP_VERSION as string | undefined)?.trim() || null) ??
    (import.meta.env.DEV ? "Development Build" : null);

  useEffect(() => {
    if (!aboutOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") setAboutOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => {
      window.removeEventListener("keydown", onKeyDown);
    };
  }, [aboutOpen]);

  useEffect(() => {
    const root = document.documentElement;
    const body = document.body;
    const previousRootOverflow = root.style.overflow;
    const previousBodyOverflow = body.style.overflow;
    const previousBodyOverscroll = body.style.overscrollBehavior;
    body.classList.add("il-home-route");
    root.style.overflow = "hidden";
    body.style.overflow = "hidden";
    body.style.overscrollBehavior = "none";
    return () => {
      body.classList.remove("il-home-route");
      root.style.overflow = previousRootOverflow;
      body.style.overflow = previousBodyOverflow;
      body.style.overscrollBehavior = previousBodyOverscroll;
    };
  }, []);

  return (
    <InterlinkedPageShell className="il-home" centered>
      <div className="il-home-atmosphere" aria-hidden="true">
        <span className="il-home-trace is-a" />
        <span className="il-home-trace is-b" />
        <span className="il-home-trace is-c" />
        <span className="il-home-node is-a" />
        <span className="il-home-node is-b" />
        <span className="il-home-node is-c" />
        <span className="il-home-pulse" />
      </div>

      <section className="il-home-stage">
        <header className="il-home-identity" aria-label="Interlinked">
          <h1 className="il-home-wordmark">INTERLINKED</h1>
          <p className="il-home-sublabel">Build the network. Run the system.</p>
        </header>

        <section className="il-home-primary" aria-label="Primary Actions">
          <div className="il-home-continue-block">
            <InterlinkedButton
              className="il-home-primary-action is-continue"
              tone="primary"
              onClick={props.onContinueGame}
              disabled={!props.canContinue}
            >
              Continue
            </InterlinkedButton>
            <div className={`il-home-resume-meta ${props.canContinue ? "" : "is-empty"}`}>
              {props.canContinue ? (
                <>
                  <p className="il-home-resume-label">Resuming</p>
                  <p className="il-home-resume-name">{latestGameName}</p>
                  <p className="il-home-resume-detail">
                    {latestGameLocation}
                    {latestPlayedLabel ? `  •  ${latestPlayedLabel}` : ""}
                  </p>
                </>
              ) : (
                <p className="il-home-resume-empty">No active game save available.</p>
              )}
            </div>
          </div>
          <InterlinkedButton className="il-home-primary-action" tone="secondary" onClick={props.onLoadGame}>
            Load Game
          </InterlinkedButton>
          <InterlinkedButton className="il-home-primary-action" tone="secondary" onClick={props.onNewGame}>
            New Game
          </InterlinkedButton>
          <InterlinkedButton className="il-home-primary-action" tone="ghost" onClick={props.onNewScenario}>
            New Scenario
          </InterlinkedButton>
        </section>

        <footer className="il-home-utility" aria-label="Utility Actions">
          <button className="il-home-utility-button" type="button" onClick={props.onOpenSettings}>
            Settings
          </button>
          <button className="il-home-utility-button" type="button" onClick={() => setAboutOpen(true)}>
            About
          </button>
        </footer>
      </section>

      {aboutOpen ? (
        <div className="il-home-about-overlay" role="presentation" onClick={() => setAboutOpen(false)}>
          <aside
            className="il-home-about"
            role="dialog"
            aria-modal="true"
            aria-label="About Interlinked"
            onClick={(event) => event.stopPropagation()}
          >
            <p className="il-home-about-eyebrow">About</p>
            <h2>INTERLINKED</h2>
            <p>Transport simulation for building, operating, and refining public transit networks.</p>
            <div className="il-home-about-meta">
              <p>
                <span>Created by</span>
                <strong>William Cook</strong>
              </p>
              {buildLabel ? (
                <p>
                  <span>Build</span>
                  <strong>{buildLabel}</strong>
                </p>
              ) : null}
            </div>
            <div className="il-home-about-actions">
              <InterlinkedButton size="sm" tone="ghost" onClick={() => setAboutOpen(false)}>
                Close
              </InterlinkedButton>
            </div>
          </aside>
        </div>
      ) : null}
    </InterlinkedPageShell>
  );
}
