import { HashRouter, Navigate, Route, Routes } from "react-router-dom";
import { modules } from "./ModuleRegistry";
import Shell from "./Shell";

// HashRouter, not BrowserRouter: the packaged app serves the frontend from the
// filesystem via Tauri's asset protocol, not a real HTTP server with history-API
// fallback support.
export default function AppRouter() {
  return (
    <HashRouter>
      <Routes>
        <Route element={<Shell />}>
          {modules.map((m) => (
            <Route key={m.id} path={m.path} element={<m.Component />} />
          ))}
          <Route path="*" element={<Navigate to={modules[0].path} replace />} />
        </Route>
      </Routes>
    </HashRouter>
  );
}
