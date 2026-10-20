import React from "react";
import { createRoot } from "react-dom/client";
import App from "./App";
import { installBrowserPreview } from "./dev/browserPreview";

// The Vite URL normally runs inside the Tauri webview. If it is opened in a
// normal browser there is no IPC and the first invoke() would crash the
// renderer, so install the dev-only preview shim (sample data, in-memory
// settings) before mounting. Production builds drop this branch.
if (import.meta.env.DEV) installBrowserPreview();

/**
 * Last-resort error boundary. Without it, any render error in a screen
 * unmounts the whole tree and leaves a blank window. With it, a crash shows
 * a readable message and the error is logged to the webview console (which
 * surfaces in the dev log) so the real cause is visible instead of a blank.
 */
class ErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { error: Error | null }
> {
  state: { error: Error | null } = { error: null };

  static getDerivedStateFromError(error: Error) {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    // eslint-disable-next-line no-console
    console.error("[teletype] render error:", error, info.componentStack);
  }

  render() {
    if (this.state.error) {
      return (
        <div
          style={{
            padding: 24,
            fontFamily: "inherit",
            fontSize: 14,
            color: "var(--danger)",
            whiteSpace: "pre-wrap",
          }}
        >
          <div style={{ fontWeight: 600, marginBottom: 8 }}>
            Teletype hit a display error
          </div>
          <div style={{ color: "var(--text-secondary)", fontSize: 13 }}>
            {String(this.state.error?.message || this.state.error)}
          </div>
          <button
            style={{ marginTop: 16, padding: "6px 12px", cursor: "pointer" }}
            onClick={() => this.setState({ error: null })}
          >
            Try again
          </button>
        </div>
      );
    }
    return this.props.children;
  }
}

createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <ErrorBoundary>
      <App />
    </ErrorBoundary>
  </React.StrictMode>
);
