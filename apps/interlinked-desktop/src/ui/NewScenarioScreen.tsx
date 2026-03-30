export default function NewScenarioScreen(props: {
  scenarioName: string;
  busy: boolean;
  onNameChange: (v: string) => void;
  onCreate: () => void;
  onBack: () => void;
}) {
  return (
    <div className="form-screen">
      <header>
        <h2>New Scenario</h2>
        <p>Create a managed planning project.</p>
      </header>
      <div className="form-card">
        <label>Scenario Name</label>
        <input
          value={props.scenarioName}
          onChange={(e) => props.onNameChange(e.target.value)}
          placeholder="West Yorkshire Baseline"
        />
      </div>
      <div className="form-actions">
        <button onClick={props.onBack}>Back to Menu</button>
        <button onClick={props.onCreate} disabled={props.busy}>
          Create Scenario
        </button>
      </div>
    </div>
  );
}
