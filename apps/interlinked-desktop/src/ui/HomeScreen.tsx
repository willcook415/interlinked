import {
  InterlinkedActionCard,
  InterlinkedButton,
  InterlinkedHero,
  InterlinkedMetaPill,
  InterlinkedPageShell,
  InterlinkedPanelHeader,
  InterlinkedSaveSlotCard,
  InterlinkedSectionPanel,
} from "./primitives";

export default function HomeScreen(props: {
  onContinueGame: () => void;
  onOpenRecentGame: (saveId: string) => void;
  onLoadGame: () => void;
  onNewGame: () => void;
  onNewScenario: () => void;
  onLoadScenario: () => void;
  canContinue: boolean;
  latestGameName?: string | null;
  latestGameOpenedAt?: string | null;
  recentGames: Array<{
    project_id: string;
    name: string;
    last_opened_at: string;
    start_city?: string | null;
    start_country?: string | null;
  }>;
}) {
  const latestGameLabel = props.latestGameName?.trim() || "Latest save";
  const latestOpened = props.latestGameOpenedAt
    ? new Date(props.latestGameOpenedAt).toLocaleString()
    : null;
  const recentGames = props.recentGames.slice(0, 4);
  const mostRecentCity = recentGames[0]?.start_city ?? "No active city";
  const recentCountLabel =
    props.recentGames.length === 1 ? "1 save" : `${props.recentGames.length} saves`;
  const continueMeta = props.canContinue
    ? `${latestGameLabel}${latestOpened ? ` • ${latestOpened}` : ""}`
    : "No active game saves yet.";

  return (
    <InterlinkedPageShell className="il-home" centered>
      <InterlinkedHero
        eyebrow="Interlinked Platform"
        title="Interlinked"
        subtitle="Infrastructure control room for transport simulation, planning, and worldbuilding."
      >
        <div className="il-home-hero-title-row">
          <div className="il-home-meta-row">
            <InterlinkedMetaPill tone="brand">Control Room</InterlinkedMetaPill>
            <InterlinkedMetaPill tone="warning">UK-First Simulation Stack</InterlinkedMetaPill>
            <InterlinkedMetaPill>Planning + Operations + Sandbox</InterlinkedMetaPill>
          </div>
          <p className="il-home-credit">Created by William Cook</p>
        </div>
        <div className="il-home-hero-actions">
          <InterlinkedButton tone="primary" onClick={props.canContinue ? props.onContinueGame : props.onNewGame}>
            {props.canContinue ? "Continue Latest Save" : "Start New Game"}
          </InterlinkedButton>
          <InterlinkedButton tone="secondary" onClick={props.onLoadGame}>
            Open Save Browser
          </InterlinkedButton>
          <InterlinkedButton tone="ghost" onClick={props.onNewScenario}>
            New Scenario
          </InterlinkedButton>
        </div>
        <div className="il-home-system-strip">
          <article className="il-home-system-item">
            <p className="il-home-system-label">Simulation Mode</p>
            <p className="il-home-system-value">World Sandbox</p>
            <p className="il-home-system-hint">Transport infrastructure planning and live operations.</p>
          </article>
          <article className="il-home-system-item">
            <p className="il-home-system-label">Primary Region</p>
            <p className="il-home-system-value">{mostRecentCity}</p>
            <p className="il-home-system-hint">Latest active city context for rapid re-entry.</p>
          </article>
          <article className="il-home-system-item">
            <p className="il-home-system-label">Save Stack</p>
            <p className="il-home-system-value">{recentCountLabel}</p>
            <p className="il-home-system-hint">Persistent simulations and planning sessions.</p>
          </article>
        </div>
      </InterlinkedHero>

      <section className="il-home-grid">
        <InterlinkedSectionPanel className="il-home-panel-games">
          <InterlinkedPanelHeader
            title="Games"
            description="Open world sandbox progression and live operations saves."
            meta={<InterlinkedMetaPill tone="brand">Primary Journey</InterlinkedMetaPill>}
          />
          <div className="il-home-action-stack is-dual">
            <InterlinkedActionCard
              onClick={props.onContinueGame}
              disabled={!props.canContinue}
              title="Continue Game"
              description="Jump directly into your latest active save."
              meta={continueMeta}
            />
            <InterlinkedActionCard
              onClick={props.onLoadGame}
              title="Load Game"
              description="Browse all saves, including archived projects."
              meta="Open save list"
            />
            <InterlinkedActionCard
              onClick={props.onNewGame}
              title="New Game"
              description="Start a fresh transport network with guided setup."
              meta="Create new run"
            />
          </div>
        </InterlinkedSectionPanel>

        <InterlinkedSectionPanel className="il-home-panel-scenarios">
          <InterlinkedPanelHeader
            title="Scenarios"
            description="Planning projects, experimentation, and reporting runs."
            meta={<InterlinkedMetaPill>Sandbox Studio</InterlinkedMetaPill>}
          />
          <div className="il-home-action-stack">
            <InterlinkedActionCard
              onClick={props.onNewScenario}
              title="New Scenario"
              description="Create a planning brief with full scenario controls."
              meta="Start drafting"
            />
            <InterlinkedActionCard
              onClick={props.onLoadScenario}
              title="Load Scenario"
              description="Resume previous planning and experimentation sessions."
              meta="Open scenario library"
            />
          </div>
        </InterlinkedSectionPanel>

        <InterlinkedSectionPanel className="il-home-panel-recents">
          <InterlinkedPanelHeader
            title="Recent Saves"
            description="Jump back into your latest sessions quickly."
            meta={<InterlinkedMetaPill>{recentCountLabel}</InterlinkedMetaPill>}
          />
          <div className="il-home-recent-stack">
            {recentGames.length === 0 ? (
              <InterlinkedSaveSlotCard empty title="No game saves yet. Start with New Game." />
            ) : (
              recentGames.map((save) => (
                <InterlinkedSaveSlotCard
                  key={save.project_id}
                  onClick={() => props.onOpenRecentGame(save.project_id)}
                  title={save.name}
                  subtitle={`${save.start_city ?? "Unknown City"}${
                    save.start_country ? `, ${save.start_country}` : ""
                  }`}
                  meta={new Date(save.last_opened_at).toLocaleString()}
                />
              ))
            )}
          </div>
        </InterlinkedSectionPanel>
      </section>
    </InterlinkedPageShell>
  );
}
