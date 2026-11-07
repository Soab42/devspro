import { Component, ErrorInfo, ReactNode } from "react";
import { Button } from "./Button";
import { RefreshCw } from "./icons";
import { Callout } from "./Status";
import "./ErrorBoundary.css";

interface ErrorBoundaryProps {
  children: ReactNode;
}

interface ErrorBoundaryState {
  error: Error | null;
}

/**
 * Shows what broke instead of nothing. Without one, React unmounts the whole
 * tree on a render error and the window goes black with no clue why. The
 * shell keys one per page, so a crashing page leaves the sidebar working and
 * moving to another page starts it afresh.
 */
export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
  state: ErrorBoundaryState = { error: null };

  static getDerivedStateFromError(error: Error): ErrorBoundaryState {
    return { error };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error(error, info.componentStack);
  }

  render() {
    const { error } = this.state;
    if (!error) return this.props.children;
    return (
      <div className="error-boundary">
        <Callout
          tone="danger"
          title="Something went wrong on this screen"
          actions={
            <>
              <Button variant="secondary" size="sm" onClick={() => this.setState({ error: null })}>
                Try again
              </Button>
              <Button variant="secondary" size="sm" icon={RefreshCw} onClick={() => window.location.reload()}>
                Reload
              </Button>
            </>
          }
        >
          <pre className="error-boundary-detail">{error.stack ?? String(error)}</pre>
        </Callout>
      </div>
    );
  }
}

