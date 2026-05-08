import { type CSSProperties, useEffect, useMemo, useState } from "react";
import { InterlinkedButton, InterlinkedPageShell } from "./primitives";
import type { SaveBrowserEntry } from "../types";
import landingBackgroundImage from "../assets/branding/interlinked-landing-background.png";
import logoWordmarkImage from "../assets/branding/interlinked-logo-wordmark.png";

const COMMUNITY_LINKS = [
  { label: "Discord", url: "https://discord.com" },
  { label: "Wiki", url: "https://github.com/willcook415/interlinked/wiki" },
  { label: "Feedback", url: "https://github.com/willcook415/interlinked/issues/new/choose" },
] as const;

function parseDate(value: string | null | undefined): Date | null {
  const raw = (value ?? "").trim();
  if (!raw) return null;
  const numeric = Number(raw);
  const parsed = Number.isFinite(numeric)
    ? new Date(numeric >= 1_000_000_000_000 ? numeric : numeric * 1000)
    : new Date(raw);
  if (Number.isNaN(parsed.getTime())) return null;
  return parsed;
}

function formatDateLabel(value: string | null | undefined): string | null {
  const parsed = parseDate(value);
  if (!parsed) return null;
  return parsed.toLocaleString(undefined, {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

async function closeDesktopWindow(): Promise<void> {
  const tauriRuntime = (window as Window & { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__;
  if (tauriRuntime) {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("quit_app");
      return;
    } catch {
      // Continue to window-level fallbacks.
    }
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().destroy();
      return;
    } catch {
      // Continue to web fallback.
    }
  }

  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().close();
    return;
  } catch {
    // Fall back gracefully for non-Tauri preview contexts.
  }
  window.close();
}

async function openExternalLink(url: string): Promise<void> {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
    return;
  } catch {
    // Browser fallback for non-Tauri preview contexts.
  }
  window.open(url, "_blank", "noopener,noreferrer");
}

export default function HomeScreen(props: {
  onContinueGame: () => void;
  onLoadGame: () => void;
  onNewGame: () => void;
  onOpenSettings: () => void;
  canContinue: boolean;
  latestGame: SaveBrowserEntry | null;
}) {
  const [aboutOpen, setAboutOpen] = useState(false);
  const latestGameName = props.latestGame?.name?.trim() || "Latest Save";
  const locationParts = [props.latestGame?.start_city?.trim(), props.latestGame?.start_country?.trim()]
    .filter((value): value is string => Boolean(value));
  const latestGameLocation = locationParts.length > 0 ? locationParts.join(", ") : "Location unavailable";
  const latestPlayedLabel = formatDateLabel(props.latestGame?.last_played_at);
  const inGameDateLabel = formatDateLabel(props.latestGame?.in_game_date);
  const networkSizeLabel =
    props.latestGame?.network_size === null || props.latestGame?.network_size === undefined
      ? null
      : Math.round(props.latestGame.network_size).toLocaleString();
  const buildLabel = ((import.meta.env.VITE_APP_VERSION as string | undefined)?.trim() || null) ??
    (import.meta.env.DEV ? "Development Build" : null);
  const buildChannel = import.meta.env.DEV ? "Preview" : "Release";
  const buildStamp = buildLabel ?? "Unavailable";
  const backgroundStyle = useMemo(
    () =>
      ({
        "--il-home-bg-image": `url("${landingBackgroundImage}")`,
      }) as CSSProperties,
    []
  );

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
    <InterlinkedPageShell
      className="il-home"
      style={backgroundStyle}
      onContextMenu={(event) => {
        event.preventDefault();
      }}
    >
      <section className="il-home-layout">
        <section className="il-home-column">
          <header className="il-home-brand" aria-label="Interlinked">
            <div className="il-home-logo-shell">
              <img className="il-home-logo-wordmark" src={logoWordmarkImage} alt="Interlinked" />
            </div>
            <p className="il-home-brand-strapline">Strategic Public Transport Simulation</p>
          </header>

          <section className="il-home-actions" aria-label="Primary Actions">
            <article className={`il-home-continue-module ${props.canContinue ? "" : "is-unavailable"}`}>
              <InterlinkedButton
                className="il-home-action is-continue"
                tone="primary"
                onClick={props.onContinueGame}
                disabled={!props.canContinue}
              >
                Continue
              </InterlinkedButton>

              <div className="il-home-continue-info">
                {props.canContinue ? (
                  <>
                    <p className="il-home-continue-eyebrow">Latest Project</p>
                    <p className="il-home-continue-name">{latestGameName}</p>
                    <dl className="il-home-continue-meta">
                      <div>
                        <dt>Region</dt>
                        <dd>{latestGameLocation}</dd>
                      </div>
                      <div>
                        <dt>Last Played</dt>
                        <dd>{latestPlayedLabel ?? "Unavailable"}</dd>
                      </div>
                      {inGameDateLabel ? (
                        <div>
                          <dt>Simulation Date</dt>
                          <dd>{inGameDateLabel}</dd>
                        </div>
                      ) : null}
                      {networkSizeLabel ? (
                        <div>
                          <dt>Network Stops</dt>
                          <dd>{networkSizeLabel}</dd>
                        </div>
                      ) : null}
                    </dl>
                  </>
                ) : (
                  <p className="il-home-continue-empty">
                    No resumable save found. Start a new project or load an existing one.
                  </p>
                )}
              </div>
            </article>

            <InterlinkedButton className="il-home-action is-secondary-action" tone="secondary" onClick={props.onLoadGame}>
              Load
            </InterlinkedButton>
            <InterlinkedButton className="il-home-action is-secondary-action" tone="secondary" onClick={props.onNewGame}>
              New
            </InterlinkedButton>
          </section>

          <div className="il-home-lower-shell">
            <section className="il-home-utility-rail" aria-label="Utility Actions">
              <button className="il-home-quiet-action" type="button" onClick={props.onOpenSettings}>
                Settings
              </button>
              <button className="il-home-quiet-action" type="button" onClick={() => setAboutOpen(true)}>
                About
              </button>
              <button
                className="il-home-quiet-action"
                type="button"
                onClick={() => {
                  void closeDesktopWindow();
                }}
              >
                Exit
              </button>
            </section>

            <div className="il-home-utility-divider" aria-hidden="true" />
          </div>
        </section>

        <div className="il-home-scene-spacer" aria-hidden="true">
          <div className="il-home-scene-glow" />
        </div>
      </section>
      <footer className="il-home-page-footer" aria-label="Community Utilities">
        {COMMUNITY_LINKS.map((link) => (
          <button
            key={link.label}
            className="il-home-page-footer-link"
            type="button"
            onClick={() => {
              void openExternalLink(link.url);
            }}
          >
            {link.label}
          </button>
        ))}
      </footer>
      <p className="il-home-build-stamp">
        <span className="il-home-build-channel">{buildChannel}</span>
        <span className="il-home-build-divider" aria-hidden="true">
          /
        </span>
        <span className="il-home-build-version">{buildStamp}</span>
      </p>

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
