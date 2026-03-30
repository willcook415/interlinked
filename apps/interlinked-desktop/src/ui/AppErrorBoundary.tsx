import { Component, type ErrorInfo, type ReactNode } from "react";

type BoundaryState = {
  error: Error | null;
  resetToken: number;
};

export default class AppErrorBoundary extends Component<
  { children: ReactNode },
  BoundaryState
> {
  state: BoundaryState = {
    error: null,
    resetToken: 0,
  };

  static getDerivedStateFromError(error: Error): Partial<BoundaryState> {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo): void {
    console.error("AppErrorBoundary caught render error", error, info);
  }

  private recover = (): void => {
    this.setState((prev) => ({
      error: null,
      resetToken: prev.resetToken + 1,
    }));
  };

  render(): ReactNode {
    if (this.state.error) {
      return (
        <div className="app-crash-screen">
          <div className="app-crash-card">
            <p>Session interrupted</p>
            <h1>The UI hit an unexpected error.</h1>
            <p className="app-crash-message">{this.state.error.message || "Unknown render error."}</p>
            <div className="app-crash-actions">
              <button onClick={this.recover}>Try Recover</button>
              <button onClick={() => window.location.reload()}>Reload App</button>
            </div>
          </div>
        </div>
      );
    }

    return (
      <div key={this.state.resetToken} style={{ width: "100%", height: "100%" }}>
        {this.props.children}
      </div>
    );
  }
}
