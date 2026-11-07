// Order matters: tokens define the custom properties everything else reads,
// base consumes them for element defaults, utilities layer on top.
// Primitive styles are imported by their own components in src/ui/.
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/utilities.css";

import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { AuthProvider } from "./auth/AuthContext";
import { ThemeProvider } from "./app/ThemeContext";
import { ConfirmProvider } from "./app/ConfirmContext";
import { ZoomIndicator } from "./app/ZoomIndicator";
import { installZoom } from "./app/zoom";
import { installDragShim } from "./app/dragShim";
import { isWindows } from "./app/platform";
import { ErrorBoundary } from "./ui/ErrorBoundary";

// Every window (main, pop-outs, new ones) runs this bundle, so each applies
// the stored zoom and handles the shortcuts itself.
installZoom();

// In-app drag and drop on Windows, where WebView2 delivers no HTML5 drag
// events while Tauri takes desktop file drops (see dragShim.ts).
if (isWindows && "__TAURI_INTERNALS__" in window) installDragShim();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    {/* The last resort: the shell has its own per page, so this one only
        catches what breaks the app around them. */}
    <ErrorBoundary>
      <ThemeProvider>
        <ConfirmProvider>
          <AuthProvider>
            <App />
            <ZoomIndicator />
          </AuthProvider>
        </ConfirmProvider>
      </ThemeProvider>
    </ErrorBoundary>
  </React.StrictMode>,
);
