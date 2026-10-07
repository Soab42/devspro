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

// Every window (main, pop-outs, new ones) runs this bundle, so each applies
// the stored zoom and handles the shortcuts itself.
installZoom();

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ThemeProvider>
      <ConfirmProvider>
        <AuthProvider>
          <App />
          <ZoomIndicator />
        </AuthProvider>
      </ConfirmProvider>
    </ThemeProvider>
  </React.StrictMode>,
);
